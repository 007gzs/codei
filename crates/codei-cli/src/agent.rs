use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use codei_agent::{AgentEvent, AgentLoop};
use codei_config::ResolvedConfig;
use codei_llm::create_provider;
use codei_mcp::McpManager;
use codei_session::{Session, SessionStore};
use codei_tools::{handler_for_policy, ApprovalPolicy, ToolContext};
use codei_tui::{run_repl, run_tui, InteractiveLaunch, ReplOptions, TuiOptions};
use tokio::sync::mpsc;

use crate::cli::Cli;

struct AgentRuntime {
    config: Arc<ResolvedConfig>,
    provider: Arc<dyn codei_llm::LlmProvider>,
    provider_name: String,
    model: Arc<RwLock<String>>,
    session: Session,
    store: SessionStore,
}

pub async fn run_agent(cli: &Cli, resolved: ResolvedConfig) -> Result<()> {
    let config = Arc::new(resolved);
    let provider_name = config.config.defaults.provider.clone();
    let provider = create_provider(&config).context("create LLM provider")?;
    let store =
        SessionStore::open_for_config(&config.config.session).context("open session store")?;

    let session = load_session(cli, &store, &config.cwd)?;
    let model = Arc::new(RwLock::new(config.config.defaults.model.clone()));
    let mcp = McpManager::connect_optional().await;
    if let Some(ref manager) = mcp {
        tracing::info!(
            servers = manager.connections().len(),
            tools = manager.tool_count(),
            "MCP connected"
        );
    }
    let runtime = AgentRuntime {
        config,
        provider,
        provider_name,
        model,
        session,
        store,
    };

    if cli.print {
        let prompt = cli
            .prompt
            .clone()
            .context("print mode requires a prompt argument")?;
        return run_print(cli, runtime, mcp, prompt).await;
    }

    if let Some(prompt) = cli.prompt.clone() {
        return run_print(cli, runtime, mcp, prompt).await;
    }

    let opts = TuiOptions {
        auto_approve: cli.auto_approve(),
    };
    let repl_opts = ReplOptions {
        auto_approve: cli.auto_approve(),
    };

    let launch = InteractiveLaunch {
        config: runtime.config,
        provider: runtime.provider,
        provider_name: runtime.provider_name,
        model: runtime.model,
        session: runtime.session,
        store: runtime.store,
        mcp,
    };

    if cli.no_tui {
        run_repl(launch, repl_opts).await
    } else {
        run_tui(launch, opts).await
    }
}

async fn run_print(
    cli: &Cli,
    mut runtime: AgentRuntime,
    mcp: Option<Arc<McpManager>>,
    prompt: String,
) -> Result<()> {
    let policy = if cli.auto_approve() {
        ApprovalPolicy::Never
    } else {
        ApprovalPolicy::OnDestructive
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tool_ctx = ToolContext {
        cwd: runtime.config.cwd.clone(),
        config: Arc::clone(&runtime.config),
        approval: Arc::from(handler_for_policy(policy)),
    };
    let agent = AgentLoop::new(
        runtime.config,
        runtime.model,
        runtime.provider,
        runtime.provider_name,
        tool_ctx,
        mcp,
        Some(tx),
    );

    let agent_task = async {
        agent
            .run_turn(&mut runtime.session, &prompt, &runtime.store)
            .await
            .context("agent turn failed")
    };

    tokio::pin!(agent_task);

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(AgentEvent::AssistantDelta { text }) => print!("{text}"),
                    Some(AgentEvent::ToolStarted { name, args }) => {
                        eprintln!("\n[tool:{name}] {args}");
                    }
                    Some(AgentEvent::ToolFinished { name, result }) => {
                        eprintln!("[tool:{name}] {}", result.content);
                    }
                    Some(AgentEvent::TurnComplete { .. }) => break,
                    Some(AgentEvent::Error { message }) => {
                        eprintln!("Error: {message}");
                        break;
                    }
                    None => break,
                }
            }
            result = &mut agent_task => {
                result?;
                break;
            }
        }
    }

    println!();
    Ok(())
}

fn load_session(cli: &Cli, store: &SessionStore, cwd: &std::path::Path) -> Result<Session> {
    if let Some(id) = &cli.resume {
        return store
            .load(id)
            .with_context(|| format!("resume session {id}"));
    }
    if cli.r#continue {
        if let Some(session) = store.latest()? {
            return Ok(session);
        }
    }
    Ok(Session::new(cwd.to_path_buf()))
}

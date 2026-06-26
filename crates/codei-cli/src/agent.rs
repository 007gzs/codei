use std::sync::Arc;

use anyhow::{Context, Result};
use codei_agent::AgentEvent;
use codei_config::ResolvedConfig;
use codei_sdk::{
    approval_policy, build_interactive_launch, resolve_session, run_turn_with_events,
    InteractiveLaunch,
};
use codei_session::SessionStore;
use codei_tui::{run_repl, run_tui, ReplOptions, TuiOptions};
use tracing::info;

use crate::cli::Cli;

pub async fn run_agent(cli: &Cli, resolved: ResolvedConfig) -> Result<()> {
    let config = Arc::new(resolved);
    let store = Arc::new(
        SessionStore::open_for_config(&config.config.session).context("open session store")?,
    );

    let session = resolve_session(
        store.as_ref(),
        &config.cwd,
        cli.resume.as_deref(),
        cli.r#continue,
    )
    .context("load session")?;

    let launch = build_interactive_launch(config, session, store).await?;

    if let Some(ref manager) = launch.mcp {
        info!(
            servers = manager.connections().len(),
            tools = manager.tool_count(),
            "MCP connected"
        );
    }

    if cli.print {
        let prompt = cli
            .prompt
            .clone()
            .context("print mode requires a prompt argument")?;
        return run_print(cli, launch, prompt).await;
    }

    if let Some(prompt) = cli.prompt.clone() {
        return run_print(cli, launch, prompt).await;
    }

    let opts = TuiOptions {
        auto_approve: cli.auto_approve(),
    };
    let repl_opts = ReplOptions {
        auto_approve: cli.auto_approve(),
    };

    if cli.no_tui {
        run_repl(launch, repl_opts).await
    } else {
        run_tui(launch, opts).await
    }
}

async fn run_print(cli: &Cli, mut launch: InteractiveLaunch, prompt: String) -> Result<()> {
    let policy = approval_policy(cli.auto_approve());
    let runtime = launch.runtime();

    run_turn_with_events(
        &runtime,
        &mut launch.session,
        &prompt,
        policy,
        |event| match event {
            AgentEvent::AssistantDelta { text } => print!("{text}"),
            AgentEvent::ToolStarted { name, args } => {
                eprintln!("\n[tool:{name}] {args}");
            }
            AgentEvent::ToolFinished { name, result } => {
                eprintln!("[tool:{name}] {}", result.content);
            }
            AgentEvent::TurnComplete { .. } => {}
            AgentEvent::Error { message } => eprintln!("Error: {message}"),
        },
    )
    .await?;

    println!();
    Ok(())
}

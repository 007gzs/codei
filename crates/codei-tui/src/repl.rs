use std::io::{self, Write};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use codei_agent::{AgentEvent, AgentLoop};
use codei_commands::{parse_input, Input};
use codei_i18n::t_fmt;
use codei_llm::Usage;
use codei_tools::{handler_for_policy, ApprovalPolicy, ToolContext};
use tokio::sync::mpsc;

use crate::launch::InteractiveLaunch;
use crate::slash::{handle_slash, SlashContext};

pub struct ReplOptions {
    pub auto_approve: bool,
}

pub async fn run_repl(launch: InteractiveLaunch, opts: ReplOptions) -> Result<()> {
    let InteractiveLaunch {
        config,
        provider,
        provider_name,
        model,
        mut session,
        store,
        mcp,
    } = launch;
    let policy = if opts.auto_approve {
        ApprovalPolicy::Never
    } else {
        ApprovalPolicy::OnDestructive
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tool_ctx = ToolContext {
        cwd: config.cwd.clone(),
        config: Arc::clone(&config),
        approval: Arc::from(handler_for_policy(policy)),
    };
    let provider_name = Arc::new(RwLock::new(provider_name));
    let agent = AgentLoop::new(
        Arc::clone(&config),
        Arc::clone(&model),
        provider,
        provider_name.read().expect("provider lock").clone(),
        tool_ctx,
        mcp,
        Some(tx),
    );

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut token_usage = Usage::default();
    let mut last_turn_usage = None;

    loop {
        while let Ok(event) = rx.try_recv() {
            render_event(&mut stdout, &event, &mut token_usage, &mut last_turn_usage)?;
        }

        write!(stdout, "\n> ")?;
        stdout.flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }

        match parse_input(&line) {
            Input::SlashCommand(cmd) => {
                let mut ctx = SlashContext {
                    session: &mut session,
                    store: &store,
                    model: &model,
                    provider_name: &provider_name,
                    agent: &agent,
                    token_usage: &mut token_usage,
                    last_turn_usage: &mut last_turn_usage,
                };
                match handle_slash(cmd, &mut ctx).await? {
                    crate::slash::SlashAction::Exit => break,
                    crate::slash::SlashAction::Message(text) => writeln!(stdout, "{text}")?,
                    crate::slash::SlashAction::Continue => {}
                }
            }
            Input::UserMessage(msg) if msg.is_empty() => {}
            Input::UserMessage(msg) => {
                if let Err(err) = agent.run_turn(&mut session, &msg, &store).await {
                    writeln!(
                        stdout,
                        "{}",
                        t_fmt("tui_error_prefix", &[("message", &err.to_string())])
                    )?;
                }
                while let Ok(event) = rx.try_recv() {
                    render_event(&mut stdout, &event, &mut token_usage, &mut last_turn_usage)?;
                }
            }
        }
    }

    Ok(())
}

fn render_event(
    stdout: &mut impl Write,
    event: &AgentEvent,
    token_usage: &mut Usage,
    last_turn_usage: &mut Option<Usage>,
) -> io::Result<()> {
    match event {
        AgentEvent::AssistantDelta { text } => write!(stdout, "{text}"),
        AgentEvent::ToolStarted { name, args } => writeln!(stdout, "\n[tool:{name}] {args}"),
        AgentEvent::ToolFinished { name, result } => {
            writeln!(stdout, "[tool:{name}]\n{}", result.content)
        }
        AgentEvent::TurnComplete { usage } => {
            if let Some(u) = *usage {
                token_usage.add_assign(u);
                *last_turn_usage = Some(u);
                writeln!(
                    stdout,
                    "\n{}",
                    t_fmt(
                        "slash_tokens_turn",
                        &[
                            ("input", &u.input_tokens.to_string()),
                            ("output", &u.output_tokens.to_string()),
                        ],
                    )
                )
            } else {
                writeln!(stdout)
            }
        }
        AgentEvent::Error { message } => writeln!(
            stdout,
            "{}",
            t_fmt("tui_error_prefix", &[("message", message)])
        ),
    }
}

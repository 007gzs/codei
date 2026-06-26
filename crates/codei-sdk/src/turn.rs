use std::sync::Arc;

use codei_agent::{AgentError, AgentEvent, AgentLoop, TurnOutcome};
use codei_config::ResolvedConfig;
use codei_session::{Session, SessionStore};
use codei_tools::{handler_for_policy, ApprovalPolicy, ToolContext};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;

use crate::error::SdkError;
use crate::runtime::AgentRuntime;
use crate::session::SessionHandle;

pub fn approval_policy(auto_approve: bool) -> ApprovalPolicy {
    if auto_approve {
        ApprovalPolicy::Never
    } else {
        ApprovalPolicy::OnDestructive
    }
}

pub fn tool_context(config: &Arc<ResolvedConfig>, policy: ApprovalPolicy) -> ToolContext {
    ToolContext {
        cwd: config.cwd.clone(),
        config: Arc::clone(config),
        approval: Arc::from(handler_for_policy(policy)),
    }
}

pub fn agent_loop(
    runtime: &AgentRuntime,
    tool_ctx: ToolContext,
    events: Option<UnboundedSender<AgentEvent>>,
) -> AgentLoop {
    AgentLoop::new(
        Arc::clone(&runtime.config),
        Arc::clone(&runtime.model),
        Arc::clone(&runtime.provider),
        runtime.provider_name.clone(),
        tool_ctx,
        runtime.mcp.clone(),
        events,
    )
}

pub async fn run_turn(
    agent: &AgentLoop,
    session: &mut Session,
    prompt: &str,
    store: &SessionStore,
) -> Result<TurnOutcome, AgentError> {
    agent.run_turn(session, prompt, store).await
}

pub async fn run_turn_with_events<F>(
    runtime: &AgentRuntime,
    session: &mut Session,
    prompt: &str,
    policy: ApprovalPolicy,
    mut on_event: F,
) -> Result<TurnOutcome, SdkError>
where
    F: FnMut(AgentEvent),
{
    let (tx, mut rx) = mpsc::unbounded_channel();
    let agent = agent_loop(runtime, tool_context(&runtime.config, policy), Some(tx));
    let prompt = prompt.to_string();
    let store = runtime.store.clone();

    let mut agent_task = Box::pin(async {
        agent
            .run_turn(session, &prompt, store.as_ref())
            .await
            .map_err(SdkError::Agent)
    });

    let mut outcome = TurnOutcome::default();
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(AgentEvent::TurnComplete { usage }) => {
                        outcome.usage = usage;
                        on_event(AgentEvent::TurnComplete { usage: outcome.usage });
                        break;
                    }
                    Some(other) => on_event(other),
                    None => break,
                }
            }
            result = &mut agent_task => {
                outcome = result?;
                break;
            }
        }
    }

    Ok(outcome)
}

pub fn spawn_turn(
    handle: Arc<SessionHandle>,
    prompt: String,
    policy: ApprovalPolicy,
    tx: UnboundedSender<AgentEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _guard = handle.turn_lock.lock().await;
        let tool_ctx = tool_context(&handle.runtime.config, policy);
        let agent = agent_loop(&handle.runtime, tool_ctx, Some(tx.clone()));
        let session = Arc::clone(&handle.session);
        let store = Arc::clone(&handle.runtime.store);
        let mut session = session.write().await;
        if let Err(err) = agent.run_turn(&mut session, &prompt, store.as_ref()).await {
            let _ = tx.send(AgentEvent::Error {
                message: err.to_string(),
            });
        }
    })
}

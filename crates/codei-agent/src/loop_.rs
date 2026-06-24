use std::sync::{Arc, RwLock};

use codei_config::{load_plugins, run_hooks, HookEvent, ResolvedConfig};
use codei_llm::{create_provider_by_name, ChatRequest, LlmProvider, StreamEvent, ToolCall, Usage};
use codei_mcp::McpManager;
use codei_session::{ContextBuilder, Session, SessionStore, ToolCallRecord};
use codei_tools::{
    default_registry, register_mcp_tools, tool_definitions, ToolContext, ToolRegistry,
};
use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::error::AgentError;
use crate::event::AgentEvent;
use crate::prompt::{build_system_prompt, load_project_instructions};
use crate::task_tool::{TaskDeps, TaskTool};
use crate::tool_args::repair_tool_args;

#[derive(Debug, Clone, Default)]
pub struct TurnOutcome {
    pub usage: Option<Usage>,
}

pub struct AgentLoop {
    config: Arc<ResolvedConfig>,
    model: Arc<RwLock<String>>,
    provider_name: Arc<RwLock<String>>,
    provider: Arc<RwLock<Arc<dyn LlmProvider>>>,
    tools: ToolRegistry,
    tool_ctx: ToolContext,
    system_prompt: String,
    max_tool_rounds: u32,
    events: Option<UnboundedSender<AgentEvent>>,
}

impl AgentLoop {
    pub fn new(
        config: Arc<ResolvedConfig>,
        model: Arc<RwLock<String>>,
        provider: Arc<dyn LlmProvider>,
        provider_name: String,
        tool_ctx: ToolContext,
        mcp: Option<Arc<McpManager>>,
        events: Option<UnboundedSender<AgentEvent>>,
    ) -> Self {
        let project = load_project_instructions(&config);
        let system_prompt = build_system_prompt(&config, &project);
        let max_tool_rounds = config.config.agent.max_tool_rounds_per_turn;
        let max_sub_rounds = (max_tool_rounds / 2).clamp(3, 12);

        let mut tools = default_registry(&config);
        if let Some(ref manager) = mcp {
            register_mcp_tools(&mut tools, manager);
        }

        let deps = Arc::new(TaskDeps {
            config: Arc::clone(&config),
            model: Arc::clone(&model),
            provider: Arc::new(RwLock::new(provider.clone())),
            provider_name: Arc::new(RwLock::new(provider_name.clone())),
            tool_ctx: tool_ctx.clone(),
            mcp: mcp.clone(),
            max_sub_rounds,
            system_prompt: system_prompt.clone(),
        });
        tools.register(Box::new(TaskTool::new(deps)));

        Self {
            config,
            model,
            provider_name: Arc::new(RwLock::new(provider_name)),
            provider: Arc::new(RwLock::new(provider)),
            tools,
            tool_ctx,
            system_prompt,
            max_tool_rounds,
            events,
        }
    }

    pub(crate) fn with_tools(parts: AgentParts) -> Self {
        Self {
            config: parts.config,
            model: parts.model,
            provider_name: Arc::new(RwLock::new(parts.provider_name)),
            provider: Arc::new(RwLock::new(parts.provider)),
            tools: parts.tools,
            tool_ctx: parts.tool_ctx,
            system_prompt: parts.system_prompt,
            max_tool_rounds: parts.max_tool_rounds,
            events: parts.events,
        }
    }

    pub fn provider_name(&self) -> Arc<RwLock<String>> {
        Arc::clone(&self.provider_name)
    }

    pub fn set_provider(&self, name: &str) -> Result<(), AgentError> {
        let provider = create_provider_by_name(&self.config, name)?;
        *self
            .provider_name
            .write()
            .map_err(|_| AgentError::Stopped("provider lock poisoned".into()))? = name.to_string();
        *self
            .provider
            .write()
            .map_err(|_| AgentError::Stopped("provider lock poisoned".into()))? = provider;
        Ok(())
    }

    pub async fn run_turn(
        &self,
        session: &mut Session,
        user_input: &str,
        store: &SessionStore,
    ) -> Result<TurnOutcome, AgentError> {
        if let Some(root) = &self.config.project_root {
            let plugins = load_plugins(root);
            run_hooks(
                &plugins,
                HookEvent::BeforeTurn,
                &self.config.cwd,
                &[("CODEI_PROMPT", user_input.to_string())],
            )
            .map_err(AgentError::Config)?;
        }

        session.push_user(user_input);
        store.save(session)?;

        let mut usage = None;
        let mut rounds = 0u32;

        loop {
            if rounds >= self.max_tool_rounds {
                return Err(AgentError::MaxToolRounds);
            }
            rounds += 1;

            let model = self.model.read().expect("model lock poisoned").clone();
            let provider = self
                .provider
                .read()
                .expect("provider lock poisoned")
                .clone();
            let request = ChatRequest {
                model: model.clone(),
                messages: ContextBuilder::build_with_config(
                    session,
                    &self.system_prompt,
                    Some(&self.config.config.agent),
                ),
                tools: Some(tool_definitions(&self.tools)),
                temperature: Some(self.config.config.defaults.temperature),
                max_tokens: Some(self.config.config.defaults.max_tokens),
            };

            debug!(
                round = rounds,
                model = %model,
                provider = %self.provider_name.read().expect("provider lock poisoned"),
                message_count = request.messages.len(),
                "agent llm round start"
            );
            for (index, msg) in request.messages.iter().enumerate() {
                debug!(
                    index,
                    role = ?msg.role,
                    tool_calls = msg.tool_calls.as_ref().map(|c| c.len()).unwrap_or(0),
                    content = %truncate_opt(msg.content.as_deref(), 300),
                    tool_call_id = ?msg.tool_call_id,
                    "agent request message"
                );
                if let Some(calls) = &msg.tool_calls {
                    for call in calls {
                        debug!(
                            id = %call.id,
                            name = %call.name,
                            arguments = %call.arguments,
                            "agent request tool_call"
                        );
                    }
                }
            }

            let stream = provider.chat(request).await?;
            let response = self.collect_stream(stream).await?;

            debug!(
                round = rounds,
                content_len = response.content.len(),
                tool_count = response.tool_calls.len(),
                "agent stream collected"
            );
            if response.tool_calls.is_empty() {
                debug!(
                    round = rounds,
                    content_preview = %truncate(&response.content, 500),
                    "agent text-only response (no tool calls)"
                );
            }
            for call in &response.tool_calls {
                debug!(
                    id = %call.id,
                    name = %call.name,
                    arguments = %call.arguments,
                    "agent tool_call final"
                );
            }
            if response
                .tool_calls
                .iter()
                .any(|c| c.arguments.trim().is_empty() || c.arguments.trim() == "{}")
            {
                warn!(
                    round = rounds,
                    "agent received tool_call with empty or {{}} arguments"
                );
            }

            if let Some(u) = response.usage {
                usage = Some(u);
            }

            if response.tool_calls.is_empty() {
                session.push_assistant(response.content, None);
                store.save(session)?;
                self.emit(AgentEvent::TurnComplete { usage });
                self.run_after_turn_hooks(user_input)?;
                return Ok(TurnOutcome { usage });
            }

            let records: Vec<ToolCallRecord> = response
                .tool_calls
                .iter()
                .map(|tc| ToolCallRecord {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect();
            let assistant_content = response.content.clone();
            session.push_assistant(response.content, Some(records));
            store.save(session)?;

            for call in &response.tool_calls {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": call.arguments }));
                let args = repair_tool_args(&call.name, &assistant_content, args);
                debug!(name = %call.name, args = %args, "agent tool execute");
                self.emit(AgentEvent::ToolStarted {
                    name: call.name.clone(),
                    args: args.clone(),
                });

                let result = match self.tools.execute(&self.tool_ctx, &call.name, args).await {
                    Ok(result) => result,
                    Err(err) => codei_tools::ToolResult {
                        content: err.to_string(),
                        is_error: true,
                    },
                };
                debug!(
                    name = %call.name,
                    is_error = result.is_error,
                    content = %truncate(&result.content, 800),
                    "agent tool result"
                );
                self.emit(AgentEvent::ToolFinished {
                    name: call.name.clone(),
                    result: result.clone(),
                });
                session.push_tool(&call.id, result.content);
                store.save(session)?;
            }
        }
    }

    fn run_after_turn_hooks(&self, user_input: &str) -> Result<(), AgentError> {
        if let Some(root) = &self.config.project_root {
            let plugins = load_plugins(root);
            run_hooks(
                &plugins,
                HookEvent::AfterTurn,
                &self.config.cwd,
                &[("CODEI_PROMPT", user_input.to_string())],
            )
            .map_err(AgentError::Config)?;
        }
        Ok(())
    }

    async fn collect_stream(
        &self,
        mut stream: codei_llm::ChatStream,
    ) -> Result<StreamedResponse, AgentError> {
        let mut content = String::new();
        let mut usage = None;
        let mut pending_tools: std::collections::BTreeMap<
            u32,
            (Option<String>, Option<String>, String),
        > = std::collections::BTreeMap::new();

        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::TextDelta(text) => {
                    self.emit(AgentEvent::AssistantDelta { text: text.clone() });
                    content.push_str(&text);
                }
                StreamEvent::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments,
                } => {
                    debug!(
                        index,
                        id = ?id,
                        name = ?name,
                        arguments = ?arguments,
                        "agent tool_call delta"
                    );
                    let entry = pending_tools.entry(index).or_default();
                    if let Some(id) = id {
                        entry.0 = Some(id);
                    }
                    if let Some(name) = name {
                        entry.1 = Some(name);
                    }
                    if let Some(args) = arguments {
                        entry.2.push_str(&args);
                    }
                }
                StreamEvent::Usage(u) => usage = Some(u),
                StreamEvent::Done => {}
            }
        }

        let mut tool_calls = Vec::new();
        for (_, (id, name, arguments)) in pending_tools {
            if let Some(name) = name {
                let id = id.unwrap_or_else(|| {
                    warn!(
                        name = %name,
                        "tool call missing id; using synthetic id (function calling mode)"
                    );
                    format!("call_{name}")
                });
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        Ok(StreamedResponse {
            content,
            tool_calls,
            usage,
        })
    }

    fn emit(&self, event: AgentEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(event);
        }
    }
}

struct StreamedResponse {
    content: String,
    tool_calls: Vec<ToolCall>,
    usage: Option<Usage>,
}

pub(crate) struct AgentParts {
    pub config: Arc<ResolvedConfig>,
    pub model: Arc<RwLock<String>>,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_name: String,
    pub tool_ctx: ToolContext,
    pub tools: ToolRegistry,
    pub max_tool_rounds: u32,
    pub system_prompt: String,
    pub events: Option<UnboundedSender<AgentEvent>>,
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    format!(
        "{}… [truncated, total {} bytes]",
        &value[..max],
        value.len()
    )
}

fn truncate_opt(value: Option<&str>, max: usize) -> String {
    match value {
        Some(text) => truncate(text, max),
        None => String::from("<none>"),
    }
}

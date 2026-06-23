use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use codei_config::ResolvedConfig;
use codei_llm::LlmProvider;
use codei_mcp::McpManager;
use codei_session::{Session, SessionStore};
use codei_tools::{default_registry, register_mcp_tools, Tool, ToolContext, ToolError, ToolResult};
use serde_json::{json, Value};
use tempfile::NamedTempFile;

use crate::loop_::{AgentLoop, AgentParts};

/// Shared dependencies for spawning sub-agents from the task tool.
pub struct TaskDeps {
    pub config: Arc<ResolvedConfig>,
    pub model: Arc<RwLock<String>>,
    pub provider: Arc<RwLock<Arc<dyn LlmProvider>>>,
    pub provider_name: Arc<RwLock<String>>,
    pub tool_ctx: ToolContext,
    pub mcp: Option<Arc<McpManager>>,
    pub max_sub_rounds: u32,
    pub system_prompt: String,
}

pub struct TaskTool {
    deps: Arc<TaskDeps>,
}

impl TaskTool {
    pub fn new(deps: Arc<TaskDeps>) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn name(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Delegate a focused sub-task to a child agent with read/search tools (no nested task)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Detailed instructions for the sub-agent"
                },
                "description": {
                    "type": "string",
                    "description": "Short label for logging"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing prompt".into()))?
            .to_string();

        let deps = Arc::clone(&self.deps);
        let cwd = ctx.cwd.clone();
        let content = run_sub_agent(deps, cwd, prompt).await?;

        Ok(ToolResult {
            content,
            is_error: false,
        })
    }
}

async fn run_sub_agent(
    deps: Arc<TaskDeps>,
    cwd: std::path::PathBuf,
    prompt: String,
) -> Result<String, ToolError> {
    let mut sub_tools = default_registry(&deps.config);
    if let Some(mcp) = &deps.mcp {
        register_mcp_tools(&mut sub_tools, mcp);
    }

    let provider = deps
        .provider
        .read()
        .map_err(|_| ToolError::Failed {
            name: "task".into(),
            message: "provider lock poisoned".into(),
        })?
        .clone();
    let provider_name = deps
        .provider_name
        .read()
        .map_err(|_| ToolError::Failed {
            name: "task".into(),
            message: "provider lock poisoned".into(),
        })?
        .clone();

    let sub_agent = AgentLoop::with_tools(AgentParts {
        config: Arc::clone(&deps.config),
        model: Arc::clone(&deps.model),
        provider,
        provider_name,
        tool_ctx: deps.tool_ctx.clone(),
        tools: sub_tools,
        max_tool_rounds: deps.max_sub_rounds,
        system_prompt: deps.system_prompt.clone(),
        events: None,
    });

    let mut session = Session::new(cwd);
    let tmp = NamedTempFile::new().map_err(ToolError::Io)?;
    let store = SessionStore::open(tmp.path()).map_err(|err| ToolError::Failed {
        name: "task".into(),
        message: err.to_string(),
    })?;

    sub_agent
        .run_turn(&mut session, &prompt, &store)
        .await
        .map_err(|err| ToolError::Failed {
            name: "task".into(),
            message: err.to_string(),
        })?;

    Ok(last_assistant_text(&session).unwrap_or_else(|| "Sub-agent finished.".into()))
}

fn last_assistant_text(session: &Session) -> Option<String> {
    use codei_session::{MessageContent, Role};
    session
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .map(|m| match &m.content {
            MessageContent::Text(text) => text.clone(),
        })
}

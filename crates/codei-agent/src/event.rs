use codei_llm::Usage;
use codei_tools::ToolResult;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    AssistantDelta { text: String },
    ToolStarted { name: String, args: Value },
    ToolFinished { name: String, result: ToolResult },
    TurnComplete { usage: Option<Usage> },
    Error { message: String },
}

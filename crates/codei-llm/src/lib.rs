//! LLM provider abstraction for CodeI.

mod error;
mod factory;
mod message;
mod provider;
mod stream;
mod tool;
mod tool_format;

pub use error::LlmError;
pub use factory::{create_provider, create_provider_by_name};
pub use message::{Message, Role, ToolCall};
pub use provider::LlmProvider;
pub use stream::{collect_response, ChatStream, StreamEvent, Usage};
pub use tool::ToolDefinition;
pub use tool_format::ToolFormat;

use serde::{Deserialize, Serialize};

/// Request sent to an LLM provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Aggregated assistant response after consuming a stream.
#[derive(Debug, Clone, Default)]
pub struct AssistantResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

/// OpenAI-compatible streaming chunk (internal).
#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionChunk {
    pub choices: Vec<StreamChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamChoice {
    pub delta: StreamDelta,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct StreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
    pub function_call: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamToolCallDelta {
    #[serde(default)]
    pub index: u32,
    pub id: Option<String>,
    pub function: Option<StreamFunctionDelta>,
    /// Some OpenAI-compatible servers (e.g. certain vLLM builds) flatten these fields.
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

impl StreamToolCallDelta {
    pub(crate) fn id(&self) -> Option<String> {
        self.id.clone()
    }

    pub(crate) fn name(&self) -> Option<String> {
        self.function
            .as_ref()
            .and_then(|f| f.name.clone())
            .or_else(|| self.name.clone())
    }

    pub(crate) fn arguments(&self) -> Option<String> {
        self.function
            .as_ref()
            .and_then(|f| f.arguments.clone())
            .or_else(|| self.arguments.clone())
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

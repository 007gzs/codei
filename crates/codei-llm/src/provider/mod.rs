mod anthropic;
mod openai;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;

use async_trait::async_trait;

use crate::{ChatRequest, ChatStream, LlmError};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;

    fn supports_tools(&self) -> bool {
        true
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatStream, LlmError>;
}

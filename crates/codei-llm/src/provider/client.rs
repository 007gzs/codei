use reqwest::Client;

use crate::LlmError;

/// Shared HTTP client for LLM providers with a consistent User-Agent.
pub fn build_http_client() -> Result<Client, LlmError> {
    Client::builder()
        .user_agent(format!("codei/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(Into::into)
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider {provider} is not configured")]
    ProviderNotConfigured { provider: String },

    #[error("missing API key: set {env} environment variable or configure api_key")]
    MissingApiKey { env: String },

    #[error("configuration error: {0}")]
    Config(String),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("failed to parse streaming response: {0}")]
    StreamParse(String),

    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
}

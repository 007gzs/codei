use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(#[from] codei_llm::LlmError),

    #[error("tool error: {0}")]
    Tool(#[from] codei_tools::ToolError),

    #[error("session error: {0}")]
    Session(#[from] codei_session::SessionError),

    #[error("configuration error: {0}")]
    Config(#[from] codei_config::ConfigError),

    #[error("agent stopped: {0}")]
    Stopped(String),

    #[error("max tool rounds exceeded")]
    MaxToolRounds,
}

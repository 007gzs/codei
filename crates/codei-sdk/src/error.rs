use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("config error: {0}")]
    Config(#[from] codei_config::ConfigError),
    #[error("llm error: {0}")]
    Llm(#[from] codei_llm::LlmError),
    #[error("session error: {0}")]
    Session(#[from] codei_session::SessionError),
    #[error("agent error: {0}")]
    Agent(#[from] codei_agent::AgentError),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

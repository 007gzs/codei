use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(Box<figment::Error>),

    #[error("failed to serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("failed to write configuration to {path}: {source}")]
    Write {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse MCP configuration: {0}")]
    McpParse(String),

    #[error("failed to create configuration directory {path}: {source}")]
    CreateDir {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("invalid language {language}: expected zh-CN or en-US")]
    InvalidLanguage { language: String },

    #[error("invalid agent.compaction_threshold {threshold}: expected a value between 0.0 and 1.0")]
    InvalidCompactionThreshold { threshold: f32 },

    #[error("missing API key: set {env} or configure providers.*.api_key")]
    MissingApiKey { env: String },

    #[error("plugin hook failed: `{command}` (exit {code:?})")]
    HookFailed { command: String, code: Option<i32> },
}

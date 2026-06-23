use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("path not allowed: {0}")]
    PathNotAllowed(String),

    #[error("tool execution denied by user")]
    Denied,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("tool {name} failed: {message}")]
    Failed { name: String, message: String },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

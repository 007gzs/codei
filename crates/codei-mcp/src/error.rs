use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
    #[error("failed to spawn MCP server `{name}`: {source}")]
    Spawn {
        name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("MCP server `{name}` exited unexpectedly")]
    Exited { name: String },

    #[error("MCP I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid JSON-RPC message: {0}")]
    Json(#[from] serde_json::Error),

    #[error("MCP protocol error on `{server}`: {message}")]
    Protocol { server: String, message: String },

    #[error("MCP request timed out after {secs}s")]
    Timeout { secs: u64 },

    #[error("no MCP servers configured")]
    NoServers,
}

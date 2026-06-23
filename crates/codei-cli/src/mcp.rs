use clap::Subcommand;

#[derive(Subcommand)]
pub enum McpCommands {
    /// List configured MCP servers.
    List,
    /// Create default MCP config file.
    Init,
    /// Add an MCP server (`codei mcp add myserver -- node server.js`).
    Add {
        /// Server name.
        name: String,
        /// Command and arguments after `--`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

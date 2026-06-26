use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::mcp::McpCommands;
use crate::session::SessionCommands;

#[derive(Parser)]
#[command(
    name = "codei",
    version,
    about = "CodeI ai-coding agent",
    propagate_version = true
)]
pub struct Cli {
    /// Optional one-shot prompt (print or agent mode).
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Print mode: stream response to stdout (implies non-TUI).
    #[arg(short, long)]
    pub print: bool,

    /// Continue the most recent session.
    #[arg(short, long)]
    pub r#continue: bool,

    /// Resume a specific session by ID.
    #[arg(short, long)]
    pub resume: Option<String>,

    /// Auto-approve tool calls.
    #[arg(short, long)]
    pub yes: bool,

    /// Auto-approve all tool calls (same as --yes).
    #[arg(long)]
    pub yolo: bool,

    /// Disable full-screen TUI; use line-based REPL.
    #[arg(long)]
    pub no_tui: bool,

    /// Working directory for project discovery and relative paths.
    #[arg(long, global = true)]
    pub cwd: Option<PathBuf>,

    /// Default LLM model (overrides config).
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// LLM provider name (overrides config).
    #[arg(long, global = true)]
    pub provider: Option<String>,

    /// UI language: zh-CN or en-US (overrides config).
    #[arg(long, global = true)]
    pub language: Option<String>,

    /// Enable debug logging.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Manage chat sessions.
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Manage MCP servers.
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Start the web UI server.
    Server {
        /// Host to bind
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to listen on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
    /// Print version information.
    Version,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show merged configuration.
    Show,
    /// Create the user config file with defaults.
    Init,
}

impl Cli {
    pub fn auto_approve(&self) -> bool {
        self.yes || self.yolo
    }

    pub fn load_options(&self) -> codei_config::LoadOptions {
        codei_config::LoadOptions {
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            provider: self.provider.clone(),
            language: self.language.clone(),
        }
    }
}

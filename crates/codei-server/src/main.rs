use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use codei_sdk::{run_server, ServerOptions};

#[derive(Parser)]
#[command(name = "codei-server", about = "HTTP server for CodeI web UI")]
struct Cli {
    /// Host to bind
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Default working directory for new sessions
    #[arg(long, value_name = "DIR")]
    cwd: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let default_cwd = match cli.cwd {
        Some(path) => path,
        None => std::env::current_dir()?,
    };

    run_server(ServerOptions {
        host: cli.host,
        port: cli.port,
        default_cwd,
        verbose: cli.verbose,
    })
    .await
}

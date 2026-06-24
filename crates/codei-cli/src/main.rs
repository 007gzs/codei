mod agent;
mod cli;
mod commands;
mod mcp;
mod session;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn init_tracing(verbose: bool) {
    use std::fs::OpenOptions;
    use std::io;

    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let default = if verbose {
        "codei=info,codei_llm=debug,codei_agent=debug,codei_tools=debug"
    } else {
        "warn"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    if verbose {
        let log_path = codei_config::debug_log_path();
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new().create(true).append(true).open(&log_path);
        match file {
            Ok(file) => {
                eprintln!("CodeI debug log: {}", log_path.display());
                let file_layer = fmt::layer()
                    .with_writer(file)
                    .with_ansi(false)
                    .with_target(true);
                tracing_subscriber::registry()
                    .with(filter)
                    .with(file_layer)
                    .init();
            }
            Err(err) => {
                eprintln!(
                    "Warning: could not open log file {}: {err}",
                    log_path.display()
                );
                fmt().with_env_filter(filter).init();
            }
        }
    } else {
        fmt().with_env_filter(filter).with_writer(io::stderr).init();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    commands::run(cli).await
}

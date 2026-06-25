mod api;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::state::AppState;

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

fn init_tracing(verbose: bool) {
    let default = if verbose {
        "codei_server=info,codei_agent=debug,codei_llm=debug"
    } else {
        "codei_server=info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let default_cwd = match cli.cwd {
        Some(path) => path,
        None => std::env::current_dir()?,
    };
    let default_cwd = default_cwd.canonicalize().unwrap_or(default_cwd);

    let state = Arc::new(AppState::new(default_cwd.clone())?);
    let app = Router::new()
        .merge(api::routes(default_cwd))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port).parse()?;
    tracing::info!("CodeI server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

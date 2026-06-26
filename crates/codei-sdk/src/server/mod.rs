mod api;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::session::SessionService;

/// Options for the CodeI HTTP server.
pub struct ServerOptions {
    pub host: String,
    pub port: u16,
    pub default_cwd: PathBuf,
    pub verbose: bool,
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

/// Start the CodeI web server and block until it shuts down.
pub async fn run_server(opts: ServerOptions) -> anyhow::Result<()> {
    init_tracing(opts.verbose);

    let default_cwd = opts.default_cwd.canonicalize().unwrap_or(opts.default_cwd);

    let service = Arc::new(SessionService::new(default_cwd.clone()).await?);
    let app = Router::new()
        .merge(api::routes(default_cwd))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(service);

    let addr: SocketAddr = format!("{}:{}", opts.host, opts.port).parse()?;
    tracing::info!("CodeI server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

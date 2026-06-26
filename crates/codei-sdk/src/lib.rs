//! Programmatic SDK for CodeI.

mod client;
mod error;
mod runtime;
mod session;
mod turn;

#[cfg(feature = "server")]
pub mod server;

pub use client::{CodeiClient, CodeiClientBuilder, RunResult};
pub use error::SdkError;
pub use runtime::{
    build_agent_runtime, build_interactive_launch, open_session_store, resolve_session,
    AgentRuntime, InteractiveLaunch,
};
pub use session::{SessionHandle, SessionService};
pub use turn::{
    agent_loop, approval_policy, run_turn, run_turn_with_events, spawn_turn, tool_context,
};

#[cfg(feature = "server")]
pub use server::{run_server, ServerOptions};

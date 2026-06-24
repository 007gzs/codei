//! Agent loop orchestrating LLM, tools, and session.

mod compact;
mod error;
mod event;
mod loop_;
mod prompt;
mod task_tool;
mod tool_args;

pub use error::AgentError;
pub use event::AgentEvent;
pub use loop_::{AgentLoop, TurnOutcome};
pub use prompt::build_system_prompt;

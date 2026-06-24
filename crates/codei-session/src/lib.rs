//! Session management and persistence for CodeI.

mod compact;
mod context;
mod error;
mod model;
mod store;

pub use compact::{
    compact_messages, estimate_tokens, format_transcript, should_compact_session, TokenBudget,
};
pub use context::ContextBuilder;
pub use error::SessionError;
pub use model::{
    MessageContent, MessageId, Role, Session, SessionId, StoredMessage, ToolCallRecord,
};
pub use store::SessionStore;

//! Session management and persistence for CodeI.

mod compact;
mod context;
mod error;
mod model;
mod store;

pub use compact::{compact_messages, estimate_tokens, TokenBudget};
pub use context::ContextBuilder;
pub use error::SessionError;
pub use model::{
    MessageContent, MessageId, Role, Session, SessionId, StoredMessage, ToolCallRecord,
};
pub use store::SessionStore;

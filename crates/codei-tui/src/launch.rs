use std::sync::{Arc, RwLock};

use codei_config::ResolvedConfig;
use codei_llm::LlmProvider;
use codei_mcp::McpManager;
use codei_session::{Session, SessionStore};

/// Shared runtime state for interactive TUI / REPL modes.
pub struct InteractiveLaunch {
    pub config: Arc<ResolvedConfig>,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_name: String,
    pub model: Arc<RwLock<String>>,
    pub session: Session,
    pub store: SessionStore,
    pub mcp: Option<Arc<McpManager>>,
}

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use codei_config::{load, LoadOptions, ResolvedConfig};
use codei_llm::{create_provider, LlmProvider};
use codei_mcp::McpManager;
use codei_session::{Session, SessionStore};

use crate::error::SdkError;

/// Shared LLM / tool runtime for an interactive session.
#[derive(Clone)]
pub struct AgentRuntime {
    pub config: Arc<ResolvedConfig>,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_name: String,
    pub model: Arc<RwLock<String>>,
    pub store: Arc<SessionStore>,
    pub mcp: Option<Arc<McpManager>>,
}

/// Launch bundle for TUI / REPL interactive modes.
pub struct InteractiveLaunch {
    pub config: Arc<ResolvedConfig>,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_name: String,
    pub model: Arc<RwLock<String>>,
    pub session: Session,
    pub store: Arc<SessionStore>,
    pub mcp: Option<Arc<McpManager>>,
}

impl InteractiveLaunch {
    pub fn runtime(&self) -> AgentRuntime {
        AgentRuntime {
            config: Arc::clone(&self.config),
            provider: Arc::clone(&self.provider),
            provider_name: self.provider_name.clone(),
            model: Arc::clone(&self.model),
            store: Arc::clone(&self.store),
            mcp: self.mcp.clone(),
        }
    }
}

pub fn resolve_session(
    store: &SessionStore,
    cwd: &Path,
    resume: Option<&str>,
    continue_latest: bool,
) -> Result<Session, codei_session::SessionError> {
    if let Some(id) = resume {
        return store.load(id);
    }
    if continue_latest {
        if let Some(session) = store.latest()? {
            return Ok(session);
        }
    }
    Ok(Session::new(cwd.to_path_buf()))
}

pub async fn build_interactive_launch(
    config: Arc<ResolvedConfig>,
    session: Session,
    store: Arc<SessionStore>,
) -> Result<InteractiveLaunch, SdkError> {
    let runtime = build_agent_runtime(config, store).await?;
    Ok(InteractiveLaunch {
        config: runtime.config,
        provider: runtime.provider,
        provider_name: runtime.provider_name,
        model: runtime.model,
        session,
        store: runtime.store,
        mcp: runtime.mcp,
    })
}

pub async fn build_agent_runtime(
    config: Arc<ResolvedConfig>,
    store: Arc<SessionStore>,
) -> Result<AgentRuntime, SdkError> {
    let provider_name = config.config.defaults.provider.clone();
    let provider = create_provider(&config)?;
    let mcp = McpManager::connect_optional().await;
    let model = Arc::new(RwLock::new(config.config.defaults.model.clone()));
    Ok(AgentRuntime {
        config,
        provider,
        provider_name,
        model,
        store,
        mcp,
    })
}

pub async fn open_session_store(default_cwd: PathBuf) -> Result<Arc<SessionStore>, SdkError> {
    let resolved = load(&LoadOptions {
        cwd: Some(default_cwd),
        ..Default::default()
    })?;
    Ok(Arc::new(SessionStore::open_for_config(
        &resolved.config.session,
    )?))
}

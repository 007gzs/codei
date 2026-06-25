use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use codei_config::{load, LoadOptions, ResolvedConfig};
use codei_llm::create_provider;
use codei_mcp::McpManager;
use codei_session::{Session, SessionError, SessionStore};
use tokio::sync::{Mutex, RwLock as AsyncRwLock};

const LIST_LIMIT: usize = 200;

pub struct AppState {
    store: Arc<SessionStore>,
    sessions: RwLock<HashMap<String, Arc<ActiveSession>>>,
}

pub struct ActiveSession {
    pub config: Arc<ResolvedConfig>,
    pub session: Arc<AsyncRwLock<Session>>,
    pub store: Arc<SessionStore>,
    pub model: Arc<RwLock<String>>,
    pub provider: Arc<dyn codei_llm::LlmProvider>,
    pub provider_name: String,
    pub mcp: Option<Arc<McpManager>>,
    pub turn_lock: Mutex<()>,
}

impl AppState {
    pub fn new(default_cwd: PathBuf) -> Result<Self> {
        let resolved = load(&LoadOptions {
            cwd: Some(default_cwd),
            ..Default::default()
        })
        .context("load config for session store")?;
        let store = Arc::new(
            SessionStore::open_for_config(&resolved.config.session)
                .context("open session store")?,
        );
        Ok(Self {
            store,
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, SessionError> {
        let mut sessions = self.store.list(LIST_LIMIT)?;
        let guard = self.sessions.read().expect("sessions lock poisoned");
        for active in guard.values() {
            if let Ok(session) = active.session.try_read() {
                if sessions.iter().any(|s| s.id == session.id) {
                    continue;
                }
                sessions.push(session.clone());
            }
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(LIST_LIMIT);
        Ok(sessions)
    }

    pub fn message_count(&self, session: &Session) -> usize {
        if !session.messages.is_empty() {
            return session.messages.len();
        }
        self.store
            .load(&session.id)
            .map(|s| s.messages.len())
            .unwrap_or(0)
    }

    pub async fn create_session(&self, cwd: PathBuf) -> Result<Session> {
        if !cwd.is_dir() {
            anyhow::bail!("working directory does not exist: {}", cwd.display());
        }

        let session = Session::new(cwd);
        self.store.save(&session)?;

        let active = self.hydrate(session.clone()).await?;
        self.sessions
            .write()
            .expect("sessions lock poisoned")
            .insert(session.id.clone(), active);

        Ok(session)
    }

    pub async fn get_or_load(&self, id: &str) -> Result<Arc<ActiveSession>> {
        if let Some(active) = self.get(id) {
            return Ok(active);
        }

        let session = self
            .store
            .load(id)
            .map_err(|e| match e {
                SessionError::NotFound(_) => anyhow::anyhow!("session not found"),
                other => anyhow::Error::new(other),
            })?;

        let active = self.hydrate(session).await?;
        self.sessions
            .write()
            .expect("sessions lock poisoned")
            .insert(id.to_string(), Arc::clone(&active));
        Ok(active)
    }

    async fn hydrate(&self, session: Session) -> Result<Arc<ActiveSession>> {
        let resolved = load(&LoadOptions {
            cwd: Some(session.cwd.clone()),
            ..Default::default()
        })
        .context("load config")?;

        let config = Arc::new(resolved);
        let provider_name = config.config.defaults.provider.clone();
        let provider = create_provider(&config).context("create LLM provider")?;
        let mcp = McpManager::connect_optional().await;
        let model = Arc::new(RwLock::new(config.config.defaults.model.clone()));

        Ok(Arc::new(ActiveSession {
            config,
            session: Arc::new(AsyncRwLock::new(session)),
            store: Arc::clone(&self.store),
            model,
            provider,
            provider_name,
            mcp,
            turn_lock: Mutex::new(()),
        }))
    }

    fn get(&self, id: &str) -> Option<Arc<ActiveSession>> {
        let guard = self.sessions.read().ok()?;
        guard.get(id).cloned()
    }
}

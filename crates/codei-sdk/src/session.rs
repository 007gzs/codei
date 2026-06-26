use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use codei_session::{Session, SessionError, SessionStore};
use tokio::sync::{Mutex, RwLock as AsyncRwLock};

use crate::error::SdkError;
use crate::runtime::{build_agent_runtime, open_session_store, AgentRuntime};

const LIST_LIMIT: usize = 200;

/// Persistent session registry with in-memory agent runtime cache.
pub struct SessionService {
    store: Arc<SessionStore>,
    sessions: RwLock<HashMap<String, Arc<SessionHandle>>>,
}

pub struct SessionHandle {
    pub runtime: AgentRuntime,
    pub session: Arc<AsyncRwLock<Session>>,
    pub turn_lock: Mutex<()>,
}

impl SessionService {
    pub async fn new(default_cwd: PathBuf) -> Result<Self, SdkError> {
        let store = open_session_store(default_cwd).await?;
        Ok(Self {
            store,
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub fn store(&self) -> &Arc<SessionStore> {
        &self.store
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, SessionError> {
        let mut sessions = self.store.list(LIST_LIMIT)?;
        let guard = self.sessions.read().expect("sessions lock poisoned");
        for handle in guard.values() {
            if let Ok(session) = handle.session.try_read() {
                if sessions.iter().any(|s| s.id == session.id) {
                    continue;
                }
                sessions.push(session.clone());
            }
        }
        sessions.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
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

    pub async fn create_session(&self, cwd: PathBuf) -> Result<Session, SdkError> {
        if !cwd.is_dir() {
            return Err(SdkError::Other(anyhow::anyhow!(
                "working directory does not exist: {}",
                cwd.display()
            )));
        }

        let session = Session::new(cwd);
        self.store.save(&session)?;

        let handle = self.hydrate(session.clone()).await?;
        self.sessions
            .write()
            .expect("sessions lock poisoned")
            .insert(session.id.clone(), handle);

        Ok(session)
    }

    pub async fn get_or_load(&self, id: &str) -> Result<Arc<SessionHandle>, SdkError> {
        if let Some(handle) = self.get(id) {
            return Ok(handle);
        }

        let session = self.store.load(id).map_err(|e| match e {
            SessionError::NotFound(_) => SdkError::Other(anyhow::anyhow!("session not found")),
            other => SdkError::Session(other),
        })?;

        let handle = self.hydrate(session).await?;
        self.sessions
            .write()
            .expect("sessions lock poisoned")
            .insert(id.to_string(), Arc::clone(&handle));
        Ok(handle)
    }

    async fn hydrate(&self, session: Session) -> Result<Arc<SessionHandle>, SdkError> {
        let config = Arc::new(codei_config::load(&codei_config::LoadOptions {
            cwd: Some(session.cwd.clone()),
            ..Default::default()
        })?);
        let runtime = build_agent_runtime(config, Arc::clone(&self.store)).await?;
        Ok(Arc::new(SessionHandle {
            runtime,
            session: Arc::new(AsyncRwLock::new(session)),
            turn_lock: Mutex::new(()),
        }))
    }

    fn get(&self, id: &str) -> Option<Arc<SessionHandle>> {
        let guard = self.sessions.read().ok()?;
        guard.get(id).cloned()
    }
}

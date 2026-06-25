mod json;
mod sqlite;

use std::path::{Path, PathBuf};

use codei_config::{expand_tilde, SessionConfig, SessionStorage};

use crate::error::SessionError;
use crate::model::Session;

use json::JsonSessionStore;
use sqlite::SqliteSessionStore;

enum Backend {
    Sqlite(SqliteSessionStore),
    Json(JsonSessionStore),
}

/// Persistent session storage (SQLite or JSONL files, selected by config).
pub struct SessionStore {
    backend: Backend,
}

impl SessionStore {
    /// Open storage using `[session]` config (`storage` + `dir`).
    pub fn open_for_config(config: &SessionConfig) -> Result<Self, SessionError> {
        let dir = expand_tilde(&config.dir);
        let backend = match config.storage {
            SessionStorage::Sqlite => {
                Backend::Sqlite(SqliteSessionStore::open(&resolve_sqlite_path(&dir))?)
            }
            SessionStorage::Json => Backend::Json(JsonSessionStore::open(&dir)?),
        };
        Ok(Self { backend })
    }

    /// Open with default session config (SQLite under `~/.local/share/codei/sessions/`).
    pub fn open_default() -> Result<Self, SessionError> {
        Self::open_for_config(&SessionConfig::default())
    }

    /// Open a SQLite database at an explicit path (for tests and ephemeral sub-agents).
    pub fn open(path: &Path) -> Result<Self, SessionError> {
        Ok(Self {
            backend: Backend::Sqlite(SqliteSessionStore::open(path)?),
        })
    }

    pub fn save(&self, session: &Session) -> Result<(), SessionError> {
        match &self.backend {
            Backend::Sqlite(store) => store.save(session),
            Backend::Json(store) => store.save(session),
        }
    }

    pub fn load(&self, id: &str) -> Result<Session, SessionError> {
        match &self.backend {
            Backend::Sqlite(store) => store.load(id),
            Backend::Json(store) => store.load(id),
        }
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Session>, SessionError> {
        match &self.backend {
            Backend::Sqlite(store) => store.list(limit),
            Backend::Json(store) => store.list(limit),
        }
    }

    pub fn latest(&self) -> Result<Option<Session>, SessionError> {
        match &self.backend {
            Backend::Sqlite(store) => store.latest(),
            Backend::Json(store) => store.latest(),
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), SessionError> {
        match &self.backend {
            Backend::Sqlite(store) => store.delete(id),
            Backend::Json(store) => store.delete(id),
        }
    }

    pub fn export_jsonl(&self, id: &str) -> Result<String, SessionError> {
        let session = self.load(id)?;
        let mut lines = Vec::new();
        for msg in &session.messages {
            let line = serde_json::json!({
                "id": msg.id,
                "role": msg.role,
                "content": msg.content,
                "tool_calls": msg.tool_calls,
                "tool_call_id": msg.tool_call_id,
                "created_at": msg.created_at,
            });
            lines.push(serde_json::to_string(&line)?);
        }
        Ok(lines.join("\n"))
    }
}

/// SQLite DB path: `{dir}/sessions.db`, with legacy fallback.
fn resolve_sqlite_path(dir: &Path) -> PathBuf {
    let primary = dir.join("sessions.db");
    if primary.exists() {
        return primary;
    }
    let legacy = expand_tilde("~/.local/share/codei/sessions.db");
    if legacy.exists() {
        return legacy;
    }
    primary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Session;
    use codei_config::SessionStorage;

    #[test]
    fn open_json_backend() {
        let dir = tempfile::tempdir().unwrap();
        let config = SessionConfig {
            storage: SessionStorage::Json,
            dir: dir.path().to_string_lossy().into_owned(),
        };
        let store = SessionStore::open_for_config(&config).unwrap();
        let mut session = Session::new(dir.path().to_path_buf());
        session.push_user("hello");
        store.save(&session).unwrap();
        assert_eq!(store.load(&session.id).unwrap().messages.len(), 1);
    }
}

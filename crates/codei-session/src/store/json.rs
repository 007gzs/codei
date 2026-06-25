use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::SessionError;
use crate::model::{Session, StoredMessage};

/// Small sidecar with session metadata; messages live in `{id}.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMeta {
    id: String,
    title: Option<String>,
    cwd: PathBuf,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    message_count: usize,
}

pub struct JsonSessionStore {
    dir: PathBuf,
}

impl JsonSessionStore {
    pub fn open(dir: &Path) -> Result<Self, SessionError> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.meta.json"))
    }

    fn messages_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.jsonl"))
    }

    pub fn save(&self, session: &Session) -> Result<(), SessionError> {
        let previous_count = self
            .read_meta(&session.id)
            .ok()
            .map(|m| m.message_count)
            .unwrap_or(0);

        let messages_path = self.messages_path(&session.id);
        if previous_count == 0 || session.messages.len() < previous_count {
            self.rewrite_messages(&messages_path, &session.messages)?;
        } else if session.messages.len() > previous_count {
            self.append_messages(&messages_path, &session.messages[previous_count..])?;
        }

        let meta = SessionMeta {
            id: session.id.clone(),
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
            message_count: session.messages.len(),
        };
        self.write_meta(&meta)?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Session, SessionError> {
        if !self.meta_path(id).is_file() && !self.messages_path(id).is_file() {
            return Err(SessionError::NotFound(id.to_string()));
        }
        self.load_jsonl(id)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Session>, SessionError> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".meta.json") {
                continue;
            }
            let id = name.trim_end_matches(".meta.json");
            let meta = match self.read_meta(id) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            sessions.push(meta_to_session(&meta));
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(limit);
        Ok(sessions)
    }

    pub fn latest(&self) -> Result<Option<Session>, SessionError> {
        Ok(self.list(1)?.into_iter().next())
    }

    pub fn delete(&self, id: &str) -> Result<(), SessionError> {
        let meta = self.meta_path(id);
        let messages = self.messages_path(id);
        if !meta.is_file() && !messages.is_file() {
            return Err(SessionError::NotFound(id.to_string()));
        }
        let _ = fs::remove_file(meta);
        let _ = fs::remove_file(messages);
        Ok(())
    }

    fn load_jsonl(&self, id: &str) -> Result<Session, SessionError> {
        let meta = self.read_meta(id)?;
        let messages = self.read_messages(&self.messages_path(id))?;
        Ok(Session {
            id: meta.id,
            title: meta.title,
            cwd: meta.cwd,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            messages,
        })
    }

    fn read_meta(&self, id: &str) -> Result<SessionMeta, SessionError> {
        let json = fs::read_to_string(self.meta_path(id))?;
        serde_json::from_str(&json).map_err(SessionError::from)
    }

    fn write_meta(&self, meta: &SessionMeta) -> Result<(), SessionError> {
        let path = self.meta_path(&meta.id);
        let tmp = path.with_extension("meta.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(meta)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<StoredMessage>, SessionError> {
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            messages.push(serde_json::from_str(&line)?);
        }
        Ok(messages)
    }

    fn rewrite_messages(
        &self,
        path: &Path,
        messages: &[StoredMessage],
    ) -> Result<(), SessionError> {
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut file = File::create(&tmp)?;
            for msg in messages {
                writeln!(file, "{}", serde_json::to_string(msg)?)?;
            }
        }
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn append_messages(&self, path: &Path, messages: &[StoredMessage]) -> Result<(), SessionError> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        for msg in messages {
            writeln!(file, "{}", serde_json::to_string(msg)?)?;
        }
        Ok(())
    }
}

fn meta_to_session(meta: &SessionMeta) -> Session {
    Session {
        id: meta.id.clone(),
        title: meta.title.clone(),
        cwd: meta.cwd.clone(),
        created_at: meta.created_at,
        updated_at: meta.updated_at,
        messages: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Session;

    #[test]
    fn roundtrip_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSessionStore::open(dir.path()).unwrap();

        let mut session = Session::new(PathBuf::from("/tmp/project"));
        session.push_user("hello");
        session.push_assistant("hi".into(), None);

        store.save(&session).unwrap();
        let loaded = store.load(&session.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].text(), Some("hello"));
    }

    #[test]
    fn append_only_new_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSessionStore::open(dir.path()).unwrap();

        let mut session = Session::new(PathBuf::from("/tmp"));
        session.push_user("one");
        store.save(&session).unwrap();

        let before = fs::read_to_string(store.messages_path(&session.id)).unwrap();
        session.push_user("two");
        store.save(&session).unwrap();
        let after = fs::read_to_string(store.messages_path(&session.id)).unwrap();

        assert!(after.starts_with(&before));
        assert!(after.contains("two"));
        assert_eq!(store.load(&session.id).unwrap().messages.len(), 2);
    }

    #[test]
    fn compact_rewrites_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSessionStore::open(dir.path()).unwrap();

        let mut session = Session::new(PathBuf::from("/tmp"));
        for i in 0..5 {
            session.push_user(format!("msg {i}"));
        }
        store.save(&session).unwrap();

        session.compact(2);
        store.save(&session).unwrap();

        let loaded = store.load(&session.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        let lines = fs::read_to_string(store.messages_path(&session.id))
            .unwrap()
            .lines()
            .count();
        assert_eq!(lines, 2);
    }

    #[test]
    fn list_orders_by_updated_at_without_loading_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSessionStore::open(dir.path()).unwrap();

        let mut older = Session::new(PathBuf::from("/a"));
        older.push_user("first");
        store.save(&older).unwrap();

        let mut newer = Session::new(PathBuf::from("/b"));
        newer.push_user("second");
        store.save(&newer).unwrap();

        let sessions = store.list(10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, newer.id);
        assert!(sessions[0].messages.is_empty());
    }

    #[test]
    fn delete_removes_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonSessionStore::open(dir.path()).unwrap();
        let session = Session::new(PathBuf::from("/tmp"));
        store.save(&session).unwrap();
        store.delete(&session.id).unwrap();
        assert!(store.load(&session.id).is_err());
    }
}

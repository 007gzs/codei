use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::error::SessionError;
use crate::model::{MessageContent, Role, Session, StoredMessage};

pub struct SqliteSessionStore {
    conn: Mutex<Connection>,
}

impl SqliteSessionStore {
    pub fn open(path: &Path) -> Result<Self, SessionError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, SessionError> {
        self.conn.lock().map_err(|_| SessionError::LockPoisoned)
    }

    fn init_schema(&self) -> Result<(), SessionError> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                cwd TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                tool_call_id TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            "#,
        )?;
        Ok(())
    }

    pub fn save(&self, session: &Session) -> Result<(), SessionError> {
        let conn = self.conn()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO sessions (id, title, cwd, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                cwd = excluded.cwd,
                updated_at = excluded.updated_at",
            params![
                session.id,
                session.title,
                session.cwd.to_string_lossy().to_string(),
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
            ],
        )?;
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session.id],
        )?;
        for msg in &session.messages {
            let tool_calls = msg
                .tool_calls
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            let content = match &msg.content {
                MessageContent::Text(s) => s.clone(),
            };
            tx.execute(
                "INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    msg.id,
                    session.id,
                    role_to_str(msg.role),
                    content,
                    tool_calls,
                    msg.tool_call_id,
                    msg.created_at.to_rfc3339(),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Session, SessionError> {
        let conn = self.conn()?;
        Self::load_with_conn(&conn, id)
    }

    fn load_with_conn(conn: &Connection, id: &str) -> Result<Session, SessionError> {
        let mut stmt = conn
            .prepare("SELECT id, title, cwd, created_at, updated_at FROM sessions WHERE id = ?1")?;
        let session_row = stmt
            .query_row(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|_| SessionError::NotFound(id.to_string()))?;

        let messages = Self::load_messages(conn, id)?;

        Ok(Session {
            id: session_row.0,
            title: session_row.1,
            cwd: PathBuf::from(session_row.2),
            created_at: parse_ts(&session_row.3)?,
            updated_at: parse_ts(&session_row.4)?,
            messages,
        })
    }

    pub fn list(&self, limit: usize) -> Result<Vec<Session>, SessionError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id FROM sessions ORDER BY updated_at DESC LIMIT ?1")?;
        let ids: Vec<String> = stmt
            .query_map(params![limit as i64], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.iter()
            .map(|id| Self::load_with_conn(&conn, id))
            .collect()
    }

    pub fn latest(&self) -> Result<Option<Session>, SessionError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id FROM sessions ORDER BY updated_at DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            Ok(Some(Self::load_with_conn(&conn, &id)?))
        } else {
            Ok(None)
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), SessionError> {
        let conn = self.conn()?;
        let changed = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(SessionError::NotFound(id.to_string()));
        }
        Ok(())
    }

    fn load_messages(
        conn: &Connection,
        session_id: &str,
    ) -> Result<Vec<StoredMessage>, SessionError> {
        let mut stmt = conn.prepare(
            "SELECT id, role, content, tool_calls, tool_call_id, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let role: String = row.get(1)?;
            let tool_calls: Option<String> = row.get(3)?;
            let parsed_tool_calls = tool_calls
                .as_ref()
                .map(|s| serde_json::from_str(s))
                .transpose()
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            Ok(StoredMessage {
                id: row.get(0)?,
                role: str_to_role(&role),
                content: MessageContent::Text(row.get(2)?),
                tool_calls: parsed_tool_calls,
                tool_call_id: row.get(4)?,
                created_at: parse_ts(&row.get::<_, String>(5)?).unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(SessionError::from)
    }
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn str_to_role(role: &str) -> Role {
    match role {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

fn parse_ts(value: &str) -> Result<DateTime<Utc>, SessionError> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| SessionError::Database(rusqlite::Error::InvalidParameterName(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Session;

    #[test]
    fn roundtrip_session() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.db");
        let store = SqliteSessionStore::open(&db).unwrap();

        let mut session = Session::new(PathBuf::from("/tmp/project"));
        session.push_user("hello");
        session.push_assistant("hi".into(), None);

        store.save(&session).unwrap();
        let loaded = store.load(&session.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].text(), Some("hello"));
    }

    #[test]
    fn list_does_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteSessionStore::open(&dir.path().join("test.db")).unwrap();

        let mut session = Session::new(PathBuf::from("/tmp/project"));
        session.push_user("hello");
        store.save(&session).unwrap();

        let sessions = store.list(10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session.id);
    }
}

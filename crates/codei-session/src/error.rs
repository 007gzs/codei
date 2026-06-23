use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session store lock poisoned")]
    LockPoisoned,
}

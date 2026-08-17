//! 统一错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Database(String),
    #[error("driver '{0}' not found")]
    DriverNotFound(String),
    #[error("connection '{0}' not found")]
    ConnectionNotFound(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("query cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// 序列化为 `{"message": "..."}`，供 Tauri 命令作为错误类型返回。
impl serde::Serialize for DbError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("message", &self.to_string())?;
        map.end()
    }
}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> Self {
        DbError::Other(e.to_string())
    }
}

impl From<serde_json::Error> for DbError {
    fn from(e: serde_json::Error) -> Self {
        DbError::Config(e.to_string())
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Storage(e.to_string())
    }
}

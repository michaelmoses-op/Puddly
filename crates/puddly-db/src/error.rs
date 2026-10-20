use thiserror::Error;

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Message(String),
    #[error("connection error: {0}")]
    Connect(String),
    #[error("transaction error: {0}")]
    Transaction(String),
    #[error("unsupported value conversion: {0}")]
    Value(String),
}

impl DbError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

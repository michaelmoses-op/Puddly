use thiserror::Error;

pub type ServerResult<T> = Result<T, ServerError>;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("{0}")]
    Message(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad request: {0}")]
    BadRequest(String),
}

impl ServerError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

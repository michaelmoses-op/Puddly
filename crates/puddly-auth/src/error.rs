use thiserror::Error;

pub type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    #[error("{0}")]
    Message(String),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("email already registered")]
    EmailTaken,
    #[error("session expired or missing")]
    SessionInvalid,
    #[error("password hashing failed")]
    PasswordHash,
    #[error("access denied")]
    AccessDenied,
}

impl AuthError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

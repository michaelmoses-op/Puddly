use puddly_actions::ActionError;
use puddly_auth::AuthError;
use puddly_db::DbError;
use thiserror::Error;

pub type RpcResult<T> = Result<T, RpcError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RpcError {
    #[error("{0}")]
    Message(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    Csrf(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("method not allowed for this target")]
    MethodNotAllowed,
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error("database: {0}")]
    Db(String),
}

impl RpcError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::BadRequest(_) | Self::Csrf(_) => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound(_) => 404,
            Self::MethodNotAllowed => 405,
            Self::Auth(AuthError::InvalidCredentials) => 401,
            Self::Auth(AuthError::AccessDenied) => 403,
            Self::Auth(AuthError::EmailTaken) => 409,
            _ => 500,
        }
    }
}

impl From<DbError> for RpcError {
    fn from(e: DbError) -> Self {
        Self::Db(e.to_string())
    }
}

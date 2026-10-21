use crate::error_codes::IpcErrorCode;
use thiserror::Error;

pub type ActionResult<T> = Result<T, ActionError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ActionError {
    #[error("{0}")]
    Message(String),
    #[error("action file missing: {0}")]
    MissingFile(String),
    #[error("action `{resource}.{action}` source loaded from `{path}` but no executable runner is configured")]
    NotExecutable {
        resource: String,
        action: String,
        path: String,
    },
    /// Wire failure with a frozen [`IpcErrorCode`] — nested `ctx.db` (ADR-046)
    /// or sidecar run response (ADR-049).
    #[error("{code}: {message}")]
    Db {
        code: IpcErrorCode,
        message: String,
    },
}

impl ActionError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    pub fn db(code: IpcErrorCode, message: impl Into<String>) -> Self {
        Self::Db {
            code,
            message: message.into(),
        }
    }

    /// Frozen wire code when present (sidecar / `ctx.db`); else `None`.
    pub fn ipc_code(&self) -> Option<IpcErrorCode> {
        match self {
            Self::Db { code, .. } => Some(*code),
            _ => None,
        }
    }
}

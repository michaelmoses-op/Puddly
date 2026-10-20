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
}

impl ActionError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

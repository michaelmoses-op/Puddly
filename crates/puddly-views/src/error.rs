//! View / island compile and SSR errors.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ViewError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },

    #[error("unsupported view dialect in {}: {message}", path.display())]
    Dialect { path: PathBuf, message: String },

    #[error("unknown view `{name}`")]
    UnknownView { name: String },

    #[error("unknown island `{name}`")]
    UnknownIsland { name: String },

    #[error("render: {0}")]
    Render(String),

    #[error("{0}")]
    Msg(String),
}

pub type ViewResult<T> = Result<T, ViewError>;

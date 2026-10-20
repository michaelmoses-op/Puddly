use thiserror::Error;

pub type DataResult<T> = Result<T, DataError>;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DataError {
    #[error("{0}")]
    Message(String),
    #[error("field `{0}` is not filterable")]
    NotFilterable(String),
    #[error("field `{0}` is not sortable")]
    NotSortable(String),
    #[error("unknown field `{0}`")]
    UnknownField(String),
    #[error("operator `{0}` is not in the closed vocabulary")]
    UnknownOperator(String),
    #[error("page size {0} exceeds max {1}")]
    PageTooLarge(u32, u32),
}

impl DataError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

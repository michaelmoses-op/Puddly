use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Parameter / cell value crossing the executor boundary (no reflection).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SqlValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// UUID rendered as text for driver portability in Step 4.
    Uuid(String),
}

impl SqlValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::Uuid(s) => Some(s),
            _ => None,
        }
    }
}

/// One result row: column name → value (BTreeMap for deterministic iteration).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DbRow {
    pub columns: BTreeMap<String, SqlValue>,
}

impl DbRow {
    pub fn new() -> Self {
        Self {
            columns: BTreeMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&SqlValue> {
        self.columns.get(name)
    }

    pub fn insert(&mut self, name: impl Into<String>, value: SqlValue) {
        self.columns.insert(name.into(), value);
    }
}

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Desired database schema — pure function of app IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaIr {
    pub version: u32,
    pub dialect: String,
    /// Sorted by table name.
    pub tables: Vec<TableIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableIr {
    pub name: String,
    pub columns: Vec<ColumnIr>,
    pub primary_key: String,
    pub foreign_keys: Vec<ForeignKeyIr>,
    pub indexes: Vec<IndexIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnIr {
    pub name: String,
    pub sql_type: String,
    pub not_null: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_sql: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyIr {
    pub column: String,
    pub references_table: String,
    pub references_column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexIr {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

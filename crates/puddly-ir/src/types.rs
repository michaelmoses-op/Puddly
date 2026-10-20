use crate::span::{SourceFile, Span};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Fully validated application IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIr {
    /// Resources sorted by name for determinism.
    pub resources: Vec<ResourceIr>,
    /// Source files sorted by path string.
    pub files: Vec<SourceFile>,
}

/// One resource declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceIr {
    pub name: String,
    pub export_name: String,
    pub path: PathBuf,
    pub span: Span,
    /// Fields in declaration order.
    pub fields: Vec<FieldIr>,
    pub constraints: Vec<ConstraintIr>,
    pub access: AccessIr,
    pub actions: Vec<ActionIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldIr {
    pub name: String,
    pub span: Span,
    pub kind: FieldKind,
    pub options: FieldOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    Id,
    Text,
    Bool,
    Date,
    Timestamp,
    Ref { target: String },
    /// Computed fields store a source snippet; expression subset is open (see docs §16).
    Computed { source: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldOptions {
    pub optional: Option<bool>,
    pub default: Option<LiteralValue>,
    pub max: Option<i64>,
    pub filterable: Option<bool>,
    pub sortable: Option<bool>,
    pub auto: Option<bool>,
    pub renamed_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum LiteralValue {
    Bool(bool),
    Int(i64),
    Float(String),
    String(String),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConstraintIr {
    Limit {
        span: Span,
        per: String,
        where_fields: Vec<(String, LiteralValue)>,
        max: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessIr {
    pub span: Span,
    /// Operations sorted by name.
    pub operations: Vec<AccessOpIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessOpIr {
    pub name: String,
    pub span: Span,
    pub rule: RuleIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleIr {
    Authenticated { span: Span },
    Owner { span: Span, field: String },
    Role { span: Span, role: String },
    Member { span: Span, field: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionIr {
    pub name: String,
    pub span: Span,
    pub input: Vec<FieldIr>,
    pub run: String,
    pub run_span: Span,
    /// Optional declarative access (ADR-025). When `None`, RPC requires any authenticated user.
    pub access: Option<RuleIr>,
}

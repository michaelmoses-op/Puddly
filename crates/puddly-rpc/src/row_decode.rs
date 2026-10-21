//! Soft-plug typed row decode into CRUD/RPC boundaries (ADR-044 / Step 26).
//!
//! Mirrors generated `*Row::from_db_row` rules from [`crate::rs_rows`] without
//! compiling app `.puddly/generated/rows.rs` into the framework. Executor still
//! returns [`DbRow`] maps (ADR-042).

use puddly_db::{DbRow, SqlValue};
use puddly_ir::{FieldIr, FieldKind, ResourceIr};
use serde_json::{json, Map, Value};
use thiserror::Error;

/// Decode failure when a column is missing or the wrong [`SqlValue`] variant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RowDecodeError {
    #[error("missing column `{0}`")]
    Missing(String),
    #[error("wrong type for column `{0}`")]
    Type(String),
}

fn field_is_optional(f: &FieldIr) -> bool {
    f.options.optional == Some(true) || f.options.auto == Some(true)
}

fn require_string(row: &DbRow, col: &str) -> Result<String, RowDecodeError> {
    match row.get(col) {
        Some(SqlValue::Text(s) | SqlValue::Uuid(s)) => Ok(s.clone()),
        Some(SqlValue::Null) | None => Err(RowDecodeError::Missing(col.into())),
        Some(_) => Err(RowDecodeError::Type(col.into())),
    }
}

fn optional_string(row: &DbRow, col: &str) -> Result<Option<String>, RowDecodeError> {
    match row.get(col) {
        None | Some(SqlValue::Null) => Ok(None),
        Some(SqlValue::Text(s) | SqlValue::Uuid(s)) => Ok(Some(s.clone())),
        Some(_) => Err(RowDecodeError::Type(col.into())),
    }
}

fn require_bool(row: &DbRow, col: &str) -> Result<bool, RowDecodeError> {
    match row.get(col) {
        Some(SqlValue::Bool(b)) => Ok(*b),
        Some(SqlValue::Null) | None => Err(RowDecodeError::Missing(col.into())),
        Some(_) => Err(RowDecodeError::Type(col.into())),
    }
}

fn optional_bool(row: &DbRow, col: &str) -> Result<Option<bool>, RowDecodeError> {
    match row.get(col) {
        None | Some(SqlValue::Null) => Ok(None),
        Some(SqlValue::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(RowDecodeError::Type(col.into())),
    }
}

/// Decode one [`DbRow`] using `resource` field IR → JSON object keyed by IR names.
///
/// Skips `Computed` fields. Extra DB columns not in IR are dropped. Type / missing
/// required columns → [`RowDecodeError`] (schema drift).
pub fn decode_row_to_json(resource: &ResourceIr, row: &DbRow) -> Result<Value, RowDecodeError> {
    let mut map = Map::new();
    for f in &resource.fields {
        if matches!(f.kind, FieldKind::Computed { .. }) {
            continue;
        }
        let optional = field_is_optional(f);
        let col = f.name.as_str();
        let value = match (&f.kind, optional) {
            (FieldKind::Bool, false) => Value::Bool(require_bool(row, col)?),
            (FieldKind::Bool, true) => match optional_bool(row, col)? {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            },
            (_, false) => Value::String(require_string(row, col)?),
            (_, true) => match optional_string(row, col)? {
                Some(s) => Value::String(s),
                None => Value::Null,
            },
        };
        map.insert(f.name.clone(), value);
    }
    Ok(Value::Object(map))
}

/// Decode many rows; fails on the first bad row.
pub fn decode_rows_to_json_vec(
    resource: &ResourceIr,
    rows: &[DbRow],
) -> Result<Vec<Value>, RowDecodeError> {
    rows.iter()
        .map(|r| decode_row_to_json(resource, r))
        .collect()
}

/// Lenient dump of all columns (tests / diagnostics). Prefer [`decode_row_to_json`].
pub fn row_to_json_raw(row: &DbRow) -> Value {
    let mut map = Map::new();
    for (k, v) in &row.columns {
        map.insert(k.clone(), sql_to_json(v));
    }
    Value::Object(map)
}

fn sql_to_json(v: &SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(b) => Value::Bool(*b),
        SqlValue::Int(i) => json!(i),
        SqlValue::Float(f) => json!(f),
        SqlValue::Text(s) | SqlValue::Uuid(s) => Value::String(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use puddly_ir::{AccessIr, FieldIr, FieldOptions, ResourceIr, Span};
    use std::path::PathBuf;

    fn field(name: &str, kind: FieldKind, optional: bool) -> FieldIr {
        FieldIr {
            name: name.into(),
            span: Span::dummy(),
            kind,
            options: FieldOptions {
                optional: optional.then_some(true),
                ..FieldOptions::default()
            },
        }
    }

    fn todo_like() -> ResourceIr {
        ResourceIr {
            name: "todo".into(),
            export_name: "Todo".into(),
            path: PathBuf::from("todo.resource.ts"),
            span: Span::dummy(),
            fields: vec![
                field("id", FieldKind::Id, false),
                field("title", FieldKind::Text, false),
                field("done", FieldKind::Bool, false),
                field("dueDate", FieldKind::Date, true),
            ],
            constraints: vec![],
            access: AccessIr {
                span: Span::dummy(),
                operations: vec![],
            },
            actions: vec![],
        }
    }

    #[test]
    fn decode_happy_path() {
        let mut row = DbRow::new();
        row.insert("id", SqlValue::Uuid("a".into()));
        row.insert("title", SqlValue::Text("hi".into()));
        row.insert("done", SqlValue::Bool(false));
        row.insert("dueDate", SqlValue::Null);
        row.insert("extra", SqlValue::Text("dropped".into()));
        let v = decode_row_to_json(&todo_like(), &row).unwrap();
        assert_eq!(v["id"], "a");
        assert_eq!(v["title"], "hi");
        assert_eq!(v["done"], false);
        assert!(v["dueDate"].is_null());
        assert!(v.get("extra").is_none());
    }

    #[test]
    fn decode_type_mismatch() {
        let mut row = DbRow::new();
        row.insert("id", SqlValue::Uuid("a".into()));
        row.insert("title", SqlValue::Text("hi".into()));
        row.insert("done", SqlValue::Text("nope".into()));
        let err = decode_row_to_json(&todo_like(), &row).unwrap_err();
        assert!(matches!(err, RowDecodeError::Type(_)));
    }

    #[test]
    fn decode_missing_required() {
        let mut row = DbRow::new();
        row.insert("id", SqlValue::Uuid("a".into()));
        row.insert("done", SqlValue::Bool(true));
        let err = decode_row_to_json(&todo_like(), &row).unwrap_err();
        assert!(matches!(err, RowDecodeError::Missing(_)));
    }
}

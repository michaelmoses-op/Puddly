//! Postgres-oriented schema IR — pure function of [`AppIr`] (Step 3).
//!
//! Computed fields are omitted (opaque / not stored). `renamedFrom` is preserved
//! on columns for migration rename emission.

mod types;

use puddly_ir::{AppIr, FieldIr, FieldKind, LiteralValue, ResourceIr};
use sha2::{Digest, Sha256};

pub use types::*;

/// Build a deterministic [`SchemaIr`] from a validated app.
pub fn schema_from_app(ir: &AppIr) -> SchemaIr {
    let mut tables: Vec<TableIr> = ir.resources.iter().map(table_from_resource).collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    // Attach FKs after all tables exist (validation already ensured refs resolve).
    let names: std::collections::BTreeSet<String> =
        tables.iter().map(|t| t.name.clone()).collect();
    for table in &mut tables {
        let resource = ir
            .resources
            .iter()
            .find(|r| r.name == table.name)
            .expect("table matches resource");
        for field in &resource.fields {
            if let FieldKind::Ref { target } = &field.kind {
                if names.contains(target) {
                    table.foreign_keys.push(ForeignKeyIr {
                        column: field.name.clone(),
                        references_table: target.clone(),
                        references_column: "id".into(),
                    });
                }
            }
        }
        table.foreign_keys.sort_by(|a, b| {
            (&a.column, &a.references_table).cmp(&(&b.column, &b.references_table))
        });
    }

    SchemaIr {
        version: SCHEMA_VERSION,
        dialect: "postgres".into(),
        tables,
    }
}

fn table_from_resource(r: &ResourceIr) -> TableIr {
    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut primary_key = None;

    for f in &r.fields {
        if matches!(f.kind, FieldKind::Computed { .. }) {
            continue; // opaque — not persisted
        }
        let col = column_from_field(f);
        if matches!(f.kind, FieldKind::Id) {
            primary_key = Some(f.name.clone());
        }
        if f.options.filterable == Some(true) || f.options.sortable == Some(true) {
            indexes.push(IndexIr {
                name: format!("idx_{}_{}", r.name, f.name),
                columns: vec![f.name.clone()],
                unique: false,
            });
        }
        columns.push(col);
    }

    indexes.sort_by(|a, b| a.name.cmp(&b.name));

    TableIr {
        name: r.name.clone(),
        columns,
        primary_key: primary_key.unwrap_or_else(|| "id".into()),
        foreign_keys: Vec::new(),
        indexes,
    }
}

fn column_from_field(f: &FieldIr) -> ColumnIr {
    let (sql_type, default_sql) = match &f.kind {
        FieldKind::Id => ("UUID".into(), Some("gen_random_uuid()".into())),
        FieldKind::Text => {
            let ty = match f.options.max {
                Some(max) if max > 0 => format!("VARCHAR({max})"),
                _ => "TEXT".into(),
            };
            (ty, literal_default(&f.options.default))
        }
        FieldKind::Bool => ("BOOLEAN".into(), literal_default(&f.options.default)),
        FieldKind::Date => ("DATE".into(), literal_default(&f.options.default)),
        FieldKind::Timestamp => {
            let def = if f.options.auto == Some(true) {
                Some("now()".into())
            } else {
                literal_default(&f.options.default)
            };
            ("TIMESTAMPTZ".into(), def)
        }
        FieldKind::Ref { .. } => ("UUID".into(), None),
        FieldKind::Computed { .. } => unreachable!("computed skipped"),
    };

    let not_null = match &f.kind {
        FieldKind::Id => true,
        _ => f.options.optional != Some(true),
    };

    ColumnIr {
        name: f.name.clone(),
        sql_type,
        not_null,
        default_sql,
        renamed_from: f.options.renamed_from.clone(),
    }
}

fn literal_default(v: &Option<LiteralValue>) -> Option<String> {
    match v {
        None => None,
        Some(LiteralValue::Bool(b)) => Some(if *b { "true" } else { "false" }.into()),
        Some(LiteralValue::Int(i)) => Some(i.to_string()),
        Some(LiteralValue::Float(s)) => Some(s.clone()),
        Some(LiteralValue::String(s)) => Some(format!("'{}'", s.replace('\'', "''"))),
        Some(LiteralValue::Null) => Some("NULL".into()),
    }
}

/// Canonical JSON for hashing / drift (byte-stable).
pub fn canonical_schema_json(schema: &SchemaIr) -> String {
    let mut s = serde_json::to_string(schema).expect("SchemaIr serializes");
    // serde field order is stable; compact form for hashing
    s.push('\n');
    s
}

/// Hex SHA-256 of the canonical schema JSON.
pub fn schema_fingerprint(schema: &SchemaIr) -> String {
    let json = canonical_schema_json(schema);
    let digest = Sha256::digest(json.as_bytes());
    hex::encode(digest)
}

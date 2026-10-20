//! Generate committed SQL migrations and detect schema drift (ADR-013).
//!
//! Each migration embeds:
//! - `-- puddly-schema-fingerprint: <sha256>`
//! - `-- puddly-schema-json: <canonical SchemaIr>` for the next diff baseline

mod diff;
mod render;
mod store;

use puddly_core::{CLI_NAME, DISPLAY_NAME, MIGRATIONS_DIR};
use puddly_ir::AppIr;
use puddly_schema::{schema_fingerprint, schema_from_app, SchemaIr, SCHEMA_VERSION};
use std::path::{Path, PathBuf};

pub use diff::{diff_schemas, DestructiveChange, SchemaDiff};
pub use render::render_schema_sql;
pub use store::{
    list_migrations, migrations_dir, parse_fingerprint, read_latest_fingerprint, MigrationFile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftStatus {
    Missing,
    Fresh,
    Drift { expected: String, found: String },
}

#[derive(Debug, Clone)]
pub struct GenerateResult {
    pub path: Option<PathBuf>,
    pub message: String,
    pub schema: SchemaIr,
    pub fingerprint: String,
    pub warnings: Vec<String>,
}

pub fn desired_schema(ir: &AppIr) -> (SchemaIr, String) {
    let schema = schema_from_app(ir);
    let fp = schema_fingerprint(&schema);
    (schema, fp)
}

pub fn drift_status(root: &Path, ir: &AppIr) -> DriftStatus {
    let (_schema, expected) = desired_schema(ir);
    match read_latest_fingerprint(root) {
        Ok(None) => DriftStatus::Missing,
        Ok(Some(found)) => {
            if found == expected {
                DriftStatus::Fresh
            } else {
                DriftStatus::Drift { expected, found }
            }
        }
        Err(_) => DriftStatus::Missing,
    }
}

pub fn drift_message(status: &DriftStatus) -> String {
    match status {
        DriftStatus::Fresh => format!("{DISPLAY_NAME} migrate: fresh"),
        DriftStatus::Missing => format!(
            "{DISPLAY_NAME} migrate: missing — run `{CLI_NAME} migrate generate` and commit `{MIGRATIONS_DIR}/`"
        ),
        DriftStatus::Drift { .. } => format!(
            "{DISPLAY_NAME} migrate: drift — run `{CLI_NAME} migrate generate` and commit the new SQL"
        ),
    }
}

/// Generate the next migration (or initial `0001_init.sql`).
///
/// Destructive drops require `allow_destructive = true` (ADR-013).
pub fn generate_migration(
    root: &Path,
    ir: &AppIr,
    allow_destructive: bool,
) -> std::io::Result<GenerateResult> {
    let (schema, fingerprint) = desired_schema(ir);
    let existing = list_migrations(root)?;

    if existing.is_empty() {
        let body = render_schema_sql(&schema);
        let sql = format_migration_file("0001_init", &fingerprint, &body, &[], &schema);
        let path = write_migration(root, "0001_init.sql", &sql)?;
        return Ok(GenerateResult {
            path: Some(path),
            message: format!("wrote initial migration ({MIGRATIONS_DIR}/0001_init.sql)"),
            schema,
            fingerprint,
            warnings: Vec::new(),
        });
    }

    let latest_fp = read_latest_fingerprint(root)?.unwrap_or_default();
    if latest_fp == fingerprint {
        return Ok(GenerateResult {
            path: None,
            message: "migrations already up to date".into(),
            schema,
            fingerprint,
            warnings: Vec::new(),
        });
    }

    let before = store::read_latest_schema_json(root)?.unwrap_or_else(|| SchemaIr {
        version: SCHEMA_VERSION,
        dialect: "postgres".into(),
        tables: Vec::new(),
    });

    let diff = diff_schemas(&before, &schema);
    let mut warnings: Vec<String> = diff
        .destructive
        .iter()
        .map(|d| format!("destructive: {} — {}", d.kind, d.target))
        .collect();

    if !diff.destructive.is_empty() && !allow_destructive {
        return Ok(GenerateResult {
            path: None,
            message: format!(
                "refusing to generate: {} destructive change(s); re-run with --allow-destructive",
                diff.destructive.len()
            ),
            schema,
            fingerprint,
            warnings,
        });
    }

    if diff.is_empty() {
        warnings.push("schema fingerprint changed without structural SQL diff".into());
    }

    let next_n = existing.len() + 1;
    let name = format!("{next_n:04}_sync");
    let body = render::render_diff_sql(&diff, allow_destructive);
    let sql = format_migration_file(&name, &fingerprint, &body, &warnings, &schema);
    let filename = format!("{name}.sql");
    let path = write_migration(root, &filename, &sql)?;

    Ok(GenerateResult {
        path: Some(path),
        message: format!("wrote {MIGRATIONS_DIR}/{filename}"),
        schema,
        fingerprint,
        warnings,
    })
}

fn format_migration_file(
    name: &str,
    fingerprint: &str,
    body: &str,
    warnings: &[String],
    schema: &SchemaIr,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("-- puddly migration {name}\n"));
    out.push_str(&format!("-- puddly-schema-fingerprint: {fingerprint}\n"));
    out.push_str(&format!(
        "-- Generated by {DISPLAY_NAME}; review before applying.\n"
    ));
    for w in warnings {
        out.push_str(&format!("-- warning: {w}\n"));
    }
    out.push('\n');
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    let json = serde_json::to_string(schema).expect("schema json");
    out.push_str("-- puddly-schema-json: ");
    out.push_str(&json);
    out.push('\n');
    out
}

fn write_migration(root: &Path, filename: &str, sql: &str) -> std::io::Result<PathBuf> {
    let dir = migrations_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    std::fs::write(&path, sql)?;
    Ok(path)
}

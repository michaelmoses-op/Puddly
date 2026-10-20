use puddly_core::MIGRATIONS_DIR;
use puddly_schema::SchemaIr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct MigrationFile {
    pub name: String,
    pub path: PathBuf,
}

pub fn migrations_dir(root: &Path) -> PathBuf {
    root.join(MIGRATIONS_DIR)
}

/// Sorted list of `*.sql` migration files.
pub fn list_migrations(root: &Path) -> io::Result<Vec<MigrationFile>> {
    let dir = migrations_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sql") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        files.push(MigrationFile { name, path });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

pub fn parse_fingerprint(sql: &str) -> Option<String> {
    for line in sql.lines() {
        if let Some(rest) = line.strip_prefix("-- puddly-schema-fingerprint: ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

pub fn read_latest_fingerprint(root: &Path) -> io::Result<Option<String>> {
    let files = list_migrations(root)?;
    // Auth scaffold migrations (e.g. `0002_puddly_auth.sql`) have no schema fingerprint;
    // use the latest resource migration that does.
    for file in files.iter().rev() {
        let sql = fs::read_to_string(&file.path)?;
        if let Some(fp) = parse_fingerprint(&sql) {
            return Ok(Some(fp));
        }
    }
    Ok(None)
}

pub fn read_schema_json_from_file(path: &Path) -> io::Result<Option<SchemaIr>> {
    let sql = fs::read_to_string(path)?;
    Ok(parse_schema_json(&sql))
}

pub fn parse_schema_json(sql: &str) -> Option<SchemaIr> {
    for line in sql.lines() {
        if let Some(rest) = line.strip_prefix("-- puddly-schema-json: ") {
            return serde_json::from_str(rest.trim()).ok();
        }
    }
    None
}

pub fn read_latest_schema_json(root: &Path) -> io::Result<Option<SchemaIr>> {
    let files = list_migrations(root)?;
    for file in files.iter().rev() {
        if let Some(schema) = read_schema_json_from_file(&file.path)? {
            return Ok(Some(schema));
        }
    }
    Ok(None)
}

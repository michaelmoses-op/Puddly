//! Deterministic `manifest.json` generation (ADR-005 / ADR-017).
//!
//! The manifest is a pure function of [`AppIr`]: same IR → byte-identical JSON.

mod init;
mod types;

pub use init::{init_app, InitError, InitResult};

use puddly_core::{CLI_NAME, DISPLAY_NAME, MANIFEST_DIR, MANIFEST_FILE};
use puddly_ir::{
    AccessIr, ActionIr, AppIr, ConstraintIr, FieldIr, FieldKind, LiteralValue, ResourceIr, RuleIr,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use types::*;

/// Build a deterministic [`Manifest`] from validated IR.
///
/// Paths in the manifest are relative to `root` when possible.
pub fn build_manifest(root: &Path, ir: &AppIr) -> Manifest {
    let mut resources: Vec<ManifestResource> = ir
        .resources
        .iter()
        .map(|r| resource_to_manifest(root, r))
        .collect();
    resources.sort_by(|a, b| a.name.cmp(&b.name));

    // referencedBy derived after all resources known
    let mut inbound: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for r in &resources {
        for target in &r.refs {
            inbound
                .entry(target.clone())
                .or_default()
                .push(r.name.clone());
        }
    }
    for r in &mut resources {
        if let Some(mut refs) = inbound.remove(&r.name) {
            refs.sort();
            refs.dedup();
            r.referenced_by = refs;
        }
    }

    Manifest {
        version: MANIFEST_VERSION,
        framework: DISPLAY_NAME.to_string(),
        resources,
    }
}

fn resource_to_manifest(root: &Path, r: &ResourceIr) -> ManifestResource {
    let mut refs: Vec<String> = r
        .fields
        .iter()
        .filter_map(|f| match &f.kind {
            FieldKind::Ref { target } => Some(target.clone()),
            _ => None,
        })
        .collect();
    refs.sort();
    refs.dedup();

    ManifestResource {
        name: r.name.clone(),
        export_name: r.export_name.clone(),
        path: relativize(root, &r.path),
        fields: r.fields.iter().map(field_to_manifest).collect(),
        constraints: r.constraints.iter().map(constraint_to_manifest).collect(),
        access: access_to_manifest(&r.access),
        actions: r.actions.iter().map(action_to_manifest).collect(),
        refs,
        referenced_by: Vec::new(),
    }
}

fn field_to_manifest(f: &FieldIr) -> ManifestField {
    let (kind, target, computed) = match &f.kind {
        FieldKind::Id => ("id".into(), None, None),
        FieldKind::Text => ("text".into(), None, None),
        FieldKind::Bool => ("bool".into(), None, None),
        FieldKind::Date => ("date".into(), None, None),
        FieldKind::Timestamp => ("timestamp".into(), None, None),
        FieldKind::Ref { target } => ("ref".into(), Some(target.clone()), None),
        FieldKind::Computed { source } => {
            // Opaque by owner decision — do not expand expression AST.
            ("computed".into(), None, Some(ComputedMeta {
                opaque: true,
                source_shape: source.clone(),
            }))
        }
    };
    ManifestField {
        name: f.name.clone(),
        kind,
        target,
        computed,
        optional: f.options.optional,
        default: f.options.default.as_ref().map(literal_json),
        max: f.options.max,
        filterable: f.options.filterable,
        sortable: f.options.sortable,
        auto: f.options.auto,
        renamed_from: f.options.renamed_from.clone(),
    }
}

fn constraint_to_manifest(c: &ConstraintIr) -> ManifestConstraint {
    match c {
        ConstraintIr::Limit {
            per,
            where_fields,
            max,
            ..
        } => {
            let mut where_map = Vec::new();
            for (k, v) in where_fields {
                where_map.push(ManifestWhereEntry {
                    field: k.clone(),
                    value: literal_json(v),
                });
            }
            where_map.sort_by(|a, b| a.field.cmp(&b.field));
            ManifestConstraint {
                kind: "limit".into(),
                per: Some(per.clone()),
                where_fields: where_map,
                max: Some(*max),
            }
        }
    }
}

fn access_to_manifest(a: &AccessIr) -> Vec<ManifestAccessOp> {
    let mut ops: Vec<_> = a
        .operations
        .iter()
        .map(|op| ManifestAccessOp {
            name: op.name.clone(),
            rule: rule_to_manifest(&op.rule),
        })
        .collect();
    ops.sort_by(|a, b| a.name.cmp(&b.name));
    ops
}

fn rule_to_manifest(rule: &RuleIr) -> ManifestRule {
    match rule {
        RuleIr::Authenticated { .. } => ManifestRule {
            kind: "authenticated".into(),
            field: None,
            role: None,
        },
        RuleIr::Owner { field, .. } => ManifestRule {
            kind: "owner".into(),
            field: Some(field.clone()),
            role: None,
        },
        RuleIr::Role { role, .. } => ManifestRule {
            kind: "role".into(),
            field: None,
            role: Some(role.clone()),
        },
        RuleIr::Member { field, .. } => ManifestRule {
            kind: "member".into(),
            field: Some(field.clone()),
            role: None,
        },
    }
}

fn action_to_manifest(a: &ActionIr) -> ManifestAction {
    ManifestAction {
        name: a.name.clone(),
        run: a.run.clone(),
        input: a.input.iter().map(field_to_manifest).collect(),
        access: a.access.as_ref().map(rule_to_manifest),
    }
}

fn literal_json(v: &LiteralValue) -> serde_json::Value {
    match v {
        LiteralValue::Bool(b) => serde_json::Value::Bool(*b),
        LiteralValue::Int(i) => serde_json::json!(*i),
        LiteralValue::Float(s) => serde_json::Value::String(s.clone()),
        LiteralValue::String(s) => serde_json::Value::String(s.clone()),
        LiteralValue::Null => serde_json::Value::Null,
    }
}

fn relativize(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Absolute path to the committed manifest file.
pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_DIR).join(MANIFEST_FILE)
}

/// Serialize with stable pretty JSON + trailing newline (byte-deterministic).
pub fn render_manifest_json(manifest: &Manifest) -> String {
    let mut s = serde_json::to_string_pretty(manifest).expect("Manifest serializes");
    s.push('\n');
    s
}

/// Write `.puddly/manifest.json` under `root`.
pub fn write_manifest(root: &Path, manifest: &Manifest) -> io::Result<PathBuf> {
    let dir = root.join(MANIFEST_DIR);
    fs::create_dir_all(&dir)?;
    let path = dir.join(MANIFEST_FILE);
    fs::write(&path, render_manifest_json(manifest))?;
    Ok(path)
}

/// Read an existing manifest, if present.
pub fn read_manifest(root: &Path) -> io::Result<Option<Manifest>> {
    let path = manifest_path(root);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let m: Manifest = serde_json::from_str(&text).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {}: {e}", path.display()),
        )
    })?;
    Ok(Some(m))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleStatus {
    /// No committed manifest on disk.
    Missing,
    /// On-disk bytes differ from freshly generated output.
    Stale,
    /// Matches generated output exactly.
    Fresh,
}

/// Compare on-disk manifest to a freshly built one (byte-identical JSON).
pub fn stale_status(root: &Path, expected: &Manifest) -> StaleStatus {
    let path = manifest_path(root);
    let expected_json = render_manifest_json(expected);
    match fs::read_to_string(&path) {
        Ok(on_disk) => {
            if on_disk == expected_json {
                StaleStatus::Fresh
            } else {
                StaleStatus::Stale
            }
        }
        Err(_) => StaleStatus::Missing,
    }
}

/// Human message for CI / CLI.
pub fn stale_message(status: &StaleStatus) -> String {
    match status {
        StaleStatus::Fresh => format!("{DISPLAY_NAME} manifest: fresh"),
        StaleStatus::Missing => format!(
            "{DISPLAY_NAME} manifest: missing — run `{CLI_NAME} manifest generate` and commit `{MANIFEST_DIR}/{MANIFEST_FILE}`"
        ),
        StaleStatus::Stale => format!(
            "{DISPLAY_NAME} manifest: stale — run `{CLI_NAME} manifest generate` and commit the updated file"
        ),
    }
}

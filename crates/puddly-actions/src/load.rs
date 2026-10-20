use crate::error::{ActionError, ActionResult};
use puddly_ir::{ActionIr, AppIr, FieldIr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedAction {
    pub resource: String,
    pub name: String,
    pub run: String,
    pub path: PathBuf,
    pub source: String,
    pub input: Vec<FieldIr>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionCatalog {
    /// Keyed by `resource.action` for deterministic lookup.
    pub actions: BTreeMap<String, LoadedAction>,
}

impl ActionCatalog {
    pub fn get(&self, resource: &str, action: &str) -> Option<&LoadedAction> {
        self.actions.get(&format!("{resource}.{action}"))
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// Resolve `./foo.action.ts` relative to the resource file, then to `root` if needed.
pub fn resolve_action_path(root: &Path, resource_path: &Path, run: &str) -> PathBuf {
    let resource_abs = resource_abs_path(root, resource_path);
    let base = resource_abs
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    let candidate = base.join(run);
    if candidate.exists() {
        return candidate;
    }
    root.join(run.trim_start_matches("./"))
}

/// `discover_resource_files(root)` uses `read_dir`; when `root` is relative, paths are
/// cwd-relative and already prefixed with `root` (e.g. `examples/app/features/x.resource.ts`).
fn resource_abs_path(root: &Path, resource_path: &Path) -> PathBuf {
    if resource_path.is_absolute() {
        return resource_path.to_path_buf();
    }
    if let Ok(rest) = resource_path.strip_prefix(root) {
        return root.join(rest);
    }
    root.join(resource_path)
}

pub fn load_actions(root: &Path, ir: &AppIr) -> ActionResult<ActionCatalog> {
    let mut actions = BTreeMap::new();
    for resource in &ir.resources {
        for action in &resource.actions {
            let loaded = load_one(root, &resource.name, &resource.path, action)?;
            let key = format!("{}.{}", resource.name, action.name);
            actions.insert(key, loaded);
        }
    }
    Ok(ActionCatalog { actions })
}

fn load_one(
    root: &Path,
    resource_name: &str,
    resource_path: &Path,
    action: &ActionIr,
) -> ActionResult<LoadedAction> {
    let path = resolve_action_path(root, resource_path, &action.run);
    if !path.is_file() {
        return Err(ActionError::MissingFile(path.display().to_string()));
    }
    let source = fs::read_to_string(&path).map_err(|e| {
        ActionError::msg(format!("failed to read {}: {e}", path.display()))
    })?;
    Ok(LoadedAction {
        resource: resource_name.to_string(),
        name: action.name.clone(),
        run: action.run.clone(),
        path,
        source,
        input: action.input.clone(),
    })
}

//! Action-file loading for the RPC layer (ADR-002 / ADR-016).
//!
//! Step 6 loads declared `*.action.ts` sources into an [`ActionCatalog`].
//! Execution of TypeScript bodies is pluggable via [`ActionRunner`] — the default
//! is a stub until a JS runtime or codegen lands.

mod error;
mod load;

pub use error::{ActionError, ActionResult};
pub use load::{load_actions, ActionCatalog, LoadedAction};

use puddly_ir::AppIr;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Context passed into a native / pluggable action runner.
#[derive(Debug, Clone)]
pub struct ActionCall<'a> {
    pub resource: &'a str,
    pub action: &'a str,
    pub input: &'a Value,
    /// Absolute path of the loaded source file.
    pub source_path: &'a Path,
    /// Full TypeScript source (for runners that interpret or compile it).
    pub source: &'a str,
}

/// Pluggable executor for custom actions.
pub trait ActionRunner {
    fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value>;
}

/// Default runner: refuses to execute TS (skeleton). Catalog still proves loading works.
#[derive(Debug, Default)]
pub struct StubActionRunner;

impl ActionRunner for StubActionRunner {
    fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
        Err(ActionError::NotExecutable {
            resource: call.resource.to_string(),
            action: call.action.to_string(),
            path: call.source_path.display().to_string(),
        })
    }
}

/// In-memory native handlers keyed by `resource.action` (tests / built-ins).
#[derive(Default)]
pub struct NativeActionRunner {
    handlers: BTreeMap<String, Box<dyn FnMut(&Value) -> ActionResult<Value> + Send>>,
}

impl NativeActionRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F>(&mut self, resource: &str, action: &str, f: F)
    where
        F: FnMut(&Value) -> ActionResult<Value> + Send + 'static,
    {
        self.handlers
            .insert(format!("{resource}.{action}"), Box::new(f));
    }
}

impl ActionRunner for NativeActionRunner {
    fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
        let key = format!("{}.{}", call.resource, call.action);
        match self.handlers.get_mut(&key) {
            Some(h) => h(call.input),
            None => Err(ActionError::NotExecutable {
                resource: call.resource.to_string(),
                action: call.action.to_string(),
                path: call.source_path.display().to_string(),
            }),
        }
    }
}

/// Load the catalog for an app root + IR.
pub fn catalog_from_app(root: &Path, ir: &AppIr) -> ActionResult<ActionCatalog> {
    load_actions(root, ir)
}

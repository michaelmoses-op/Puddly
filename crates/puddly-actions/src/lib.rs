//! Action-file loading for the RPC layer (ADR-002 / ADR-016).
//!
//! Step 6 loads declared `*.action.ts` sources into an [`ActionCatalog`].
//! Step 9 executes them via the in-repo Node sidecar (`IpcActionRunner`) over
//! the JSONL protocol. Step 10 makes [`ActionRunner`] async (ADR-028).
//! Step 13: protocol v2 (`id`), long-lived worker, packaging B, frozen error codes
//! (ADR-031). Step 28: nested `ctx.db` via [`ActionDbHost`] (ADR-046).
//! Step 31: [`IpcErrorCode::http_status`] for RPC/REST (ADR-049).

mod db_host;
mod error;
mod error_codes;
mod ipc;
mod load;
mod package;

pub use db_host::{ActionDbHost, MockActionDbHost};
pub use error::{ActionError, ActionResult};
pub use error_codes::IpcErrorCode;
pub use ipc::{
    find_generated_sidecar, find_sidecar_workspace, IpcActionRunner, IpcDbRequest, IpcErrorBody,
    IpcId, IpcRequest, IpcResponse, IpcUser, ACTION_SIDECAR_ENV, ACTION_SIDECAR_ONESHOT_ENV,
    DEFAULT_SIDECAR_SCRIPT, GENERATED_SIDECAR_SCRIPT, IPC_PROTOCOL_VERSION,
};
pub use load::{load_actions, ActionCatalog, LoadedAction};
pub use package::package_action_sidecar;

use async_trait::async_trait;
use puddly_ir::AppIr;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Context passed into a native / pluggable action runner.
pub struct ActionCall<'a> {
    pub resource: &'a str,
    pub action: &'a str,
    pub input: &'a Value,
    /// Absolute path of the loaded source file.
    pub source_path: &'a Path,
    /// Full TypeScript source (for runners that interpret or compile it).
    pub source: &'a str,
    /// Authenticated user from RPC context (wired for IPC → sidecar `ctx.user`).
    pub user: Option<IpcUser>,
    /// Nested DB host for sidecar `ctx.db.*` (ADR-046). Native/stub runners ignore this.
    pub db_host: Option<&'a mut dyn ActionDbHost>,
}

/// Pluggable executor for custom actions (async; ADR-028).
#[async_trait]
pub trait ActionRunner: Send {
    async fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value>;
}

/// Default runner: refuses to execute TS (skeleton). Catalog still proves loading works.
#[derive(Debug, Default)]
pub struct StubActionRunner;

#[async_trait]
impl ActionRunner for StubActionRunner {
    async fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
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

#[async_trait]
impl ActionRunner for NativeActionRunner {
    async fn run(&mut self, call: ActionCall<'_>) -> ActionResult<Value> {
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

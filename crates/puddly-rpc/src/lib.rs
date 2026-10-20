//! RPC skeleton: `POST /rpc/<resource>.<operation>` (ADR-008).
//!
//! Step 6 provides library dispatch (no full HTTP server yet):
//! - CSRF custom-header check on mutations
//! - Auth resolver → [`RpcContext`]
//! - Access-rule enforcement on list/get/create/update/delete
//! - Built-in `auth.login` / `auth.register` / `auth.logout`
//! - Custom actions via loaded catalog + pluggable [`ActionRunner`]

mod access_gate;
mod auth_ops;
mod context;
mod crud;
mod csrf;
mod dispatch;
mod error;
mod list_body;
mod request;

pub use access_gate::{access_op_name, enforce, find_rule, require_rule, row_values_from_db};
pub use context::RpcContext;
pub use csrf::check_csrf;
pub use dispatch::RpcRuntime;
pub use error::{RpcError, RpcResult};
pub use request::{RpcErrorBody, RpcRequest, RpcResponse, RpcTarget};

use puddly_actions::{catalog_from_app, ActionCatalog, ActionRunner, StubActionRunner};
use puddly_auth::{MemorySessionStore, SessionStore};
use puddly_data::{statement_book_from_app, StatementBook};
use puddly_db::Executor;
use puddly_ir::AppIr;
use std::path::Path;

/// Convenience harness for tests / embedders with in-memory sessions + stub actions.
pub struct RpcServer<S: SessionStore = MemorySessionStore, R: ActionRunner = StubActionRunner> {
    pub ir: AppIr,
    pub book: StatementBook,
    pub actions: ActionCatalog,
    pub sessions: S,
    pub runner: R,
    pub secure_cookies: bool,
}

impl RpcServer<MemorySessionStore, StubActionRunner> {
    pub fn from_app(root: &Path, ir: AppIr) -> Result<Self, String> {
        let book = statement_book_from_app(&ir);
        let actions = catalog_from_app(root, &ir).map_err(|e| e.to_string())?;
        Ok(Self {
            ir,
            book,
            actions,
            sessions: MemorySessionStore::new(),
            runner: StubActionRunner,
            secure_cookies: false,
        })
    }
}

impl<S: SessionStore, R: ActionRunner> RpcServer<S, R> {
    pub fn handle(&mut self, req: &RpcRequest, exec: &mut dyn Executor) -> RpcResponse {
        let mut rt = RpcRuntime {
            ir: &self.ir,
            book: &self.book,
            actions: &self.actions,
            sessions: &mut self.sessions,
            runner: &mut self.runner,
            secure_cookies: self.secure_cookies,
        };
        rt.handle(req, exec)
    }
}

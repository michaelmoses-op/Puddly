//! RPC skeleton: `POST /rpc/<resource>.<operation>` (ADR-008).
//!
//! - CSRF double-submit (cookie `puddly_csrf` === header `X-Puddly-CSRF`) on mutations
//! - Auth resolver → [`RpcContext`]
//! - Access-rule enforcement on list/get/create/update/delete
//! - Built-in `auth.login` / `auth.register` / `auth.logout`
//! - Custom actions via loaded catalog + pluggable [`ActionRunner`]
//! - Async dispatch (ADR-028)
//! - Opt-in REST + OpenAPI (ADR-045 / Step 27)
//! - Action `ctx.db` nested CRUD host (ADR-046 / Step 28)
//! - IPC / action error codes → HTTP status (ADR-049 / Step 31)

mod access_gate;
mod action_db;
mod auth_ops;
mod context;
mod crud;
mod csrf;
mod dispatch;
mod error;
mod list_body;
mod openapi;
mod request;
mod rest;
mod row_decode;
mod rs_rows;
mod ts_client;

pub use access_gate::{
    access_op_name, enforce, enforce_member, find_rule, list_owner_field, member_join,
    require_rule, row_values_from_db, MemberJoin,
};
pub use action_db::CrudActionDbHost;
pub use context::RpcContext;
pub use csrf::check_csrf;
pub use dispatch::RpcRuntime;
pub use error::{RpcError, RpcResult};
pub use openapi::{generate_openapi, write_openapi, GENERATED_OPENAPI_JSON};
pub use request::{RpcErrorBody, RpcRequest, RpcResponse, RpcTarget};
pub use rest::{looks_like_uuid, map_rest_to_rpc, RestMapping, REST_API_PREFIX};
pub use row_decode::{
    decode_row_to_json, decode_rows_to_json_vec, row_to_json_raw, RowDecodeError,
};
pub use rs_rows::{generate_rs_rows, write_rs_rows, GENERATED_ROWS_RS};
pub use ts_client::{generate_ts_client, write_ts_client, GENERATED_CLIENT_TS};

use puddly_actions::{
    catalog_from_app, ActionCatalog, ActionRunner, IpcActionRunner, StubActionRunner,
};
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

impl RpcServer<MemorySessionStore, IpcActionRunner> {
    /// Load IR/actions and use the in-repo Node sidecar (or `$PUDDLY_ACTION_SIDECAR`).
    pub fn from_app_with_sidecar(root: &Path, ir: AppIr) -> Result<Self, String> {
        let book = statement_book_from_app(&ir);
        let actions = catalog_from_app(root, &ir).map_err(|e| e.to_string())?;
        Ok(Self {
            ir,
            book,
            actions,
            sessions: MemorySessionStore::new(),
            runner: IpcActionRunner::from_env_or_workspace(root),
            secure_cookies: false,
        })
    }
}

impl<S: SessionStore, R: ActionRunner> RpcServer<S, R> {
    pub async fn handle(&mut self, req: &RpcRequest, exec: &mut dyn Executor) -> RpcResponse {
        let mut rt = RpcRuntime {
            ir: &self.ir,
            book: &self.book,
            actions: &self.actions,
            sessions: &mut self.sessions,
            runner: &mut self.runner,
            secure_cookies: self.secure_cookies,
        };
        rt.handle(req, exec).await
    }

    /// Hot-reload IR, statement book, and action catalog (keeps sessions + runner).
    pub fn reload_catalog(&mut self, root: &Path, ir: AppIr) -> Result<(), String> {
        let book = statement_book_from_app(&ir);
        let actions = catalog_from_app(root, &ir).map_err(|e| e.to_string())?;
        self.ir = ir;
        self.book = book;
        self.actions = actions;
        Ok(())
    }

    /// Reload action sources only (same IR). Used when `*.action.ts` changes.
    pub fn reload_actions(&mut self, root: &Path) -> Result<(), String> {
        self.actions = catalog_from_app(root, &self.ir).map_err(|e| e.to_string())?;
        Ok(())
    }
}

//! Top-level RPC dispatch: CSRF → auth context → CRUD / auth / custom actions.

use crate::auth_ops::handle_auth_op;
use crate::context::RpcContext;
use crate::crud::handle_crud;
use crate::csrf::check_csrf;
use crate::error::{RpcError, RpcResult};
use crate::request::{RpcRequest, RpcResponse, RpcTarget};
use puddly_actions::{ActionCall, ActionCatalog, ActionRunner};
use puddly_auth::SessionStore;
use puddly_data::StatementBook;
use puddly_db::Executor;
use puddly_ir::AppIr;
/// Runtime knobs for one RPC server process / test harness.
pub struct RpcRuntime<'a, S: SessionStore, R: ActionRunner> {
    pub ir: &'a AppIr,
    pub book: &'a StatementBook,
    pub actions: &'a ActionCatalog,
    pub sessions: &'a mut S,
    pub runner: &'a mut R,
    /// When true, session cookies include `Secure`.
    pub secure_cookies: bool,
}

impl<'a, S: SessionStore, R: ActionRunner> RpcRuntime<'a, S, R> {
    pub fn handle(
        &mut self,
        req: &RpcRequest,
        exec: &mut dyn Executor,
    ) -> RpcResponse {
        match self.handle_inner(req, exec) {
            Ok(resp) => resp,
            Err(e) => map_err(e),
        }
    }

    fn handle_inner(
        &mut self,
        req: &RpcRequest,
        exec: &mut dyn Executor,
    ) -> RpcResult<RpcResponse> {
        let target = RpcTarget::parse(&req.target).map_err(RpcError::BadRequest)?;
        check_csrf(req, &target)?;

        if target.is_auth() {
            return handle_auth_op(
                &target.operation,
                &req.body,
                exec,
                self.sessions,
                self.secure_cookies,
            );
        }

        let ctx = RpcContext::from_sessions(self.sessions, req.cookie_header_value());

        let resource = self
            .ir
            .resources
            .iter()
            .find(|r| r.name == target.resource)
            .ok_or_else(|| RpcError::NotFound(format!("resource `{}`", target.resource)))?;

        // Built-in CRUD
        if matches!(
            target.operation.as_str(),
            "list" | "get" | "create" | "update" | "delete"
        ) {
            let data = handle_crud(
                resource,
                &target.operation,
                &req.body,
                &ctx,
                self.book,
                exec,
            )?;
            return Ok(RpcResponse::ok(data));
        }

        // Custom action declared on the resource
        let action_ir = resource
            .actions
            .iter()
            .find(|a| a.name == target.operation);
        if action_ir.is_some() {
            // Custom actions require authentication by default in the skeleton
            // (declarative per-action access is a later open question).
            if ctx.user.is_none() {
                return Err(RpcError::Unauthorized);
            }
            let loaded = self
                .actions
                .get(&target.resource, &target.operation)
                .ok_or_else(|| {
                    RpcError::NotFound(format!(
                        "action `{}.{}` not loaded",
                        target.resource, target.operation
                    ))
                })?;
            let data = self.runner.run(ActionCall {
                resource: &target.resource,
                action: &target.operation,
                input: &req.body,
                source_path: &loaded.path,
                source: &loaded.source,
            })?;
            return Ok(RpcResponse::ok(data));
        }

        Err(RpcError::NotFound(format!(
            "{}.{}",
            target.resource, target.operation
        )))
    }
}

fn map_err(e: RpcError) -> RpcResponse {
    let status = e.status_hint();
    let code = match &e {
        RpcError::Csrf(_) => "csrf",
        RpcError::Unauthorized => "unauthorized",
        RpcError::Forbidden => "forbidden",
        RpcError::NotFound(_) => "not_found",
        RpcError::BadRequest(_) => "bad_request",
        RpcError::Auth(_) => "auth",
        RpcError::Action(_) => "action",
        RpcError::Db(_) => "db",
        _ => "error",
    };
    RpcResponse::err(code, e.to_string(), status)
}

//! Top-level RPC dispatch: CSRF → auth context → CRUD / auth / custom actions.

use crate::access_gate::{
    enforce, enforce_member, list_owner_field, member_join, row_values_from_db,
};
use crate::action_db::CrudActionDbHost;
use crate::auth_ops::handle_auth_op;
use crate::context::RpcContext;
use crate::crud::handle_crud;
use crate::csrf::check_csrf;
use crate::error::{RpcError, RpcResult};
use crate::request::{RpcRequest, RpcResponse, RpcTarget};
use puddly_actions::{ActionCall, ActionCatalog, ActionRunner, IpcUser};
use puddly_auth::{AuthUser, SessionStore};
use puddly_data::{exec_prepared, query_prepared, PreparedKind, StatementBook};
use puddly_db::{Executor, SqlValue};
use puddly_ir::{ActionIr, AppIr, ResourceIr};
use serde_json::Value;

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
    pub async fn handle(
        &mut self,
        req: &RpcRequest,
        exec: &mut dyn Executor,
    ) -> RpcResponse {
        match self.handle_inner(req, exec).await {
            Ok(resp) => resp,
            Err(e) => map_err(e),
        }
    }

    async fn handle_inner(
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
            )
            .await;
        }

        let ctx = RpcContext::from_sessions(self.sessions, req.cookie_header_value()).await;

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
                self.ir,
            )
            .await?;
            return Ok(RpcResponse::ok(data));
        }

        // Custom action declared on the resource
        let action_ir = resource
            .actions
            .iter()
            .find(|a| a.name == target.operation);
        if let Some(action_ir) = action_ir {
            enforce_action_access(action_ir, resource, &req.body, &ctx, self.book, exec).await?;
            let loaded = self
                .actions
                .get(&target.resource, &target.operation)
                .ok_or_else(|| {
                    RpcError::NotFound(format!(
                        "action `{}.{}` not loaded",
                        target.resource, target.operation
                    ))
                })?;
            // Nested ctx.db helpers reuse CRUD + the same session user (ADR-046).
            let data = {
                let mut db_host = CrudActionDbHost {
                    ir: self.ir,
                    book: self.book,
                    ctx: &ctx,
                    exec,
                };
                self.runner
                    .run(ActionCall {
                        resource: &target.resource,
                        action: &target.operation,
                        input: &req.body,
                        source_path: &loaded.path,
                        source: &loaded.source,
                        user: ctx.user.as_ref().map(auth_user_to_ipc),
                        db_host: Some(&mut db_host),
                    })
                    .await?
            };
            // Archive actions historically returned metadata only; persist by deleting
            // the owner-checked row so the list view updates (ADR-036).
            if target.operation == "archive" {
                if let Some(id) = req.body.get("id").and_then(|v| v.as_str()) {
                    let stmt = self
                        .book
                        .get(&resource.name, PreparedKind::DeleteById)
                        .ok_or_else(|| {
                            RpcError::msg(format!("no delete statement for {}", resource.name))
                        })?;
                    exec_prepared(exec, stmt, &[SqlValue::Uuid(id.to_string())]).await?;
                }
            }
            return Ok(RpcResponse::ok(data));
        }

        Err(RpcError::NotFound(format!(
            "{}.{}",
            target.resource, target.operation
        )))
    }
}

/// When `action.access` is set, enforce it (loading the row by `input.id` for owner/member).
/// When omitted, require any authenticated user (Step 6 default).
async fn enforce_action_access(
    action: &ActionIr,
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<()> {
    match &action.access {
        None => {
            if ctx.user.is_none() {
                return Err(RpcError::Unauthorized);
            }
            Ok(())
        }
        Some(rule) => {
            if let Some(join) = member_join(rule) {
                if ctx.user.is_none() {
                    return Err(RpcError::Unauthorized);
                }
                let id = body
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        RpcError::BadRequest(
                            "id required in action input for owner/member access".into(),
                        )
                    })?;
                // Ensure the row exists (same not_found behavior as owner path).
                let stmt = book
                    .get(&resource.name, PreparedKind::FindById)
                    .ok_or_else(|| {
                        RpcError::msg(format!("no find statement for {}", resource.name))
                    })?;
                let rows =
                    query_prepared(exec, stmt, &[SqlValue::Uuid(id.to_string())]).await?;
                if rows.is_empty() {
                    return Err(RpcError::NotFound(format!("{} not found", resource.name)));
                }
                return enforce_member(&join, id, ctx.user.as_ref(), exec).await;
            }
            let row_values = if list_owner_field(rule).is_some() {
                // Auth first — avoid leaking existence via not_found to anonymous callers.
                if ctx.user.is_none() {
                    return Err(RpcError::Unauthorized);
                }
                let id = body
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        RpcError::BadRequest(
                            "id required in action input for owner/member access".into(),
                        )
                    })?;
                let stmt = book
                    .get(&resource.name, PreparedKind::FindById)
                    .ok_or_else(|| {
                        RpcError::msg(format!("no find statement for {}", resource.name))
                    })?;
                let rows =
                    query_prepared(exec, stmt, &[SqlValue::Uuid(id.to_string())]).await?;
                let row = rows.into_iter().next().ok_or_else(|| {
                    RpcError::NotFound(format!("{} not found", resource.name))
                })?;
                Some(row_values_from_db(&row))
            } else {
                None
            };
            enforce(rule, ctx.user.as_ref(), row_values.as_ref())
        }
    }
}

fn auth_user_to_ipc(user: &AuthUser) -> IpcUser {
    IpcUser {
        id: user.id.clone(),
        email: user.email.clone(),
        roles: user.roles.clone(),
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
        // Prefer frozen IPC wire code when present (ADR-049); else generic `action`.
        RpcError::Action(a) => match a.ipc_code() {
            Some(c) => c.as_str(),
            None => "action",
        },
        RpcError::Db(_) => "db",
        _ => "error",
    };
    RpcResponse::err(code, e.to_string(), status)
}

//! RPC implementation of [`ActionDbHost`] — nested action `ctx.db` (ADR-046).

use crate::context::RpcContext;
use crate::crud::handle_crud;
use crate::error::RpcError;
use async_trait::async_trait;
use puddly_actions::{ActionDbHost, ActionError, ActionResult, IpcErrorCode};
use puddly_data::StatementBook;
use puddly_db::Executor;
use puddly_ir::AppIr;
use serde_json::Value;

/// Forwards sidecar `ctx.db.*` into [`handle_crud`] with the same session user.
pub struct CrudActionDbHost<'a> {
    pub ir: &'a AppIr,
    pub book: &'a StatementBook,
    pub ctx: &'a RpcContext,
    pub exec: &'a mut dyn Executor,
}

fn rpc_to_action(e: RpcError) -> ActionError {
    match e {
        RpcError::Unauthorized => ActionError::db(IpcErrorCode::Unauthorized, "unauthorized"),
        RpcError::Forbidden => ActionError::db(IpcErrorCode::Forbidden, "forbidden"),
        RpcError::NotFound(m) => ActionError::db(IpcErrorCode::NotFound, m),
        RpcError::BadRequest(m) => ActionError::db(IpcErrorCode::BadRequest, m),
        other => ActionError::db(IpcErrorCode::DbFailed, other.to_string()),
    }
}

#[async_trait]
impl ActionDbHost for CrudActionDbHost<'_> {
    async fn db_op(
        &mut self,
        method: &str,
        resource: &str,
        input: &Value,
    ) -> ActionResult<Value> {
        match method {
            "get" | "list" | "create" | "update" | "delete" => {}
            other => {
                return Err(ActionError::db(
                    IpcErrorCode::BadRequest,
                    format!("unsupported db method `{other}`"),
                ));
            }
        }
        let res = self
            .ir
            .resources
            .iter()
            .find(|r| r.name == resource)
            .ok_or_else(|| {
                ActionError::db(
                    IpcErrorCode::NotFound,
                    format!("resource `{resource}` not found"),
                )
            })?;
        handle_crud(
            res,
            method,
            input,
            self.ctx,
            self.book,
            self.exec,
            self.ir,
        )
        .await
        .map_err(rpc_to_action)
    }
}

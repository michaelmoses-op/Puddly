//! Nested DB host bridge for action `ctx.db` (ADR-046 / Step 28).
//!
//! Sidecar helpers round-trip here; the RPC layer implements this with
//! [`handle_crud`](../../puddly-rpc) so access rules stay on the host.

use crate::error::ActionResult;
use async_trait::async_trait;
use serde_json::Value;

/// Host-side executor for sidecar `ctx.db.*` calls.
#[async_trait]
pub trait ActionDbHost: Send {
    /// Run one CRUD-shaped op: `get` | `list` | `create` | `update` | `delete`.
    async fn db_op(
        &mut self,
        method: &str,
        resource: &str,
        input: &Value,
    ) -> ActionResult<Value>;
}

/// Test double that returns canned JSON (or a fixed error).
pub struct MockActionDbHost {
    pub handler: Box<dyn FnMut(&str, &str, &Value) -> ActionResult<Value> + Send>,
}

impl MockActionDbHost {
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(&str, &str, &Value) -> ActionResult<Value> + Send + 'static,
    {
        Self {
            handler: Box::new(f),
        }
    }

    pub fn ok_json(value: Value) -> Self {
        Self::new(move |_m, _r, _i| Ok(value.clone()))
    }
}

#[async_trait]
impl ActionDbHost for MockActionDbHost {
    async fn db_op(
        &mut self,
        method: &str,
        resource: &str,
        input: &Value,
    ) -> ActionResult<Value> {
        (self.handler)(method, resource, input)
    }
}

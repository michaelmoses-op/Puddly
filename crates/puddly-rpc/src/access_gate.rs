//! Enforce declarative access rules for CRUD ops.

use crate::error::{RpcError, RpcResult};
use puddly_auth::{check_rule, AccessDecision, AuthUser, RowValues};
use puddly_db::{DbRow, Executor, SqlValue};
use puddly_ir::{AccessIr, ResourceIr, RuleIr};

/// Map RPC operation → access rule name in IR.
/// `get` reuses `list` when no dedicated `get` rule exists (IR only has list/create/update/delete).
pub fn access_op_name(operation: &str) -> &str {
    match operation {
        "get" => "list",
        other => other,
    }
}

pub fn find_rule<'a>(access: &'a AccessIr, operation: &str) -> Option<&'a RuleIr> {
    let name = access_op_name(operation);
    access
        .operations
        .iter()
        .find(|op| op.name == name)
        .map(|op| &op.rule)
}

pub fn require_rule<'a>(resource: &'a ResourceIr, operation: &str) -> RpcResult<&'a RuleIr> {
    find_rule(&resource.access, operation).ok_or_else(|| {
        RpcError::Forbidden // no rule declared for this op
    })
}

pub fn enforce(
    rule: &RuleIr,
    user: Option<&AuthUser>,
    row: Option<&RowValues>,
) -> RpcResult<()> {
    match check_rule(rule, user, row) {
        AccessDecision::Allow => Ok(()),
        AccessDecision::Deny => {
            if user.is_none() {
                Err(RpcError::Unauthorized)
            } else {
                Err(RpcError::Forbidden)
            }
        }
    }
}

/// Build [`RowValues`] from a DB row for owner checks.
pub fn row_values_from_db(row: &DbRow) -> RowValues {
    use puddly_db::SqlValue;
    let mut values = RowValues::new();
    for (k, v) in &row.columns {
        let text = match v {
            SqlValue::Null => continue,
            SqlValue::Bool(b) => b.to_string(),
            SqlValue::Int(i) => i.to_string(),
            SqlValue::Float(f) => f.to_string(),
            SqlValue::Text(s) | SqlValue::Uuid(s) => s.clone(),
        };
        values = values.set(k.clone(), text);
    }
    values
}

/// For `list` + `owner`: require authenticated user; caller should filter by field.
pub fn list_owner_field(rule: &RuleIr) -> Option<&str> {
    match rule {
        RuleIr::Owner { field, .. } => Some(field.as_str()),
        _ => None,
    }
}

/// Join-table membership spec (ADR-047).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberJoin {
    pub via: String,
    pub on: String,
    pub user_field: String,
}

pub fn member_join(rule: &RuleIr) -> Option<MemberJoin> {
    match rule {
        RuleIr::Member {
            via,
            on,
            user_field,
            ..
        } => Some(MemberJoin {
            via: via.clone(),
            on: on.clone(),
            user_field: user_field.clone(),
        }),
        _ => None,
    }
}

/// `SELECT 1 FROM via WHERE on = $id AND user = $userId LIMIT 1`.
pub async fn enforce_member(
    join: &MemberJoin,
    resource_id: &str,
    user: Option<&AuthUser>,
    exec: &mut dyn Executor,
) -> RpcResult<()> {
    let Some(user) = user else {
        return Err(RpcError::Unauthorized);
    };
    let sql = format!(
        "SELECT 1 AS \"ok\" FROM {} WHERE {} = $1 AND {} = $2 LIMIT 1",
        q(&join.via),
        q(&join.on),
        q(&join.user_field)
    );
    let rows = exec
        .query(
            &sql,
            &[
                SqlValue::Uuid(resource_id.to_string()),
                SqlValue::Uuid(user.id.clone()),
            ],
        )
        .await?;
    if rows.is_empty() {
        Err(RpcError::Forbidden)
    } else {
        Ok(())
    }
}

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

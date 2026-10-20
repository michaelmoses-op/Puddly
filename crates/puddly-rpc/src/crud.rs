//! Built-in list/get/create/update/delete RPC using compiled statements + access rules.

use crate::access_gate::{enforce, list_owner_field, require_rule, row_values_from_db};
use crate::context::RpcContext;
use crate::error::{RpcError, RpcResult};
use crate::list_body::apply_list_body;
use futures::future::FutureExt;
use puddly_data::{
    exec_prepared, query_prepared, PreparedKind, QueryBuilder, StatementBook,
};
use puddly_db::{with_transaction, DbRow, Executor, SqlValue};
use puddly_ir::ResourceIr;
use serde_json::{json, Map, Value};

pub async fn handle_crud(
    resource: &ResourceIr,
    operation: &str,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    match operation {
        "list" => list(resource, body, ctx, book, exec).await,
        "get" => get(resource, body, ctx, book, exec).await,
        "create" => create(resource, body, ctx, book, exec).await,
        "update" => update(resource, body, ctx, book, exec).await,
        "delete" => delete(resource, body, ctx, book, exec).await,
        other => Err(RpcError::NotFound(format!(
            "{}.{}",
            resource.name, other
        ))),
    }
}

async fn list(
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    _book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    let rule = require_rule(resource, "list")?;
    let mut qb = QueryBuilder::for_resource(resource);

    // Owner/member: require sign-in and inject equality that ANDs with body filters.
    if let Some(field) = list_owner_field(rule) {
        let user = ctx.user.as_ref().ok_or(RpcError::Unauthorized)?;
        qb = qb
            .require_eq(field, SqlValue::Uuid(user.id.clone()))
            .map_err(|e| RpcError::BadRequest(e.to_string()))?;
    } else {
        enforce(rule, ctx.user.as_ref(), None)?;
    }

    qb = apply_list_body(qb, body)?;
    let page = qb
        .build()
        .map_err(|e| RpcError::BadRequest(e.to_string()))?;
    let rows = exec.query(&page.sql, &page.params).await?;
    Ok(json!({
        "items": rows_to_json(&rows),
        "limit": page.limit,
    }))
}

async fn get(
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    let id = body_id(body)?;
    let stmt = book
        .get(&resource.name, PreparedKind::FindById)
        .ok_or_else(|| RpcError::msg(format!("no find statement for {}", resource.name)))?;
    let rows = query_prepared(exec, stmt, &[SqlValue::Uuid(id)]).await?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| RpcError::NotFound(format!("{} not found", resource.name)))?;
    let values = row_values_from_db(&row);
    let rule = require_rule(resource, "get")?;
    enforce(rule, ctx.user.as_ref(), Some(&values))?;
    Ok(row_to_json(&row))
}

async fn create(
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    let rule = require_rule(resource, "create")?;
    enforce(rule, ctx.user.as_ref(), None)?;
    let stmt = book
        .get(&resource.name, PreparedKind::Insert)
        .ok_or_else(|| RpcError::msg(format!("no insert statement for {}", resource.name)))?
        .clone();
    let params = params_for_insert(&stmt, body)?;
    with_transaction(exec, |tx| {
        async move {
            exec_prepared(tx, &stmt, &params).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(json!({ "created": true }))
}

async fn update(
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    let id = body_id(body)?;
    // Load existing row for owner checks.
    let find = book
        .get(&resource.name, PreparedKind::FindById)
        .ok_or_else(|| RpcError::msg(format!("no find statement for {}", resource.name)))?;
    let existing = query_prepared(exec, find, &[SqlValue::Uuid(id.clone())])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| RpcError::NotFound(format!("{} not found", resource.name)))?;
    let values = row_values_from_db(&existing);
    let rule = require_rule(resource, "update")?;
    enforce(rule, ctx.user.as_ref(), Some(&values))?;

    let stmt = book
        .get(&resource.name, PreparedKind::UpdateById)
        .ok_or_else(|| RpcError::msg(format!("no update statement for {}", resource.name)))?;
    let params = params_for_update(stmt, body, &id)?;
    exec_prepared(exec, stmt, &params).await?;
    Ok(json!({ "updated": true }))
}

async fn delete(
    resource: &ResourceIr,
    body: &Value,
    ctx: &RpcContext,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<Value> {
    let id = body_id(body)?;
    let find = book
        .get(&resource.name, PreparedKind::FindById)
        .ok_or_else(|| RpcError::msg(format!("no find statement for {}", resource.name)))?;
    let existing = query_prepared(exec, find, &[SqlValue::Uuid(id.clone())])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| RpcError::NotFound(format!("{} not found", resource.name)))?;
    let values = row_values_from_db(&existing);
    let rule = require_rule(resource, "delete")?;
    enforce(rule, ctx.user.as_ref(), Some(&values))?;

    let stmt = book
        .get(&resource.name, PreparedKind::DeleteById)
        .ok_or_else(|| RpcError::msg(format!("no delete statement for {}", resource.name)))?;
    exec_prepared(exec, stmt, &[SqlValue::Uuid(id)]).await?;
    Ok(json!({ "deleted": true }))
}

fn body_id(body: &Value) -> RpcResult<String> {
    body.get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| RpcError::BadRequest("id required".into()))
}

fn json_to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Float(f)
            } else {
                SqlValue::Text(n.to_string())
            }
        }
        Value::String(s) => {
            // Heuristic: UUID-shaped strings as Uuid.
            if s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4 {
                SqlValue::Uuid(s.clone())
            } else {
                SqlValue::Text(s.clone())
            }
        }
        other => SqlValue::Text(other.to_string()),
    }
}

fn params_for_insert(
    stmt: &puddly_data::PreparedStatement,
    body: &Value,
) -> RpcResult<Vec<SqlValue>> {
    let obj = body
        .as_object()
        .ok_or_else(|| RpcError::BadRequest("body must be an object".into()))?;
    let mut params = Vec::new();
    for name in &stmt.param_names {
        let v = obj.get(name).unwrap_or(&Value::Null);
        params.push(json_to_sql(v));
    }
    Ok(params)
}

fn params_for_update(
    stmt: &puddly_data::PreparedStatement,
    body: &Value,
    id: &str,
) -> RpcResult<Vec<SqlValue>> {
    let obj = body
        .as_object()
        .ok_or_else(|| RpcError::BadRequest("body must be an object".into()))?;
    let mut params = Vec::new();
    for name in &stmt.param_names {
        if name == "id" {
            params.push(SqlValue::Uuid(id.to_string()));
            continue;
        }
        let v = obj.get(name).unwrap_or(&Value::Null);
        params.push(json_to_sql(v));
    }
    Ok(params)
}

fn row_to_json(row: &DbRow) -> Value {
    let mut map = Map::new();
    for (k, v) in &row.columns {
        map.insert(k.clone(), sql_to_json(v));
    }
    Value::Object(map)
}

fn rows_to_json(rows: &[DbRow]) -> Value {
    Value::Array(rows.iter().map(row_to_json).collect())
}

fn sql_to_json(v: &SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(b) => Value::Bool(*b),
        SqlValue::Int(i) => json!(i),
        SqlValue::Float(f) => json!(f),
        SqlValue::Text(s) | SqlValue::Uuid(s) => Value::String(s.clone()),
    }
}

//! Built-in list/get/create/update/delete RPC using compiled statements + access rules.

use crate::access_gate::{enforce, list_owner_field, require_rule, row_values_from_db};
use crate::context::RpcContext;
use crate::error::{RpcError, RpcResult};
use crate::list_body::apply_list_body;
use futures::future::FutureExt;
use puddly_auth::{generate_user_id, AuthUser};
use puddly_data::{
    exec_prepared, query_prepared, PreparedKind, QueryBuilder, StatementBook,
};
use puddly_db::{with_transaction, DbRow, Executor, SqlValue};
use puddly_ir::{FieldKind, LiteralValue, ResourceIr};
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
    let mut body = prepare_create_body(resource, body, ctx.user.as_ref())?;
    // When `project` is omitted on create, ensure a personal project for the owner
    // (ADR-036) so the example form does not need a magic hidden UUID.
    if let Some(user) = ctx.user.as_ref() {
        ensure_default_project(resource, &mut body, user, book, exec).await?;
    }
    let stmt = book
        .get(&resource.name, PreparedKind::Insert)
        .ok_or_else(|| RpcError::msg(format!("no insert statement for {}", resource.name)))?
        .clone();
    let params = params_for_insert(&stmt, &body)?;
    with_transaction(exec, |tx| {
        async move {
            exec_prepared(tx, &stmt, &params).await?;
            Ok(())
        }
        .boxed()
    })
    .await?;
    Ok(json!({ "created": true, "id": body.get("id").cloned().unwrap_or(Value::Null) }))
}

/// If `resource` has a required `project` ref and the body omits it, reuse or create
/// a personal project owned by `user`.
async fn ensure_default_project(
    resource: &ResourceIr,
    body: &mut Value,
    user: &AuthUser,
    book: &StatementBook,
    exec: &mut dyn Executor,
) -> RpcResult<()> {
    let has_project_ref = resource.fields.iter().any(|f| {
        f.name == "project" && matches!(f.kind, FieldKind::Ref { ref target } if target == "project")
    });
    if !has_project_ref {
        return Ok(());
    }
    let obj = body
        .as_object_mut()
        .ok_or_else(|| RpcError::BadRequest("body must be an object".into()))?;
    if !value_missing(obj.get("project")) {
        return Ok(());
    }

    let existing = exec
        .query(
            "SELECT \"id\" FROM \"project\" WHERE \"owner\" = $1 LIMIT 1",
            &[SqlValue::Uuid(user.id.clone())],
        )
        .await?;
    if let Some(row) = existing.first() {
        if let Some(id) = row.get("id").and_then(|v| match v {
            SqlValue::Uuid(s) | SqlValue::Text(s) => Some(s.clone()),
            _ => None,
        }) {
            obj.insert("project".into(), Value::String(id));
            return Ok(());
        }
    }

    let project_id = generate_user_id();
    let project_body = json!({
        "id": project_id,
        "title": "Personal",
        "owner": user.id,
    });
    let stmt = book
        .get("project", PreparedKind::Insert)
        .ok_or_else(|| RpcError::msg("no insert statement for project"))?
        .clone();
    let params = params_for_insert(&stmt, &project_body)?;
    exec_prepared(exec, &stmt, &params).await?;
    obj.insert("project".into(), Value::String(project_id));
    Ok(())
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
    // Merge body over existing row so partial updates (e.g. `{ id, done }`) do not null out
    // other columns.
    let params = params_for_update(stmt, body, &id, &existing)?;
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

/// Fill create body: session `owner` when missing, generate `id`, apply field defaults.
fn prepare_create_body(
    resource: &ResourceIr,
    body: &Value,
    user: Option<&AuthUser>,
) -> RpcResult<Value> {
    let mut obj = match body {
        Value::Object(m) => m.clone(),
        Value::Null => Map::new(),
        _ => {
            return Err(RpcError::BadRequest(
                "body must be an object".into(),
            ))
        }
    };

    if let Some(user) = user {
        if resource.fields.iter().any(|f| f.name == "owner") && value_missing(obj.get("owner")) {
            obj.insert("owner".into(), Value::String(user.id.clone()));
        }
    }

    if resource
        .fields
        .iter()
        .any(|f| matches!(f.kind, FieldKind::Id))
        && value_missing(obj.get("id"))
    {
        obj.insert("id".into(), Value::String(generate_user_id()));
    }

    for field in &resource.fields {
        if matches!(field.kind, FieldKind::Computed { .. }) {
            continue;
        }
        if !value_missing(obj.get(&field.name)) {
            continue;
        }
        if let Some(def) = &field.options.default {
            obj.insert(field.name.clone(), literal_to_json(def));
        }
    }

    Ok(Value::Object(obj))
}

fn value_missing(v: Option<&Value>) -> bool {
    match v {
        None => true,
        Some(Value::Null) => true,
        Some(Value::String(s)) if s.is_empty() => true,
        _ => false,
    }
}

fn literal_to_json(lit: &LiteralValue) -> Value {
    match lit {
        LiteralValue::Bool(b) => Value::Bool(*b),
        LiteralValue::Int(i) => json!(i),
        LiteralValue::Float(s) | LiteralValue::String(s) => Value::String(s.clone()),
        LiteralValue::Null => Value::Null,
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
    existing: &DbRow,
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
        if let Some(v) = obj.get(name) {
            if !v.is_null() {
                params.push(json_to_sql(v));
                continue;
            }
        }
        if let Some(prev) = existing.get(name) {
            params.push(prev.clone());
        } else {
            params.push(SqlValue::Null);
        }
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

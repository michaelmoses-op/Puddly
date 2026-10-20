//! Apply closed-vocabulary list options from the RPC JSON body onto a [`QueryBuilder`].

use crate::error::{RpcError, RpcResult};
use puddly_data::{FilterOp, OrderDir, QueryBuilder};
use puddly_db::SqlValue;
use serde_json::Value;

/// Extend `qb` with body filters / order / limit / cursor.
///
/// Accepted shapes (all optional):
/// ```json
/// {
///   "filter": [{ "field": "done", "op": "eq", "value": false }],
///   "where": { "done": false },
///   "orderBy": [{ "field": "createdAt", "dir": "desc" }],
///   "limit": 10,
///   "cursor": "<id>"
/// }
/// ```
/// `where` entries are sugar for `{ op: "eq" }`. Filters AND together (and with any
/// prior `require_eq` owner/member predicates already on the builder).
pub fn apply_list_body(mut qb: QueryBuilder, body: &Value) -> RpcResult<QueryBuilder> {
    if let Some(where_obj) = body.get("where").and_then(|v| v.as_object()) {
        for (field, value) in where_obj {
            qb = qb
                .eq(field, json_to_sql(value)?)
                .map_err(data_err)?;
        }
    }

    if let Some(filters) = body.get("filter").and_then(|v| v.as_array()) {
        for entry in filters {
            let field = entry
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::BadRequest("filter.field required".into()))?;
            let op_str = entry
                .get("op")
                .and_then(|v| v.as_str())
                .unwrap_or("eq");
            let op = FilterOp::parse(op_str).map_err(data_err)?;
            let value = entry
                .get("value")
                .ok_or_else(|| RpcError::BadRequest("filter.value required".into()))?;
            let sql_v = json_to_sql(value)?;
            qb = match op {
                FilterOp::Eq => qb.eq(field, sql_v).map_err(data_err)?,
                FilterOp::Before => qb.before(field, sql_v).map_err(data_err)?,
                FilterOp::After => qb.after(field, sql_v).map_err(data_err)?,
                FilterOp::In => qb.filter_in(field, sql_v).map_err(data_err)?,
                FilterOp::Contains => qb.contains(field, sql_v).map_err(data_err)?,
            };
        }
    }

    if let Some(orders) = body.get("orderBy").and_then(|v| v.as_array()) {
        for entry in orders {
            let field = entry
                .get("field")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::BadRequest("orderBy.field required".into()))?;
            let dir = match entry.get("dir").and_then(|v| v.as_str()).unwrap_or("asc") {
                "desc" | "DESC" => OrderDir::Desc,
                _ => OrderDir::Asc,
            };
            qb = qb.order_by(field, dir).map_err(data_err)?;
        }
    }

    if let Some(limit) = body.get("limit") {
        let n = limit
            .as_u64()
            .ok_or_else(|| RpcError::BadRequest("limit must be a number".into()))?;
        qb = qb.limit(n as u32).map_err(data_err)?;
    }

    if let Some(cursor) = body.get("cursor") {
        if !cursor.is_null() {
            qb = qb.cursor(json_to_sql(cursor)?);
        }
    }

    if let Some(include_val) = body.get("include") {
        match include_val {
            Value::Array(includes) => {
                for inc in includes {
                    let name = inc.as_str().ok_or_else(|| {
                        RpcError::BadRequest("include entries must be strings".into())
                    })?;
                    qb = qb.include(name).map_err(data_err)?;
                }
            }
            Value::String(name) => {
                qb = qb.include(name).map_err(data_err)?;
            }
            _ => {
                return Err(RpcError::BadRequest(
                    "include must be a string or array of strings".into(),
                ));
            }
        }
    }

    Ok(qb)
}

fn data_err(e: puddly_data::DataError) -> RpcError {
    RpcError::BadRequest(e.to_string())
}

fn json_to_sql(v: &Value) -> RpcResult<SqlValue> {
    Ok(match v {
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
            if s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4 {
                SqlValue::Uuid(s.clone())
            } else {
                SqlValue::Text(s.clone())
            }
        }
        Value::Array(arr) => {
            // Encode IN lists as JSON text for the ANY() sketch.
            SqlValue::Text(serde_json::to_string(arr).unwrap_or_else(|_| "[]".into()))
        }
        other => {
            return Err(RpcError::BadRequest(format!(
                "unsupported filter value: {other}"
            )));
        }
    })
}

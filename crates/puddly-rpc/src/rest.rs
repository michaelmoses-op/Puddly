//! Opt-in REST adapter over RPC targets (ADR-008 / ADR-045 / Step 27).
//!
//! Thin mapping only — CSRF, session, and access rules stay in [`crate::RpcRuntime`].

use serde_json::{json, Map, Value};

/// Default URL prefix for the opt-in REST surface.
pub const REST_API_PREFIX: &str = "/api";

/// Result of mapping an HTTP method + path (+ JSON body) onto an RPC call.
#[derive(Debug, Clone, PartialEq)]
pub struct RestMapping {
    pub target: String,
    pub body: Value,
}

/// Map REST → RPC. Returns `None` when the path is not under `/api/…`.
///
/// | Method | Path | RPC |
/// | --- | --- | --- |
/// | GET | `/api/<resource>` | `<resource>.list` |
/// | POST | `/api/<resource>` | `<resource>.create` |
/// | GET | `/api/<resource>/<id>` | `<resource>.get` |
/// | PUT/PATCH | `/api/<resource>/<id>` | `<resource>.update` |
/// | DELETE | `/api/<resource>/<id>` | `<resource>.delete` |
/// | POST | `/api/<resource>/<action>` | `<resource>.<action>` (non-UUID segment) |
/// | POST | `/api/auth/login` etc. | `auth.login` … |
pub fn map_rest_to_rpc(method: &str, path: &str, body: Value) -> Option<RestMapping> {
    let path = path.split('?').next().unwrap_or(path);
    let path = path.trim_end_matches('/');
    let rest = path.strip_prefix(REST_API_PREFIX)?;
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        return None;
    }

    let mut parts = rest.split('/');
    let resource = parts.next()?.to_string();
    if resource.is_empty() || resource.contains('.') {
        return None;
    }
    let second = parts.next();
    if parts.next().is_some() {
        return None; // only /api/r or /api/r/x
    }

    let method = method.to_ascii_uppercase();
    match (method.as_str(), second) {
        ("GET", None) => Some(RestMapping {
            target: format!("{resource}.list"),
            body: body_or_empty(body),
        }),
        ("POST", None) => Some(RestMapping {
            target: format!("{resource}.create"),
            body: body_or_empty(body),
        }),
        ("GET", Some(id)) => Some(RestMapping {
            target: format!("{resource}.get"),
            body: merge_id(body, id),
        }),
        ("PUT" | "PATCH", Some(id)) => Some(RestMapping {
            target: format!("{resource}.update"),
            body: merge_id(body, id),
        }),
        ("DELETE", Some(id)) => Some(RestMapping {
            target: format!("{resource}.delete"),
            body: merge_id(body, id),
        }),
        ("POST", Some(action)) if !looks_like_uuid(action) => Some(RestMapping {
            target: format!("{resource}.{action}"),
            body: body_or_empty(body),
        }),
        // POST /api/todo/<uuid> is not a create — reject (use collection POST).
        ("POST", Some(_)) => None,
        _ => None,
    }
}

fn body_or_empty(body: Value) -> Value {
    if body.is_null() {
        json!({})
    } else {
        body
    }
}

fn merge_id(body: Value, id: &str) -> Value {
    let mut obj = match body {
        Value::Object(m) => m,
        Value::Null => Map::new(),
        other => {
            let mut m = Map::new();
            m.insert("_body".into(), other);
            m
        }
    };
    obj.entry("id".to_string())
        .or_insert_with(|| Value::String(id.to_string()));
    Value::Object(obj)
}

/// UUID-ish path segment (36 chars, 4 dashes) → treat as id, not action name.
pub fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_collection_and_item() {
        let m = map_rest_to_rpc("GET", "/api/todo", json!({})).unwrap();
        assert_eq!(m.target, "todo.list");
        let m = map_rest_to_rpc("POST", "/api/todo", json!({"title": "x"})).unwrap();
        assert_eq!(m.target, "todo.create");
        let m = map_rest_to_rpc(
            "GET",
            "/api/todo/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            json!({}),
        )
        .unwrap();
        assert_eq!(m.target, "todo.get");
        assert_eq!(m.body["id"], "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        let m = map_rest_to_rpc(
            "PATCH",
            "/api/todo/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            json!({"done": true}),
        )
        .unwrap();
        assert_eq!(m.target, "todo.update");
        assert_eq!(m.body["done"], true);
        let m = map_rest_to_rpc(
            "DELETE",
            "/api/todo/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            json!({}),
        )
        .unwrap();
        assert_eq!(m.target, "todo.delete");
    }

    #[test]
    fn maps_custom_action() {
        let m = map_rest_to_rpc("POST", "/api/todo/archive", json!({"id": "t1"})).unwrap();
        assert_eq!(m.target, "todo.archive");
    }

    #[test]
    fn maps_auth() {
        let m = map_rest_to_rpc("POST", "/api/auth/login", json!({"email": "a@b.co"})).unwrap();
        assert_eq!(m.target, "auth.login");
    }

    #[test]
    fn ignores_non_api() {
        assert!(map_rest_to_rpc("GET", "/rpc/todo.list", json!({})).is_none());
        assert!(map_rest_to_rpc("GET", "/events", json!({})).is_none());
    }
}

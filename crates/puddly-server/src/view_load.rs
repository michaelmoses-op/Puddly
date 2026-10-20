//! Resolve view `load` specs via resource CRUD + session auth (ADR-032).

use puddly_actions::ActionRunner;
use puddly_auth::SessionStore;
use puddly_db::Executor;
use puddly_rpc::{RpcRequest, RpcServer};
use puddly_views::{LoadSource, LoadSpec};
use serde_json::{json, Map, Value};

/// Outcome of running a view's declarative load.
#[derive(Debug)]
pub enum LoadOutcome {
    Props(Value),
    /// HTML body for a simple unauthorized page (login UI is Step 17).
    Unauthorized,
    /// Missing executor/pool while load needs DB.
    Unavailable(String),
    BadRequest(String),
    Error(String),
}

/// Run `load` entries against CRUD (same access rules as RPC).
///
/// - Static `value` entries need no DB.
/// - `list` / `get` require `exec`; otherwise → [`LoadOutcome::Unavailable`].
/// - Owner-gated ops without a session → [`LoadOutcome::Unauthorized`].
pub async fn resolve_load<S, R>(
    load: &LoadSpec,
    query_string: Option<&str>,
    cookie_header: Option<&str>,
    mut rpc: Option<&mut RpcServer<S, R>>,
    mut exec: Option<&mut dyn Executor>,
) -> LoadOutcome
where
    S: SessionStore,
    R: ActionRunner,
{
    if load.needs_db() && (rpc.is_none() || exec.is_none()) {
        return LoadOutcome::Unavailable(
            "this view's `load` requires a database (set DATABASE_URL)".into(),
        );
    }

    let mut props = Map::new();
    for entry in &load.entries {
        match &entry.source {
            LoadSource::Value { value } => {
                props.insert(entry.key.clone(), value.clone());
            }
            LoadSource::List { resource } => {
                let rpc = rpc.as_deref_mut().expect("needs_db checked");
                let exec = exec.as_deref_mut().expect("needs_db checked");
                match run_crud(rpc, exec, &format!("{resource}.list"), json!({}), cookie_header)
                    .await
                {
                    Ok(data) => {
                        props.insert(entry.key.clone(), data);
                    }
                    Err(LoadRunErr::Unauthorized) => return LoadOutcome::Unauthorized,
                    Err(LoadRunErr::BadRequest(m)) => return LoadOutcome::BadRequest(m),
                    Err(LoadRunErr::Other(m)) => return LoadOutcome::Error(m),
                }
            }
            LoadSource::Get {
                resource,
                id_from_query,
            } => {
                let id = match query_param(query_string, id_from_query) {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return LoadOutcome::BadRequest(format!(
                            "missing query param `{id_from_query}` for load.{}.get",
                            entry.key
                        ));
                    }
                };
                let rpc = rpc.as_deref_mut().expect("needs_db checked");
                let exec = exec.as_deref_mut().expect("needs_db checked");
                match run_crud(
                    rpc,
                    exec,
                    &format!("{resource}.get"),
                    json!({ "id": id }),
                    cookie_header,
                )
                .await
                {
                    Ok(data) => {
                        props.insert(entry.key.clone(), data);
                    }
                    Err(LoadRunErr::Unauthorized) => return LoadOutcome::Unauthorized,
                    Err(LoadRunErr::BadRequest(m)) => return LoadOutcome::BadRequest(m),
                    Err(LoadRunErr::Other(m)) => return LoadOutcome::Error(m),
                }
            }
        }
    }

    LoadOutcome::Props(Value::Object(props))
}

/// Resolve loads that are purely static (`value:` entries). No RPC / DB.
pub fn resolve_static_load(load: &LoadSpec) -> LoadOutcome {
    if load.needs_db() {
        return LoadOutcome::Unavailable(
            "this view's `load` requires a database (set DATABASE_URL)".into(),
        );
    }
    let mut props = Map::new();
    for entry in &load.entries {
        match &entry.source {
            LoadSource::Value { value } => {
                props.insert(entry.key.clone(), value.clone());
            }
            LoadSource::List { .. } | LoadSource::Get { .. } => unreachable!("needs_db"),
        }
    }
    LoadOutcome::Props(Value::Object(props))
}

enum LoadRunErr {
    Unauthorized,
    BadRequest(String),
    Other(String),
}

async fn run_crud<S, R>(
    rpc: &mut RpcServer<S, R>,
    exec: &mut dyn Executor,
    target: &str,
    body: Value,
    cookie_header: Option<&str>,
) -> Result<Value, LoadRunErr>
where
    S: SessionStore,
    R: ActionRunner,
{
    let mut req = RpcRequest::new(target, body);
    if let Some(c) = cookie_header {
        req = req.with_cookie(c);
    }
    let resp = rpc.handle(&req, exec).await;
    if resp.ok {
        return Ok(resp.data.unwrap_or(Value::Null));
    }
    let err = resp.error.unwrap_or_else(|| puddly_rpc::RpcErrorBody {
        code: "error".into(),
        message: "unknown".into(),
        status: 500,
    });
    match err.code.as_str() {
        "unauthorized" => Err(LoadRunErr::Unauthorized),
        "bad_request" => Err(LoadRunErr::BadRequest(err.message)),
        other => Err(LoadRunErr::Other(format!("{other}: {}", err.message))),
    }
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let q = query?;
    for pair in q.split('&') {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next()?;
        if k == name {
            let v = parts.next().unwrap_or("");
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Simple HTML body for 401 (no login form until Step 17).
pub fn unauthorized_html() -> String {
    "<!DOCTYPE html><html><head><meta charset=\"utf-8\" /><title>Sign in required</title></head><body><h1>Sign in required</h1><p>This page needs an authenticated session. Login UI arrives in a later step.</p></body></html>".into()
}

/// Simple HTML body for 503 when load needs DB but none is configured.
pub fn unavailable_html(message: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\" /><title>Unavailable</title></head><body><h1>Unavailable</h1><p>{}</p></body></html>",
        escape_html(message)
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

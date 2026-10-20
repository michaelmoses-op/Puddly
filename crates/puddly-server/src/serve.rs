//! Bind + accept loop and single-exchange dispatch onto `RpcServer`.

use crate::error::{ServerError, ServerResult};
use crate::http::{
    parse_http_request, write_http_response, HttpRequest, HttpResponse,
};
use puddly_core::{CSRF_HEADER_NAME, DISPLAY_NAME};
use puddly_db::Executor;
use puddly_rpc::{RpcRequest, RpcResponse, RpcServer};
use serde_json::{json, Value};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: String,
    pub root: PathBuf,
    pub secure_cookies: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".into(),
            root: PathBuf::from("."),
            secure_cookies: false,
        }
    }
}

/// Blocking accept loop. `exec` is shared across connections (sync ADR-024).
pub fn serve<S, R>(
    config: &ServerConfig,
    rpc: Arc<Mutex<RpcServer<S, R>>>,
    exec: Arc<Mutex<Box<dyn Executor + Send>>>,
) -> ServerResult<()>
where
    S: puddly_auth::SessionStore + Send + 'static,
    R: puddly_actions::ActionRunner + Send + 'static,
{
    let listener = TcpListener::bind(&config.bind)?;
    eprintln!(
        "{DISPLAY_NAME} server listening on http://{}",
        config.bind
    );
    for stream in listener.incoming() {
        let mut stream = stream?;
        let rpc = Arc::clone(&rpc);
        let exec = Arc::clone(&exec);
        // Sync: handle inline (no thread pool yet).
        if let Err(e) = handle_stream(&mut stream, &rpc, &exec) {
            eprintln!("request error: {e}");
        }
    }
    Ok(())
}

/// Handle a single already-accepted stream (tests / one-shot).
pub fn serve_once<S, R>(
    stream: &mut TcpStream,
    rpc: &Mutex<RpcServer<S, R>>,
    exec: &Mutex<Box<dyn Executor + Send>>,
) -> ServerResult<()>
where
    S: puddly_auth::SessionStore,
    R: puddly_actions::ActionRunner,
{
    handle_stream(stream, rpc, exec)
}

fn handle_stream<S, R>(
    stream: &mut TcpStream,
    rpc: &Mutex<RpcServer<S, R>>,
    exec: &Mutex<Box<dyn Executor + Send>>,
) -> ServerResult<()>
where
    S: puddly_auth::SessionStore,
    R: puddly_actions::ActionRunner,
{
    let http_req = parse_http_request(stream)?;
    let http_resp = dispatch_http(&http_req, rpc, exec)?;
    write_http_response(stream, &http_resp)
}

/// Pure dispatch used by the accept loop and unit tests.
pub fn dispatch_http<S, R>(
    http_req: &HttpRequest,
    rpc: &Mutex<RpcServer<S, R>>,
    exec: &Mutex<Box<dyn Executor + Send>>,
) -> ServerResult<HttpResponse>
where
    S: puddly_auth::SessionStore,
    R: puddly_actions::ActionRunner,
{
    if http_req.method.eq_ignore_ascii_case("GET")
        && (http_req.path == "/health" || http_req.path == "/healthz")
    {
        return Ok(HttpResponse::json(200, "OK", &json!({ "ok": true })));
    }

    if !http_req.method.eq_ignore_ascii_case("POST") {
        return Ok(HttpResponse::text(405, "Method Not Allowed", "POST only for RPC"));
    }

    let target = extract_rpc_target(&http_req.path).ok_or_else(|| {
        ServerError::BadRequest(format!(
            "expected path /rpc/<resource>.<operation>, got {}",
            http_req.path
        ))
    })?;

    let body: Value = if http_req.body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&http_req.body)
            .map_err(|e| ServerError::BadRequest(format!("invalid JSON: {e}")))?
    };

    let mut rpc_req = RpcRequest::new(target, body);
    for (k, v) in &http_req.headers {
        rpc_req.headers.insert(k.clone(), v.clone());
    }
    if let Some(cookie) = http_req.headers.get("cookie") {
        rpc_req.cookie_header = Some(cookie.clone());
    }
    // Convenience: if CSRF header missing but present under canonical name, already in map.
    let _ = CSRF_HEADER_NAME;

    let rpc_resp = {
        let mut rpc = rpc
            .lock()
            .map_err(|_| ServerError::msg("rpc lock poisoned"))?;
        let mut exec = exec
            .lock()
            .map_err(|_| ServerError::msg("exec lock poisoned"))?;
        rpc.handle(&rpc_req, exec.as_mut())
    };

    Ok(rpc_to_http(rpc_resp))
}

fn extract_rpc_target(path: &str) -> Option<String> {
    let path = path.split('?').next().unwrap_or(path);
    let path = path.trim_start_matches('/');
    let rest = path.strip_prefix("rpc/")?;
    if rest.is_empty() || !rest.contains('.') {
        return None;
    }
    Some(rest.to_string())
}

fn rpc_to_http(resp: RpcResponse) -> HttpResponse {
    let status = resp
        .error
        .as_ref()
        .map(|e| e.status)
        .unwrap_or(200);
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Error",
    };
    let body = serde_json::to_value(&resp).unwrap_or_else(|_| json!({ "ok": false }));
    let mut http = HttpResponse::json(status, reason, &body);
    http.set_cookies = resp.set_cookies;
    // Recalculate content-length after cookies don't affect body
    http
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rpc_path() {
        assert_eq!(
            extract_rpc_target("/rpc/todo.list").as_deref(),
            Some("todo.list")
        );
        assert_eq!(extract_rpc_target("/health"), None);
    }
}

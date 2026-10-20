//! Async bind + accept loop and single-exchange dispatch onto `RpcServer`.

use crate::dev::{inject_dev_overlay_script, DevHandles, DevState, DEV_OVERLAY_JS};
use crate::error::{ServerError, ServerResult};
use crate::http::{
    parse_http_request, write_http_response, HttpRequest, HttpResponse,
};
use puddly_actions::ActionRunner;
use puddly_auth::SessionStore;
use puddly_core::{CSRF_HEADER_NAME, DISPLAY_NAME};
use puddly_db::{Executor, PgPool};
use puddly_rpc::{RpcRequest, RpcResponse, RpcServer};
use puddly_views::{demo_todo_props, render_page, ViewBundle, ISLANDS_RUNTIME_JS};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

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

/// Async accept loop. Check out a pool connection per RPC request (no executor mutex).
/// View / health / island-runtime GETs skip the pool. `RpcServer` remains behind a mutex
/// for in-memory session state.
pub async fn serve<S, R>(
    config: &ServerConfig,
    rpc: Arc<Mutex<RpcServer<S, R>>>,
    pool: PgPool,
    views: Arc<ViewBundle>,
) -> ServerResult<()>
where
    S: SessionStore + Send + 'static,
    R: ActionRunner + Send + 'static,
{
    let listener = TcpListener::bind(&config.bind).await?;
    eprintln!(
        "{DISPLAY_NAME} server listening on http://{}",
        config.bind
    );
    loop {
        let (stream, _) = listener.accept().await?;
        let rpc = Arc::clone(&rpc);
        let pool = pool.clone();
        let views = Arc::clone(&views);
        tokio::spawn(async move {
            if let Err(e) = handle_stream(stream, &rpc, &pool, &views).await {
                eprintln!("request error: {e}");
            }
        });
    }
}

/// Dev accept loop: hot-swappable views, optional RPC pool, `/__dev` panel.
pub async fn serve_dev<S, R>(
    config: &ServerConfig,
    handles: Arc<DevHandles<S, R>>,
    pool: Option<PgPool>,
) -> ServerResult<()>
where
    S: SessionStore + 'static,
    R: ActionRunner + 'static,
{
    let listener = TcpListener::bind(&config.bind).await?;
    let mode = if pool.is_some() {
        "full (RPC + views)"
    } else {
        "view-only (no DATABASE_URL)"
    };
    eprintln!(
        "{DISPLAY_NAME} dev listening on http://{} ({mode})",
        config.bind
    );
    loop {
        let (stream, _) = listener.accept().await?;
        let handles = Arc::clone(&handles);
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream_dev(stream, &handles, pool.as_ref()).await {
                eprintln!("request error: {e}");
            }
        });
    }
}

/// Handle a single already-accepted stream (tests / one-shot) with a checked-out executor.
pub async fn serve_once<S, R>(
    stream: &mut TcpStream,
    rpc: &Mutex<RpcServer<S, R>>,
    exec: &mut dyn Executor,
    views: Option<&ViewBundle>,
) -> ServerResult<()>
where
    S: SessionStore,
    R: ActionRunner,
{
    let http_req = parse_http_request(stream).await?;
    let http_resp = dispatch_http(&http_req, rpc, exec, views).await?;
    write_http_response(stream, &http_resp).await
}

async fn handle_stream<S, R>(
    mut stream: TcpStream,
    rpc: &Mutex<RpcServer<S, R>>,
    pool: &PgPool,
    views: &ViewBundle,
) -> ServerResult<()>
where
    S: SessionStore,
    R: ActionRunner,
{
    let http_req = parse_http_request(&mut stream).await?;

    // Health / views / island runtime never touch Postgres.
    if let Some(http_resp) = try_dispatch_get(&http_req, Some(views))? {
        return write_http_response(&mut stream, &http_resp).await;
    }

    let mut exec = pool
        .get()
        .await
        .map_err(|e| ServerError::msg(e.to_string()))?;
    let http_resp = dispatch_http(&http_req, rpc, &mut exec, Some(views)).await?;
    write_http_response(&mut stream, &http_resp).await
}

async fn handle_stream_dev<S, R>(
    mut stream: TcpStream,
    handles: &DevHandles<S, R>,
    pool: Option<&PgPool>,
) -> ServerResult<()>
where
    S: SessionStore,
    R: ActionRunner,
{
    let http_req = parse_http_request(&mut stream).await?;

    if let Some(http_resp) = try_dispatch_get_dev(&http_req, handles).await? {
        return write_http_response(&mut stream, &http_resp).await;
    }

    // RPC requires pool + RpcServer.
    let (Some(pool), Some(rpc)) = (pool, handles.rpc.as_ref()) else {
        let resp = HttpResponse::json(
            503,
            "Service Unavailable",
            &json!({
                "ok": false,
                "error": "RPC unavailable in view-only `puddly dev` (set DATABASE_URL)"
            }),
        );
        return write_http_response(&mut stream, &resp).await;
    };

    let mut exec = pool
        .get()
        .await
        .map_err(|e| ServerError::msg(e.to_string()))?;
    let http_resp = dispatch_http(&http_req, rpc, &mut exec, None).await?;
    write_http_response(&mut stream, &http_resp).await
}

/// Handle GET routes that do not need an [`Executor`]. Returns `None` if the request
/// should fall through to RPC (POST) dispatch.
pub fn try_dispatch_get(
    http_req: &HttpRequest,
    views: Option<&ViewBundle>,
) -> ServerResult<Option<HttpResponse>> {
    try_dispatch_get_inner(http_req, views, false)
}

/// Dev GET dispatch: `/__dev`, overlay script, hot views + error overlay injection.
pub async fn try_dispatch_get_dev<S, R>(
    http_req: &HttpRequest,
    handles: &DevHandles<S, R>,
) -> ServerResult<Option<HttpResponse>>
where
    S: SessionStore,
    R: ActionRunner,
{
    if !http_req.method.eq_ignore_ascii_case("GET") {
        return Ok(None);
    }
    let path = http_req.path.split('?').next().unwrap_or(&http_req.path);

    if path == "/__dev" {
        let st = handles.state.read().await;
        return Ok(Some(HttpResponse::json(200, "OK", &serde_json::to_value(st.status()).unwrap_or_else(|_| json!({ "ok": false })))));
    }
    if path == "/__dev/overlay.js" {
        return Ok(Some(HttpResponse::js(200, "OK", DEV_OVERLAY_JS)));
    }

    let views = handles.views.read().await;
    try_dispatch_get_inner(http_req, Some(&views), true)
}

fn try_dispatch_get_inner(
    http_req: &HttpRequest,
    views: Option<&ViewBundle>,
    inject_overlay: bool,
) -> ServerResult<Option<HttpResponse>> {
    if !http_req.method.eq_ignore_ascii_case("GET") {
        return Ok(None);
    }
    let path = http_req.path.split('?').next().unwrap_or(&http_req.path);

    if path == "/health" || path == "/healthz" {
        return Ok(Some(HttpResponse::json(200, "OK", &json!({ "ok": true }))));
    }
    if path == "/puddly-islands.js" {
        return Ok(Some(HttpResponse::js(200, "OK", ISLANDS_RUNTIME_JS)));
    }
    if let Some(bundle) = views {
        if path == "/" {
            let name = bundle.default_view_name().unwrap_or("todo");
            return Ok(Some(render_view_response(bundle, name, inject_overlay)?));
        }
        if let Some(name) = path.strip_prefix("/v/") {
            if !name.is_empty() && !name.contains('/') {
                return Ok(Some(render_view_response(bundle, name, inject_overlay)?));
            }
        }
    }
    // Unknown GET
    Ok(Some(HttpResponse::text(404, "Not Found", "not found")))
}

/// Build `/__dev` JSON from a [`DevState`] (unit-test helper; no async).
pub fn dev_status_response(state: &DevState) -> HttpResponse {
    let value = serde_json::to_value(state.status()).unwrap_or_else(|_| json!({ "ok": false }));
    HttpResponse::json(200, "OK", &value)
}

/// Async dispatch used by the accept loop and unit tests.
/// Caller supplies an already-checked-out (or in-memory) [`Executor`].
/// `views` enables SSR routes (`GET /`, `GET /v/<name>`, island runtime).
pub async fn dispatch_http<S, R>(
    http_req: &HttpRequest,
    rpc: &Mutex<RpcServer<S, R>>,
    exec: &mut dyn Executor,
    views: Option<&ViewBundle>,
) -> ServerResult<HttpResponse>
where
    S: SessionStore,
    R: ActionRunner,
{
    if let Some(resp) = try_dispatch_get(http_req, views)? {
        return Ok(resp);
    }

    if !http_req.method.eq_ignore_ascii_case("POST") {
        return Ok(HttpResponse::text(405, "Method Not Allowed", "POST only for RPC"));
    }

    let path = http_req.path.split('?').next().unwrap_or(&http_req.path);
    let target = extract_rpc_target(path).ok_or_else(|| {
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
    let _ = CSRF_HEADER_NAME;

    let rpc_resp = {
        let mut rpc = rpc.lock().await;
        rpc.handle(&rpc_req, exec).await
    };

    Ok(rpc_to_http(rpc_resp))
}

/// Dev-aware dispatch for tests (GET `/__dev` + views with overlay; optional RPC).
pub async fn dispatch_http_dev<S, R>(
    http_req: &HttpRequest,
    handles: &DevHandles<S, R>,
    exec: Option<&mut dyn Executor>,
) -> ServerResult<HttpResponse>
where
    S: SessionStore,
    R: ActionRunner,
{
    if let Some(resp) = try_dispatch_get_dev(http_req, handles).await? {
        return Ok(resp);
    }

    if !http_req.method.eq_ignore_ascii_case("POST") {
        return Ok(HttpResponse::text(405, "Method Not Allowed", "POST only for RPC"));
    }

    let (Some(rpc), Some(exec)) = (handles.rpc.as_ref(), exec) else {
        return Ok(HttpResponse::json(
            503,
            "Service Unavailable",
            &json!({
                "ok": false,
                "error": "RPC unavailable in view-only mode"
            }),
        ));
    };

    let path = http_req.path.split('?').next().unwrap_or(&http_req.path);
    let target = extract_rpc_target(path).ok_or_else(|| {
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

    let rpc_resp = {
        let mut rpc = rpc.lock().await;
        rpc.handle(&rpc_req, exec).await
    };
    Ok(rpc_to_http(rpc_resp))
}

fn render_view_response(
    bundle: &ViewBundle,
    name: &str,
    inject_overlay: bool,
) -> ServerResult<HttpResponse> {
    let props = demo_todo_props();
    match render_page(bundle, name, &props) {
        Ok(html) => {
            let html = if inject_overlay {
                inject_dev_overlay_script(&html)
            } else {
                html
            };
            Ok(HttpResponse::html(200, "OK", html))
        }
        Err(puddly_views::ViewError::UnknownView { name }) => Ok(HttpResponse::text(
            404,
            "Not Found",
            format!("unknown view `{name}`"),
        )),
        Err(e) => Err(ServerError::msg(e.to_string())),
    }
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

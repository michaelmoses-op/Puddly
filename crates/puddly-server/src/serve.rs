//! Async bind + accept loop and single-exchange dispatch onto `RpcServer`.

use crate::dev::{inject_dev_overlay_script, DevHandles, DevState, DEV_OVERLAY_JS};
use crate::error::{ServerError, ServerResult};
use crate::http::{
    parse_http_request, write_http_response, HttpRequest, HttpResponse,
};
use crate::view_load::{
    resolve_load, resolve_static_load, unauthorized_html, unavailable_html, LoadOutcome,
};
use puddly_actions::ActionRunner;
use puddly_auth::SessionStore;
use puddly_core::{CSRF_HEADER_NAME, DISPLAY_NAME};
use puddly_db::{Executor, PgPool};
use puddly_rpc::{RpcRequest, RpcResponse, RpcServer};
use puddly_views::{render_page, ViewBundle, ISLANDS_RUNTIME_JS};
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

/// Async accept loop. Check out a pool connection per RPC / view-load request.
/// Health / island-runtime GETs skip the pool. `RpcServer` remains behind a mutex
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

    if let Some(http_resp) = try_dispatch_static_get(&http_req)? {
        return write_http_response(&mut stream, &http_resp).await;
    }

    if let Some(view_name) = view_route_name(&http_req, Some(views)) {
        let needs_db = views
            .views
            .get(&view_name)
            .map(|m| m.load.needs_db())
            .unwrap_or(false);
        let http_resp = if needs_db {
            let mut exec = pool
                .get()
                .await
                .map_err(|e| ServerError::msg(e.to_string()))?;
            render_view_response(
                views,
                &view_name,
                &http_req,
                false,
                Some(rpc),
                Some(&mut exec),
            )
            .await?
        } else {
            render_view_response(views, &view_name, &http_req, false, Some(rpc), None).await?
        };
        return write_http_response(&mut stream, &http_resp).await;
    }

    let mut exec = pool
        .get()
        .await
        .map_err(|e| ServerError::msg(e.to_string()))?;
    let http_resp = dispatch_http(&http_req, rpc, &mut exec, None).await?;
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

    if let Some(http_resp) = try_dispatch_dev_meta(&http_req, handles).await? {
        return write_http_response(&mut stream, &http_resp).await;
    }

    if let Some(http_resp) = try_dispatch_static_get(&http_req)? {
        return write_http_response(&mut stream, &http_resp).await;
    }

    {
        let views = handles.views.read().await;
        if let Some(view_name) = view_route_name(&http_req, Some(&views)) {
            let needs_db = views
                .views
                .get(&view_name)
                .map(|m| m.load.needs_db())
                .unwrap_or(false);
            let http_resp = if needs_db {
                match (pool, handles.rpc.as_ref()) {
                    (Some(pool), Some(rpc)) => {
                        let mut exec = pool
                            .get()
                            .await
                            .map_err(|e| ServerError::msg(e.to_string()))?;
                        render_view_response(
                            &views,
                            &view_name,
                            &http_req,
                            true,
                            Some(rpc),
                            Some(&mut exec),
                        )
                        .await?
                    }
                    _ => {
                        render_view_response(
                            &views,
                            &view_name,
                            &http_req,
                            true,
                            handles.rpc.as_ref(),
                            None,
                        )
                        .await?
                    }
                }
            } else {
                render_view_response(
                    &views,
                    &view_name,
                    &http_req,
                    true,
                    handles.rpc.as_ref(),
                    None,
                )
                .await?
            };
            return write_http_response(&mut stream, &http_resp).await;
        }
    }

    // Unknown GET
    if http_req.method.eq_ignore_ascii_case("GET") {
        return write_http_response(
            &mut stream,
            &HttpResponse::text(404, "Not Found", "not found"),
        )
        .await;
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

/// Handle GET routes that do not need an [`Executor`] or view load (health / islands JS).
/// View SSR is async — use [`dispatch_http`] / [`dispatch_http_dev`] instead.
pub fn try_dispatch_get(
    http_req: &HttpRequest,
    views: Option<&ViewBundle>,
) -> ServerResult<Option<HttpResponse>> {
    let _ = views;
    try_dispatch_static_get(http_req)
}

fn try_dispatch_static_get(http_req: &HttpRequest) -> ServerResult<Option<HttpResponse>> {
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
    Ok(None)
}

async fn try_dispatch_dev_meta<S, R>(
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
        return Ok(Some(HttpResponse::json(
            200,
            "OK",
            &serde_json::to_value(st.status()).unwrap_or_else(|_| json!({ "ok": false })),
        )));
    }
    if path == "/__dev/overlay.js" {
        return Ok(Some(HttpResponse::js(200, "OK", DEV_OVERLAY_JS)));
    }
    Ok(None)
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
    if let Some(resp) = try_dispatch_dev_meta(http_req, handles).await? {
        return Ok(Some(resp));
    }
    if let Some(resp) = try_dispatch_static_get(http_req)? {
        return Ok(Some(resp));
    }

    let views = handles.views.read().await;
    let Some(view_name) = view_route_name(http_req, Some(&views)) else {
        if http_req.method.eq_ignore_ascii_case("GET") {
            return Ok(Some(HttpResponse::text(404, "Not Found", "not found")));
        }
        return Ok(None);
    };

    // No pool on this helper — DB loads → 503. Tests that need data use dispatch_http_dev + exec.
    render_view_response(
        &views,
        &view_name,
        http_req,
        true,
        handles.rpc.as_ref(),
        None,
    )
    .await
    .map(Some)
}

fn view_route_name(http_req: &HttpRequest, views: Option<&ViewBundle>) -> Option<String> {
    if !http_req.method.eq_ignore_ascii_case("GET") {
        return None;
    }
    let bundle = views?;
    let path = http_req.path.split('?').next().unwrap_or(&http_req.path);
    if path == "/" {
        return Some(bundle.default_view_name().unwrap_or("todo").to_string());
    }
    // Convenience aliases (ADR-035): `/login` → login view, `/register` → register.
    if path == "/login" && bundle.views.contains_key("login") {
        return Some("login".to_string());
    }
    if path == "/register" && bundle.views.contains_key("register") {
        return Some("register".to_string());
    }
    if let Some(name) = path.strip_prefix("/v/") {
        if !name.is_empty() && !name.contains('/') {
            return Some(name.to_string());
        }
    }
    None
}

/// Build `/__dev` JSON from a [`DevState`] (unit-test helper; no async).
pub fn dev_status_response(state: &DevState) -> HttpResponse {
    let value = serde_json::to_value(state.status()).unwrap_or_else(|_| json!({ "ok": false }));
    HttpResponse::json(200, "OK", &value)
}

/// Async dispatch used by the accept loop and unit tests.
/// Caller supplies an already-checked-out (or in-memory) [`Executor`].
/// `views` enables SSR routes (`GET /`, `GET /v/<name>`, plus `/login`/`/register` aliases).
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
    if let Some(resp) = try_dispatch_static_get(http_req)? {
        return Ok(resp);
    }

    if let Some(view_name) = view_route_name(http_req, views) {
        let bundle = views.expect("view_route_name implies views");
        return render_view_response(bundle, &view_name, http_req, false, Some(rpc), Some(exec))
            .await;
    }

    if http_req.method.eq_ignore_ascii_case("GET") {
        return Ok(HttpResponse::text(404, "Not Found", "not found"));
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
    mut exec: Option<&mut dyn Executor>,
) -> ServerResult<HttpResponse>
where
    S: SessionStore,
    R: ActionRunner,
{
    if let Some(resp) = try_dispatch_dev_meta(http_req, handles).await? {
        return Ok(resp);
    }
    if let Some(resp) = try_dispatch_static_get(http_req)? {
        return Ok(resp);
    }

    {
        let views = handles.views.read().await;
        if let Some(view_name) = view_route_name(http_req, Some(&views)) {
            return render_view_response(
                &views,
                &view_name,
                http_req,
                true,
                handles.rpc.as_ref(),
                exec.take(),
            )
            .await;
        }
    }

    if http_req.method.eq_ignore_ascii_case("GET") {
        return Ok(HttpResponse::text(404, "Not Found", "not found"));
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

async fn render_view_response<S, R>(
    bundle: &ViewBundle,
    name: &str,
    http_req: &HttpRequest,
    inject_overlay: bool,
    rpc: Option<&Mutex<RpcServer<S, R>>>,
    exec: Option<&mut dyn Executor>,
) -> ServerResult<HttpResponse>
where
    S: SessionStore,
    R: ActionRunner,
{
    let Some(module) = bundle.views.get(name) else {
        return Ok(HttpResponse::text(
            404,
            "Not Found",
            format!("unknown view `{name}`"),
        ));
    };

    let query = http_req.path.split('?').nth(1);
    let cookie = http_req.headers.get("cookie").map(String::as_str);

    let outcome = if !module.load.needs_db() {
        resolve_static_load(&module.load)
    } else if let (Some(rpc), Some(exec)) = (rpc, exec) {
        let mut rpc = rpc.lock().await;
        resolve_load(&module.load, query, cookie, Some(&mut *rpc), Some(exec)).await
    } else {
        resolve_static_load(&module.load) // → Unavailable when needs_db
    };

    let maybe_overlay = |html: String| -> String {
        if inject_overlay {
            inject_dev_overlay_script(&html)
        } else {
            html
        }
    };

    match outcome {
        LoadOutcome::Props(props) => match render_page(bundle, name, &props) {
            Ok(html) => Ok(HttpResponse::html(200, "OK", maybe_overlay(html))),
            Err(puddly_views::ViewError::UnknownView { name }) => Ok(HttpResponse::text(
                404,
                "Not Found",
                format!("unknown view `{name}`"),
            )),
            Err(e) => Err(ServerError::msg(e.to_string())),
        },
        LoadOutcome::Unauthorized => Ok(HttpResponse::html(
            401,
            "Unauthorized",
            maybe_overlay(unauthorized_html()),
        )),
        LoadOutcome::Unavailable(msg) => Ok(HttpResponse::html(
            503,
            "Service Unavailable",
            maybe_overlay(unavailable_html(&msg)),
        )),
        LoadOutcome::BadRequest(msg) => Ok(HttpResponse::text(400, "Bad Request", msg)),
        LoadOutcome::Error(msg) => Err(ServerError::msg(msg)),
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

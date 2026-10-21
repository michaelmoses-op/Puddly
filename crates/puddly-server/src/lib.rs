//! Minimal **async** HTTP server over [`puddly_rpc::RpcServer`] (ADR-028).
//!
//! Hand-rolled HTTP/1.1 parser on `tokio::net::TcpListener` / AsyncRead/Write.
//! Concurrent accepts spawn one task per connection; each request checks out a
//! pool connection (no global `Mutex` on the executor).
//!
//! Step 12 adds [`serve_dev`] / [`DevState`] for `puddly dev` (ADR-030).
//! Step 22 adds SSE / `live: true` via [`SseHub`] + `GET /events` (ADR-040).
//! Step 23 adds graceful SSE drain, `/ready`, env validation, `/metrics` (ADR-041).
//! Step 25 adds multi-instance SSE via Postgres LISTEN/NOTIFY (ADR-043 / ADR-010).

mod dev;
mod env;
mod error;
mod http;
mod live_fanout;
mod metrics;
mod serve;
mod sse;
mod view_load;
mod watch;

pub use dev::{
    apply_changes, classify_watch_path, errors_from_check, hot_swap_views,
    inject_dev_overlay_script, reload_actions, reload_resources, ChangeKind, DevError,
    DevHandles, DevState, DevStatus, ReloadOutcome, DEV_DEBOUNCE_MS, DEV_OVERLAY_JS,
};
pub use env::{resolve_serve_env, validate_database_url, DeployEnv, ServeEnv};
pub use error::{ServerError, ServerResult};
pub use http::{
    parse_http_request, parse_http_request_bytes, write_http_response, write_sse_headers,
    HttpRequest, HttpResponse,
};
pub use metrics::Metrics;
pub use serve::{
    dev_status_response, dispatch_http, dispatch_http_dev, readiness_from_pool, serve,
    serve_dev, serve_once, serve_once_with_hub, serve_with_hub, serve_with_hub_metrics,
    stream_sse, try_dispatch_get, try_dispatch_get_dev, ServerConfig, DEFAULT_DRAIN_TIMEOUT,
};
pub use live_fanout::{
    apply_remote_payload, extract_resource_id, extract_row_version, fetch_row_for_live,
    spawn_fanout_task, LiveBridge, LiveNotifyPayload, LIVE_NOTIFY_SCHEMA_V,
};
pub use sse::{
    event_from_rpc, SseConnectionGuard, SseEvent, SseHub, SSE_KEEPALIVE, SSE_SHUTDOWN_FRAME,
};
pub use watch::watch_dev_loop;

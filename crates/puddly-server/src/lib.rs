//! Minimal **async** HTTP server over [`puddly_rpc::RpcServer`] (ADR-028).
//!
//! Hand-rolled HTTP/1.1 parser on `tokio::net::TcpListener` / AsyncRead/Write.
//! Concurrent accepts spawn one task per connection; each request checks out a
//! pool connection (no global `Mutex` on the executor).
//!
//! Step 12 adds [`serve_dev`] / [`DevState`] for `puddly dev` (ADR-030).

mod dev;
mod error;
mod http;
mod serve;
mod watch;

pub use dev::{
    apply_changes, classify_watch_path, errors_from_check, hot_swap_views,
    inject_dev_overlay_script, reload_actions, reload_resources, ChangeKind, DevError,
    DevHandles, DevState, DevStatus, ReloadOutcome, DEV_DEBOUNCE_MS, DEV_OVERLAY_JS,
};
pub use error::{ServerError, ServerResult};
pub use http::{
    parse_http_request, parse_http_request_bytes, write_http_response, HttpRequest, HttpResponse,
};
pub use serve::{
    dev_status_response, dispatch_http, dispatch_http_dev, serve, serve_dev, serve_once,
    try_dispatch_get, try_dispatch_get_dev, ServerConfig,
};
pub use watch::watch_dev_loop;

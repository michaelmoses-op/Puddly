//! Minimal **sync** HTTP server over [`puddly_rpc::RpcServer`] (ADR-024).
//!
//! Intentionally uses `std::net` + a tiny HTTP/1.1 parser — no Tokio/axum yet —
//! so the Step 4 sync `postgres` executor and stub pool stay coherent.

mod error;
mod http;
mod serve;

pub use error::{ServerError, ServerResult};
pub use http::{
    parse_http_request, parse_http_request_bytes, write_http_response, HttpRequest, HttpResponse,
};
pub use serve::{dispatch_http, serve, serve_once, ServerConfig};

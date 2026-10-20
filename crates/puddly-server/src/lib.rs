//! Minimal **async** HTTP server over [`puddly_rpc::RpcServer`] (ADR-028).
//!
//! Hand-rolled HTTP/1.1 parser on `tokio::net::TcpListener` / AsyncRead/Write.
//! Concurrent accepts spawn one task per connection; each request checks out a
//! pool connection (no global `Mutex` on the executor).

mod error;
mod http;
mod serve;

pub use error::{ServerError, ServerResult};
pub use http::{
    parse_http_request, parse_http_request_bytes, write_http_response, HttpRequest, HttpResponse,
};
pub use serve::{dispatch_http, serve, serve_once, ServerConfig};

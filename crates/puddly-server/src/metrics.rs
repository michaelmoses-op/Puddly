//! Minimal structured metrics for production serve (ADR-041 / Step 23).
//!
//! JSON at `GET /metrics` — not Prometheus exposition (keep the surface small).

use crate::sse::SseHub;
use puddly_db::{PgPool, PoolStatus};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Process-wide counters shared across accept tasks.
#[derive(Debug, Default)]
pub struct Metrics {
    pub http_requests: AtomicU64,
    pub rpc_ok: AtomicU64,
    pub rpc_err: AtomicU64,
    pub sse_connects: AtomicU64,
    pub sse_rejected: AtomicU64,
}

impl Metrics {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn inc_http(&self) {
        self.http_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rpc_ok(&self) {
        self.rpc_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_rpc_err(&self) {
        self.rpc_err.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_sse_connect(&self) {
        self.sse_connects.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_sse_rejected(&self) {
        self.sse_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot for `/metrics`.
    pub fn snapshot(&self, hub: Option<&SseHub>, pool: Option<&PgPool>) -> Value {
        let pool_json = pool.map(|p| {
            let s: PoolStatus = p.status();
            json!({
                "size": s.size,
                "available": s.available,
                "max_size": s.max_size,
            })
        });
        json!({
            "ok": true,
            "http_requests": self.http_requests.load(Ordering::Relaxed),
            "rpc_ok": self.rpc_ok.load(Ordering::Relaxed),
            "rpc_err": self.rpc_err.load(Ordering::Relaxed),
            "sse_connects": self.sse_connects.load(Ordering::Relaxed),
            "sse_rejected": self.sse_rejected.load(Ordering::Relaxed),
            "sse_active": hub.map(|h| h.active_connections()).unwrap_or(0),
            "sse_shutting_down": hub.map(|h| h.is_shutting_down()).unwrap_or(false),
            "pool": pool_json,
        })
    }
}

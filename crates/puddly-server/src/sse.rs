//! In-process SSE hub for realtime push (ADR-040 / Step 22).
//!
//! Single-instance only: [`tokio::sync::broadcast`] of JSON events. Multi-instance
//! fan-out via Postgres `LISTEN/NOTIFY` is deferred (extends ADR-010).
//!
//! Step 23 adds graceful drain: [`SseHub::begin_shutdown`] stops new streams and
//! waits for active writers to finish (ADR-041).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Notify};

/// Default broadcast capacity (lagging subscribers drop oldest).
const DEFAULT_CAPACITY: usize = 256;

/// SSE comment frame sent when the hub begins draining.
pub const SSE_SHUTDOWN_FRAME: &str = ": puddly-shutdown\n\n";

/// One realtime event published after a successful mutating RPC.
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    /// e.g. `todo.updated`
    pub type_name: String,
    pub resource: String,
    pub op: String,
    pub data: Value,
}

impl SseEvent {
    pub fn new(
        resource: impl Into<String>,
        op: impl Into<String>,
        data: Value,
    ) -> Self {
        let resource = resource.into();
        let op = op.into();
        let past = past_tense(&op);
        let type_name = format!("{resource}.{past}");
        Self {
            type_name,
            resource,
            op,
            data,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "type": self.type_name,
            "resource": self.resource,
            "op": self.op,
            "data": self.data,
        })
    }

    /// SSE wire frame: `data: <json>\n\n`
    pub fn to_sse_frame(&self) -> String {
        let payload = serde_json::to_string(&self.to_json()).unwrap_or_else(|_| "{}".into());
        format!("data: {payload}\n\n")
    }
}

fn past_tense(op: &str) -> &'static str {
    match op {
        "create" => "created",
        "update" => "updated",
        "delete" => "deleted",
        "archive" => "archived",
        _ => "changed",
    }
}

/// Keep-alive comment frame (SSE comment lines start with `:`).
pub const SSE_KEEPALIVE: &str = ": keep-alive\n\n";

/// Shared in-process broadcast hub with active-connection tracking for drain.
#[derive(Debug)]
pub struct SseHub {
    tx: broadcast::Sender<String>,
    /// Live [`crate::serve::stream_sse`] writers (not bare `subscribe()` receivers).
    active: AtomicUsize,
    shutting_down: AtomicBool,
    notify: Notify,
}

impl SseHub {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self {
            tx,
            active: AtomicUsize::new(0),
            shutting_down: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Publish a framed SSE event to all subscribers. Returns subscriber count
    /// that received the send (0 if nobody is listening — still Ok).
    pub fn publish(&self, event: &SseEvent) -> usize {
        self.publish_frame(event.to_sse_frame())
    }

    /// Publish a raw SSE frame (tests / keep-alive injection helpers).
    pub fn publish_frame(&self, frame: impl Into<String>) -> usize {
        match self.tx.send(frame.into()) {
            Ok(n) => n,
            Err(_) => 0, // no receivers
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Broadcast receiver count (includes lagging subscribers). Prefer
    /// [`Self::active_connections`] for drain.
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Number of active SSE write loops tracked by [`Self::connection_guard`].
    pub fn active_connections(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// Mark the hub draining: reject new streams, notify writers, send a
    /// shutdown comment so clients see a clean end.
    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let _ = self.publish_frame(SSE_SHUTDOWN_FRAME);
        self.notify.notify_waiters();
    }

    /// Wait until [`Self::active_connections`] is zero or `timeout` elapses.
    /// Returns `true` if fully drained.
    pub async fn wait_drained(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.active_connections() == 0 {
                return true;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return self.active_connections() == 0;
            }
            tokio::select! {
                _ = self.notify.notified() => {}
                _ = tokio::time::sleep(remaining) => {
                    return self.active_connections() == 0;
                }
            }
        }
    }

    /// Resolves when [`Self::begin_shutdown`] has been called (or was already).
    pub async fn wait_shutdown(&self) {
        let notified = self.notify.notified();
        if self.is_shutting_down() {
            return;
        }
        notified.await;
    }

    /// Track one live SSE writer. Returns `None` if the hub is shutting down.
    pub fn try_acquire(self: &Arc<Self>) -> Option<SseConnectionGuard> {
        if self.is_shutting_down() {
            return None;
        }
        self.active.fetch_add(1, Ordering::SeqCst);
        // Re-check after increment to close a narrow race with begin_shutdown.
        if self.is_shutting_down() {
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.notify.notify_waiters();
            return None;
        }
        Some(SseConnectionGuard {
            hub: Arc::clone(self),
        })
    }
}

impl Default for SseHub {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII counter for one SSE write loop.
#[derive(Debug)]
pub struct SseConnectionGuard {
    hub: Arc<SseHub>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.hub.active.fetch_sub(1, Ordering::SeqCst);
        self.hub.notify.notify_waiters();
    }
}

/// Map a successful RPC target + response data into an optional live event.
/// Only mutating ops: create / update / delete / archive.
pub fn event_from_rpc(target: &str, data: &Value) -> Option<SseEvent> {
    let (resource, op) = target.split_once('.')?;
    if !matches!(op, "create" | "update" | "delete" | "archive") {
        return None;
    }
    Some(SseEvent::new(resource, op, data.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hub_publish_subscribe() {
        let hub = SseHub::new();
        let mut rx = hub.subscribe();
        let event = SseEvent::new("todo", "update", json!({ "id": "abc", "done": true }));
        assert_eq!(event.type_name, "todo.updated");
        let n = hub.publish(&event);
        assert_eq!(n, 1);
        let frame = rx.recv().await.expect("frame");
        assert!(frame.starts_with("data: "));
        assert!(frame.contains("todo.updated"));
        assert!(frame.contains("\"op\":\"update\""));
        assert!(frame.ends_with("\n\n"));
    }

    #[test]
    fn event_from_rpc_filters_reads() {
        assert!(event_from_rpc("todo.list", &json!({})).is_none());
        assert!(event_from_rpc("todo.get", &json!({})).is_none());
        let ev = event_from_rpc("todo.create", &json!({ "id": "1" })).unwrap();
        assert_eq!(ev.type_name, "todo.created");
        let ev = event_from_rpc("todo.archive", &json!({ "archived": "1" })).unwrap();
        assert_eq!(ev.type_name, "todo.archived");
    }

    #[tokio::test]
    async fn drain_rejects_new_and_waits() {
        let hub = SseHub::shared();
        let guard = hub.try_acquire().expect("open");
        assert_eq!(hub.active_connections(), 1);
        hub.begin_shutdown();
        assert!(hub.is_shutting_down());
        assert!(hub.try_acquire().is_none());
        drop(guard);
        assert!(hub.wait_drained(Duration::from_secs(1)).await);
        assert_eq!(hub.active_connections(), 0);
    }
}

//! In-process SSE hub for realtime push (ADR-040 / Step 22).
//!
//! Single-instance only: [`tokio::sync::broadcast`] of JSON events. Multi-instance
//! fan-out via Postgres `LISTEN/NOTIFY` is deferred (extends ADR-010).

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Default broadcast capacity (lagging subscribers drop oldest).
const DEFAULT_CAPACITY: usize = 256;

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

/// Shared in-process broadcast hub.
#[derive(Debug, Clone)]
pub struct SseHub {
    tx: broadcast::Sender<String>,
}

impl SseHub {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self { tx }
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

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for SseHub {
    fn default() -> Self {
        Self::new()
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
}

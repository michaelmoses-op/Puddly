//! Multi-instance SSE fan-out via Postgres LISTEN/NOTIFY (ADR-010 / ADR-043 / Step 25).
//!
//! Local mutations still publish rich SSE via [`crate::SseHub`]. Remotes receive a
//! compact NOTIFY (`resource` + `id` + optional `version`), skip self-origin echoes,
//! fetch the row (payload size limits), then publish to their local hub.
//!
//! Single-instance: leave [`LiveBridge::enabled`] false — hub-only path unchanged.

use crate::sse::{SseEvent, SseHub};
use puddly_auth::generate_user_id;
use puddly_core::DISPLAY_NAME;
use puddly_db::{
    pg_notify, spawn_listen, DbRow, Executor, PgPool, SqlValue, PUDDLY_LIVE_CHANNEL,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Payload schema version for `puddly_live` NOTIFY JSON.
pub const LIVE_NOTIFY_SCHEMA_V: u32 = 1;

/// Compact NOTIFY body (well under Postgres ~8KB limit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveNotifyPayload {
    pub v: u32,
    pub resource: String,
    pub id: String,
    pub op: String,
    pub origin: String,
    /// Row version when available (`updatedAt` / `createdAt`); else omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl LiveNotifyPayload {
    pub fn new(
        resource: impl Into<String>,
        id: impl Into<String>,
        op: impl Into<String>,
        origin: impl Into<String>,
        version: Option<String>,
    ) -> Self {
        Self {
            v: LIVE_NOTIFY_SCHEMA_V,
            resource: resource.into(),
            id: id.into(),
            op: op.into(),
            origin: origin.into(),
            version,
        }
    }

    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn parse(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Per-process bridge: instance id + whether NOTIFY is active.
#[derive(Debug, Clone)]
pub struct LiveBridge {
    pub instance_id: String,
    pub enabled: bool,
    pub channel: String,
}

impl LiveBridge {
    pub fn disabled() -> Self {
        Self {
            instance_id: generate_user_id(),
            enabled: false,
            channel: PUDDLY_LIVE_CHANNEL.to_string(),
        }
    }

    pub fn enabled_new() -> Self {
        Self {
            instance_id: generate_user_id(),
            enabled: true,
            channel: PUDDLY_LIVE_CHANNEL.to_string(),
        }
    }

    pub fn shared(enabled: bool) -> Arc<Self> {
        Arc::new(if enabled {
            Self::enabled_new()
        } else {
            Self::disabled()
        })
    }

    /// After local hub publish: best-effort NOTIFY for other instances.
    pub async fn notify_after_local(
        &self,
        exec: &mut dyn Executor,
        event: &SseEvent,
        request_body: &Value,
    ) {
        if !self.enabled {
            return;
        }
        let Some(id) = extract_resource_id(&event.data, request_body) else {
            return;
        };
        let version = extract_row_version(&event.data);
        let payload = LiveNotifyPayload::new(
            &event.resource,
            id,
            &event.op,
            &self.instance_id,
            version,
        );
        let Ok(json) = payload.to_json_string() else {
            return;
        };
        if let Err(e) = pg_notify(exec, &self.channel, &json).await {
            eprintln!("{DISPLAY_NAME} live NOTIFY failed: {e}");
        }
    }
}

/// Extract resource id from RPC response data or request body.
pub fn extract_resource_id(data: &Value, body: &Value) -> Option<String> {
    json_id(data.get("id"))
        .or_else(|| json_id(body.get("id")))
        .or_else(|| {
            // archive / delete helpers sometimes nest
            data.get("archived")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
}

fn json_id(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Prefer `updatedAt`, then `createdAt`, as a soft row version string.
pub fn extract_row_version(data: &Value) -> Option<String> {
    data.get("updatedAt")
        .or_else(|| data.get("createdAt"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn quote_ident(name: &str) -> Option<String> {
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(format!("\"{name}\""))
}

fn sql_to_json(v: &SqlValue) -> Value {
    match v {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(b) => Value::Bool(*b),
        SqlValue::Int(i) => json!(i),
        SqlValue::Float(f) => json!(f),
        SqlValue::Text(s) | SqlValue::Uuid(s) => Value::String(s.clone()),
    }
}

fn db_row_to_json(row: &DbRow) -> Value {
    let mut map = Map::new();
    for (k, v) in &row.columns {
        map.insert(k.clone(), sql_to_json(v));
    }
    Value::Object(map)
}

/// Fetch one row by id for remote fan-out (no access gate — SSE is open today).
pub async fn fetch_row_for_live(
    exec: &mut dyn Executor,
    resource: &str,
    id: &str,
) -> Option<Value> {
    let table = quote_ident(resource)?;
    let sql = format!("SELECT * FROM {table} WHERE \"id\" = $1");
    let rows = exec
        .query(&sql, &[SqlValue::Uuid(id.to_string())])
        .await
        .ok()?;
    rows.first().map(db_row_to_json)
}

/// Apply a remote NOTIFY to the local hub (skips self-origin).
pub async fn apply_remote_payload(
    hub: &SseHub,
    bridge: &LiveBridge,
    payload: &LiveNotifyPayload,
    exec: &mut dyn Executor,
) {
    if payload.origin == bridge.instance_id {
        return;
    }
    if payload.v != LIVE_NOTIFY_SCHEMA_V {
        eprintln!(
            "{DISPLAY_NAME} live: ignoring notify schema v={}",
            payload.v
        );
        return;
    }

    let data = match payload.op.as_str() {
        "delete" | "archive" => {
            // Row may already be gone — invalidate by id.
            json!({ "id": payload.id })
        }
        _ => match fetch_row_for_live(exec, &payload.resource, &payload.id).await {
            Some(row) => row,
            None => json!({ "id": payload.id }),
        },
    };

    let event = SseEvent::new(&payload.resource, &payload.op, data);
    hub.publish(&event);
}

/// Spawn LISTEN → fetch → local hub publish. Returns join handle + keeps bridge.
pub fn spawn_fanout_task(
    database_url: String,
    hub: Arc<SseHub>,
    bridge: Arc<LiveBridge>,
    pool: PgPool,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (listen_join, mut rx) = match spawn_listen(&database_url, &bridge.channel).await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("{DISPLAY_NAME} live LISTEN not started: {e}");
                return;
            }
        };
        eprintln!(
            "{DISPLAY_NAME} live: LISTEN {} (instance {})",
            bridge.channel, bridge.instance_id
        );

        while let Some(note) = rx.recv().await {
            if note.channel != bridge.channel {
                continue;
            }
            let payload = match LiveNotifyPayload::parse(&note.payload) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{DISPLAY_NAME} live: bad NOTIFY payload: {e}");
                    continue;
                }
            };
            if payload.origin == bridge.instance_id {
                continue;
            }
            let mut exec = match pool.get().await {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("{DISPLAY_NAME} live: pool checkout failed: {e}");
                    continue;
                }
            };
            apply_remote_payload(&hub, &bridge, &payload, &mut exec).await;
        }

        listen_join.abort();
    })
}

/// Test helper: drive fan-out from an in-memory payload channel (no Postgres).
#[allow(dead_code)]
pub async fn drain_mem_notify(
    hub: &SseHub,
    bridge: &LiveBridge,
    rx: &mut mpsc::UnboundedReceiver<LiveNotifyPayload>,
    exec: &mut dyn Executor,
) {
    while let Ok(payload) = rx.try_recv() {
        apply_remote_payload(hub, bridge, &payload, exec).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use puddly_db::RecordingExecutor;

    #[test]
    fn payload_roundtrip() {
        let p = LiveNotifyPayload::new("todo", "abc", "update", "origin-1", Some("t1".into()));
        let s = p.to_json_string().unwrap();
        assert!(s.contains("\"v\":1"));
        assert!(s.len() < 500);
        let back = LiveNotifyPayload::parse(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn extract_id_from_body_for_delete() {
        let data = json!({ "deleted": true });
        let body = json!({ "id": "row-1" });
        assert_eq!(
            extract_resource_id(&data, &body).as_deref(),
            Some("row-1")
        );
    }

    #[test]
    fn extract_id_from_create_data() {
        let data = json!({ "created": true, "id": "new-1" });
        let body = json!({ "title": "x" });
        assert_eq!(
            extract_resource_id(&data, &body).as_deref(),
            Some("new-1")
        );
    }

    #[tokio::test]
    async fn skip_self_origin() {
        let hub = SseHub::new();
        let mut rx = hub.subscribe();
        let bridge = LiveBridge {
            instance_id: "me".into(),
            enabled: true,
            channel: PUDDLY_LIVE_CHANNEL.into(),
        };
        let payload = LiveNotifyPayload::new("todo", "1", "update", "me", None);
        let mut exec = RecordingExecutor::new();
        apply_remote_payload(&hub, &bridge, &payload, &mut exec).await;
        assert!(rx.try_recv().is_err(), "self-origin must not publish");
    }

    #[tokio::test]
    async fn remote_delete_publishes_id() {
        let hub = SseHub::new();
        let mut rx = hub.subscribe();
        let bridge = LiveBridge {
            instance_id: "me".into(),
            enabled: true,
            channel: PUDDLY_LIVE_CHANNEL.into(),
        };
        let payload = LiveNotifyPayload::new("todo", "gone", "delete", "other", None);
        let mut exec = RecordingExecutor::new();
        apply_remote_payload(&hub, &bridge, &payload, &mut exec).await;
        let frame = rx.try_recv().expect("frame");
        assert!(frame.contains("todo.deleted"));
        assert!(frame.contains("gone"));
    }
}

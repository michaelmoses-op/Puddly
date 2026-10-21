//! Step 25: LISTEN/NOTIFY fan-out payload + self-skip (no live Postgres required).

use puddly_db::{validate_channel, RecordingExecutor, PUDDLY_LIVE_CHANNEL};
use puddly_server::{
    apply_remote_payload, extract_resource_id, LiveBridge, LiveNotifyPayload, SseEvent, SseHub,
    LIVE_NOTIFY_SCHEMA_V,
};
use serde_json::json;

#[test]
fn live_channel_is_valid_identifier() {
    assert!(validate_channel(PUDDLY_LIVE_CHANNEL).is_ok());
}

#[test]
fn notify_payload_is_compact() {
    let p = LiveNotifyPayload::new(
        "todo",
        "550e8400-e29b-41d4-a716-446655440000",
        "update",
        "inst-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        Some("2026-10-21T00:00:00Z".into()),
    );
    let s = p.to_json_string().unwrap();
    assert!(s.len() < 512, "payload too large for NOTIFY: {s}");
    assert!(s.contains(&format!("\"v\":{LIVE_NOTIFY_SCHEMA_V}")));
    let back = LiveNotifyPayload::parse(&s).unwrap();
    assert_eq!(back.resource, "todo");
    assert_eq!(back.op, "update");
}

#[test]
fn extract_id_prefers_response_then_body() {
    assert_eq!(
        extract_resource_id(&json!({"created":true,"id":"a"}), &json!({})).as_deref(),
        Some("a")
    );
    assert_eq!(
        extract_resource_id(&json!({"deleted":true}), &json!({"id":"b"})).as_deref(),
        Some("b")
    );
}

#[tokio::test]
async fn bridge_disabled_skips_notify_sql() {
    let bridge = LiveBridge::disabled();
    assert!(!bridge.enabled);
    let hub = SseHub::new();
    let event = SseEvent::new("todo", "create", json!({"created":true,"id":"x"}));
    hub.publish(&event);
    let mut exec = RecordingExecutor::new();
    bridge
        .notify_after_local(&mut exec, &event, &json!({}))
        .await;
    assert!(
        exec.log.is_empty(),
        "disabled bridge must not emit pg_notify: {:?}",
        exec.log
    );
}

#[tokio::test]
async fn bridge_enabled_records_pg_notify() {
    let bridge = LiveBridge::enabled_new();
    let event = SseEvent::new("todo", "update", json!({"id":"row-1","title":"Hi"}));
    let mut exec = RecordingExecutor::new();
    bridge
        .notify_after_local(&mut exec, &event, &json!({}))
        .await;
    assert_eq!(exec.log.len(), 1);
    assert!(exec.log[0].sql.contains("pg_notify"));
    let payload = match &exec.log[0].params[1] {
        puddly_db::SqlValue::Text(s) => s.clone(),
        other => panic!("expected text payload, got {other:?}"),
    };
    let parsed = LiveNotifyPayload::parse(&payload).unwrap();
    assert_eq!(parsed.id, "row-1");
    assert_eq!(parsed.origin, bridge.instance_id);
    assert_eq!(parsed.resource, "todo");
}

#[tokio::test]
async fn remote_update_without_row_falls_back_to_id() {
    let hub = SseHub::new();
    let mut rx = hub.subscribe();
    let bridge = LiveBridge {
        instance_id: "local".into(),
        enabled: true,
        channel: PUDDLY_LIVE_CHANNEL.into(),
    };
    let payload = LiveNotifyPayload::new("todo", "missing", "update", "remote", None);
    let mut exec = RecordingExecutor::new(); // no canned rows → fallback
    apply_remote_payload(&hub, &bridge, &payload, &mut exec).await;
    let frame = rx.try_recv().expect("sse frame");
    assert!(frame.contains("todo.updated"));
    assert!(frame.contains("missing"));
}

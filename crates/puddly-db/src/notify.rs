//! Postgres `LISTEN` / `NOTIFY` helpers for multi-instance fan-out (ADR-010 / Step 25).
//!
//! Uses a **dedicated owned connection** for `LISTEN` — never a pool checkout
//! (checkouts return to deadpool and cannot hold a long-lived listen).

use crate::error::{DbError, DbResult};
use crate::Executor;
use crate::SqlValue;
use futures::future;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_postgres::{AsyncMessage, NoTls};

/// Default channel for live / SSE fan-out (ADR-010).
pub const PUDDLY_LIVE_CHANNEL: &str = "puddly_live";

/// Postgres NOTIFY payload soft limit (~8KB). Compact JSON stays well under this.
pub const NOTIFY_PAYLOAD_MAX_BYTES: usize = 7800;

/// Validate a LISTEN/NOTIFY channel name (identifier only).
pub fn validate_channel(name: &str) -> DbResult<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(DbError::msg("empty NOTIFY channel name"));
    };
    if name.len() > 63 {
        return Err(DbError::msg(format!(
            "NOTIFY channel name too long: {name}"
        )));
    }
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(DbError::msg(format!(
            "invalid NOTIFY channel name: {name}"
        )));
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err(DbError::msg(format!(
            "invalid NOTIFY channel name: {name}"
        )))
    }
}

/// `SELECT pg_notify($1, $2)` on any short-lived executor (pool checkout OK).
pub async fn pg_notify(
    exec: &mut dyn Executor,
    channel: &str,
    payload: &str,
) -> DbResult<u64> {
    validate_channel(channel)?;
    if payload.len() > NOTIFY_PAYLOAD_MAX_BYTES {
        return Err(DbError::msg(format!(
            "NOTIFY payload exceeds {} bytes ({})",
            NOTIFY_PAYLOAD_MAX_BYTES,
            payload.len()
        )));
    }
    exec.execute(
        "SELECT pg_notify($1::text, $2::text)",
        &[
            SqlValue::Text(channel.to_string()),
            SqlValue::Text(payload.to_string()),
        ],
    )
    .await
}

/// One Postgres notification delivered to the listen task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgNotification {
    pub channel: String,
    pub payload: String,
    pub process_id: i32,
}

/// Connect with a dedicated client, `LISTEN channel`, forward notifications.
///
/// Abort the returned [`JoinHandle`] on shutdown. The client is held inside the
/// task for the lifetime of the listen.
pub async fn spawn_listen(
    url: &str,
    channel: &str,
) -> DbResult<(JoinHandle<()>, mpsc::UnboundedReceiver<PgNotification>)> {
    validate_channel(channel)?;
    if url.is_empty() {
        return Err(DbError::Connect(
            "empty connection string for LISTEN".into(),
        ));
    }

    let (client, mut connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(|e| DbError::Connect(e.to_string()))?;

    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let listen_sql = format!("LISTEN {channel}");

    let join = tokio::spawn(async move {
        let listen = client.batch_execute(&listen_sql);
        tokio::pin!(listen);
        let mut listen_done = false;

        loop {
            tokio::select! {
                biased;
                result = &mut listen, if !listen_done => {
                    listen_done = true;
                    if let Err(e) = result {
                        eprintln!("puddly LISTEN failed: {e}");
                        return;
                    }
                }
                msg = future::poll_fn(|cx| connection.poll_message(cx)) => {
                    match msg {
                        Some(Ok(AsyncMessage::Notification(n))) => {
                            let note = PgNotification {
                                channel: n.channel().to_string(),
                                payload: n.payload().to_string(),
                                process_id: n.process_id(),
                            };
                            if out_tx.send(note).is_err() {
                                return;
                            }
                        }
                        Some(Ok(AsyncMessage::Notice(_))) => {}
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            eprintln!("puddly LISTEN connection error: {e}");
                            return;
                        }
                        None => return,
                    }
                }
            }
        }
    });

    Ok((join, out_rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_validation() {
        assert!(validate_channel(PUDDLY_LIVE_CHANNEL).is_ok());
        assert!(validate_channel("a").is_ok());
        assert!(validate_channel("").is_err());
        assert!(validate_channel("bad-name").is_err());
        assert!(validate_channel("1bad").is_err());
    }
}

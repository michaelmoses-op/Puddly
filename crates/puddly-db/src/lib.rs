//! Internal database interface — Postgres first (ADR-012 / ADR-014 / ADR-028).
//!
//! All runtime SQL goes through async [`Executor`]. Drivers implement the trait;
//! generate/query logic never talks to a vendor API directly.

mod error;
mod recording;
mod value;

#[cfg(feature = "postgres")]
mod notify;
#[cfg(feature = "postgres")]
mod pool;
#[cfg(feature = "postgres")]
mod postgres_exec;

pub use error::{DbError, DbResult};
pub use recording::RecordingExecutor;
pub use value::{DbRow, SqlValue};

#[cfg(feature = "postgres")]
pub use notify::{
    pg_notify, spawn_listen, validate_channel, PgNotification, NOTIFY_PAYLOAD_MAX_BYTES,
    PUDDLY_LIVE_CHANNEL,
};
#[cfg(feature = "postgres")]
pub use pool::{PgPool, PoolStatus};
#[cfg(feature = "postgres")]
pub use postgres_exec::PostgresExecutor;

use async_trait::async_trait;
use futures::future::BoxFuture;

/// Async executor used by migrate apply, data access, and the HTTP server (ADR-028).
///
/// Dyn-compatible via [`async_trait`]: use [`begin`] / [`commit`] / [`rollback`]
/// instead of a generic `transaction` method. Helpers wrap the pattern for callers.
#[async_trait]
pub trait Executor: Send {
    async fn execute(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<u64>;
    async fn query(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<Vec<DbRow>>;
    async fn begin(&mut self) -> DbResult<()>;
    async fn commit(&mut self) -> DbResult<()>;
    async fn rollback(&mut self) -> DbResult<()>;
}

/// Run `body` inside a transaction on a dyn Executor.
pub async fn with_transaction<T>(
    exec: &mut dyn Executor,
    body: impl for<'a> FnOnce(&'a mut dyn Executor) -> BoxFuture<'a, DbResult<T>>,
) -> DbResult<T> {
    exec.begin().await?;
    match body(exec).await {
        Ok(v) => {
            exec.commit().await?;
            Ok(v)
        }
        Err(e) => {
            let _ = exec.rollback().await;
            Err(e)
        }
    }
}

/// Pool configuration (ADR-014 / ADR-028).
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub connection_string: String,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            connection_string: std::env::var("DATABASE_URL").unwrap_or_default(),
        }
    }
}

impl PoolConfig {
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            max_connections: 10,
            connection_string: url.into(),
        }
    }

    pub fn with_max_connections(mut self, n: u32) -> Self {
        self.max_connections = n.max(1);
        self
    }
}

//! Real Postgres connection pool via `deadpool-postgres` (ADR-028).

use crate::error::{DbError, DbResult};
use crate::postgres_exec::PostgresExecutor;
use crate::PoolConfig;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

/// Shared async pool. Check out a [`PostgresExecutor`] per request — do not wrap
/// the pool (or a single executor) in a global `Mutex`.
#[derive(Clone)]
pub struct PgPool {
    inner: Pool,
    /// Original URL — used for dedicated LISTEN connections (Step 25).
    connection_string: String,
}

impl PgPool {
    /// Build a pool from [`PoolConfig`].
    pub async fn connect(config: &PoolConfig) -> DbResult<Self> {
        if config.connection_string.is_empty() {
            return Err(DbError::Connect(
                "empty connection string (set DATABASE_URL or PoolConfig::from_url)".into(),
            ));
        }
        let mut cfg = Config::new();
        cfg.url = Some(config.connection_string.clone());
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(
            config.max_connections as usize,
        ));
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| DbError::Connect(e.to_string()))?;
        // Touch the pool so misconfigured URLs fail early when possible.
        // A failed first checkout still leaves a usable pool for retries.
        let _ = pool.get().await;
        Ok(Self {
            inner: pool,
            connection_string: config.connection_string.clone(),
        })
    }

    /// Connection string used to build this pool (LISTEN / diagnostics).
    pub fn connection_string(&self) -> &str {
        &self.connection_string
    }

    pub async fn from_url(url: &str) -> DbResult<Self> {
        Self::connect(&PoolConfig::from_url(url)).await
    }

    /// Check out one connection as a [`PostgresExecutor`]. Drop returns it to the pool.
    pub async fn get(&self) -> DbResult<PostgresExecutor> {
        let object = self
            .inner
            .get()
            .await
            .map_err(|e| DbError::Connect(format!("pool checkout: {e}")))?;
        Ok(PostgresExecutor::from_pooled(object))
    }

    /// Current configured max size (best-effort; for diagnostics / tests).
    pub fn status(&self) -> PoolStatus {
        let s = self.inner.status();
        PoolStatus {
            size: s.size,
            available: s.available,
            max_size: s.max_size,
        }
    }
}

/// Snapshot of pool metrics (tests / health).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStatus {
    pub size: usize,
    pub available: usize,
    pub max_size: usize,
}

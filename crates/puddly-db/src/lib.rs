//! Internal database interface — Postgres first (ADR-012 / ADR-014).
//!
//! All runtime SQL goes through [`Executor`]. Drivers implement the trait;
//! generate/query logic never talks to a vendor API directly.

mod error;
mod recording;
mod value;

#[cfg(feature = "postgres")]
mod postgres_exec;

pub use error::{DbError, DbResult};
pub use recording::RecordingExecutor;
pub use value::{DbRow, SqlValue};

#[cfg(feature = "postgres")]
pub use postgres_exec::PostgresExecutor;

/// Sync executor used by migrate apply and the Step 4 data layer.
///
/// Dyn-compatible: use [`begin`]/ [`commit`], [`rollback`] instead of a
/// generic `transaction` method. Helpers wrap the pattern for callers.
pub trait Executor {
    fn execute(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<u64>;
    fn query(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<Vec<DbRow>>;
    fn begin(&mut self) -> DbResult<()>;
    fn commit(&mut self) -> DbResult<()>;
    fn rollback(&mut self) -> DbResult<()>;
}

/// Run `body` inside a transaction on a dyn Executor.
pub fn with_transaction<T>(
    exec: &mut dyn Executor,
    body: impl FnOnce(&mut dyn Executor) -> DbResult<T>,
) -> DbResult<T> {
    exec.begin()?;
    match body(exec) {
        Ok(v) => {
            exec.commit()?;
            Ok(v)
        }
        Err(e) => {
            let _ = exec.rollback();
            Err(e)
        }
    }
}

/// Pool configuration placeholder (small pool; ADR-014).
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
}

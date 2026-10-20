use crate::error::{DbError, DbResult};
use crate::value::{DbRow, SqlValue};
use crate::Executor;
use serde::{Deserialize, Serialize};

/// Records SQL for tests — never touches a network.
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    pub log: Vec<RecordedQuery>,
    /// Optional canned responses for `query`, FIFO.
    pub canned: Vec<Vec<DbRow>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedQuery {
    pub kind: String,
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl RecordingExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_canned(&mut self, rows: Vec<DbRow>) {
        self.canned.push(rows);
    }

    pub fn fail(msg: &str) -> DbError {
        DbError::msg(msg)
    }
}

impl Executor for RecordingExecutor {
    fn execute(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<u64> {
        self.log.push(RecordedQuery {
            kind: "execute".into(),
            sql: sql.to_string(),
            params: params.to_vec(),
        });
        Ok(1)
    }

    fn query(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<Vec<DbRow>> {
        self.log.push(RecordedQuery {
            kind: "query".into(),
            sql: sql.to_string(),
            params: params.to_vec(),
        });
        if self.canned.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(self.canned.remove(0))
        }
    }

    fn begin(&mut self) -> DbResult<()> {
        self.log.push(RecordedQuery {
            kind: "begin".into(),
            sql: "BEGIN".into(),
            params: Vec::new(),
        });
        Ok(())
    }

    fn commit(&mut self) -> DbResult<()> {
        self.log.push(RecordedQuery {
            kind: "commit".into(),
            sql: "COMMIT".into(),
            params: Vec::new(),
        });
        Ok(())
    }

    fn rollback(&mut self) -> DbResult<()> {
        self.log.push(RecordedQuery {
            kind: "rollback".into(),
            sql: "ROLLBACK".into(),
            params: Vec::new(),
        });
        Ok(())
    }
}

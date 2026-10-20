//! Thin Postgres adapter (`postgres` crate). Only used when `DATABASE_URL` is set.

use crate::error::{DbError, DbResult};
use crate::value::{DbRow, SqlValue};
use crate::Executor;
use postgres::types::ToSql;
use postgres::{Client, NoTls, Row};

pub struct PostgresExecutor {
    client: Client,
}

impl PostgresExecutor {
    pub fn connect(url: &str) -> DbResult<Self> {
        let client = Client::connect(url, NoTls).map_err(|e| DbError::Connect(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Executor for PostgresExecutor {
    fn execute(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<u64> {
        let boxed = bind_params(params)?;
        let refs: Vec<&(dyn ToSql + Sync)> = boxed.iter().map(|b| b.as_ref()).collect();
        let n = self
            .client
            .execute(sql, refs.as_slice())
            .map_err(|e| DbError::msg(e.to_string()))?;
        Ok(n)
    }

    fn query(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<Vec<DbRow>> {
        let boxed = bind_params(params)?;
        let refs: Vec<&(dyn ToSql + Sync)> = boxed.iter().map(|b| b.as_ref()).collect();
        let rows = self
            .client
            .query(sql, refs.as_slice())
            .map_err(|e| DbError::msg(e.to_string()))?;
        Ok(rows.iter().map(row_to_db).collect())
    }

    fn begin(&mut self) -> DbResult<()> {
        self.client
            .batch_execute("BEGIN")
            .map_err(|e| DbError::Transaction(e.to_string()))
    }

    fn commit(&mut self) -> DbResult<()> {
        self.client
            .batch_execute("COMMIT")
            .map_err(|e| DbError::Transaction(e.to_string()))
    }

    fn rollback(&mut self) -> DbResult<()> {
        self.client
            .batch_execute("ROLLBACK")
            .map_err(|e| DbError::Transaction(e.to_string()))
    }
}

fn bind_params(params: &[SqlValue]) -> DbResult<Vec<Box<dyn ToSql + Sync>>> {
    let mut out: Vec<Box<dyn ToSql + Sync>> = Vec::with_capacity(params.len());
    for p in params {
        match p {
            SqlValue::Null => out.push(Box::new(None::<String>)),
            SqlValue::Bool(b) => out.push(Box::new(*b)),
            SqlValue::Int(i) => out.push(Box::new(*i)),
            SqlValue::Float(f) => out.push(Box::new(*f)),
            SqlValue::Text(s) | SqlValue::Uuid(s) => out.push(Box::new(s.clone())),
        }
    }
    Ok(out)
}

fn row_to_db(row: &Row) -> DbRow {
    let mut db = DbRow::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let value = if let Ok(v) = row.try_get::<_, bool>(col.name()) {
            SqlValue::Bool(v)
        } else if let Ok(v) = row.try_get::<_, i64>(col.name()) {
            SqlValue::Int(v)
        } else if let Ok(v) = row.try_get::<_, f64>(col.name()) {
            SqlValue::Float(v)
        } else if let Ok(v) = row.try_get::<_, String>(col.name()) {
            SqlValue::Text(v)
        } else if let Ok(v) = row.try_get::<_, Option<String>>(col.name()) {
            match v {
                Some(s) => SqlValue::Text(s),
                None => SqlValue::Null,
            }
        } else {
            SqlValue::Null
        };
        db.insert(name, value);
    }
    db
}

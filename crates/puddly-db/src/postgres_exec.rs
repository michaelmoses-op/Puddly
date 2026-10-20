//! Thin async Postgres adapter (`tokio-postgres` + optional deadpool checkout).

use crate::error::{DbError, DbResult};
use crate::value::{DbRow, SqlValue};
use crate::Executor;
use async_trait::async_trait;
use bytes::BytesMut;
use std::error::Error;
use chrono::{DateTime, FixedOffset, NaiveDate};
use tokio_postgres::types::{to_sql_checked, IsNull, ToSql, Type};
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

fn sql_value_accepts(ty: &Type) -> bool {
    <bool as ToSql>::accepts(ty)
        || <i64 as ToSql>::accepts(ty)
        || <f64 as ToSql>::accepts(ty)
        || <String as ToSql>::accepts(ty)
        || <Uuid as ToSql>::accepts(ty)
        || <NaiveDate as ToSql>::accepts(ty)
        || <DateTime<FixedOffset> as ToSql>::accepts(ty)
        || matches!(
            *ty,
            Type::BOOL
                | Type::DATE
                | Type::TIMESTAMP
                | Type::TIMESTAMPTZ
                | Type::UUID
                | Type::TEXT
                | Type::VARCHAR
                | Type::UNKNOWN
        )
}

/// Async executor over a `tokio_postgres::Client` (owned or pooled).
pub struct PostgresExecutor {
    client: ClientKind,
}

enum ClientKind {
    /// Direct connect (CLI migrate / one-shot). Connection task is spawned.
    Owned(Client),
    /// Checked out from [`crate::PgPool`]; returns to the pool on drop.
    Pooled(deadpool_postgres::Object),
}

impl PostgresExecutor {
    /// Connect with a single owned client (spawns the connection driver task).
    pub async fn connect(url: &str) -> DbResult<Self> {
        let (client, connection) = tokio_postgres::connect(url, NoTls)
            .await
            .map_err(|e| DbError::Connect(e.to_string()))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres connection error: {e}");
            }
        });
        Ok(Self {
            client: ClientKind::Owned(client),
        })
    }

    /// Wrap a deadpool checkout (used by [`crate::PgPool::get`]).
    pub(crate) fn from_pooled(object: deadpool_postgres::Object) -> Self {
        Self {
            client: ClientKind::Pooled(object),
        }
    }

    fn client_mut(&mut self) -> &mut Client {
        match &mut self.client {
            ClientKind::Owned(c) => c,
            ClientKind::Pooled(o) => o,
        }
    }
}

impl ToSql for SqlValue {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self {
            SqlValue::Null => Ok(IsNull::Yes),
            SqlValue::Bool(b) => b.to_sql(ty, out),
            SqlValue::Int(i) => i.to_sql(ty, out),
            SqlValue::Float(f) => f.to_sql(ty, out),
            SqlValue::Text(s) => {
                if *ty == Type::DATE {
                    if s.is_empty() {
                        return Ok(IsNull::Yes);
                    }
                    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e.to_string(),
                        )) as Box<dyn Error + Sync + Send>
                    })?;
                    return d.to_sql(ty, out);
                }
                if matches!(*ty, Type::TIMESTAMP | Type::TIMESTAMPTZ) {
                    if s.is_empty() {
                        return Ok(IsNull::Yes);
                    }
                    let dt = parse_timestamp(s).map_err(|e| {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e.to_string(),
                        )) as Box<dyn Error + Sync + Send>
                    })?;
                    return dt.to_sql(ty, out);
                }
                s.to_sql(ty, out)
            }
            SqlValue::Uuid(s) => {
                if *ty == Type::UUID {
                    let id = Uuid::parse_str(s).map_err(|e| {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            e.to_string(),
                        )) as Box<dyn Error + Sync + Send>
                    })?;
                    id.to_sql(ty, out)
                } else {
                    s.to_sql(ty, out)
                }
            }
        }
    }

    fn accepts(ty: &Type) -> bool {
        sql_value_accepts(ty)
    }

    to_sql_checked!();
}

#[async_trait]
impl Executor for PostgresExecutor {
    async fn execute(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<u64> {
        let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as _).collect();
        let n = self
            .client_mut()
            .execute(sql, refs.as_slice())
            .await
            .map_err(|e| DbError::msg(e.to_string()))?;
        Ok(n)
    }

    async fn query(&mut self, sql: &str, params: &[SqlValue]) -> DbResult<Vec<DbRow>> {
        let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p as _).collect();
        let rows = self
            .client_mut()
            .query(sql, refs.as_slice())
            .await
            .map_err(|e| DbError::msg(e.to_string()))?;
        Ok(rows.iter().map(row_to_db).collect())
    }

    async fn begin(&mut self) -> DbResult<()> {
        self.client_mut()
            .batch_execute("BEGIN")
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))
    }

    async fn commit(&mut self) -> DbResult<()> {
        self.client_mut()
            .batch_execute("COMMIT")
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))
    }

    async fn rollback(&mut self) -> DbResult<()> {
        self.client_mut()
            .batch_execute("ROLLBACK")
            .await
            .map_err(|e| DbError::Transaction(e.to_string()))
    }
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
        } else if let Ok(v) = row.try_get::<_, Uuid>(col.name()) {
            SqlValue::Uuid(v.to_string())
        } else if let Ok(v) = row.try_get::<_, Option<Uuid>>(col.name()) {
            match v {
                Some(u) => SqlValue::Uuid(u.to_string()),
                None => SqlValue::Null,
            }
        } else if let Ok(v) = row.try_get::<_, NaiveDate>(col.name()) {
            SqlValue::Text(v.format("%Y-%m-%d").to_string())
        } else if let Ok(v) = row.try_get::<_, Option<NaiveDate>>(col.name()) {
            match v {
                Some(d) => SqlValue::Text(d.format("%Y-%m-%d").to_string()),
                None => SqlValue::Null,
            }
        } else if let Ok(v) = row.try_get::<_, DateTime<FixedOffset>>(col.name()) {
            SqlValue::Text(v.to_rfc3339())
        } else if let Ok(v) = row.try_get::<_, Option<DateTime<FixedOffset>>>(col.name()) {
            match v {
                Some(dt) => SqlValue::Text(dt.to_rfc3339()),
                None => SqlValue::Null,
            }
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

fn parse_timestamp(s: &str) -> Result<DateTime<FixedOffset>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(s).or_else(|_| {
        DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f %z")
    })
}

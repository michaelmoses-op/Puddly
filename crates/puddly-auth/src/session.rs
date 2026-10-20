//! Server-side sessions: opaque token in cookie, hashed at rest.

use crate::error::{AuthError, AuthResult};
use crate::user::AuthUser;
use puddly_db::{DbRow, Executor, SqlValue};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default session lifetime (7 days).
pub const SESSION_TTL_SECS: u64 = 60 * 60 * 24 * 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub user: AuthUser,
    pub expires_at_unix: u64,
}

impl Session {
    pub fn is_expired(&self, now_unix: u64) -> bool {
        now_unix >= self.expires_at_unix
    }
}

pub trait SessionStore {
    fn create(&mut self, user: &AuthUser, ttl_secs: u64) -> AuthResult<String>;
    fn get(&mut self, raw_token: &str) -> AuthResult<Option<Session>>;
    fn delete(&mut self, raw_token: &str) -> AuthResult<()>;
}

/// Generate a 32-byte CSPRNG token, hex-encoded.
pub fn generate_session_token() -> String {
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Hash token for at-rest storage (SHA-256).
pub fn hash_token(raw_token: &str) -> String {
    let digest = Sha256::digest(raw_token.as_bytes());
    hex::encode(digest)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

/// In-memory store for tests and single-process dev.
#[derive(Debug, Default)]
pub struct MemorySessionStore {
    inner: HashMap<String, Session>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionStore for MemorySessionStore {
    fn create(&mut self, user: &AuthUser, ttl_secs: u64) -> AuthResult<String> {
        let raw = generate_session_token();
        let hash = hash_token(&raw);
        self.inner.insert(
            hash,
            Session {
                user: user.clone(),
                expires_at_unix: now_unix().saturating_add(ttl_secs),
            },
        );
        Ok(raw)
    }

    fn get(&mut self, raw_token: &str) -> AuthResult<Option<Session>> {
        let hash = hash_token(raw_token);
        match self.inner.get(&hash) {
            Some(s) if !s.is_expired(now_unix()) => Ok(Some(s.clone())),
            Some(_) => {
                self.inner.remove(&hash);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn delete(&mut self, raw_token: &str) -> AuthResult<()> {
        self.inner.remove(&hash_token(raw_token));
        Ok(())
    }
}

/// Session store backed by `puddly_sessions` via [`Executor`].
pub struct DbSessionStore<'a> {
    pub exec: &'a mut dyn Executor,
}

fn row_to_session(row: &DbRow) -> AuthResult<Session> {
    let user_id = row
        .get("user_id")
        .and_then(|v| v.as_text())
        .ok_or_else(|| AuthError::msg("session missing user_id"))?
        .to_string();
    let email = row
        .get("email")
        .and_then(|v| v.as_text())
        .unwrap_or("")
        .to_string();
    let roles_json = row.get("roles").and_then(|v| v.as_text()).unwrap_or("[]");
    let roles: Vec<String> = serde_json::from_str(roles_json).unwrap_or_default();
    let expires = match row.get("expires_at") {
        Some(SqlValue::Int(i)) => *i as u64,
        Some(SqlValue::Text(s)) => s.parse().unwrap_or(0),
        _ => 0,
    };
    Ok(Session {
        user: AuthUser {
            id: user_id,
            email,
            roles,
        },
        expires_at_unix: expires,
    })
}

impl SessionStore for DbSessionStore<'_> {
    fn create(&mut self, user: &AuthUser, ttl_secs: u64) -> AuthResult<String> {
        let raw = generate_session_token();
        let hash = hash_token(&raw);
        let expires = now_unix().saturating_add(ttl_secs);
        let roles = serde_json::to_string(&user.roles).unwrap_or_else(|_| "[]".into());
        self.exec
            .execute(
                "INSERT INTO \"puddly_sessions\" (\"token_hash\", \"user_id\", \"email\", \"roles\", \"expires_at\") \
                 VALUES ($1, $2, $3, $4, to_timestamp($5))",
                &[
                    SqlValue::Text(hash),
                    SqlValue::Uuid(user.id.clone()),
                    SqlValue::Text(user.email.clone()),
                    SqlValue::Text(roles),
                    SqlValue::Int(expires as i64),
                ],
            )
            .map_err(|e| AuthError::msg(e.to_string()))?;
        Ok(raw)
    }

    fn get(&mut self, raw_token: &str) -> AuthResult<Option<Session>> {
        let hash = hash_token(raw_token);
        let rows = self
            .exec
            .query(
                "SELECT \"user_id\", \"email\", \"roles\", EXTRACT(EPOCH FROM \"expires_at\")::bigint AS \"expires_at\" \
                 FROM \"puddly_sessions\" WHERE \"token_hash\" = $1",
                &[SqlValue::Text(hash)],
            )
            .map_err(|e| AuthError::msg(e.to_string()))?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let session = row_to_session(&row)?;
        if session.is_expired(now_unix()) {
            return Ok(None);
        }
        Ok(Some(session))
    }

    fn delete(&mut self, raw_token: &str) -> AuthResult<()> {
        let hash = hash_token(raw_token);
        self.exec
            .execute(
                "DELETE FROM \"puddly_sessions\" WHERE \"token_hash\" = $1",
                &[SqlValue::Text(hash)],
            )
            .map_err(|e| AuthError::msg(e.to_string()))?;
        Ok(())
    }
}

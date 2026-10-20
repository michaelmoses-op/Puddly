//! Pluggable auth providers. Built-in: email/password.

use crate::error::{AuthError, AuthResult};
use crate::password::{hash_password, verify_password};
use crate::session::{SessionStore, SESSION_TTL_SECS};
use crate::user::AuthUser;
use async_trait::async_trait;
use puddly_db::{Executor, SqlValue};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterInput {
    pub email: String,
    pub password: String,
    pub user_id: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[async_trait]
pub trait AuthProvider: Send {
    async fn register(&mut self, input: RegisterInput) -> AuthResult<AuthUser>;
    async fn login(&mut self, email: &str, password: &str) -> AuthResult<(AuthUser, String)>;
    async fn logout(&mut self, raw_token: &str) -> AuthResult<()>;
}

/// Built-in email/password provider using `puddly_auth_accounts` + a [`SessionStore`].
pub struct EmailPasswordProvider<'a, S: SessionStore> {
    pub exec: &'a mut dyn Executor,
    pub sessions: &'a mut S,
    pub session_ttl_secs: u64,
}

impl<'a, S: SessionStore> EmailPasswordProvider<'a, S> {
    pub fn new(exec: &'a mut dyn Executor, sessions: &'a mut S) -> Self {
        Self {
            exec,
            sessions,
            session_ttl_secs: SESSION_TTL_SECS,
        }
    }
}

#[async_trait]
impl<S: SessionStore> AuthProvider for EmailPasswordProvider<'_, S> {
    async fn register(&mut self, input: RegisterInput) -> AuthResult<AuthUser> {
        let email = normalize_email(&input.email);
        if email.is_empty() || input.password.len() < 8 {
            return Err(AuthError::msg(
                "email required and password must be at least 8 characters",
            ));
        }
        // Existence check
        let existing = self
            .exec
            .query(
                "SELECT \"id\" FROM \"puddly_auth_accounts\" WHERE \"email\" = $1",
                &[SqlValue::Text(email.clone())],
            )
            .await
            .map_err(|e| AuthError::msg(e.to_string()))?;
        if !existing.is_empty() {
            return Err(AuthError::EmailTaken);
        }

        let password_hash = hash_password(&input.password)?;
        let roles = serde_json::to_string(&input.roles).unwrap_or_else(|_| "[]".into());
        self.exec
            .execute(
                "INSERT INTO \"puddly_auth_accounts\" (\"email\", \"password_hash\", \"user_id\", \"roles\") \
                 VALUES ($1, $2, $3, $4)",
                &[
                    SqlValue::Text(email.clone()),
                    SqlValue::Text(password_hash),
                    SqlValue::Uuid(input.user_id.clone()),
                    SqlValue::Text(roles),
                ],
            )
            .await
            .map_err(|e| AuthError::msg(e.to_string()))?;

        Ok(AuthUser {
            id: input.user_id,
            email,
            roles: input.roles,
        })
    }

    async fn login(&mut self, email: &str, password: &str) -> AuthResult<(AuthUser, String)> {
        let email = normalize_email(email);
        let rows = self
            .exec
            .query(
                "SELECT \"user_id\", \"email\", \"password_hash\", \"roles\" FROM \"puddly_auth_accounts\" WHERE \"email\" = $1",
                &[SqlValue::Text(email)],
            )
            .await
            .map_err(|e| AuthError::msg(e.to_string()))?;

        // Constant-time-ish: always verify against a dummy hash if missing,
        // so timing doesn't leak account existence as obviously.
        let dummy =
            "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let (user, hash) = if let Some(row) = rows.into_iter().next() {
            let user_id = row
                .get("user_id")
                .and_then(|v| v.as_text())
                .unwrap_or("")
                .to_string();
            let email = row
                .get("email")
                .and_then(|v| v.as_text())
                .unwrap_or("")
                .to_string();
            let hash = row
                .get("password_hash")
                .and_then(|v| v.as_text())
                .unwrap_or(dummy)
                .to_string();
            let roles_json = row.get("roles").and_then(|v| v.as_text()).unwrap_or("[]");
            let roles: Vec<String> = serde_json::from_str(roles_json).unwrap_or_default();
            (
                AuthUser {
                    id: user_id,
                    email,
                    roles,
                },
                hash,
            )
        } else {
            (AuthUser::new("", ""), dummy.to_string())
        };

        let ok = verify_password(password, &hash).unwrap_or(false);
        // Extra constant-time gate on empty user id
        let id_ok = !bool::from(user.id.as_bytes().ct_eq(b""));
        if !(ok && id_ok) {
            return Err(AuthError::InvalidCredentials);
        }

        let token = self.sessions.create(&user, self.session_ttl_secs).await?;
        Ok((user, token))
    }

    async fn logout(&mut self, raw_token: &str) -> AuthResult<()> {
        self.sessions.delete(raw_token).await
    }
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

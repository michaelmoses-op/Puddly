//! Built-in auth: email/password + server-side cookie sessions (ADR-009).
//!
//! Pluggable contract: [`resolve_user`] / [`AuthResolver`] maps a request → `User | null`.
//! Crypto uses vetted libraries only (Argon2id, OsRng, subtle constant-time compares).
//! OAuth is out of scope.

mod access;
mod cookie;
mod error;
mod password;
mod provider;
mod resolve;
mod session;
mod user;

pub use access::{check_rule, check_rules, AccessDecision, RowValues};
pub use cookie::{
    clear_session_cookie_header, parse_cookie_header, parse_session_cookie, session_cookie_header,
};
pub use error::{AuthError, AuthResult};
pub use password::{hash_password, verify_password};
pub use provider::{AuthProvider, EmailPasswordProvider, RegisterInput};
pub use resolve::{resolve_user, AuthResolver, RequestView, SessionAuthResolver};
pub use session::{
    generate_session_token, hash_token, DbSessionStore, MemorySessionStore, Session, SessionStore,
    SESSION_TTL_SECS,
};
pub use user::{generate_user_id, AuthUser};

/// SQL to create framework auth tables (apply as a committed migration in apps).
pub const AUTH_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS "puddly_auth_accounts" (
  "id" UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  "email" VARCHAR(320) NOT NULL UNIQUE,
  "password_hash" TEXT NOT NULL,
  "user_id" UUID NOT NULL,
  "roles" TEXT NOT NULL DEFAULT '[]',
  "created_at" TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS "puddly_sessions" (
  "token_hash" TEXT PRIMARY KEY,
  "user_id" UUID NOT NULL,
  "email" VARCHAR(320) NOT NULL,
  "roles" TEXT NOT NULL DEFAULT '[]',
  "expires_at" TIMESTAMPTZ NOT NULL,
  "created_at" TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#;

mod scaffold;
pub use scaffold::{scaffold_auth_migration, ScaffoldResult};

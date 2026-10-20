//! Pluggable `(request) => user | null` contract.

use crate::cookie::parse_session_cookie;
use crate::session::SessionStore;
use crate::user::AuthUser;
use async_trait::async_trait;
use puddly_core::SESSION_COOKIE_NAME;

/// Minimal request view — HTTP server adapters fill this in later.
#[derive(Debug, Clone, Default)]
pub struct RequestView {
    /// Raw `Cookie` header value, if any.
    pub cookie_header: Option<String>,
    /// Optional pre-extracted session token (tests / non-HTTP adapters).
    pub session_token: Option<String>,
}

impl RequestView {
    pub fn from_cookie_header(header: impl Into<String>) -> Self {
        Self {
            cookie_header: Some(header.into()),
            session_token: None,
        }
    }

    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            cookie_header: None,
            session_token: Some(token.into()),
        }
    }

    pub fn session_token(&self) -> Option<String> {
        if let Some(t) = &self.session_token {
            return Some(t.clone());
        }
        self.cookie_header
            .as_deref()
            .and_then(parse_session_cookie)
    }
}

/// Resolver contract: map a request to an authenticated user or `None`.
#[async_trait]
pub trait AuthResolver: Send {
    async fn resolve(&mut self, request: &RequestView) -> Option<AuthUser>;
}

/// Default resolver: load session from cookie via [`SessionStore`].
pub struct SessionAuthResolver<'a, S: SessionStore> {
    pub sessions: &'a mut S,
    pub cookie_name: &'static str,
}

impl<'a, S: SessionStore> SessionAuthResolver<'a, S> {
    pub fn new(sessions: &'a mut S) -> Self {
        Self {
            sessions,
            cookie_name: SESSION_COOKIE_NAME,
        }
    }
}

#[async_trait]
impl<S: SessionStore> AuthResolver for SessionAuthResolver<'_, S> {
    async fn resolve(&mut self, request: &RequestView) -> Option<AuthUser> {
        let token = request.session_token()?;
        self.sessions
            .get(&token)
            .await
            .ok()
            .flatten()
            .map(|s| s.user)
    }
}

/// Convenience: resolve with an explicit session store.
pub async fn resolve_user<S: SessionStore>(
    sessions: &mut S,
    request: &RequestView,
) -> Option<AuthUser> {
    SessionAuthResolver::new(sessions).resolve(request).await
}

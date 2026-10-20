use puddly_auth::{resolve_user, AuthUser, RequestView, SessionStore};

/// Per-request RPC context after auth resolution.
#[derive(Debug, Clone)]
pub struct RpcContext {
    pub user: Option<AuthUser>,
}

impl RpcContext {
    pub async fn from_sessions<S: SessionStore>(
        sessions: &mut S,
        cookie_header: Option<&str>,
    ) -> Self {
        let request = match cookie_header {
            Some(h) => RequestView::from_cookie_header(h),
            None => RequestView::default(),
        };
        Self {
            user: resolve_user(sessions, &request).await,
        }
    }

    pub fn anonymous() -> Self {
        Self { user: None }
    }
}

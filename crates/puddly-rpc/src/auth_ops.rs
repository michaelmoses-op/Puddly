//! Built-in `auth.login` / `auth.register` / `auth.logout` RPC ops.

use crate::error::{RpcError, RpcResult};
use crate::request::RpcResponse;
use puddly_auth::{
    clear_session_cookie_header, generate_user_id, session_cookie_header, AuthProvider, AuthUser,
    EmailPasswordProvider, RegisterInput, SessionStore, SESSION_TTL_SECS,
};
use puddly_db::Executor;
use serde_json::{json, Value};

pub fn handle_auth_op<S: SessionStore>(
    operation: &str,
    body: &Value,
    exec: &mut dyn Executor,
    sessions: &mut S,
    secure_cookie: bool,
) -> RpcResult<RpcResponse> {
    match operation {
        "register" => register(body, exec, sessions, secure_cookie),
        "login" => login(body, exec, sessions, secure_cookie),
        "logout" => logout(body, sessions, secure_cookie),
        other => Err(RpcError::NotFound(format!("auth.{other}"))),
    }
}

fn register<S: SessionStore>(
    body: &Value,
    exec: &mut dyn Executor,
    sessions: &mut S,
    secure_cookie: bool,
) -> RpcResult<RpcResponse> {
    let email = body
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest("email required".into()))?;
    let password = body
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest("password required".into()))?;
    // Optional userId (Step 5/6 lock B): generate a UUID when omitted.
    let user_id = body
        .get("userId")
        .or_else(|| body.get("user_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(generate_user_id);
    let roles = body
        .get("roles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut provider = EmailPasswordProvider::new(exec, sessions);
    let user = provider.register(RegisterInput {
        email: email.to_string(),
        password: password.to_string(),
        user_id,
        roles,
    })?;
    // Auto-login after register
    let token = sessions.create(&user, SESSION_TTL_SECS)?;
    Ok(session_response(&user, &token, secure_cookie))
}

fn login<S: SessionStore>(
    body: &Value,
    exec: &mut dyn Executor,
    sessions: &mut S,
    secure_cookie: bool,
) -> RpcResult<RpcResponse> {
    let email = body
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest("email required".into()))?;
    let password = body
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::BadRequest("password required".into()))?;

    let mut provider = EmailPasswordProvider::new(exec, sessions);
    let (user, token) = provider.login(email, password)?;
    Ok(session_response(&user, &token, secure_cookie))
}

fn logout<S: SessionStore>(
    body: &Value,
    sessions: &mut S,
    secure_cookie: bool,
) -> RpcResult<RpcResponse> {
    // Token from body or will be cleared via cookie anyway.
    if let Some(token) = body.get("token").and_then(|v| v.as_str()) {
        sessions.delete(token)?;
    }
    Ok(RpcResponse::ok_with_cookies(
        json!({ "loggedOut": true }),
        vec![clear_session_cookie_header(secure_cookie)],
    ))
}

fn session_response(user: &AuthUser, token: &str, secure_cookie: bool) -> RpcResponse {
    RpcResponse::ok_with_cookies(
        json!({
            "user": {
                "id": user.id,
                "email": user.email,
                "roles": user.roles,
            }
        }),
        vec![session_cookie_header(token, SESSION_TTL_SECS, secure_cookie)],
    )
}

//! Session + CSRF cookie helpers (Secure / SameSite=Lax parity; ADR-048).

use puddly_core::{CSRF_COOKIE_NAME, SESSION_COOKIE_NAME};

/// Parse `Cookie` header and return the named cookie value if present.
pub fn parse_cookie_header(cookie_header: &str, name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub fn parse_session_cookie(cookie_header: &str) -> Option<String> {
    parse_cookie_header(cookie_header, SESSION_COOKIE_NAME)
}

pub fn parse_csrf_cookie(cookie_header: &str) -> Option<String> {
    parse_cookie_header(cookie_header, CSRF_COOKIE_NAME)
}

/// `Set-Cookie` value for a new session (caller attaches to response).
pub fn session_cookie_header(raw_token: &str, max_age_secs: u64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={raw_token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_flag}"
    )
}

pub fn clear_session_cookie_header(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}"
    )
}

/// Double-submit CSRF cookie — **not** HttpOnly so browser JS can copy it into
/// `X-Puddly-CSRF`. Same Path / SameSite / Secure / Max-Age as the session cookie.
pub fn csrf_cookie_header(raw_token: &str, max_age_secs: u64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{CSRF_COOKIE_NAME}={raw_token}; Path=/; SameSite=Lax; Max-Age={max_age_secs}{secure_flag}"
    )
}

pub fn clear_csrf_cookie_header(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!("{CSRF_COOKIE_NAME}=; Path=/; SameSite=Lax; Max-Age=0{secure_flag}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_finds_session_cookie() {
        let h = "a=1; puddly_session=abc123; b=2";
        assert_eq!(parse_session_cookie(h).as_deref(), Some("abc123"));
    }

    #[test]
    fn parse_finds_csrf_cookie() {
        let h = "puddly_session=abc; puddly_csrf=tok99";
        assert_eq!(parse_csrf_cookie(h).as_deref(), Some("tok99"));
    }

    #[test]
    fn csrf_cookie_is_not_httponly() {
        let h = csrf_cookie_header("abc", 60, true);
        assert!(h.contains("Secure"));
        assert!(h.contains("SameSite=Lax"));
        assert!(!h.to_ascii_lowercase().contains("httponly"));
    }
}

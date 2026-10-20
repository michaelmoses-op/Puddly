//! Session cookie helpers (HttpOnly, Secure, SameSite=Lax by default).

use puddly_core::SESSION_COOKIE_NAME;

/// Parse `Cookie` header and return the session token if present.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_finds_session_cookie() {
        let h = "a=1; puddly_session=abc123; b=2";
        assert_eq!(parse_session_cookie(h).as_deref(), Some("abc123"));
    }
}

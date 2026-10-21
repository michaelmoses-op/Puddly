use crate::error::{RpcError, RpcResult};
use crate::request::{RpcRequest, RpcTarget};
use puddly_auth::parse_csrf_cookie;
use puddly_core::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME};

/// Mutations require double-submit CSRF (ADR-048): header `X-Puddly-CSRF` must
/// equal cookie `puddly_csrf` (both non-empty). Safe methods (`list` / `get`) skip.
pub fn check_csrf(req: &RpcRequest, target: &RpcTarget) -> RpcResult<()> {
    if !target.is_mutation() {
        return Ok(());
    }
    let header = req.header(CSRF_HEADER_NAME).unwrap_or("");
    if header.is_empty() {
        return Err(RpcError::Csrf(format!(
            "csrf header `{CSRF_HEADER_NAME}` missing or empty"
        )));
    }
    let cookie = req
        .cookie_header_value()
        .and_then(parse_csrf_cookie)
        .unwrap_or_default();
    if cookie.is_empty() {
        return Err(RpcError::Csrf(format!(
            "csrf cookie `{CSRF_COOKIE_NAME}` missing"
        )));
    }
    if !csrf_tokens_equal(header, &cookie) {
        return Err(RpcError::Csrf(
            "csrf header and cookie mismatch".to_string(),
        ));
    }
    Ok(())
}

fn csrf_tokens_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_skips_csrf() {
        let req = RpcRequest::new("todo.list", json!({}));
        let t = RpcTarget::parse("todo.list").unwrap();
        assert!(check_csrf(&req, &t).is_ok());
    }

    #[test]
    fn create_requires_matching_cookie_and_header() {
        let req = RpcRequest::new("todo.create", json!({}));
        let t = RpcTarget::parse("todo.create").unwrap();
        assert!(check_csrf(&req, &t).is_err());

        let req = req.with_header(CSRF_HEADER_NAME, "tok");
        let err = check_csrf(&req, &t).unwrap_err();
        assert!(err.to_string().contains("cookie"), "{err}");

        let req = req.with_cookie(format!("{CSRF_COOKIE_NAME}=tok"));
        assert!(check_csrf(&req, &t).is_ok());

        let bad = RpcRequest::new("todo.create", json!({}))
            .with_header(CSRF_HEADER_NAME, "a")
            .with_cookie(format!("{CSRF_COOKIE_NAME}=b"));
        let err = check_csrf(&bad, &t).unwrap_err();
        assert!(err.to_string().contains("mismatch"), "{err}");
    }
}

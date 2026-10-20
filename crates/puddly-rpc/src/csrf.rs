use crate::error::{RpcError, RpcResult};
use crate::request::{RpcRequest, RpcTarget};
use puddly_core::CSRF_HEADER_NAME;

/// Mutations must send a non-empty `X-Puddly-CSRF` header (Step 5 rec 3B).
pub fn check_csrf(req: &RpcRequest, target: &RpcTarget) -> RpcResult<()> {
    if !target.is_mutation() {
        return Ok(());
    }
    let value = req.header(CSRF_HEADER_NAME).unwrap_or("");
    if value.is_empty() {
        return Err(RpcError::Csrf(CSRF_HEADER_NAME.to_string()));
    }
    Ok(())
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
    fn create_requires_header() {
        let req = RpcRequest::new("todo.create", json!({}));
        let t = RpcTarget::parse("todo.create").unwrap();
        assert!(check_csrf(&req, &t).is_err());
        let req = req.with_header(CSRF_HEADER_NAME, "1");
        assert!(check_csrf(&req, &t).is_ok());
    }
}

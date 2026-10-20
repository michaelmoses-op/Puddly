use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Incoming RPC call — HTTP adapters fill this from `POST /rpc/<resource>.<operation>`.
#[derive(Debug, Clone, Default)]
pub struct RpcRequest {
    /// Full path or suffix, e.g. `/rpc/todo.list` or `todo.list`.
    pub target: String,
    /// Lowercased header map (adapters should lowercase keys).
    pub headers: BTreeMap<String, String>,
    /// Raw `Cookie` header if not already split into `headers`.
    pub cookie_header: Option<String>,
    /// JSON body (object for most ops).
    pub body: Value,
}

impl RpcRequest {
    pub fn new(target: impl Into<String>, body: Value) -> Self {
        Self {
            target: target.into(),
            headers: BTreeMap::new(),
            cookie_header: None,
            body,
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn with_cookie(mut self, cookie: impl Into<String>) -> Self {
        self.cookie_header = Some(cookie.into());
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn cookie_header_value(&self) -> Option<&str> {
        self.cookie_header
            .as_deref()
            .or_else(|| self.header("cookie"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcTarget {
    pub resource: String,
    pub operation: String,
}

impl RpcTarget {
    /// Parse `todo.list`, `/rpc/todo.list`, or `rpc/todo.archive`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let s = raw.trim().trim_start_matches('/');
        let s = s.strip_prefix("rpc/").unwrap_or(s);
        let (resource, operation) = s
            .split_once('.')
            .ok_or_else(|| format!("invalid RPC target `{raw}` (expected resource.operation)"))?;
        if resource.is_empty() || operation.is_empty() {
            return Err(format!("invalid RPC target `{raw}`"));
        }
        if resource.contains('/') || operation.contains('/') {
            return Err(format!("invalid RPC target `{raw}`"));
        }
        Ok(Self {
            resource: resource.to_string(),
            operation: operation.to_string(),
        })
    }

    pub fn is_auth(&self) -> bool {
        self.resource == "auth"
    }

    pub fn is_mutation(&self) -> bool {
        if self.is_auth() {
            return matches!(
                self.operation.as_str(),
                "login" | "register" | "logout"
            );
        }
        !matches!(self.operation.as_str(), "list" | "get")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcErrorBody>,
    /// Optional `Set-Cookie` values for the HTTP adapter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_cookies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcErrorBody {
    pub code: String,
    pub message: String,
    pub status: u16,
}

impl RpcResponse {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            set_cookies: Vec::new(),
        }
    }

    pub fn ok_with_cookies(data: Value, set_cookies: Vec<String>) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            set_cookies,
        }
    }

    pub fn err(code: impl Into<String>, message: impl Into<String>, status: u16) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(RpcErrorBody {
                code: code.into(),
                message: message.into(),
                status,
            }),
            set_cookies: Vec::new(),
        }
    }
}

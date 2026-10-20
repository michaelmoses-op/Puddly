//! Minimal HTTP/1.1 request/response helpers (async I/O, no framework).

use crate::error::{ServerError, ServerResult};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: &'static str,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub set_cookies: Vec<String>,
}

impl HttpResponse {
    pub fn json(status: u16, reason: &'static str, value: &Value) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/json; charset=utf-8".into());
        headers.insert("content-length".into(), body.len().to_string());
        headers.insert("connection".into(), "close".into());
        Self {
            status,
            reason,
            headers,
            body,
            set_cookies: Vec::new(),
        }
    }

    pub fn text(status: u16, reason: &'static str, body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "text/plain; charset=utf-8".into());
        headers.insert("content-length".into(), body.len().to_string());
        headers.insert("connection".into(), "close".into());
        Self {
            status,
            reason,
            headers,
            body,
            set_cookies: Vec::new(),
        }
    }

    pub fn html(status: u16, reason: &'static str, body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "text/html; charset=utf-8".into());
        headers.insert("content-length".into(), body.len().to_string());
        headers.insert("connection".into(), "close".into());
        Self {
            status,
            reason,
            headers,
            body,
            set_cookies: Vec::new(),
        }
    }

    pub fn js(status: u16, reason: &'static str, body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".into(),
            "application/javascript; charset=utf-8".into(),
        );
        headers.insert("content-length".into(), body.len().to_string());
        headers.insert("connection".into(), "close".into());
        Self {
            status,
            reason,
            headers,
            body,
            set_cookies: Vec::new(),
        }
    }
}

/// Read one HTTP/1.1 request from `reader` (Content-Length body).
pub async fn parse_http_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> ServerResult<HttpRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    // Read until header terminator
    loop {
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 64 * 1024 {
            return Err(ServerError::BadRequest("headers too large".into()));
        }
    }
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| ServerError::BadRequest("incomplete headers".into()))?;
    let header_bytes = &buf[..header_end];
    let mut rest = buf[header_end + 4..].to_vec();

    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| ServerError::BadRequest("headers not utf-8".into()))?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing request line".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing method".into()))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing path".into()))?
        .to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    while rest.len() < content_length {
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        rest.extend_from_slice(&tmp[..n]);
    }
    rest.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body: rest,
    })
}

/// Parse from a complete byte buffer (tests) — sync helper over the same framing.
pub fn parse_http_request_bytes(raw: &[u8]) -> ServerResult<HttpRequest> {
    // Reuse framing logic without AsyncRead by parsing headers + body from the buffer.
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| ServerError::BadRequest("incomplete headers".into()))?;
    let header_bytes = &raw[..header_end];
    let rest = &raw[header_end + 4..];

    let header_str = std::str::from_utf8(header_bytes)
        .map_err(|_| ServerError::BadRequest("headers not utf-8".into()))?;
    let mut lines = header_str.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing request line".into()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing method".into()))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| ServerError::BadRequest("missing path".into()))?
        .to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = rest.to_vec();
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

pub async fn write_http_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    resp: &HttpResponse,
) -> ServerResult<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", resp.status, resp.reason);
    for (k, v) in &resp.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    for cookie in &resp.set_cookies {
        head.push_str(&format!("set-cookie: {cookie}\r\n"));
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes()).await?;
    writer.write_all(&resp.body).await?;
    writer.flush().await?;
    Ok(())
}

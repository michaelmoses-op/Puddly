//! Startup environment validation for `puddly serve` (ADR-041 / Step 23).

use std::fmt;

/// Deploy mode from `PUDDLY_ENV` (fallback: `RUST_ENV`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployEnv {
    Development,
    Production,
}

impl DeployEnv {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "production" | "prod" => Self::Production,
            _ => Self::Development,
        }
    }

    pub fn from_os() -> Self {
        if let Ok(v) = std::env::var("PUDDLY_ENV") {
            return Self::parse(&v);
        }
        if let Ok(v) = std::env::var("RUST_ENV") {
            return Self::parse(&v);
        }
        Self::Development
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

impl fmt::Display for DeployEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validated serve inputs — no migrate, no implicit defaults that hide misconfig.
#[derive(Debug, Clone)]
pub struct ServeEnv {
    pub database_url: String,
    pub deploy: DeployEnv,
    pub secure_cookies: bool,
}

/// Reject empty / non-Postgres URLs early (before pool connect).
pub fn validate_database_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("DATABASE_URL is empty".into());
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("postgres://") || lower.starts_with("postgresql://")) {
        return Err(
            "DATABASE_URL must start with postgres:// or postgresql://".into(),
        );
    }
    // Require something after the scheme (host or user@host).
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or("");
    if rest.is_empty() || rest == "/" {
        return Err("DATABASE_URL is missing a host".into());
    }
    Ok(())
}

/// Resolve serve env from CLI flag + process environment.
///
/// In [`DeployEnv::Production`], Secure cookies are forced on (TLS at the proxy).
pub fn resolve_serve_env(
    database_url: Option<String>,
    secure_cookies_flag: bool,
) -> Result<ServeEnv, String> {
    let url = database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            "serve requires --database-url or $DATABASE_URL".to_string()
        })?;
    validate_database_url(&url)?;
    let deploy = DeployEnv::from_os();
    let secure_cookies = match deploy {
        DeployEnv::Production => true,
        DeployEnv::Development => secure_cookies_flag,
    };
    Ok(ServeEnv {
        database_url: url,
        deploy,
        secure_cookies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_postgres() {
        assert!(validate_database_url("mysql://x").is_err());
        assert!(validate_database_url("").is_err());
        assert!(validate_database_url("postgres://").is_err());
        assert!(validate_database_url("postgres://localhost/db").is_ok());
        assert!(validate_database_url("postgresql://u:p@h:5432/db").is_ok());
    }

    #[test]
    fn deploy_env_parse() {
        assert_eq!(DeployEnv::parse("prod"), DeployEnv::Production);
        assert_eq!(DeployEnv::parse("production"), DeployEnv::Production);
        assert_eq!(DeployEnv::parse("dev"), DeployEnv::Development);
    }
}

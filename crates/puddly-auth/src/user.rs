use rand::RngCore;
use serde::{Deserialize, Serialize};

/// Generate a random UUID v4 string for caller-optional `userId` on register.
pub fn generate_user_id() -> String {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

/// Authenticated principal returned by the pluggable resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    /// Role names for `rule.role(...)`.
    pub roles: Vec<String>,
}

impl AuthUser {
    pub fn new(id: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            email: email.into(),
            roles: Vec::new(),
        }
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

use serde::{Deserialize, Serialize};

/// A gateway API key record stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayKey {
    pub id: i64,
    /// First 8 chars of the key for display (e.g. "hr-live_a3f2...")
    pub prefix: String,
    /// SHA-256 hash of the full key
    pub key_hash: String,
    /// Human-readable name for this key
    pub name: String,
    pub created_at: String,
    /// None if active, Some(timestamp) if revoked
    pub revoked_at: Option<String>,
}

/// Response returned when a new gateway key is created.
/// The raw `key` is shown exactly once — it cannot be retrieved again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayKeyCreateResponse {
    pub id: i64,
    /// The full plaintext key (shown only once)
    pub key: String,
    pub prefix: String,
    pub name: String,
    pub created_at: String,
}

/// Public-facing key info (never exposes the hash or raw key)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayKeyInfo {
    pub id: i64,
    pub prefix: String,
    pub name: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub is_active: bool,
}

impl From<GatewayKey> for GatewayKeyInfo {
    fn from(k: GatewayKey) -> Self {
        GatewayKeyInfo {
            id: k.id,
            prefix: k.prefix,
            name: k.name,
            is_active: k.revoked_at.is_none(),
            created_at: k.created_at,
            revoked_at: k.revoked_at,
        }
    }
}

use crate::errors::{ApiError, ApiResult};
use crate::quota::DbPool;
use chrono::Local;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;

use super::types::{GatewayKey, GatewayKeyCreateResponse, GatewayKeyInfo};

/// Gateway API Key Manager
/// Handles secure key generation, storage, validation, and revocation
pub struct GatewayKeyManager;

impl GatewayKeyManager {
    /// Initialize the gateway_keys table schema
    pub async fn init_schema(pool: &DbPool) -> ApiResult<()> {
        let guard = pool.get_connection().lock().await;
        if let Some(conn) = guard.as_ref() {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS gateway_keys (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    key_hash TEXT NOT NULL UNIQUE,
                    prefix TEXT NOT NULL,
                    name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    revoked_at TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_gateway_keys_hash ON gateway_keys(key_hash);
                CREATE INDEX IF NOT EXISTS idx_gateway_keys_revoked ON gateway_keys(revoked_at);",
            )
            .map_err(|e| {
                ApiError::DatabaseError(format!("Failed to create gateway_keys table: {}", e))
            })?
        }
        Ok(())
    }

    /// Generate a new gateway API key with `hr-live_` prefix
    /// Returns (raw_key, key_hash, prefix)
    fn generate_key() -> (String, String, String) {
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..24).map(|_| rng.gen()).collect();

        // Convert to hex string
        let hex_suffix = random_bytes.iter().fold(String::new(), |mut acc, byte| {
            let _ = write!(acc, "{:02x}", byte);
            acc
        });

        let raw_key = format!("hr-live_{}", hex_suffix);
        let prefix = format!("hr-live_{}", &hex_suffix[..8]);

        // SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(raw_key.as_bytes());
        let hash_bytes = hasher.finalize();
        let key_hash = hash_bytes.iter().fold(String::new(), |mut acc, byte| {
            let _ = write!(acc, "{:02x}", byte);
            acc
        });

        (raw_key, key_hash, prefix)
    }

    /// Create a new gateway API key and persist it
    pub async fn create_key(pool: &DbPool, name: String) -> ApiResult<GatewayKeyCreateResponse> {
        let (raw_key, key_hash, prefix) = Self::generate_key();
        let now = Local::now().to_rfc3339();

        let guard = pool.get_connection().lock().await;
        if let Some(conn) = guard.as_ref() {
            let result = conn.execute(
                "INSERT INTO gateway_keys (key_hash, prefix, name, created_at, revoked_at)
                 VALUES (?1, ?2, ?3, ?4, NULL)",
                rusqlite::params![&key_hash, &prefix, &name, &now],
            );

            match result {
                Ok(_) => {
                    let last_id = conn.last_insert_rowid();
                    Ok(GatewayKeyCreateResponse {
                        id: last_id,
                        key: raw_key,
                        prefix,
                        name,
                        created_at: now,
                    })
                }
                Err(e) => Err(ApiError::DatabaseError(format!(
                    "Failed to create gateway key: {}",
                    e
                ))),
            }
        } else {
            Err(ApiError::DatabaseError(
                "Database connection not available".to_string(),
            ))
        }
    }

    /// Validate a provided API key against stored hash
    pub async fn validate_key(pool: &DbPool, provided_key: &str) -> ApiResult<bool> {
        let mut hasher = Sha256::new();
        hasher.update(provided_key.as_bytes());
        let hash_bytes = hasher.finalize();
        let provided_hash = hash_bytes.iter().fold(String::new(), |mut acc, byte| {
            let _ = write!(acc, "{:02x}", byte);
            acc
        });

        let guard = pool.get_connection().lock().await;
        if let Some(conn) = guard.as_ref() {
            let result = conn.query_row(
                "SELECT id FROM gateway_keys WHERE key_hash = ? AND revoked_at IS NULL",
                rusqlite::params![&provided_hash],
                |_| Ok(()),
            );

            match result {
                Ok(_) => Ok(true),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
                Err(e) => Err(ApiError::DatabaseError(format!(
                    "Failed to validate key: {}",
                    e
                ))),
            }
        } else {
            Ok(false)
        }
    }

    /// List all active gateway keys (public info only)
    pub async fn list_keys(pool: &DbPool) -> ApiResult<Vec<GatewayKeyInfo>> {
        let guard = pool.get_connection().lock().await;
        if let Some(conn) = guard.as_ref() {
            let mut stmt = conn
                .prepare(
                    "SELECT id, prefix, name, created_at, revoked_at FROM gateway_keys ORDER BY created_at DESC",
                )
                .map_err(|e| ApiError::DatabaseError(format!("Failed to list keys: {}", e)))?;

            let keys = stmt
                .query_map([], |row| {
                    Ok(GatewayKey {
                        id: row.get(0)?,
                        prefix: row.get(1)?,
                        key_hash: String::new(),
                        name: row.get(2)?,
                        created_at: row.get(3)?,
                        revoked_at: row.get(4)?,
                    })
                })
                .map_err(|e| ApiError::DatabaseError(format!("Failed to query keys: {}", e)))?;

            let mut result = Vec::new();
            for key_result in keys {
                result.push(
                    key_result
                        .map_err(|e| ApiError::DatabaseError(format!("Failed to map key: {}", e)))?
                        .into(),
                );
            }
            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }

    /// Revoke a gateway key by ID
    pub async fn revoke_key(pool: &DbPool, key_id: i64) -> ApiResult<()> {
        let now = Local::now().to_rfc3339();

        let guard = pool.get_connection().lock().await;
        if let Some(conn) = guard.as_ref() {
            let rows_affected = conn
                .execute(
                    "UPDATE gateway_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL",
                    rusqlite::params![&now, key_id],
                )
                .map_err(|e| ApiError::DatabaseError(format!("Failed to revoke key: {}", e)))?;

            if rows_affected == 0 {
                return Err(ApiError::InvalidRequest(format!(
                    "Key {} not found or already revoked",
                    key_id
                )));
            }

            Ok(())
        } else {
            Err(ApiError::DatabaseError(
                "Database connection not available".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::create_memory_pool;

    #[tokio::test]
    async fn test_key_generation() {
        let (raw_key, key_hash, prefix) = GatewayKeyManager::generate_key();

        assert!(raw_key.starts_with("hr-live_"));
        assert_eq!(prefix.len(), 16);
        assert_eq!(key_hash.len(), 64);
    }

    #[tokio::test]
    async fn test_create_and_validate_key() {
        let pool = create_memory_pool().expect("pool creation failed");
        GatewayKeyManager::init_schema(&pool)
            .await
            .expect("schema init failed");

        let response = GatewayKeyManager::create_key(&pool, "test-key".to_string())
            .await
            .expect("key creation failed");

        assert!(response.key.starts_with("hr-live_"));
        assert_eq!(response.name, "test-key");

        let is_valid = GatewayKeyManager::validate_key(&pool, &response.key)
            .await
            .expect("validation failed");
        assert!(is_valid);

        let is_invalid = GatewayKeyManager::validate_key(&pool, "wrong_key")
            .await
            .expect("validation failed");
        assert!(!is_invalid);
    }

    #[tokio::test]
    async fn test_list_and_revoke_keys() {
        let pool = create_memory_pool().expect("pool creation failed");
        GatewayKeyManager::init_schema(&pool)
            .await
            .expect("schema init failed");

        let key1 = GatewayKeyManager::create_key(&pool, "key1".to_string())
            .await
            .expect("key creation failed");
        let _key2 = GatewayKeyManager::create_key(&pool, "key2".to_string())
            .await
            .expect("key creation failed");

        let keys = GatewayKeyManager::list_keys(&pool)
            .await
            .expect("list failed");
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().all(|k| k.is_active));

        GatewayKeyManager::revoke_key(&pool, key1.id)
            .await
            .expect("revoke failed");

        let keys = GatewayKeyManager::list_keys(&pool)
            .await
            .expect("list failed");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.iter().filter(|k| k.is_active).count(), 1);

        let result = GatewayKeyManager::revoke_key(&pool, key1.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoked_key_validation_fails() {
        let pool = create_memory_pool().expect("pool creation failed");
        GatewayKeyManager::init_schema(&pool)
            .await
            .expect("schema init failed");

        let response = GatewayKeyManager::create_key(&pool, "test-key".to_string())
            .await
            .expect("key creation failed");

        let is_valid = GatewayKeyManager::validate_key(&pool, &response.key)
            .await
            .expect("validation failed");
        assert!(is_valid);

        GatewayKeyManager::revoke_key(&pool, response.id)
            .await
            .expect("revoke failed");

        let is_valid = GatewayKeyManager::validate_key(&pool, &response.key)
            .await
            .expect("validation failed");
        assert!(!is_valid);
    }
}

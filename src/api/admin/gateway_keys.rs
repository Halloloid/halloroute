//! Admin endpoints for gateway API key management (create / list / revoke).
use crate::errors::{ApiError, ApiResult};
use crate::gateway_keys::{GatewayKeyCreateResponse, GatewayKeyInfo, GatewayKeyManager};
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

const MAX_KEY_NAME_LEN: usize = 64;

#[derive(Debug, Deserialize)]
pub struct CreateGatewayKeyRequest {
    pub name: String,
}

/// Validate and normalize a key name (trimmed, 1..=64 chars, no control characters).
fn validate_key_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Key name must not be empty".into(),
        ));
    }
    if name.chars().count() > MAX_KEY_NAME_LEN {
        return Err(ApiError::InvalidRequest(format!(
            "Key name must be at most {} characters",
            MAX_KEY_NAME_LEN
        )));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(ApiError::InvalidRequest(
            "Key name must not contain control characters".into(),
        ));
    }
    Ok(name.to_string())
}

/// `POST /admin/gateway-keys` — the full key is returned exactly once.
pub async fn create_gateway_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGatewayKeyRequest>,
) -> ApiResult<(StatusCode, Json<GatewayKeyCreateResponse>)> {
    let name = validate_key_name(&req.name)?;
    let created = GatewayKeyManager::create_key(&state.db_pool, name).await?;
    tracing::info!(key_id = created.id, prefix = %created.prefix, "Gateway key created");
    Ok((StatusCode::CREATED, Json(created)))
}

/// `GET /admin/gateway-keys` — public info only (never the hash or raw key).
pub async fn list_gateway_keys(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<GatewayKeyInfo>>> {
    Ok(Json(GatewayKeyManager::list_keys(&state.db_pool).await?))
}

/// `DELETE /admin/gateway-keys/:id`
pub async fn revoke_gateway_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    GatewayKeyManager::revoke_key(&state.db_pool, id).await?;
    tracing::info!(key_id = id, "Gateway key revoked");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_trimmed() {
        assert_eq!(validate_key_name("  my key ").unwrap(), "my key");
    }

    #[test]
    fn empty_and_blank_names_rejected() {
        assert!(validate_key_name("").is_err());
        assert!(validate_key_name("   ").is_err());
    }

    #[test]
    fn overlong_and_control_names_rejected() {
        assert!(validate_key_name(&"a".repeat(MAX_KEY_NAME_LEN + 1)).is_err());
        assert!(validate_key_name(&"a".repeat(MAX_KEY_NAME_LEN)).is_ok());
        assert!(validate_key_name("bad\nname").is_err());
    }
}

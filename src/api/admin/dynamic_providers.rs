//! Admin endpoints for dynamic provider management with live connection testing.
use crate::errors::{ApiError, ApiResult};
use crate::providers::{
    create_dynamic_provider, delete_dynamic_provider, discover_provider, get_dynamic_provider,
    list_dynamic_providers, parse_free_model_filter, probe_models, record_to_provider,
    update_dynamic_provider, DynamicProviderConfig, DynamicProviderRecord, PingResponse,
    ProviderTemplate,
};
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Request to ping/test a provider connection
#[derive(Debug, Deserialize)]
pub struct PingRequest {
    pub api_endpoint: String,
    pub auth_type: String, // "none", "api_key", "bearer"
    pub api_key: Option<String>,
    pub free_model_filter: String,
    /// When editing, reuse this provider's stored key if `api_key` is blank.
    #[serde(default)]
    pub provider_id: Option<String>,
}

/// Request to create a new dynamic provider
#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,
    pub api_key: Option<String>,
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub free_model_filter: String,
}

/// Request to update an existing dynamic provider
#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,
    /// Blank/absent keeps the currently stored key.
    pub api_key: Option<String>,
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub free_model_filter: String,
}

/// Response with provider record
#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub provider: DynamicProviderRecord,
}

/// Response with list of providers
#[derive(Debug, Serialize)]
pub struct ProvidersListResponse {
    pub providers: Vec<DynamicProviderRecord>,
}

fn invalid(msg: impl Into<String>) -> ApiError {
    ApiError::InvalidRequest(msg.into())
}

/// Provider IDs: 1-40 chars of `[a-z0-9-]`, starting with a letter or digit.
pub fn validate_provider_id(id: &str) -> ApiResult<()> {
    let ok = !id.is_empty()
        && id.len() <= 40
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(invalid(
            "Provider id must be 1-40 chars of lowercase letters, digits and hyphens",
        ))
    }
}

/// Shared validation for the fields common to create/update/ping.
fn validate_connection(api_endpoint: &str, auth_type: &str) -> ApiResult<String> {
    let endpoint = api_endpoint.trim().trim_end_matches('/').to_string();
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
        return Err(invalid("Base URL must start with http:// or https://"));
    }
    if !matches!(auth_type, "none" | "api_key" | "bearer") {
        return Err(invalid("auth_type must be one of: none, api_key, bearer"));
    }
    Ok(endpoint)
}

fn validate_limits(
    name: &str,
    rate_limit_rpm: i32,
    daily: Option<i64>,
    monthly: Option<i64>,
) -> ApiResult<()> {
    if name.trim().is_empty() {
        return Err(invalid("Name is required"));
    }
    if rate_limit_rpm < 1 {
        return Err(invalid("rate_limit_rpm must be at least 1"));
    }
    if daily.is_some_and(|v| v < 0) || monthly.is_some_and(|v| v < 0) {
        return Err(invalid("Quotas cannot be negative"));
    }
    Ok(())
}

fn non_blank(key: Option<String>) -> Option<String> {
    key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

/// Ping the provider's `/models` endpoint and return the free models found.
async fn discover_free_models(
    id: &str,
    endpoint: &str,
    auth_type: &str,
    api_key: Option<String>,
    filter: &str,
) -> ApiResult<Vec<String>> {
    if auth_type != "none" && api_key.is_none() {
        return Err(invalid("An API key is required for this auth type"));
    }
    let template = ProviderTemplate {
        id: id.to_string(),
        name: id.to_string(),
        api_endpoint: endpoint.to_string(),
        auth_type: auth_type.to_string(),
        env_var_name: String::new(),
        free_quota_daily: None,
        free_quota_monthly: None,
        rate_limit_rpm: 30,
        free_model_filter: parse_free_model_filter(filter)?,
        discovery_timeout_secs: 10,
    };

    match discover_provider(&template, api_key.clone()).await {
        Ok(Some(p)) => {
            // Keep only models that actually answer a test request with this key.
            let total = p.models.len();
            let report = probe_models(endpoint, api_key.as_deref(), p.models).await;
            tracing::info!(
                provider = %id,
                listed = total,
                working = report.working.len(),
                failed = report.failed.len(),
                "Model probe finished"
            );
            if report.working.is_empty() {
                let first = report
                    .failed
                    .first()
                    .map(|(m, r)| format!(" First failure ({}): {}", m, r))
                    .unwrap_or_default();
                return Err(invalid(format!(
                    "Connected, but none of the {} listed model(s) answered a test request.{}",
                    total, first
                )));
            }
            Ok(report.working)
        }
        Ok(None) => Err(invalid(
            "Connected, but no free models matched the filter. Strict free-tier filtering is enforced.",
        )),
        Err(e) => Err(ApiError::ProviderError(format!(
            "Failed to connect to provider: {}",
            e
        ))),
    }
}

/// POST /admin/providers/ping - Test provider connection and discover models
pub async fn ping_provider(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PingRequest>,
) -> ApiResult<Json<PingResponse>> {
    let fail = |error: String| {
        Json(PingResponse {
            success: false,
            models: Vec::new(),
            error: Some(error),
        })
    };

    let endpoint = match validate_connection(&req.api_endpoint, &req.auth_type) {
        Ok(e) => e,
        Err(e) => return Ok(fail(e.to_string())),
    };

    let mut api_key = non_blank(req.api_key);
    if api_key.is_none() {
        if let Some(pid) = req.provider_id.as_deref() {
            if let Ok(existing) = get_dynamic_provider(&state.db_pool, pid).await {
                api_key = non_blank(existing.api_key);
            }
        }
    }

    tracing::info!(endpoint = %endpoint, auth_type = %req.auth_type, "Pinging provider");

    match discover_free_models(
        "ping",
        &endpoint,
        &req.auth_type,
        api_key,
        &req.free_model_filter,
    )
    .await
    {
        Ok(models) => Ok(Json(PingResponse {
            success: true,
            models: models
                .into_iter()
                .map(|id| crate::providers::types::ModelInfo {
                    id,
                    object: "model".to_string(),
                    owned_by: "provider".to_string(),
                    pricing: None,
                })
                .collect(),
            error: None,
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Provider ping failed");
            Ok(fail(e.to_string()))
        }
    }
}

/// POST /admin/providers/dynamic - Create a new dynamic provider
pub async fn create_provider(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProviderRequest>,
) -> ApiResult<impl IntoResponse> {
    validate_provider_id(&req.id)?;
    let endpoint = validate_connection(&req.api_endpoint, &req.auth_type)?;
    validate_limits(
        &req.name,
        req.rate_limit_rpm,
        req.free_quota_daily,
        req.free_quota_monthly,
    )?;

    if state.providers.contains_key(&req.id) {
        return Err(invalid(format!(
            "A provider with id '{}' already exists",
            req.id
        )));
    }

    tracing::info!(provider_id = %req.id, name = %req.name, "Creating dynamic provider");

    let api_key = non_blank(req.api_key);
    let models = discover_free_models(
        &req.id,
        &endpoint,
        &req.auth_type,
        api_key.clone(),
        &req.free_model_filter,
    )
    .await?;

    let config = DynamicProviderConfig {
        id: req.id.clone(),
        name: req.name.trim().to_string(),
        api_endpoint: endpoint,
        auth_type: req.auth_type,
        api_key,
        free_quota_daily: req.free_quota_daily,
        free_quota_monthly: req.free_quota_monthly,
        rate_limit_rpm: req.rate_limit_rpm,
        free_model_filter: req.free_model_filter,
    };

    let record = create_dynamic_provider(&state.db_pool, config, models).await?;
    state
        .providers
        .insert(record.id.clone(), record_to_provider(&record));

    tracing::info!(
        provider_id = %record.id,
        model_count = record.models.len(),
        "Dynamic provider created and registered"
    );

    Ok((
        StatusCode::CREATED,
        Json(ProviderResponse { provider: record }),
    ))
}

/// GET /admin/providers/dynamic - List all dynamic providers
pub async fn list_providers(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<ProvidersListResponse>> {
    let providers = list_dynamic_providers(&state.db_pool).await?;
    Ok(Json(ProvidersListResponse { providers }))
}

/// GET /admin/providers/dynamic/:id - Get a specific dynamic provider
pub async fn get_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<ProviderResponse>> {
    let provider = get_dynamic_provider(&state.db_pool, &id).await?;
    Ok(Json(ProviderResponse { provider }))
}

/// PUT /admin/providers/dynamic/:id - Update a dynamic provider
pub async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> ApiResult<Json<ProviderResponse>> {
    // 404 for unknown ids and for built-in (non-dynamic) providers.
    let existing = get_dynamic_provider(&state.db_pool, &id).await?;

    let endpoint = validate_connection(&req.api_endpoint, &req.auth_type)?;
    validate_limits(
        &req.name,
        req.rate_limit_rpm,
        req.free_quota_daily,
        req.free_quota_monthly,
    )?;

    tracing::info!(provider_id = %id, "Updating dynamic provider");

    // Blank key keeps the stored one (unless auth was switched off).
    let api_key = if req.auth_type == "none" {
        None
    } else {
        non_blank(req.api_key).or_else(|| non_blank(existing.api_key))
    };

    let models = discover_free_models(
        &id,
        &endpoint,
        &req.auth_type,
        api_key.clone(),
        &req.free_model_filter,
    )
    .await?;

    let config = DynamicProviderConfig {
        id: id.clone(),
        name: req.name.trim().to_string(),
        api_endpoint: endpoint,
        auth_type: req.auth_type,
        api_key,
        free_quota_daily: req.free_quota_daily,
        free_quota_monthly: req.free_quota_monthly,
        rate_limit_rpm: req.rate_limit_rpm,
        free_model_filter: req.free_model_filter,
    };

    let mut record = update_dynamic_provider(&state.db_pool, &id, config, models).await?;
    record.created_at = existing.created_at;
    state
        .providers
        .insert(id.clone(), record_to_provider(&record));

    tracing::info!(provider_id = %id, "Dynamic provider updated");

    Ok(Json(ProviderResponse { provider: record }))
}

/// DELETE /admin/providers/dynamic/:id - Delete a dynamic provider
pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    tracing::info!(provider_id = %id, "Deleting dynamic provider");

    // Errors with 404 for built-in providers, so they are never removed here.
    delete_dynamic_provider(&state.db_pool, &id).await?;
    state.providers.remove(&id);

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_request_deserialization() {
        let json = r#"{
            "api_endpoint": "https://api.example.com/v1",
            "auth_type": "bearer",
            "api_key": "test-key",
            "free_model_filter": "all"
        }"#;

        let req: PingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.api_endpoint, "https://api.example.com/v1");
        assert_eq!(req.auth_type, "bearer");
        assert_eq!(req.api_key, Some("test-key".to_string()));
        assert_eq!(req.free_model_filter, "all");
        assert!(req.provider_id.is_none());
    }

    #[test]
    fn test_create_provider_request_deserialization() {
        let json = r#"{
            "id": "custom-provider",
            "name": "Custom Provider",
            "api_endpoint": "https://api.example.com/v1",
            "auth_type": "bearer",
            "api_key": "test-key",
            "free_quota_daily": 100000,
            "free_quota_monthly": null,
            "rate_limit_rpm": 30,
            "free_model_filter": "patterns:llama*,gpt*"
        }"#;

        let req: CreateProviderRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, "custom-provider");
        assert_eq!(req.free_quota_daily, Some(100000));
        assert_eq!(req.rate_limit_rpm, 30);
    }

    #[test]
    fn test_validate_provider_id() {
        assert!(validate_provider_id("my-provider-2").is_ok());
        assert!(validate_provider_id("").is_err());
        assert!(validate_provider_id("-bad").is_err());
        assert!(validate_provider_id("Upper").is_err());
        assert!(validate_provider_id("has space").is_err());
        assert!(validate_provider_id("under_score").is_err());
        assert!(validate_provider_id(&"a".repeat(41)).is_err());
    }

    #[test]
    fn test_validate_connection() {
        assert_eq!(
            validate_connection(" https://api.example.com/v1/ ", "bearer").unwrap(),
            "https://api.example.com/v1"
        );
        assert!(validate_connection("ftp://x", "bearer").is_err());
        assert!(validate_connection("api.example.com", "none").is_err());
        assert!(validate_connection("https://x", "oauth").is_err());
    }

    #[test]
    fn test_validate_limits() {
        assert!(validate_limits("n", 1, None, None).is_ok());
        assert!(validate_limits(" ", 1, None, None).is_err());
        assert!(validate_limits("n", 0, None, None).is_err());
        assert!(validate_limits("n", 1, Some(-1), None).is_err());
    }
}

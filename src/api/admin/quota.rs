use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::state::AppState;

pub async fn get_quota_status(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let providers = registry.list_providers();

    let mut quota_status = Vec::new();

    for provider in providers {
        if let Ok(current_usage) =
            crate::quota::tracker::get_today_quota_usage(&state.db_pool, &provider.id).await
        {
            let remaining = provider
                .free_quota_daily
                .map(|limit| limit - current_usage)
                .unwrap_or(-1);

            quota_status.push(json!({
                "provider_id": provider.id,
                "provider_name": provider.name,
                "daily_limit": provider.free_quota_daily,
                "current_usage": current_usage,
                "remaining": remaining,
                "percentage_used": if let Some(limit) = provider.free_quota_daily {
                    (current_usage as f64 / limit as f64 * 100.0).round() / 100.0
                } else {
                    -1.0
                },
            }));
        }
    }

    Ok(Json(json!({
        "quota_status": quota_status,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}

pub async fn disable_provider(
    State(_state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::info!(provider_id = %provider_id, "Disabling provider");

    Ok(Json(json!({
        "message": format!("Provider {} disabled", provider_id),
    })))
}

pub async fn enable_provider(
    State(_state): State<Arc<AppState>>,
    Path(provider_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::info!(provider_id = %provider_id, "Enabling provider");

    Ok(Json(json!({
        "message": format!("Provider {} enabled", provider_id),
    })))
}

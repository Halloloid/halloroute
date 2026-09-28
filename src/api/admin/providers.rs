use axum::{extract::State, Json};
use serde_json::json;
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::state::AppState;

pub async fn get_providers(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let providers = registry.list_providers();

    let provider_list: Vec<serde_json::Value> = providers
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "health_status": p.health_status,
                "rate_limit_rpm": p.rate_limit_rpm,
                "models": p.models,
                "free_quota_daily": p.free_quota_daily,
                "free_quota_monthly": p.free_quota_monthly,
            })
        })
        .collect();

    Ok(Json(json!({
        "providers": provider_list,
        "count": provider_list.len(),
    })))
}

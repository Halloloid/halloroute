use axum::{extract::State, Json};
use serde_json::json;
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::state::AppState;

pub async fn list_models(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let models = registry.list_models();

    let mut model_list: Vec<serde_json::Value> = state
        .pools
        .list()
        .into_iter()
        .map(|pool| {
            json!({
                "id": pool.id,
                "object": "model",
                "owned_by": "halloroute-pool",
                "permission": [],
            })
        })
        .collect();

    model_list.extend(models.into_iter().map(|(model_id, provider_id)| {
        json!({
            "id": model_id,
            "object": "model",
            "owned_by": provider_id,
            "permission": [],
        })
    }));

    Ok(Json(json!({
        "object": "list",
        "data": model_list,
    })))
}

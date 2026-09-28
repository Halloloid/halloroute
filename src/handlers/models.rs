use axum::{extract::State, Json};
use serde_json::json;
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::server::AppState;

pub async fn list_models(State(state): State<Arc<AppState>>) -> ApiResult<Json<serde_json::Value>> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let models = registry.list_models();

    let model_list: Vec<serde_json::Value> = models
        .into_iter()
        .map(|(model_id, provider_id)| {
            json!({
                "id": model_id,
                "object": "model",
                "owned_by": provider_id,
                "permission": [],
            })
        })
        .collect();

    Ok(Json(json!({
        "object": "list",
        "data": model_list,
    })))
}

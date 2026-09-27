use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::errors::ApiResult;
use crate::server::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: String,
    pub permission: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

pub async fn list_models(state: web::Data<AppState>) -> ApiResult<HttpResponse> {
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let models = registry.list_models();

    let model_list: Vec<ModelInfo> = models
        .into_iter()
        .map(|(model_id, provider_id)| ModelInfo {
            id: model_id,
            object: "model".to_string(),
            owned_by: provider_id,
            permission: vec![],
        })
        .collect();

    let response = ModelListResponse {
        object: "list".to_string(),
        data: model_list,
    };

    Ok(HttpResponse::Ok().json(response))
}

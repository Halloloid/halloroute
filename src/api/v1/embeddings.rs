use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::errors::{ApiError, ApiResult};
use crate::state::AppState;
use crate::token::TokenCounter;

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub input: EmbeddingInput,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

impl EmbeddingInput {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            EmbeddingInput::Single(s) => vec![s.clone()],
            EmbeddingInput::Multiple(v) => v.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingData {
    pub object: String,
    pub index: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: i32,
    pub total_tokens: i32,
}

pub async fn create_embeddings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EmbeddingRequest>,
) -> ApiResult<Json<EmbeddingResponse>> {
    tracing::debug!(model = %req.model, "Received embeddings request");

    let inputs = req.input.to_vec();
    let total_prompt_text = inputs.join(" ");
    let estimated_tokens = TokenCounter::estimate_tokens(&total_prompt_text);

    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);

    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();

    if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await?
    {
        return Err(ApiError::QuotaExceeded(provider.id.clone()));
    }

    // Call provider for embeddings
    let client = reqwest::Client::new();
    let provider_url = format!("{}/embeddings", provider.api_endpoint);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    if provider.auth_type == "api_key" {
        if let Ok(api_key) = std::env::var(&format!("{}_API_KEY", provider.id.to_uppercase())) {
            headers.insert(
                "Authorization",
                format!("Bearer {}", api_key).parse().unwrap(),
            );
        }
    }

    let provider_request = serde_json::json!({
        "model": req.model,
        "input": inputs,
    });

    let response = client
        .post(&provider_url)
        .headers(headers)
        .json(&provider_request)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ApiError::ProviderError(format!("Embedding request failed: {}", e)))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ApiError::ProviderError(format!(
            "Provider returned embedding error: {}",
            error_text
        )));
    }

    let embedding_resp: EmbeddingResponse = response.json().await.map_err(|e| {
        ApiError::ProviderError(format!("Failed to parse embedding response: {}", e))
    })?;

    // Record quota
    crate::quota::QuotaManager::record_usage(&state.db_pool, &provider.id, estimated_tokens)
        .await?;

    Ok(Json(embedding_resp))
}

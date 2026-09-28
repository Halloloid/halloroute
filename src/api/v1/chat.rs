use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

/// Streaming response chunk
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

/// Streaming choice in response chunk
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: DeltaMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Delta message for streaming
#[derive(Debug, Serialize, Deserialize)]
pub struct DeltaMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> ApiResult<Json<ChatCompletionResponse>> {
    tracing::debug!(model = %req.model, "Received chat completions request");

    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);

    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();

    tracing::info!(
        model = %req.model,
        provider_id = %provider.id,
        "Routing to provider"
    );

    let message_text = req
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let estimated_tokens = (message_text.len() / 4) as i64;

    if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await?
    {
        return Err(crate::errors::ApiError::QuotaExceeded(provider.id.clone()));
    }

    // Use provider client with circuit breaker and retry logic
    let client = std::sync::Arc::new(reqwest::Client::new());
    let provider_client = crate::providers::client::ProviderClient::new(client, provider.clone());

    let retry_config = crate::http::RetryConfig::default();
    let response = crate::http::retry_with_backoff(
        || {
            let provider_client = provider_client.clone();
            let req = req.clone();
            Box::pin(async move { provider_client.chat_completions(&req).await })
        },
        &retry_config,
    )
    .await?;

    crate::quota::QuotaManager::record_usage(&state.db_pool, &provider.id, estimated_tokens)
        .await?;

    Ok(Json(response))
}

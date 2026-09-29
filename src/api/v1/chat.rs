use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::errors::{ApiError, ApiResult};
use crate::state::AppState;
use crate::token::TokenCounter;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Choice {
    pub index: i32,
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

    let is_streaming = req.stream.unwrap_or(false);

    // 1. Check Request Cache (non-streaming requests)
    if !is_streaming {
        if let Some(cached_resp) = state.request_deduplicator.try_get(&req) {
            tracing::info!(model = %req.model, "Serving response from cache");
            return Ok(Json(cached_resp));
        }
    }

    // 2. Accurate Token Estimation & Context Window Check
    let estimated_tokens = TokenCounter::count_request(&req);
    let requested_output = req.max_tokens.unwrap_or(2000) as i64;

    if !state
        .context_window_manager
        .fits_in_context(&req.model, estimated_tokens, requested_output)
    {
        tracing::warn!(
            model = %req.model,
            estimated = estimated_tokens,
            requested_output = requested_output,
            "Request exceeds model context window"
        );
        return Err(ApiError::InvalidRequest(format!(
            "Prompt tokens ({}) + max_tokens ({}) exceed model context window",
            estimated_tokens, requested_output
        )));
    }

    // 3. Provider Routing
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);

    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();

    tracing::info!(
        model = %req.model,
        provider_id = %provider.id,
        estimated_tokens = estimated_tokens,
        "Routing to provider"
    );

    // 4. Quota Validation
    if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await?
    {
        return Err(ApiError::QuotaExceeded(provider.id.clone()));
    }

    // 5. Execute Provider Request with Resilience
    let client = Arc::new(reqwest::Client::new());
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

    // 6. Record Usage and Cache Non-Streaming Response
    let actual_prompt_tokens = response.usage.prompt_tokens as i64;
    let actual_completion_tokens = response.usage.completion_tokens as i64;
    let total_actual_tokens = if actual_prompt_tokens + actual_completion_tokens > 0 {
        actual_prompt_tokens + actual_completion_tokens
    } else {
        estimated_tokens
    };

    crate::quota::QuotaManager::record_usage(&state.db_pool, &provider.id, total_actual_tokens)
        .await?;
    state
        .token_usage_stats
        .record_usage(actual_prompt_tokens, actual_completion_tokens);

    if !is_streaming {
        state
            .request_deduplicator
            .cache_response(&req, response.clone());
    }

    Ok(Json(response))
}

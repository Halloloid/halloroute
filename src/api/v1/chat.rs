use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::errors::{ApiError, ApiResult};
use crate::middleware::BearerAuth;
use crate::state::AppState;
use crate::token::TokenCounter;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    #[serde(default, deserialize_with = "deserialize_content")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// Accepts OpenAI `content` as a string, `null`, or an array of parts
/// (`[{"type":"text","text":"..."}]`, as sent by Zed and other editors).
/// Text parts are concatenated; non-text parts (images, etc.) are ignored.
fn deserialize_content<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Array(parts) => Ok(parts
            .iter()
            .filter_map(|p| match p {
                serde_json::Value::String(s) => Some(s.as_str()),
                serde_json::Value::Object(o) => o.get("text").and_then(|t| t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")),
        other => Err(serde::de::Error::custom(format!(
            "content must be a string or an array of parts, got {}",
            other
        ))),
    }
}

#[cfg(test)]
mod content_tests {
    use super::*;

    fn parse(json: &str) -> Message {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn content_accepts_string_null_and_parts() {
        assert_eq!(parse(r#"{"role":"user","content":"hi"}"#).content, "hi");
        assert_eq!(parse(r#"{"role":"assistant","content":null}"#).content, "");
        assert_eq!(parse(r#"{"role":"assistant"}"#).content, "");
        let m = parse(
            r#"{"role":"user","content":[{"type":"text","text":"a"},{"type":"image_url","image_url":{"url":"x"}},{"type":"text","text":"b"}]}"#,
        );
        assert_eq!(m.content, "a\nb");
    }

    #[test]
    fn content_rejects_invalid_types() {
        assert!(serde_json::from_str::<Message>(r#"{"role":"user","content":5}"#).is_err());
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// Route entry for `POST /v1/chat/completions`: dispatches to the SSE handler when
/// the client sends `stream: true`, otherwise returns a regular JSON completion.
pub async fn chat_completions_entry(
    State(state): State<Arc<AppState>>,
    auth: BearerAuth,
    Json(req): Json<ChatCompletionRequest>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;

    tracing::info!(
        model = %req.model,
        messages = req.messages.len(),
        tools = req.tools.as_ref().map_or(0, Vec::len),
        streaming = req.stream.unwrap_or(false),
        "chat_request_received"
    );

    if req.stream.unwrap_or(false) {
        let sse =
            super::streaming::chat_completions_streaming(State(state), auth, Json(req)).await?;
        Ok(sse.into_response())
    } else {
        let json = chat_completions(State(state), auth, Json(req)).await?;
        Ok(json.into_response())
    }
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    auth: BearerAuth,
    Json(req): Json<ChatCompletionRequest>,
) -> ApiResult<Json<ChatCompletionResponse>> {
    // Validate API key
    if !crate::middleware::validate_api_key_async(&auth.0, &state.db_pool).await? {
        return Err(ApiError::InvalidRequest(
            "Invalid or revoked API key".into(),
        ));
    }

    tracing::debug!(model = %req.model, "Received chat completions request");

    let is_streaming = req.stream.unwrap_or(false);

    // 1. Check Request Cache (non-streaming requests)
    if !is_streaming {
        if let Some(cached_resp) = state.request_deduplicator.try_get(&req) {
            tracing::info!(model = %req.model, "Serving response from cache");
            return Ok(Json(cached_resp));
        }
    }

    // 1b. Model pool: a virtual model that load-balances and falls back across members.
    // Context limits are enforced per member upstream, so the 8k default check is skipped.
    if let Some(entry) = state.pools.get(&req.model) {
        let estimated_tokens = TokenCounter::count_request(&req);
        let response =
            crate::pools::execute_pool_chat(&state, &entry, &req, estimated_tokens).await?;
        if !is_streaming {
            state
                .request_deduplicator
                .cache_response(&req, response.clone());
        }
        return Ok(Json(response));
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

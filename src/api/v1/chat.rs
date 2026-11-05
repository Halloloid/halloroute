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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<serde_json::Value>,
}

/// Gemini-compatible gateways may place the thought signature on the
/// assistant message, a streamed delta, or a tool-call object. Normalize all
/// accepted forms back to the assistant message before forwarding upstream.
pub(crate) fn normalize_tool_metadata(req: &mut ChatCompletionRequest) {
    for message in &mut req.messages {
        if message.role != "assistant" || message.tool_calls.is_none() {
            continue;
        }
        let signature = message
            .extra_content
            .as_ref()
            .and_then(find_thought_signature)
            .or_else(|| {
                message
                    .tool_calls
                    .as_ref()
                    .and_then(|calls| calls.iter().find_map(find_thought_signature))
            });
        if let Some(signature) = signature {
            let extra = message
                .extra_content
                .get_or_insert_with(|| serde_json::json!({}));
            if !extra.is_object() {
                *extra = serde_json::json!({});
            }
            if let Some(object) = extra.as_object_mut() {
                let google = object
                    .entry("google".to_string())
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(google_object) = google.as_object_mut() {
                    google_object
                        .entry("thought_signature".to_string())
                        .or_insert(serde_json::Value::String(signature));
                }
            }
        }
    }
}

fn find_thought_signature(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(signature) = object
                .get("thought_signature")
                .and_then(serde_json::Value::as_str)
            {
                return Some(signature.to_string());
            }
            object.values().find_map(find_thought_signature)
        }
        serde_json::Value::Array(values) => values.iter().find_map(find_thought_signature),
        _ => None,
    }
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

    #[test]
    fn message_preserves_provider_extra_content() {
        let message: Message = serde_json::from_str(
            r#"{"role":"assistant","content":null,"extra_content":{"google":{"thought_signature":"sig"}}}"#,
        )
        .unwrap();
        let serialized = serde_json::to_value(message).unwrap();
        assert_eq!(
            serialized["extra_content"]["google"]["thought_signature"],
            "sig"
        );
    }

    #[test]
    fn tool_signature_is_recovered_from_tool_call_metadata() {
        let mut request = ChatCompletionRequest {
            model: "pool".into(),
            messages: vec![Message {
                role: "assistant".into(),
                content: String::new(),
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![serde_json::json!({
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "diagnostics",
                        "arguments": "{}",
                        "thought_signature": "sig-1"
                    }
                })]),
                extra_content: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: Some(true),
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            collective: None,
            collective_config: None,
        };
        normalize_tool_metadata(&mut request);
        assert_eq!(
            request.messages[0].extra_content.as_ref().unwrap()["google"]["thought_signature"],
            "sig-1"
        );
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collective: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collective_config: Option<crate::collective::CollectiveConfig>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<serde_json::Value>,
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

    if req.collective.unwrap_or(false) {
        let has_tool_continuation = req.messages.iter().any(|message| {
            message.role == "tool"
                || message
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tool_calls| !tool_calls.is_empty())
        });
        if has_tool_continuation {
            let mut continuation = req.clone();
            continuation.collective = None;
            continuation.collective_config = None;
            if continuation.stream.unwrap_or(false) {
                let sse = super::streaming::chat_completions_streaming(
                    State(state),
                    auth,
                    Json(continuation),
                )
                .await?;
                return Ok(sse.into_response());
            }
            let json = chat_completions(State(state), auth, Json(continuation)).await?;
            return Ok(json.into_response());
        }
        let response = crate::collective::run(&state, &auth, &req).await?;
        if req.stream.unwrap_or(false) {
            let events = super::streaming::response_to_stream_chunks(response)
                .into_iter()
                .map(Ok::<_, std::convert::Infallible>)
                .chain(std::iter::once(Ok(
                    axum::response::sse::Event::default().data("[DONE]")
                )));
            return Ok(axum::response::sse::Sse::new(futures::stream::iter(events))
                .keep_alive(axum::response::sse::KeepAlive::default())
                .into_response());
        }
        return Ok(Json(response).into_response());
    }

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
    let start_time = std::time::Instant::now();
    let client = Arc::new(reqwest::Client::new());
    let provider_client = crate::providers::client::ProviderClient::new(client, provider.clone());

    let retry_config = crate::http::RetryConfig::default();
    let response_result = crate::http::retry_with_backoff(
        || {
            let provider_client = provider_client.clone();
            let req = req.clone();
            Box::pin(async move { provider_client.chat_completions(&req).await })
        },
        &retry_config,
    )
    .await;

    let latency_ms = start_time.elapsed().as_millis() as i64;

    let response = match response_result {
        Ok(res) => res,
        Err(e) => {
            // Log failure
            let _ = crate::quota::tracker::log_request(
                &state.db_pool,
                &provider.id,
                &req.model,
                estimated_tokens,
                0,
                estimated_tokens,
                latency_ms,
                500, // General status code for failure
            )
            .await;
            return Err(e);
        }
    };

    // 6. Record Usage and Cache Non-Streaming Response
    let actual_prompt_tokens = response.usage.prompt_tokens as i64;
    let actual_completion_tokens = response.usage.completion_tokens as i64;
    let total_actual_tokens = if actual_prompt_tokens + actual_completion_tokens > 0 {
        actual_prompt_tokens + actual_completion_tokens
    } else {
        estimated_tokens
    };

    // Log success
    let _ = crate::quota::tracker::log_request(
        &state.db_pool,
        &provider.id,
        &req.model,
        actual_prompt_tokens,
        actual_completion_tokens,
        total_actual_tokens,
        latency_ms,
        200,
    )
    .await;

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

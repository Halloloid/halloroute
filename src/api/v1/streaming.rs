use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Sse,
    },
    Json,
};
use futures::{stream, Stream, StreamExt};
use serde_json::Value;
use std::{convert::Infallible, pin::Pin, sync::Arc};

use crate::api::v1::chat::{
    ChatCompletionRequest, ChatCompletionResponse, DeltaMessage, StreamChoice, StreamChunk,
};
use crate::errors::ApiError;
use crate::middleware::BearerAuth;
use crate::providers::client::ProviderClient;
use crate::providers::Provider;
use crate::state::AppState;

struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<Value>>,
    finish_reason: Option<String>,
    extra_content: Option<Value>,
}

type TokenStream = Pin<Box<dyn Stream<Item = Result<StreamDelta, String>> + Send>>;

/// Stream a chat completion from the provider while preserving backpressure.
pub async fn chat_completions_streaming(
    State(state): State<Arc<AppState>>,
    auth: BearerAuth,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !crate::middleware::validate_api_key_async(&auth.0, &state.db_pool).await? {
        return Err(ApiError::InvalidRequest(
            "Invalid or revoked API key".into(),
        ));
    }

    let start_time = std::time::Instant::now();
    let result = open_pool_or_provider_stream(&state, &req).await;

    let (provider, upstream_req, response) = match result {
        Ok(v) => v,
        Err(e) => {
            let latency_ms = start_time.elapsed().as_millis() as i64;
            let prompt_tokens = crate::token::TokenCounter::count_request(&req);
            let _ = crate::quota::tracker::log_request(
                &state.db_pool,
                "unknown",
                &req.model,
                prompt_tokens,
                0,
                prompt_tokens,
                latency_ms,
                500,
            )
            .await;
            return Err(e);
        }
    };
    let stream_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let model = upstream_req.model.clone();
    let db_pool = state.db_pool.clone();
    let provider_id = provider.id.clone();
    let request_for_usage = upstream_req.clone();
    tracing::debug!(
        provider_id = %provider_id,
        model = %model,
        content_type = ?response.headers().get(reqwest::header::CONTENT_TYPE),
        status = response.status().as_u16(),
        "upstream_stream_opened"
    );
    let token_stream = response_body_stream(response).await?;
    tracing::info!(
        provider_id = %provider_id,
        model = %model,
        stream_id = %stream_id,
        "stream_started"
    );

    let events = async_stream::stream! {
        yield Ok::<Event, Infallible>(json_event(StreamChunk {
            id: stream_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaMessage {
                    role: Some("assistant".to_string()),
                    content: None,
                    tool_calls: None,
                    extra_content: None,
                },
                finish_reason: None,
            }],
        }));

        let mut output_chars = 0i64;
        let mut saw_tool_calls = false;
        let mut upstream_finish_reason = None;
        tokio::pin!(token_stream);
        while let Some(item) = token_stream.next().await {
            match item {
                Ok(token) => {
                    output_chars += token.content.as_deref().unwrap_or_default().chars().count() as i64;
                    if token.tool_calls.is_some() {
                        saw_tool_calls = true;
                    }
                    if token.finish_reason.is_some() {
                        upstream_finish_reason = token.finish_reason.clone();
                    }
                    yield Ok(json_event(StreamChunk {
                        id: stream_id.clone(),
                        object: "chat.completion.chunk".to_string(),
                        created: chrono::Utc::now().timestamp(),
                        model: model.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: DeltaMessage {
                                role: None,
                                content: token.content,
                                tool_calls: token.tool_calls,
                                extra_content: token.extra_content,
                            },
                            finish_reason: None,
                        }],
                    }));
                }
                Err(error) => {
                    tracing::error!(provider_id = %provider_id, error = %error, "Upstream stream failed");
                    yield Ok(error_event(&error));
                    break;
                }
            }
        }
        let finish_reason = saw_tool_calls
            .then(|| "tool_calls".to_string())
            .or(upstream_finish_reason)
            .unwrap_or_else(|| "stop".to_string());
        tracing::info!(
            provider_id = %provider_id,
            stream_id = %stream_id,
            finish_reason = %finish_reason,
            saw_tool_calls,
            output_chars,
            "upstream_stream_completed"
        );

        let prompt_tokens = crate::token::TokenCounter::count_request(&request_for_usage);
        let output_tokens = (output_chars / 4).max(0);
        let latency_ms = start_time.elapsed().as_millis() as i64;

        // Log request
        let _ = crate::quota::tracker::log_request(
            &db_pool,
            &provider_id,
            &model,
            prompt_tokens,
            output_tokens,
            prompt_tokens + output_tokens,
            latency_ms,
            200,
        ).await;

        if let Err(error) = crate::quota::QuotaManager::record_usage(
            &db_pool,
            &provider_id,
            prompt_tokens + output_tokens,
        ).await {
            tracing::warn!(provider_id = %provider_id, error = %error, "Failed to record streaming usage");
        }

        yield Ok(json_event(StreamChunk {
            id: stream_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaMessage {
                    role: None,
                    content: None,
                    tool_calls: None,
                    extra_content: None,
                },
                finish_reason: Some(finish_reason),
            }],
        }));
        yield Ok(Event::default().data("[DONE]"));
        tracing::info!(
            provider_id = %provider_id,
            model = %model,
            stream_id = %stream_id,
            "stream_finished"
        );
    };

    Ok(Sse::new(events)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// Select a pool member and establish the upstream stream before sending headers.
/// This allows fallback when a provider is unavailable before any output is sent.
async fn open_pool_or_provider_stream(
    state: &AppState,
    req: &ChatCompletionRequest,
) -> Result<(Provider, ChatCompletionRequest, reqwest::Response), ApiError> {
    if let Some(entry) = state.pools.get(&req.model) {
        let client = Arc::new(reqwest::Client::new());
        let estimated = crate::token::TokenCounter::count_request(req);
        let mut last_error = None;

        for index in entry.order() {
            let member = &entry.pool.members[index];
            let provider = state.providers.get(&member.provider_id).map(|p| p.clone());
            let Some(provider) = provider else { continue };
            if !provider.is_available() || !provider.models.iter().any(|m| m == &member.model) {
                continue;
            }
            if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated)
                .await?
            {
                continue;
            }

            let mut member_req = req.clone();
            member_req.model = member.model.clone();
            match ProviderClient::new(client.clone(), provider.clone())
                .open_chat_stream(&member_req)
                .await
            {
                Ok(response) => {
                    entry.mark_used(index);
                    return Ok((provider, member_req, response));
                }
                Err(error) => {
                    entry.mark_failed(index);
                    last_error = Some(error);
                }
            }
        }

        return Err(last_error.unwrap_or(ApiError::AllProvidersUnavailable));
    }

    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);
    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();
    if !crate::quota::QuotaManager::check_quota(
        &state.db_pool,
        &provider,
        crate::token::TokenCounter::count_request(req),
    )
    .await?
    {
        return Err(ApiError::QuotaExceeded(provider.id.clone()));
    }

    let response = ProviderClient::new(Arc::new(reqwest::Client::new()), provider.clone())
        .open_chat_stream(req)
        .await?;
    Ok((provider, req.clone(), response))
}

/// Convert an upstream SSE response, or a JSON fallback response, into tokens.
async fn response_body_stream(response: reqwest::Response) -> Result<TokenStream, ApiError> {
    let is_event_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.starts_with("text/event-stream"))
        .unwrap_or(false);

    if !is_event_stream {
        let value: Value = response.json().await.map_err(|error| {
            ApiError::ProviderError(format!("Invalid streaming response: {}", error))
        })?;
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let tool_calls = value["choices"][0]["message"]["tool_calls"]
            .as_array()
            .cloned();
        return Ok(Box::pin(stream::once(async move {
            Ok(StreamDelta {
                content: Some(content),
                tool_calls,
                finish_reason: value["choices"][0]["finish_reason"]
                    .as_str()
                    .map(str::to_string),
                extra_content: value["choices"][0]["message"].get("extra_content").cloned(),
            })
        })));
    }

    let bytes = response.bytes_stream();
    let parsed = async_stream::try_stream! {
        let mut buffer = String::new();
        futures::pin_mut!(bytes);

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|error| format!("Upstream stream read failed: {}", error))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(position) = buffer.find('\n') {
                let line = buffer[..position].trim_end_matches('\r').to_string();
                buffer.drain(..=position);
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" { continue; }

                let value: Value = serde_json::from_str(data)
                    .map_err(|error| format!("Invalid upstream SSE JSON: {}", error))?;
                let content = value["choices"][0]["delta"]["content"]
                    .as_str()
                    .filter(|content| !content.is_empty())
                    .map(str::to_string);
                let tool_calls = value["choices"][0]["delta"]["tool_calls"]
                    .as_array()
                    .cloned();
                let finish_reason = value["choices"][0]["finish_reason"]
                    .as_str()
                    .map(str::to_string);
                let extra_content = value["choices"][0]["delta"]
                    .get("extra_content")
                    .cloned();
                if content.is_some()
                    || tool_calls.is_some()
                    || finish_reason.is_some()
                    || extra_content.is_some()
                {
                    yield StreamDelta {
                        content,
                        tool_calls,
                        finish_reason,
                        extra_content,
                    };
                }
            }
        }
    };
    Ok(Box::pin(parsed))
}

fn json_event(chunk: StreamChunk) -> Event {
    Event::default()
        .json_data(chunk)
        .unwrap_or_else(|_| Event::default())
}

fn error_event(error: &str) -> Event {
    Event::default()
        .json_data(serde_json::json!({
            "error": { "message": error, "type": "provider_stream_error" }
        }))
        .unwrap_or_else(|_| Event::default())
}

/// Compatibility helper used by pool tests and dashboard code.
pub fn response_to_stream_chunks(response: ChatCompletionResponse) -> Vec<Event> {
    vec![
        json_event(StreamChunk {
            id: response.id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: response.created,
            model: response.model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaMessage {
                    role: Some("assistant".to_string()),
                    content: None,
                    tool_calls: None,
                    extra_content: None,
                },
                finish_reason: None,
            }],
        }),
        json_event(StreamChunk {
            id: response.id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: response.created,
            model: response.model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaMessage {
                    role: None,
                    content: response
                        .choices
                        .first()
                        .map(|choice| choice.message.content.clone()),
                    tool_calls: response
                        .choices
                        .first()
                        .and_then(|choice| choice.message.tool_calls.clone()),
                    extra_content: response
                        .choices
                        .first()
                        .and_then(|choice| choice.message.extra_content.clone()),
                },
                finish_reason: None,
            }],
        }),
        json_event(StreamChunk {
            id: response.id,
            object: "chat.completion.chunk".to_string(),
            created: response.created,
            model: response.model,
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaMessage {
                    role: None,
                    content: None,
                    tool_calls: None,
                    extra_content: None,
                },
                finish_reason: Some(
                    response
                        .choices
                        .first()
                        .map(|choice| choice.finish_reason.clone())
                        .unwrap_or_else(|| "stop".to_string()),
                ),
            }],
        }),
    ]
}

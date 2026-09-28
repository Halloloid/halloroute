use axum::{
    extract::State,
    response::{sse::Event, IntoResponse, Sse},
    Json,
};
use futures::stream::Stream;
use std::sync::Arc;

use crate::api::v1::chat::{ChatCompletionRequest, DeltaMessage, StreamChoice, StreamChunk};
use crate::errors::ApiError;
use crate::state::AppState;

/// Stream a chat completion response
pub async fn chat_completions_streaming(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!(model = %req.model, stream = true, "Received streaming chat request");

    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);

    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();

    tracing::info!(
        model = %req.model,
        provider_id = %provider.id,
        "Routing streaming request to provider"
    );

    // Estimate tokens for quota check
    let message_text = req
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let estimated_tokens = (message_text.len() / 4) as i64;

    // Check quota
    if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await?
    {
        return Err(crate::errors::ApiError::QuotaExceeded(provider.id.clone()));
    }

    // Create the stream
    let stream = create_chat_stream(provider, req, state.db_pool.clone()).await?;

    Ok(Sse::new(stream))
}

/// Create a stream of chat completion chunks
async fn create_chat_stream(
    provider: crate::providers::Provider,
    req: ChatCompletionRequest,
    db_pool: crate::quota::DbPool,
) -> Result<impl Stream<Item = Result<Event, std::convert::Infallible>>, ApiError> {
    let client = std::sync::Arc::new(reqwest::Client::new());
    let provider_client = crate::providers::client::ProviderClient::new(client, provider.clone());

    // For now, return a simple stream that converts the non-streaming response
    // In Phase 3+, this should handle actual provider streaming
    match provider_client.chat_completions(&req).await {
        Ok(response) => {
            let provider_id = provider.id.clone();
            let estimated_tokens =
                (req.messages.iter().map(|m| m.content.len()).sum::<usize>() / 4) as i64;

            // Record usage
            crate::quota::QuotaManager::record_usage(&db_pool, &provider_id, estimated_tokens)
                .await?;

            let mut events = Vec::new();

            // First chunk with role
            if let Ok(ev) = Event::default().json_data(StreamChunk {
                id: response.id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: response.created,
                model: response.model.clone(),
                choices: vec![StreamChoice {
                    index: 0,
                    delta: DeltaMessage {
                        role: Some("assistant".to_string()),
                        content: None,
                    },
                    finish_reason: None,
                }],
            }) {
                events.push(Ok(ev));
            }

            // Content chunk
            let content = response.choices.get(0).map(|c| c.message.content.clone());
            if let Ok(ev) = Event::default().json_data(StreamChunk {
                id: response.id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: response.created,
                model: response.model.clone(),
                choices: vec![StreamChoice {
                    index: 0,
                    delta: DeltaMessage {
                        role: None,
                        content,
                    },
                    finish_reason: None,
                }],
            }) {
                events.push(Ok(ev));
            }

            // Final chunk with finish_reason
            if let Ok(ev) = Event::default().json_data(StreamChunk {
                id: response.id,
                object: "chat.completion.chunk".to_string(),
                created: response.created,
                model: response.model,
                choices: vec![StreamChoice {
                    index: 0,
                    delta: DeltaMessage {
                        role: None,
                        content: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
            }) {
                events.push(Ok(ev));
            }

            Ok(futures::stream::iter(events))
        }
        Err(e) => {
            tracing::error!(
                provider_id = %provider.id,
                error = %e,
                "Failed to create streaming response"
            );
            Err(e)
        }
    }
}

/// Helper to convert a regular response to streaming chunks
pub fn response_to_stream_chunks(
    response: crate::api::v1::chat::ChatCompletionResponse,
) -> Vec<Event> {
    vec![
        // Role chunk
        Event::default()
            .json_data(StreamChunk {
                id: response.id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: response.created,
                model: response.model.clone(),
                choices: vec![StreamChoice {
                    index: 0,
                    delta: DeltaMessage {
                        role: Some("assistant".to_string()),
                        content: None,
                    },
                    finish_reason: None,
                }],
            })
            .unwrap(),
        // Content chunks (split into smaller pieces for streaming effect)
        {
            let content = response
                .choices
                .get(0)
                .map(|c| c.message.content.clone())
                .unwrap_or_default();

            Event::default()
                .json_data(StreamChunk {
                    id: response.id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created: response.created,
                    model: response.model.clone(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: DeltaMessage {
                            role: None,
                            content: Some(content),
                        },
                        finish_reason: None,
                    }],
                })
                .unwrap()
        },
        // Finish chunk
        Event::default()
            .json_data(StreamChunk {
                id: response.id,
                object: "chat.completion.chunk".to_string(),
                created: response.created,
                model: response.model,
                choices: vec![StreamChoice {
                    index: 0,
                    delta: DeltaMessage {
                        role: None,
                        content: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
            })
            .unwrap(),
    ]
}

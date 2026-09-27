---
name: axum-streaming
description: OpenAI-compatible streaming responses using Server-Sent Events (SSE), chunked encoding, backpressure handling, and client disconnection recovery for RustAgent
used_by: RustAgent Gateway
---

# Axum Streaming & SSE Implementation

Implements OpenAI-compatible streaming for `/v1/chat/completions` with proper backpressure and error handling.

## OpenAI Stream Format

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,           // "text_completion.chunk"
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Serialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Delta {
    pub role: Option<String>,     // Only in first chunk
    pub content: Option<String>,  // Token chunk
}

impl StreamChunk {
    pub fn first_chunk(id: String, model: String) -> Self {
        StreamChunk {
            id,
            object: "text_completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model,
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        }
    }

    pub fn data_chunk(id: String, model: String, content: String) -> Self {
        StreamChunk {
            id,
            object: "text_completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model,
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: Some(content),
                },
                finish_reason: None,
            }],
        }
    }

    pub fn end_chunk(id: String, model: String) -> Self {
        StreamChunk {
            id,
            object: "text_completion.chunk".to_string(),
            created: chrono::Utc::now().timestamp(),
            model,
            choices: vec![StreamChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
        }
    }
}
```

## SSE Stream Handler

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use std::convert::Infallible;

pub async fn chat_completions_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let stream_id = format!("stream-{}", uuid::Uuid::new_v4());
    let model = req.model.clone();

    let stream = async_stream::stream! {
        // Send initial chunk
        let initial = StreamChunk::first_chunk(stream_id.clone(), model.clone());
        let event = Event::default()
            .data(serde_json::to_string(&initial).unwrap());
        
        yield Ok(event);

        // Get provider and validate
        let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
        let router = crate::router::Router::new(registry, crate::router::RoutingStrategy::Auto);
        
        match router.select_provider(&state.db_pool, Some(&model)).await {
            Ok(provider) => {
                let provider = provider.clone();
                
                // Call provider with streaming
                match call_provider_streaming(&provider, &req).await {
                    Ok(mut response_stream) => {
                        while let Some(chunk) = response_stream.next().await {
                            match chunk {
                                Ok(token) => {
                                    let stream_chunk = StreamChunk::data_chunk(
                                        stream_id.clone(),
                                        model.clone(),
                                        token,
                                    );
                                    let event = Event::default()
                                        .data(serde_json::to_string(&stream_chunk).unwrap());
                                    yield Ok(event);
                                }
                                Err(e) => {
                                    tracing::error!("Stream error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let error_event = Event::default()
                            .data(format!("data: {{\"error\": \"{}\"}}", e));
                        yield Ok(error_event);
                    }
                }
            }
            Err(e) => {
                let error_event = Event::default()
                    .data(format!("data: {{\"error\": \"{}\"}}", e));
                yield Ok(error_event);
            }
        }

        // Send final chunk
        let final_chunk = StreamChunk::end_chunk(stream_id, model);
        let event = Event::default()
            .data(serde_json::to_string(&final_chunk).unwrap());
        yield Ok(event);
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
```

## Provider Streaming Integration

```rust
pub async fn call_provider_streaming(
    provider: &Provider,
    req: &ChatCompletionRequest,
) -> Result<impl Stream<Item = Result<String, String>>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", provider.api_endpoint);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    
    if let Ok(api_key) = std::env::var(&format!("{}_API_KEY", provider.id.to_uppercase())) {
        headers.insert(
            "Authorization",
            format!("Bearer {}", api_key).parse().unwrap(),
        );
    }

    let mut request_body = serde_json::json!({
        "model": req.model,
        "messages": req.messages,
        "stream": true,
        "temperature": req.temperature.unwrap_or(0.7),
        "max_tokens": req.max_tokens.unwrap_or(2000),
    });

    let response = client
        .post(&url)
        .headers(headers)
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let error = response.text().await.unwrap_or_default();
        return Err(format!("Provider error: {}", error));
    }

    let stream = response
        .bytes_stream()
        .then(|chunk| async move {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    
                    // Parse SSE format: "data: {...}"
                    for line in text.lines() {
                        if line.starts_with("data: ") {
                            let json_str = &line[6..];
                            if json_str == "[DONE]" {
                                continue;
                            }
                            
                            match serde_json::from_str::<serde_json::Value>(json_str) {
                                Ok(chunk_obj) => {
                                    if let Some(content) = chunk_obj["choices"][0]["delta"]["content"].as_str() {
                                        return Ok(content.to_string());
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse chunk: {}", e);
                                }
                            }
                        }
                    }
                    Ok(String::new())
                }
                Err(e) => Err(format!("Stream error: {}", e)),
            }
        })
        .filter_map(|result| async move {
            match result {
                Ok(token) if !token.is_empty() => Some(Ok(token)),
                Err(e) => Some(Err(e)),
                _ => None,
            }
        });

    Ok(stream)
}
```

## Error Handling in Streams

```rust
pub async fn chat_stream_with_error_recovery(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let stream_id = format!("stream-{}", uuid::Uuid::new_v4());
    let model = req.model.clone();

    let stream = async_stream::stream! {
        let initial = StreamChunk::first_chunk(stream_id.clone(), model.clone());
        yield Ok(Event::default().data(serde_json::to_string(&initial).unwrap()));

        let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
        let router = crate::router::Router::new(registry, crate::router::RoutingStrategy::Auto);

        match router.select_provider(&state.db_pool, Some(&model)).await {
            Ok(provider) => {
                let provider = provider.clone();
                
                // Use circuit breaker for streaming
                match state.circuit_breaker.call(|| {
                    Box::pin(call_provider_streaming(&provider, &req))
                }).await {
                    Ok(mut response_stream) => {
                        let mut token_count = 0;
                        
                        while let Some(chunk) = response_stream.next().await {
                            match chunk {
                                Ok(token) => {
                                    token_count += 1;
                                    let stream_chunk = StreamChunk::data_chunk(
                                        stream_id.clone(),
                                        model.clone(),
                                        token,
                                    );
                                    yield Ok(Event::default()
                                        .data(serde_json::to_string(&stream_chunk).unwrap()));
                                }
                                Err(e) => {
                                    tracing::error!("Stream error after {} tokens: {}", token_count, e);
                                    // Send error marker
                                    yield Ok(Event::default()
                                        .data(format!("data: {{\"error\": \"{}\"}}", e)));
                                    break;
                                }
                            }
                        }

                        state.quota_tracker.add_tokens(&provider.id, token_count as i64).await;
                    }
                    Err(e) => {
                        yield Ok(Event::default()
                            .data(format!("data: {{\"error\": \"Circuit breaker: {}\"}}", e)));
                    }
                }
            }
            Err(e) => {
                yield Ok(Event::default()
                    .data(format!("data: {{\"error\": \"{}\"}}", e)));
            }
        }

        let final_chunk = StreamChunk::end_chunk(stream_id, model);
        yield Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
```

## Client Disconnection Handling

```rust
use tokio::sync::watch;

pub async fn chat_stream_with_cancellation(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let stream_id = format!("stream-{}", uuid::Uuid::new_v4());
    let model = req.model.clone();

    let stream = async_stream::stream! {
        // Send initial chunk
        let initial = StreamChunk::first_chunk(stream_id.clone(), model.clone());
        yield Ok(Event::default().data(serde_json::to_string(&initial).unwrap()));

        let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
        let router = crate::router::Router::new(registry, crate::router::RoutingStrategy::Auto);

        if let Ok(provider) = router.select_provider(&state.db_pool, Some(&model)).await {
            let provider = provider.clone();
            
            if let Ok(mut response_stream) = call_provider_streaming(&provider, &req).await {
                loop {
                    tokio::select! {
                        // Normal stream processing
                        chunk = response_stream.next() => {
                            match chunk {
                                Some(Ok(token)) => {
                                    let stream_chunk = StreamChunk::data_chunk(
                                        stream_id.clone(),
                                        model.clone(),
                                        token,
                                    );
                                    yield Ok(Event::default()
                                        .data(serde_json::to_string(&stream_chunk).unwrap()));
                                }
                                Some(Err(e)) => {
                                    tracing::error!("Stream error: {}", e);
                                    break;
                                }
                                None => break,
                            }
                        }
                        // Client disconnection
                        _ = cancel_rx.changed() => {
                            tracing::info!("Client disconnected, cancelling stream");
                            break;
                        }
                    }
                }
            }
        }

        let final_chunk = StreamChunk::end_chunk(stream_id, model);
        yield Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
```

## Route Configuration

```rust
pub fn create_api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/completions", post(chat_completions))
        .route("/chat/completions", post(chat_completions_stream))
        // Route selection based on stream parameter
}

pub async fn unified_chat_endpoint(
    state: web::State<Arc<AppState>>,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.stream.unwrap_or(false) {
        chat_completions_stream(state, Json(req)).await.map(|s| s.into_response())
    } else {
        chat_completions(state, Json(req)).await.map(|r| r.into_response())
    }
}
```

## Testing Streams

```rust
#[tokio::test]
async fn test_stream_chunks() {
    let stream_id = "test-123".to_string();
    let chunk = StreamChunk::first_chunk(stream_id.clone(), "gpt-4".to_string());
    
    let json = serde_json::to_string(&chunk).unwrap();
    assert!(json.contains("\"role\":\"assistant\""));
    
    let data_chunk = StreamChunk::data_chunk(stream_id, "gpt-4".to_string(), "hello".to_string());
    let json = serde_json::to_string(&data_chunk).unwrap();
    assert!(json.contains("\"content\":\"hello\""));
}
```

---
name: rust-axum-patterns
description: Axum web framework patterns for building high-performance async HTTP services in Rust with proper state management, error handling, extractors, and streaming responses
used_by: RustAgent Gateway
---

# Rust Axum Web Framework Patterns

Used for building RustAgent's HTTP API with Axum framework on Tokio async runtime.

## Server Setup with Shared State

```rust
use axum::{Router, extract::State};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: DbPool,
    pub providers: HashMap<String, Provider>,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        db_pool: init_pool().await,
        providers: init_providers(),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:20128").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Extractors (Request Parsing)

```rust
// JSON body extractor
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    // req is automatically deserialized from JSON
}

// Path parameters
pub async fn get_provider(
    Path(provider_id): Path<String>,
) -> Json<Provider> {}

// Query parameters
pub async fn list_providers(
    Query(params): Query<ListParams>,
) -> Json<Vec<Provider>> {}
```

## Error Handling with IntoResponse

```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Provider not found")]
    ProviderNotFound,
    #[error("Quota exceeded")]
    QuotaExceeded,
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            ApiError::ProviderNotFound => (StatusCode::NOT_FOUND, "Provider not found"),
            ApiError::QuotaExceeded => (StatusCode::TOO_MANY_REQUESTS, "Quota exceeded"),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, &msg),
        };

        let body = Json(json!({
            "error": error_message,
            "code": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
```

## Streaming Responses (SSE)

```rust
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, Stream};
use std::convert::Infallible;

pub async fn chat_stream(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::iter(vec![
        Ok(Event::default().data("chunk 1")),
        Ok(Event::default().data("chunk 2")),
    ]);

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

## Middleware & Layers

```rust
use tower_http::trace::TraceLayer;
use tower_http::cors::CorsLayer;

let app = Router::new()
    .route("/v1/chat/completions", post(chat_completions))
    .layer(TraceLayer::new_for_http())
    .layer(CorsLayer::permissive())
    .with_state(state);
```

## Route Organization

```rust
pub fn create_routes() -> Router<Arc<AppState>> {
    Router::new()
        .nest("/v1", api_routes())
        .nest("/admin", admin_routes())
        .route("/health", get(health_check))
}

fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/completions", post(chat_completions))
        .route("/models", get(list_models))
}

fn admin_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/providers", get(get_providers))
        .route("/quota", get(get_quota))
}
```

## Key Points

- Use `Arc<AppState>` for shared mutable state across handlers
- Extractors auto-deserialize and validate request types
- `IntoResponse` trait handles all response types consistently
- Tower middleware stack for cross-cutting concerns
- SSE native support for streaming responses
- `State` extractor provides dependency injection

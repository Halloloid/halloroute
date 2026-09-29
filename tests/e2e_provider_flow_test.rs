use axum::{
    body::Body,
    extract::Json,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use rust_agent::{
    api,
    api::v1::chat::{ChatCompletionRequest, Message},
    api::v1::embeddings::EmbeddingRequest,
    cache::RequestDeduplicator,
    providers::types::Provider,
    quota::{self, tracker::create_memory_pool},
    state::AppState,
    token::{ContextWindowManager, TokenUsageStats},
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Creates a mock provider HTTP server on an ephemeral OS port (127.0.0.1:0)
/// Returns (base_url, request_counter, shutdown_tx)
async fn start_mock_provider_server() -> (String, Arc<AtomicUsize>, tokio::sync::oneshot::Sender<()>)
{
    let call_count = Arc::new(AtomicUsize::new(0));
    let count_clone = call_count.clone();

    let mock_app = Router::new()
        .route(
            "/chat/completions",
            post(move |Json(req): Json<Value>| {
                let count = count_clone.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    let model = req["model"].as_str().unwrap_or("test-model");
                    let resp = json!({
                        "id": "mock-resp-123",
                        "object": "chat.completion",
                        "created": 1700000000,
                        "model": model,
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello! I am a simulated mock AI response."
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 12,
                            "completion_tokens": 8,
                            "total_tokens": 20
                        }
                    });
                    (StatusCode::OK, Json(resp))
                }
            }),
        )
        .route(
            "/embeddings",
            post(|Json(req): Json<Value>| async move {
                let model = req["model"].as_str().unwrap_or("text-embedding-test");
                let resp = json!({
                    "object": "list",
                    "data": [
                        {
                            "object": "embedding",
                            "index": 0,
                            "embedding": [0.01, 0.02, 0.03, 0.04, 0.05]
                        }
                    ],
                    "model": model,
                    "usage": {
                        "prompt_tokens": 5,
                        "total_tokens": 5
                    }
                });
                (StatusCode::OK, Json(resp))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, mock_app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    (base_url, call_count, shutdown_tx)
}

async fn build_gateway_with_mock_provider(mock_url: String) -> (Router, Arc<AppState>) {
    let db_pool = create_memory_pool().unwrap();
    quota::run_migrations(&db_pool).await.unwrap();

    let mut providers = HashMap::new();
    let p = Provider::new(
        "mock_groq".to_string(),
        "Mock Groq Provider".to_string(),
        mock_url,
        "none".to_string(),
        Some(100_000),
        None,
        30,
        vec![
            "llama-3-8b-8192".to_string(),
            "text-embedding-3-small".to_string(),
        ],
    );
    providers.insert("mock_groq".to_string(), p);

    let deduplicator = Arc::new(RequestDeduplicator::new(Duration::from_secs(60), 100));
    let context_window_manager = ContextWindowManager::new();
    let token_usage_stats = Arc::new(TokenUsageStats::new());

    let state = Arc::new(AppState {
        db_pool,
        providers,
        request_deduplicator: deduplicator,
        context_window_manager,
        token_usage_stats,
    });

    let router = api::routes().with_state(state.clone());
    (router, state)
}

#[tokio::test]
async fn test_e2e_chat_completions_with_mock_provider_and_caching() {
    let (mock_url, call_count, _shutdown) = start_mock_provider_server().await;
    let (gateway, state) = build_gateway_with_mock_provider(mock_url).await;

    let chat_req = ChatCompletionRequest {
        model: "llama-3-8b-8192".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: "What is 2+2?".to_string(),
        }],
        temperature: Some(0.7),
        max_tokens: Some(100),
        stream: Some(false),
    };

    // First request: should hit mock provider
    let response = gateway
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&chat_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        json["choices"][0]["message"]["content"],
        "Hello! I am a simulated mock AI response."
    );
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Verify quota recorded in database
    let usage = quota::tracker::get_today_quota_usage(&state.db_pool, "mock_groq")
        .await
        .unwrap();
    assert_eq!(usage, 20); // 12 prompt + 8 completion tokens returned by mock

    // Second identical request: should hit cache and NOT invoke mock provider
    let response2 = gateway
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&chat_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
    let body_bytes2 = response2.into_body().collect().await.unwrap().to_bytes();
    let json2: Value = serde_json::from_slice(&body_bytes2).unwrap();
    assert_eq!(
        json2["choices"][0]["message"]["content"],
        "Hello! I am a simulated mock AI response."
    );

    // Call count remains 1 because request 2 was served from cache
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_e2e_embeddings_endpoint() {
    let (mock_url, _call_count, _shutdown) = start_mock_provider_server().await;
    let (gateway, state) = build_gateway_with_mock_provider(mock_url).await;

    let embed_req = EmbeddingRequest {
        input: rust_agent::api::v1::embeddings::EmbeddingInput::Single(
            "Hello world from RustAgent integration test".to_string(),
        ),
        model: "text-embedding-3-small".to_string(),
        user: None,
        encoding_format: None,
    };

    let response = gateway
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/embeddings")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&embed_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["object"], "list");
    assert_eq!(json["model"], "text-embedding-3-small");
    let embedding_arr = json["data"][0]["embedding"].as_array().unwrap();
    assert_eq!(embedding_arr.len(), 5);

    // Verify quota tracking recorded the embedding token usage
    let usage = quota::tracker::get_today_quota_usage(&state.db_pool, "mock_groq")
        .await
        .unwrap();
    assert!(usage > 0);
}

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use rust_agent::{
    api,
    api::v1::chat::{ChatCompletionRequest, Message},
    cache::RequestDeduplicator,
    providers::types::Provider,
    quota::{self, tracker::create_memory_pool},
    state::AppState,
    token::{ContextWindowManager, TokenUsageStats},
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Server that returns 503 Service Unavailable
async fn start_failing_provider_server(
) -> (String, Arc<AtomicUsize>, tokio::sync::oneshot::Sender<()>) {
    let call_count = Arc::new(AtomicUsize::new(0));
    let count_clone = call_count.clone();

    let app = Router::new().route(
        "/chat/completions",
        post(move || {
            let count = count_clone.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Provider temporarily overloaded",
                )
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    (base_url, call_count, shutdown_tx)
}

#[tokio::test]
async fn test_chat_completions_retries_and_reports_provider_error() {
    let (failing_url, call_count, _shutdown) = start_failing_provider_server().await;

    let db_pool = create_memory_pool().unwrap();
    quota::run_migrations(&db_pool).await.unwrap();

    let mut providers = HashMap::new();
    let p = Provider::new(
        "flaky_provider".to_string(),
        "Flaky Provider".to_string(),
        failing_url,
        "none".to_string(),
        Some(100_000),
        None,
        30,
        vec!["flaky-model".to_string()],
    );
    providers.insert("flaky_provider".to_string(), p);

    let state = Arc::new(AppState {
        db_pool,
        providers,
        request_deduplicator: Arc::new(RequestDeduplicator::new(Duration::from_secs(60), 100)),
        context_window_manager: ContextWindowManager::new(),
        token_usage_stats: Arc::new(TokenUsageStats::new()),
    });

    let gateway = api::routes().with_state(state);

    let chat_req = ChatCompletionRequest {
        model: "flaky-model".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }],
        temperature: None,
        max_tokens: None,
        stream: None,
    };

    let response = gateway
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

    // Default retry config retries 3 times (1 initial + 3 retries = 4 attempts total)
    assert_eq!(call_count.load(Ordering::SeqCst), 4);
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["error"]["type"], "internal_error");
}

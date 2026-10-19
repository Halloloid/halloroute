use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use dashmap::DashMap;
use halloroute::cache::RequestDeduplicator;
use halloroute::dashboard;
use halloroute::providers::types::{Provider, ProviderHealth};
use halloroute::quota::tracker::create_memory_pool;
use halloroute::state::AppState;
use halloroute::token::{ContextWindowManager, TokenUsageStats};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

fn create_test_state() -> Arc<AppState> {
    let pool = create_memory_pool().expect("test pool");
    let providers = Arc::new(DashMap::new());

    let mut p1 = Provider::new(
        "groq".into(),
        "Groq Cloud".into(),
        "https://api.groq.com/openai/v1".into(),
        "bearer".into(),
        Some(100_000),
        None,
        30,
        vec!["llama-3.1-8b-instant".into()],
    );
    p1.health_status = ProviderHealth::Healthy;

    providers.insert("groq".into(), p1);

    Arc::new(AppState {
        db_pool: pool,
        providers,
        request_deduplicator: Arc::new(RequestDeduplicator::new(Duration::from_secs(60), 100)),
        context_window_manager: ContextWindowManager::new(),
        token_usage_stats: Arc::new(TokenUsageStats::new()),
    })
}

#[tokio::test]
async fn test_dashboard_index_render() {
    let state = create_test_state();
    let app = dashboard::routes().with_state(state);

    let req = Request::builder()
        .uri("/")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("HALLOROUTE"));
    assert!(body.contains("Overview"));
    assert!(body.contains("Providers"));
}

#[tokio::test]
async fn test_dashboard_tab_routes() {
    let state = create_test_state();
    let app = dashboard::routes().with_state(state);

    // Test tab overview
    let req = Request::builder()
        .uri("/dashboard/tab/overview")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Test tab providers
    let req = Request::builder()
        .uri("/dashboard/tab/providers")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Test tab logs
    let req = Request::builder()
        .uri("/dashboard/tab/logs")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

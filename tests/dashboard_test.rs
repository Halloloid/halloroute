use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use dashmap::DashMap;
use halloroute::cache::RequestDeduplicator;
use halloroute::dashboard;
use halloroute::gateway_keys::GatewayKeyManager;
use halloroute::providers::types::{Provider, ProviderHealth};
use halloroute::quota;
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
        pools: Arc::new(halloroute::pools::PoolRegistry::default()),
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
    assert!(body.contains("HalloRoute"));
    assert!(body.contains("Overview"));
    assert!(body.contains("Providers"));
}

#[tokio::test]
async fn test_dashboard_tab_routes() {
    let state = create_test_state();
    let app = dashboard::routes().with_state(state);

    for uri in [
        "/dashboard/tab/overview",
        "/dashboard/tab/providers",
        "/dashboard/tab/logs",
        "/dashboard/tab/quotas",
        "/dashboard/tab/client-setup",
    ] {
        let req = Request::builder()
            .uri(uri)
            .method("GET")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "route {uri}");
    }
}

async fn get_client_setup_body(state: Arc<AppState>) -> String {
    let app = dashboard::routes().with_state(state);
    let req = Request::builder()
        .uri("/dashboard/tab/client-setup")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    String::from_utf8_lossy(&bytes).to_string()
}

#[tokio::test]
async fn test_client_setup_lists_all_clients_and_steps() {
    let body = get_client_setup_body(create_test_state()).await;

    assert!(body.contains("Client Integration Hub"));
    assert!(body.contains("http://127.0.0.1:20128/v1"));

    for client in [
        "CLAUDE CODE",
        "CURSOR",
        "CODEX CLI",
        "AIDER",
        "OPENCODE",
        "PYTHON SDK",
        "NODE SDK",
    ] {
        assert!(body.contains(client), "missing client card: {client}");
    }

    // Per-client step-by-step instructions
    assert_eq!(body.matches("class=\"client-steps\"").count(), 7);

    // Global setup sequence
    assert!(body.contains("EXPORT THE ENVIRONMENT VARIABLE"));
    assert!(body.contains("VERIFY THE GATEWAY"));
    assert!(body.contains("POINT YOUR CLIENT AT THE GATEWAY"));

    // Copy buttons exist for every supported client (not Claude Code)
    assert_eq!(body.matches("data-copy=").count(), 6);
}

#[tokio::test]
async fn test_client_setup_uses_real_models_not_fake_ones() {
    let body = get_client_setup_body(create_test_state()).await;

    // Model selector is populated from healthy providers
    assert!(body.contains("<option value=\"llama-3.1-8b-instant\">"));

    // Previously hardcoded fictional models / fake state must be gone
    for fake in [
        "claude-4-sonnet",
        "llama-4-8b",
        "gpt-4.1",
        "hr-auto",
        "hr-live_default",
    ] {
        assert!(!body.contains(fake), "found fake placeholder: {fake}");
    }
    assert!(!body.contains("[CONNECTED]"));
    assert!(!body.contains("STEPS</b>"));
}

#[tokio::test]
async fn test_client_setup_snippet_templates() {
    let body = get_client_setup_body(create_test_state()).await;

    // Aider needs the openai/ prefix
    assert!(body.contains("aider --model openai/"));
    assert!(body.contains("OPENAI_API_BASE"));
    // Codex provider block
    assert!(body.contains("[model_providers.halloroute]"));
    assert!(body.contains("env_key = \"HALLOROUTE_API_KEY\""));
    // OpenCode custom provider
    assert!(body.contains("@ai-sdk/openai-compatible"));
    // SDKs
    assert!(body.contains("from openai import OpenAI"));
    assert!(body.contains("new OpenAI("));
    // Honest placeholder instead of a fake key
    assert!(body.contains("YOUR_HR_LIVE_KEY"));
}

#[tokio::test]
async fn test_client_setup_marks_claude_code_unsupported() {
    let body = get_client_setup_body(create_test_state()).await;
    assert!(body.contains("[UNSUPPORTED]"));
    assert!(body.contains("/v1/messages"));
    assert!(!body.contains("ANTHROPIC_API_KEY"));
}

#[tokio::test]
async fn test_client_setup_shows_only_active_key_prefixes() {
    let state = create_test_state();
    quota::run_migrations(&state.db_pool).await.unwrap();

    let active = GatewayKeyManager::create_key(&state.db_pool, "laptop-key".into())
        .await
        .unwrap();
    let revoked = GatewayKeyManager::create_key(&state.db_pool, "old-revoked-key".into())
        .await
        .unwrap();
    GatewayKeyManager::revoke_key(&state.db_pool, revoked.id)
        .await
        .unwrap();

    let body = get_client_setup_body(state).await;

    assert!(body.contains("laptop-key"));
    assert!(body.contains(&active.prefix));
    assert!(!body.contains("old-revoked-key"));
    // The full raw key must never be rendered by the server
    assert!(!body.contains(&active.key));
}

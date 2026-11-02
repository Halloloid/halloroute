use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use dashmap::DashMap;
use halloroute::{
    api,
    cache::RequestDeduplicator,
    providers::types::Provider,
    quota::{self, tracker::create_memory_pool},
    state::AppState,
    token::{ContextWindowManager, TokenUsageStats},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

async fn setup_test_app() -> (axum::Router, Arc<AppState>) {
    let db_pool = create_memory_pool().expect("Memory pool creation succeeds");
    quota::run_migrations(&db_pool)
        .await
        .expect("Migrations must succeed");

    let providers = Arc::new(DashMap::new());
    let p1 = Provider::new(
        "test_groq".to_string(),
        "Groq Test Provider".to_string(),
        "https://api.groq.com/openai/v1".to_string(),
        "api_key".to_string(),
        Some(500_000),
        None,
        30,
        vec![
            "llama-3-8b-8192".to_string(),
            "llama-3.3-70b-versatile".to_string(),
        ],
    );

    let p2 = Provider::new(
        "test_pollinations".to_string(),
        "Pollinations Test Fallback".to_string(),
        "https://text.pollinations.ai/openai".to_string(),
        "none".to_string(),
        Some(10_000_000),
        None,
        120,
        vec!["openai".to_string(), "qwen".to_string()],
    );

    providers.insert("test_groq".to_string(), p1);
    providers.insert("test_pollinations".to_string(), p2);

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
async fn test_health_endpoint() {
    let (app, _) = setup_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["status"], "healthy");
    assert_eq!(json["version"], "0.1.0");
    assert!(json["timestamp"].is_string());
}

#[tokio::test]
async fn test_models_endpoint() {
    let (app, _) = setup_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["object"], "list");
    let data = json["data"].as_array().expect("Data array exists");
    assert!(data.len() >= 4);

    let model_ids: Vec<&str> = data
        .iter()
        .map(|m| m["id"].as_str().expect("model id is string"))
        .collect();
    assert!(model_ids.contains(&"llama-3-8b-8192"));
    assert!(model_ids.contains(&"llama-3.3-70b-versatile"));
    assert!(model_ids.contains(&"openai"));
}

#[tokio::test]
async fn test_admin_providers_endpoints() {
    let (app, _) = setup_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let providers_arr = json["providers"].as_array().unwrap();
    assert_eq!(providers_arr.len(), 2);
}

#[tokio::test]
async fn test_admin_quota_endpoint() {
    let (app, state) = setup_test_app().await;

    quota::QuotaManager::record_usage(&state.db_pool, "test_groq", 1500)
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/quota")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let quotas = json["quota_status"]
        .as_array()
        .expect("quota_status array exists");

    let groq_quota = quotas
        .iter()
        .find(|q| q["provider_id"] == "test_groq")
        .expect("groq quota exists");
    assert_eq!(groq_quota["current_usage"], 1500);
}

#[tokio::test]
async fn test_admin_cache_and_clear_endpoints() {
    let (app, _state) = setup_test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/cache")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["entries"], 0);

    // Clear cache endpoint
    let clear_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/cache/clear")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(clear_resp.status(), StatusCode::OK);
    let clear_bytes = clear_resp.into_body().collect().await.unwrap().to_bytes();
    let clear_json: Value = serde_json::from_slice(&clear_bytes).unwrap();
    assert_eq!(clear_json["status"], "ok");
}

#[tokio::test]
async fn test_chat_completions_context_window_exceeded_rejection() {
    let (app, _) = setup_test_app().await;

    // Build a huge prompt that exceeds the 8192 context window
    let huge_content = "word ".repeat(9000);
    let payload = json!({
        "model": "llama-3-8b-8192",
        "messages": [
            { "role": "user", "content": huge_content }
        ],
        "max_tokens": 1000
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer test-key")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(json["error"]["type"], "invalid_request");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("exceed model context window"));
}

fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn test_gateway_key_create_list_revoke_flow() {
    let (app, state) = setup_test_app().await;

    // Create
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/admin/gateway-keys",
            json!({ "name": "  laptop  " }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let raw_key = created["key"].as_str().unwrap().to_string();
    let id = created["id"].as_i64().unwrap();
    assert!(raw_key.starts_with("hr-live_"));
    assert_eq!(created["name"], "laptop");

    // The new key authenticates against the DB
    assert!(
        halloroute::gateway_keys::GatewayKeyManager::validate_key(&state.db_pool, &raw_key)
            .await
            .unwrap()
    );

    // List never exposes the raw key or hash
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/gateway-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("laptop"));
    assert!(!text.contains(&raw_key));
    assert!(!text.contains("key_hash"));

    // Revoke
    let uri = format!("/admin/gateway-keys/{}", id);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        !halloroute::gateway_keys::GatewayKeyManager::validate_key(&state.db_pool, &raw_key)
            .await
            .unwrap()
    );

    // Revoking again is a client error
    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_gateway_key_create_rejects_blank_name() {
    let (app, _) = setup_test_app().await;
    let response = app
        .oneshot(json_request(
            "POST",
            "/admin/gateway-keys",
            json!({ "name": "   " }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

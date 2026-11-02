use axum::{
    body::Body,
    extract::Json,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use dashmap::DashMap;
use halloroute::{
    api,
    cache::RequestDeduplicator,
    pools::PoolRegistry,
    providers::types::Provider,
    quota::{self, tracker::create_memory_pool},
    state::AppState,
    token::{ContextWindowManager, TokenUsageStats},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Mock OpenAI-compatible upstream. `fail` makes it return 500; replies echo `label`.
async fn start_mock(label: &'static str, fail: bool) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_c = hits.clone();
    let app = Router::new().route(
        "/chat/completions",
        post(move |Json(req): Json<Value>| {
            let hits = hits_c.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                if fail {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "boom"})));
                }
                (
                    StatusCode::OK,
                    Json(json!({
                        "id": "x", "object": "chat.completion", "created": 1,
                        "model": req["model"],
                        "choices": [{"index": 0, "finish_reason": "stop",
                            "message": {"role": "assistant", "content": format!("from {}", label)}}],
                        "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
                    })),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, hits)
}

fn provider(id: &str, url: String, model: &str) -> Provider {
    Provider::new(
        id.into(),
        id.into(),
        url,
        "none".into(),
        Some(1_000_000),
        None,
        60,
        vec![model.into()],
    )
}

async fn gateway(providers: Vec<Provider>) -> (Router, Arc<AppState>) {
    let db_pool = create_memory_pool().unwrap();
    quota::run_migrations(&db_pool).await.unwrap();
    let map = Arc::new(DashMap::new());
    for p in providers {
        map.insert(p.id.clone(), p);
    }
    let state = Arc::new(AppState {
        db_pool,
        providers: map,
        request_deduplicator: Arc::new(RequestDeduplicator::new(Duration::from_secs(60), 100)),
        context_window_manager: ContextWindowManager::new(),
        token_usage_stats: Arc::new(TokenUsageStats::new()),
        pools: Arc::new(PoolRegistry::default()),
    });
    (api::routes().with_state(state.clone()), state)
}

fn req(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", "Bearer test-key");
    if body.is_some() {
        b = b.header("Content-Type", "application/json");
    }
    b.body(
        body.map(|v| Body::from(v.to_string()))
            .unwrap_or_else(Body::empty),
    )
    .unwrap()
}

async fn send(app: &Router, r: Request<Body>) -> (StatusCode, String) {
    let resp = app.clone().oneshot(r).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn create_pool(app: &Router, body: Value) -> (StatusCode, String) {
    send(app, req("POST", "/admin/pools", Some(body))).await
}

/// Ask the pool a unique question (avoids the response cache) and return the reply text.
async fn ask(app: &Router, pool: &str, n: usize) -> (StatusCode, Value) {
    let (status, body) = send(
        app,
        req(
            "POST",
            "/v1/chat/completions",
            Some(json!({"model": pool, "messages": [{"role": "user", "content": format!("q{}", n)}]})),
        ),
    )
    .await;
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn priority_pool_falls_back_when_first_member_fails() {
    let (bad_url, bad_hits) = start_mock("bad", true).await;
    let (good_url, good_hits) = start_mock("good", false).await;
    let (app, _) = gateway(vec![
        provider("bad", bad_url, "m-bad"),
        provider("good", good_url, "m-good"),
    ])
    .await;

    let (status, body) = create_pool(
        &app,
        json!({"id": "free-auto", "strategy": "priority", "members": [
            {"provider_id": "bad", "model": "m-bad"},
            {"provider_id": "good", "model": "m-good"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{}", body);

    let (status, json) = ask(&app, "free-auto", 1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["choices"][0]["message"]["content"], "from good");
    assert_eq!(json["model"], "m-good");
    assert_eq!(bad_hits.load(Ordering::SeqCst), 1);
    assert_eq!(good_hits.load(Ordering::SeqCst), 1);

    // The failed member is now deprioritised, so the next request goes straight to "good".
    let (status, _) = ask(&app, "free-auto", 2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bad_hits.load(Ordering::SeqCst), 1);
    assert_eq!(good_hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn round_robin_pool_alternates_members() {
    let (a_url, a_hits) = start_mock("a", false).await;
    let (b_url, b_hits) = start_mock("b", false).await;
    let (app, _) = gateway(vec![
        provider("a", a_url, "m-a"),
        provider("b", b_url, "m-b"),
    ])
    .await;

    let (status, _) = create_pool(
        &app,
        json!({"id": "rr", "strategy": "round_robin", "members": [
            {"provider_id": "a", "model": "m-a"},
            {"provider_id": "b", "model": "m-b"}
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for i in 0..6 {
        let (status, _) = ask(&app, "rr", i).await;
        assert_eq!(status, StatusCode::OK);
    }
    assert_eq!(a_hits.load(Ordering::SeqCst), 3);
    assert_eq!(b_hits.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn weighted_pool_respects_weights() {
    let (a_url, a_hits) = start_mock("a", false).await;
    let (b_url, b_hits) = start_mock("b", false).await;
    let (app, _) = gateway(vec![
        provider("a", a_url, "m-a"),
        provider("b", b_url, "m-b"),
    ])
    .await;

    create_pool(
        &app,
        json!({"id": "wrr", "strategy": "weighted_round_robin", "members": [
            {"provider_id": "a", "model": "m-a", "weight": 3},
            {"provider_id": "b", "model": "m-b", "weight": 1}
        ]}),
    )
    .await;

    for i in 0..8 {
        ask(&app, "wrr", i).await;
    }
    assert_eq!(a_hits.load(Ordering::SeqCst), 6);
    assert_eq!(b_hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn pool_with_all_members_failing_returns_error() {
    let (bad_url, _) = start_mock("bad", true).await;
    let (app, _) = gateway(vec![provider("bad", bad_url, "m-bad")]).await;
    create_pool(
        &app,
        json!({"id": "doomed", "strategy": "lru", "members": [{"provider_id": "bad", "model": "m-bad"}]}),
    )
    .await;

    let (status, _) = ask(&app, "doomed", 1).await;
    assert!(status.is_server_error());
}

#[tokio::test]
async fn streaming_request_to_pool_falls_back_and_ends_with_done() {
    let (bad_url, _) = start_mock("bad", true).await;
    let (good_url, _) = start_mock("good", false).await;
    let (app, _) = gateway(vec![
        provider("bad", bad_url, "m-bad"),
        provider("good", good_url, "m-good"),
    ])
    .await;
    create_pool(
        &app,
        json!({"id": "s", "strategy": "priority", "members": [
            {"provider_id": "bad", "model": "m-bad"},
            {"provider_id": "good", "model": "m-good"}
        ]}),
    )
    .await;

    let (status, body) = send(
        &app,
        req(
            "POST",
            "/v1/chat/completions",
            Some(json!({"model": "s", "stream": true, "messages": [{"role": "user", "content": "hi"}]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("from good"));
    assert!(body.contains("data: [DONE]"));
}

#[tokio::test]
async fn pool_admin_api_validation_and_lifecycle() {
    let (url, _) = start_mock("a", false).await;
    let (app, state) = gateway(vec![provider("a", url, "m-a")]).await;
    let ok_member = json!({"provider_id": "a", "model": "m-a"});

    // Bad name, real-model name, unknown provider/model, bad weight, bad strategy, empty
    for body in [
        json!({"id": "Bad Name", "strategy": "lru", "members": [ok_member.clone()]}),
        json!({"id": "m-a", "strategy": "lru", "members": [ok_member.clone()]}),
        json!({"id": "p", "strategy": "lru", "members": [{"provider_id": "zzz", "model": "m-a"}]}),
        json!({"id": "p", "strategy": "lru", "members": [{"provider_id": "a", "model": "nope"}]}),
        json!({"id": "p", "strategy": "lru", "members": [{"provider_id": "a", "model": "m-a", "weight": 0}]}),
        json!({"id": "p", "strategy": "random", "members": [ok_member.clone()]}),
        json!({"id": "p", "strategy": "lru", "members": []}),
        json!({"id": "p", "strategy": "lru", "members": [ok_member.clone(), ok_member.clone()]}),
    ] {
        let (status, text) = create_pool(&app, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{} -> {}", body, text);
    }
    assert!(state.pools.list().is_empty());

    // Create, duplicate, list, models endpoint, update, delete
    let (status, _) = create_pool(
        &app,
        json!({"id": "mine", "strategy": "lru", "members": [ok_member.clone()]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_pool(
        &app,
        json!({"id": "mine", "strategy": "lru", "members": [ok_member.clone()]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, body) = send(&app, req("GET", "/admin/pools", None)).await;
    assert!(body.contains("\"mine\""));

    let (_, body) = send(&app, req("GET", "/v1/models", None)).await;
    let models: Value = serde_json::from_str(&body).unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"mine"));
    assert!(ids.contains(&"m-a"));

    let (status, _) = send(
        &app,
        req(
            "PUT",
            "/admin/pools/mine",
            Some(json!({"strategy": "priority", "members": [ok_member.clone()]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state.pools.list()[0].strategy.as_str(), "priority");

    let (status, _) = send(
        &app,
        req(
            "PUT",
            "/admin/pools/ghost",
            Some(json!({"strategy": "lru", "members": [ok_member]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(&app, req("DELETE", "/admin/pools/mine", None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app, req("DELETE", "/admin/pools/mine", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(state.pools.list().is_empty());
}

#[tokio::test]
async fn pools_persist_to_database() {
    let (url, _) = start_mock("a", false).await;
    let (app, state) = gateway(vec![provider("a", url, "m-a")]).await;
    create_pool(
        &app,
        json!({"id": "saved", "strategy": "round_robin", "members": [{"provider_id": "a", "model": "m-a"}]}),
    )
    .await;

    let stored = halloroute::pools::store::list_pools(&state.db_pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, "saved");

    send(&app, req("DELETE", "/admin/pools/saved", None)).await;
    assert!(halloroute::pools::store::list_pools(&state.db_pool)
        .await
        .unwrap()
        .is_empty());
}

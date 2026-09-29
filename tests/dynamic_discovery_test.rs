use axum::{extract::Json, http::StatusCode, routing::get, Router};
use rust_agent::providers::{
    discover_provider,
    types::{ModelInfo, ModelsResponse, ProviderTemplate},
    FreeModelFilter,
};
use serde_json::json;

async fn mock_models_handler() -> (StatusCode, Json<ModelsResponse>) {
    let resp = ModelsResponse {
        data: vec![
            // Free models
            ModelInfo {
                id: "meta-llama/llama-3-8b-instruct:free".to_string(),
                object: "model".to_string(),
                owned_by: "meta".to_string(),
                pricing: Some(json!({
                    "prompt": "0",
                    "completion": "0"
                })),
            },
            ModelInfo {
                id: "mistralai/mistral-7b-instruct:free".to_string(),
                object: "model".to_string(),
                owned_by: "mistral".to_string(),
                pricing: Some(json!({
                    "prompt": "0",
                    "completion": "0"
                })),
            },
            // Paid models (MUST be filtered out!)
            ModelInfo {
                id: "openai/gpt-4o".to_string(),
                object: "model".to_string(),
                owned_by: "openai".to_string(),
                pricing: Some(json!({
                    "prompt": "0.000005",
                    "completion": "0.000015"
                })),
            },
            ModelInfo {
                id: "anthropic/claude-3.5-sonnet".to_string(),
                object: "model".to_string(),
                owned_by: "anthropic".to_string(),
                pricing: Some(json!({
                    "prompt": "0.000003",
                    "completion": "0.000015"
                })),
            },
        ],
    };
    (StatusCode::OK, Json(resp))
}

async fn start_mock_discovery_server() -> (String, tokio::sync::oneshot::Sender<()>) {
    let app = Router::new().route("/models", get(mock_models_handler));

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

    (base_url, shutdown_tx)
}

#[tokio::test]
async fn test_dynamic_discovery_filters_out_paid_models() {
    let (mock_url, _shutdown) = start_mock_discovery_server().await;

    let template = ProviderTemplate::new(
        "openrouter_test",
        "OpenRouter Discovery Test",
        &mock_url,
        "api_key",
        "OPENROUTER_API_KEY",
        Some(1_000_000),
        None,
        60,
        FreeModelFilter::PricingAPI {
            field: "pricing.prompt".into(),
            zero_means_free: true,
        },
    );

    let provider = discover_provider(&template, Some("dummy_key".to_string()))
        .await
        .expect("Discovery must succeed")
        .expect("Provider must be discovered");

    assert_eq!(provider.id, "openrouter_test");
    // Only 2 free models should remain, 0 paid models
    assert_eq!(provider.models.len(), 2);
    assert!(provider
        .models
        .contains(&"meta-llama/llama-3-8b-instruct:free".to_string()));
    assert!(provider
        .models
        .contains(&"mistralai/mistral-7b-instruct:free".to_string()));
    assert!(!provider.models.contains(&"openai/gpt-4o".to_string()));
    assert!(!provider
        .models
        .contains(&"anthropic/claude-3.5-sonnet".to_string()));
}

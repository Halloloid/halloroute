use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::server::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> ApiResult<Json<ChatCompletionResponse>> {
    tracing::debug!(model = %req.model, "Received chat completions request");

    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::router::Router::new(registry, crate::router::RoutingStrategy::Auto);

    let provider = router
        .select_provider(&state.db_pool, Some(&req.model))
        .await?
        .clone();

    tracing::info!(
        model = %req.model,
        provider_id = %provider.id,
        "Routing to provider"
    );

    let message_text = req
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let estimated_tokens = (message_text.len() / 4) as i64;

    if !crate::quota::QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await?
    {
        return Err(crate::errors::ApiError::QuotaExceeded(provider.id.clone()));
    }

    let client = reqwest::Client::new();
    let provider_url = format!("{}/chat/completions", provider.api_endpoint);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());

    if provider.auth_type == "api_key" {
        if let Ok(api_key) = std::env::var(format!("{}_API_KEY", provider.id.to_uppercase())) {
            headers.insert(
                "Authorization",
                format!("Bearer {}", api_key).parse().unwrap(),
            );
        }
    }

    let provider_request = json!({
        "model": req.model,
        "messages": req.messages,
        "temperature": req.temperature.unwrap_or(0.7),
        "max_tokens": req.max_tokens.unwrap_or(2000),
    });

    let response = client
        .post(&provider_url)
        .headers(headers)
        .json(&provider_request)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                provider_id = %provider.id,
                error = %e,
                "Provider request failed"
            );
            crate::errors::ApiError::ProviderError(format!("Provider request failed: {}", e))
        })?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        tracing::error!(
            provider_id = %provider.id,
            status = status.as_u16(),
            error = %error_text,
            "Provider returned error"
        );
        return Err(crate::errors::ApiError::ProviderError(format!(
            "Provider returned error: {}",
            error_text
        )));
    }

    let provider_response: serde_json::Value = response.json().await.map_err(|e| {
        crate::errors::ApiError::ProviderError(format!("Failed to parse provider response: {}", e))
    })?;

    let response_content = provider_response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("No response")
        .to_string();

    let completion_response = ChatCompletionResponse {
        id: format!("rust-agent-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: req.model.clone(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: response_content,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        },
    };

    crate::quota::QuotaManager::record_usage(&state.db_pool, &provider.id, estimated_tokens)
        .await?;

    Ok(Json(completion_response))
}

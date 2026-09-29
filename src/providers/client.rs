use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse, Choice, Message, Usage};
use crate::errors::ApiError;
use crate::providers::{CircuitBreaker, Provider};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;

/// Provider HTTP client with circuit breaker support
#[derive(Clone)]
pub struct ProviderClient {
    http_client: Arc<Client>,
    provider: Provider,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl ProviderClient {
    /// Create a new provider client
    pub fn new(http_client: Arc<Client>, provider: Provider) -> Self {
        let circuit_breaker = Arc::new(CircuitBreaker::new(provider.id.clone()));
        ProviderClient {
            http_client,
            provider,
            circuit_breaker,
        }
    }

    /// Get reference to circuit breaker
    pub fn circuit_breaker(&self) -> &CircuitBreaker {
        &self.circuit_breaker
    }

    /// Make a chat completion request to the provider
    pub async fn chat_completions(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ApiError> {
        let provider_id = self.provider.id.clone();

        // Check circuit breaker first
        let circuit_breaker = self.circuit_breaker.clone();
        let http_client = self.http_client.clone();
        let provider = self.provider.clone();
        let req_clone = req.clone();

        circuit_breaker
            .call(async {
                Self::execute_chat_request(http_client.as_ref(), &provider, &req_clone)
                    .await
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| {
                tracing::error!(
                    provider_id = %provider_id,
                    error = %e,
                    "Circuit breaker rejected request"
                );
                ApiError::ProviderError(format!("Provider unavailable (circuit open): {}", e))
            })
    }

    /// Execute the actual HTTP request to the provider
    async fn execute_chat_request(
        http_client: &Client,
        provider: &Provider,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ApiError> {
        let provider_url = format!("{}/chat/completions", provider.api_endpoint);

        // Build headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());

        // Add authentication if required
        if provider.auth_type == "api_key" {
            if let Ok(api_key) = std::env::var(format!("{}_API_KEY", provider.id.to_uppercase())) {
                headers.insert(
                    "Authorization",
                    format!("Bearer {}", api_key).parse().unwrap(),
                );
            }
        }

        // Build provider request (normalize to OpenAI format)
        let provider_request = json!({
            "model": req.model,
            "messages": req.messages,
            "temperature": req.temperature.unwrap_or(0.7),
            "max_tokens": req.max_tokens.unwrap_or(2000),
            "stream": req.stream.unwrap_or(false),
        });

        tracing::debug!(
            provider_id = %provider.id,
            provider_url = %provider_url,
            model = %req.model,
            "Sending request to provider"
        );

        let response = http_client
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
                    "HTTP request failed"
                );
                if e.is_timeout() {
                    ApiError::RequestTimeout
                } else {
                    ApiError::ProviderError(format!("Request failed: {}", e))
                }
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

            // 5xx errors should trigger circuit breaker retry
            if status.is_server_error() {
                return Err(ApiError::ProviderError(format!(
                    "Provider error ({}): {}",
                    status, error_text
                )));
            }

            // 4xx errors are client errors, don't retry
            return Err(ApiError::ProviderError(format!(
                "Provider error ({}): {}",
                status, error_text
            )));
        }

        // Parse provider response
        let provider_response: serde_json::Value = response.json().await.map_err(|e| {
            tracing::error!(
                provider_id = %provider.id,
                error = %e,
                "Failed to parse provider response"
            );
            ApiError::ProviderError(format!("Failed to parse provider response: {}", e))
        })?;

        // Extract response content
        let response_content = provider_response["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No response")
            .to_string();

        let prompt_tokens = provider_response["usage"]["prompt_tokens"]
            .as_i64()
            .unwrap_or(100) as i32;
        let completion_tokens = provider_response["usage"]["completion_tokens"]
            .as_i64()
            .unwrap_or(50) as i32;
        let total_tokens = provider_response["usage"]["total_tokens"]
            .as_i64()
            .unwrap_or((prompt_tokens + completion_tokens) as i64)
            as i32;

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
                prompt_tokens,
                completion_tokens,
                total_tokens,
            },
        };

        tracing::debug!(
            provider_id = %provider.id,
            model = %req.model,
            "Successfully received response from provider"
        );

        Ok(completion_response)
    }
}

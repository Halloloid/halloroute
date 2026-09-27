---
name: provider-integration
description: Provider API integration patterns, request/response normalization, provider-specific quirks, format adaptation for RustAgent
used_by: RustAgent Gateway
---

# Provider Integration

Call multiple free-tier AI providers with consistent interface.

## Provider Interface

```rust
#[async_trait::async_trait]
pub trait ProviderClient: Send + Sync {
    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError>;

    async fn stream_chat(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>, ProviderError>;

    async fn get_models(&self) -> Result<Vec<String>, ProviderError>;
    
    async fn health_check(&self) -> Result<bool, ProviderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Rate limited")]
    RateLimited,
    #[error("Invalid API key")]
    InvalidAuth,
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Request failed: {0}")]
    RequestFailed(String),
    #[error("Timeout")]
    Timeout,
}
```

## OpenCode Provider

```rust
pub struct OpenCodeClient {
    client: Arc<reqwest::Client>,
    base_url: String,
}

impl OpenCodeClient {
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        OpenCodeClient {
            client,
            base_url: "https://api.opencode.ai/v1".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for OpenCodeClient {
    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let opencode_req = serde_json::json!({
            "model": req.model,
            "messages": req.messages,
            "temperature": req.temperature.unwrap_or(0.7),
            "max_tokens": req.max_tokens.unwrap_or(2000),
        });

        let response = self
            .client
            .post(&url)
            .json(&opencode_req)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if response.status() == 429 {
            return Err(ProviderError::RateLimited);
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        Ok(ChatCompletionResponse {
            id: data["id"].as_str().unwrap_or("unknown").to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: req.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: data["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: data["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
                completion_tokens: data["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
                total_tokens: data["usage"]["total_tokens"].as_i64().unwrap_or(0) as i32,
            },
        })
    }

    async fn stream_chat(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>, ProviderError> {
        // Implementation similar to streaming handler
        todo!()
    }

    async fn get_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec![
            "deepseek-v3.2".to_string(),
            "nemotron-3".to_string(),
        ])
    }

    async fn health_check(&self) -> Result<bool, ProviderError> {
        let url = format!("{}/models", self.base_url);
        match self.client.get(&url).send().await {
            Ok(res) => Ok(res.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
```

## Groq Provider

```rust
pub struct GroqClient {
    client: Arc<reqwest::Client>,
    api_key: String,
    base_url: String,
}

impl GroqClient {
    pub fn new(client: Arc<reqwest::Client>, api_key: String) -> Self {
        GroqClient {
            client,
            api_key,
            base_url: "https://api.groq.com/v1".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ProviderClient for GroqClient {
    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .map_err(|_| ProviderError::InvalidAuth)?,
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&req)
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        if response.status() == 401 {
            return Err(ProviderError::InvalidAuth);
        }
        if response.status() == 429 {
            return Err(ProviderError::RateLimited);
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        Ok(ChatCompletionResponse {
            id: data["id"].as_str().unwrap_or("unknown").to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: req.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: data["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: data["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
                completion_tokens: data["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
                total_tokens: data["usage"]["total_tokens"].as_i64().unwrap_or(0) as i32,
            },
        })
    }

    async fn stream_chat(
        &self,
        req: &ChatCompletionRequest,
    ) -> Result<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>, ProviderError> {
        todo!()
    }

    async fn get_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec![
            "mixtral-8x7b-32768".to_string(),
            "llama-3-70b".to_string(),
            "llama-3-8b".to_string(),
        ])
    }

    async fn health_check(&self) -> Result<bool, ProviderError> {
        let url = format!("{}/models", self.base_url);
        match self.client.get(&url).send().await {
            Ok(res) => Ok(res.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
```

## Provider Factory

```rust
pub struct ProviderFactory {
    http_client: Arc<reqwest::Client>,
}

impl ProviderFactory {
    pub fn new(http_client: Arc<reqwest::Client>) -> Self {
        ProviderFactory { http_client }
    }

    pub fn create_client(&self, provider: &Provider) -> Result<Box<dyn ProviderClient>, ProviderError> {
        match provider.id.as_str() {
            "opencode" => Ok(Box::new(OpenCodeClient::new(Arc::clone(&self.http_client)))),
            "groq" => {
                let api_key = std::env::var("GROQ_API_KEY")
                    .map_err(|_| ProviderError::InvalidAuth)?;
                Ok(Box::new(GroqClient::new(Arc::clone(&self.http_client), api_key)))
            }
            _ => Err(ProviderError::RequestFailed("Unknown provider".to_string())),
        }
    }
}
```

## Request/Response Normalization

```rust
pub struct RequestNormalizer;

impl RequestNormalizer {
    pub fn normalize_for_provider(
        req: &ChatCompletionRequest,
        provider_id: &str,
    ) -> serde_json::Value {
        match provider_id {
            "opencode" => serde_json::json!({
                "model": req.model,
                "messages": req.messages,
                "temperature": req.temperature.unwrap_or(0.7),
                "max_tokens": req.max_tokens.unwrap_or(2000),
            }),
            "groq" => serde_json::json!({
                "model": req.model,
                "messages": req.messages,
                "temperature": req.temperature.unwrap_or(0.7),
                "max_tokens": req.max_tokens.unwrap_or(4096),
            }),
            _ => serde_json::json!({
                "model": req.model,
                "messages": req.messages,
            }),
        }
    }
}
```

## Error Handling & Retries

```rust
pub async fn call_provider_with_retry(
    client: &dyn ProviderClient,
    req: &ChatCompletionRequest,
    max_retries: u32,
) -> Result<ChatCompletionResponse, ProviderError> {
    let mut attempt = 0;
    let mut delay = std::time::Duration::from_millis(100);

    loop {
        match client.chat_completion(req).await {
            Ok(response) => return Ok(response),
            Err(ProviderError::RateLimited) if attempt < max_retries => {
                attempt += 1;
                tracing::warn!("Rate limited, retrying in {:?}", delay);
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
            Err(e) => return Err(e),
        }
    }
}
```

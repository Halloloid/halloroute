---
name: error-handling-recovery
description: Comprehensive error handling, graceful degradation, user-friendly error messages, retry/backoff decision trees
used_by: RustAgent Gateway
---

# Error Handling & Recovery

Robust error handling across all layers.

## Error Types

```rust
use serde::Serialize;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error, Serialize)]
pub enum ApiError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Quota exceeded")]
    QuotaExceeded,

    #[error("Rate limited")]
    RateLimited,

    #[error("All providers unavailable")]
    AllProvidersUnavailable,

    #[error("Model not supported: {0}")]
    ModelNotSupported(String),

    #[error("Context window exceeded")]
    ContextWindowExceeded,

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Internal server error")]
    InternalError,

    #[error("Unauthorized")]
    Unauthorized,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match self {
            ApiError::ProviderNotFound(ref p) => {
                (StatusCode::NOT_FOUND, "PROVIDER_NOT_FOUND", format!("Provider {} not found", p))
            }
            ApiError::QuotaExceeded => {
                (StatusCode::TOO_MANY_REQUESTS, "QUOTA_EXCEEDED", "Daily quota exceeded".to_string())
            }
            ApiError::RateLimited => {
                (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", "Rate limit exceeded".to_string())
            }
            ApiError::AllProvidersUnavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE", "All providers unavailable".to_string())
            }
            ApiError::ModelNotSupported(ref m) => {
                (StatusCode::BAD_REQUEST, "MODEL_NOT_SUPPORTED", format!("Model {} not supported", m))
            }
            ApiError::ContextWindowExceeded => {
                (StatusCode::BAD_REQUEST, "CONTEXT_EXCEEDED", "Request exceeds context window".to_string())
            }
            ApiError::InvalidRequest(ref msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_REQUEST", msg.clone())
            }
            ApiError::ProviderError(ref e) => {
                (StatusCode::BAD_GATEWAY, "PROVIDER_ERROR", e.clone())
            }
            ApiError::Timeout => {
                (StatusCode::GATEWAY_TIMEOUT, "TIMEOUT", "Request timeout".to_string())
            }
            ApiError::InternalError => {
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "Internal server error".to_string())
            }
            ApiError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Unauthorized".to_string())
            }
        };

        let error_response = serde_json::json!({
            "error": {
                "message": message,
                "type": error_code,
                "code": status.as_u16(),
            }
        });

        (status, axum::Json(error_response)).into_response()
    }
}
```

## Retry Logic

```rust
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

pub async fn with_retry<F, T, E>(
    mut f: F,
    config: RetryConfig,
) -> Result<T, E>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    let mut delay_ms = config.initial_delay_ms;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;

                if attempt > config.max_retries {
                    tracing::error!("Max retries exceeded: {}", e);
                    return Err(e);
                }

                let jitter = rand::random::<u64>() % 100;
                let actual_delay = std::time::Duration::from_millis((delay_ms + jitter).min(config.max_delay_ms));

                tracing::warn!(
                    attempt = attempt,
                    delay_ms = actual_delay.as_millis(),
                    "Retrying after error: {}", e
                );

                tokio::time::sleep(actual_delay).await;
                delay_ms = ((delay_ms as f64) * config.backoff_multiplier) as u64;
            }
        }
    }
}
```

## Graceful Degradation

```rust
pub async fn route_with_degradation(
    state: &Arc<AppState>,
    model: &str,
    req: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ApiError> {
    // Try primary routing strategy
    match route_optimal(state, model, req).await {
        Ok(response) => return Ok(response),
        Err(e) => tracing::warn!("Primary route failed: {}", e),
    }

    // Fall back to secondary strategy
    match route_fallback(state, model, req).await {
        Ok(response) => {
            tracing::info!("Fallback route succeeded");
            return Ok(response);
        }
        Err(e) => tracing::warn!("Fallback route failed: {}", e),
    }

    // Last resort: basic provider
    match route_basic(state, model, req).await {
        Ok(response) => {
            tracing::info!("Basic route succeeded");
            return Ok(response);
        }
        Err(e) => {
            tracing::error!("All routing strategies failed: {}", e);
            Err(ApiError::AllProvidersUnavailable)
        }
    }
}

pub async fn route_optimal(
    state: &Arc<AppState>,
    model: &str,
    req: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ApiError> {
    let provider = select_provider_by_latency(state, model).await?;
    call_provider_with_timeout(&provider, req, std::time::Duration::from_secs(30)).await
}

pub async fn route_fallback(
    state: &Arc<AppState>,
    model: &str,
    req: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ApiError> {
    let provider = select_provider_by_quota(state, model).await?;
    call_provider_with_timeout(&provider, req, std::time::Duration::from_secs(60)).await
}

pub async fn route_basic(
    state: &Arc<AppState>,
    model: &str,
    req: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ApiError> {
    let provider = select_first_available_provider(state, model).await?;
    call_provider_with_timeout(&provider, req, std::time::Duration::from_secs(120)).await
}
```

## Timeout Handling

```rust
pub async fn call_provider_with_timeout(
    provider: &Provider,
    req: &ChatCompletionRequest,
    timeout: std::time::Duration,
) -> Result<ChatCompletionResponse, ApiError> {
    tokio::time::timeout(timeout, call_provider(provider, req))
        .await
        .map_err(|_| ApiError::Timeout)?
}
```

## Error Recovery

```rust
pub struct ErrorRecoveryStrategy {
    max_failures: u32,
    recovery_delay: std::time::Duration,
}

pub struct FailedProvider {
    pub provider_id: String,
    pub failure_count: u32,
    pub last_failure: std::time::Instant,
}

pub struct ErrorRecoveryManager {
    failed_providers: Arc<DashMap<String, FailedProvider>>,
    strategy: ErrorRecoveryStrategy,
}

impl ErrorRecoveryManager {
    pub fn new(max_failures: u32) -> Self {
        ErrorRecoveryManager {
            failed_providers: Arc::new(DashMap::new()),
            strategy: ErrorRecoveryStrategy {
                max_failures,
                recovery_delay: std::time::Duration::from_secs(60),
            },
        }
    }

    pub async fn record_failure(&self, provider_id: &str) {
        self.failed_providers
            .entry(provider_id.to_string())
            .and_modify(|fp| {
                fp.failure_count += 1;
                fp.last_failure = std::time::Instant::now();
            })
            .or_insert_with(|| FailedProvider {
                provider_id: provider_id.to_string(),
                failure_count: 1,
                last_failure: std::time::Instant::now(),
            });
    }

    pub fn is_provider_available(&self, provider_id: &str) -> bool {
        if let Some(failed) = self.failed_providers.get(provider_id) {
            if failed.failure_count >= self.strategy.max_failures {
                // Check if recovery time has passed
                if failed.last_failure.elapsed() > self.strategy.recovery_delay {
                    drop(failed);
                    self.failed_providers.remove(provider_id);
                    return true;
                }
                return false;
            }
        }
        true
    }

    pub async fn reset_provider(&self, provider_id: &str) {
        self.failed_providers.remove(provider_id);
        tracing::info!("Provider {} recovered", provider_id);
    }
}
```

## User-Friendly Error Messages

```rust
pub fn user_friendly_error(error: &ApiError) -> String {
    match error {
        ApiError::QuotaExceeded => {
            "Daily token limit reached. Please try again tomorrow.".to_string()
        }
        ApiError::RateLimited => {
            "Too many requests. Please wait a moment and try again.".to_string()
        }
        ApiError::AllProvidersUnavailable => {
            "Service temporarily unavailable. Please try again in a few minutes.".to_string()
        }
        ApiError::ModelNotSupported(model) => {
            format!("Model '{}' is not available. Please try a different model.", model)
        }
        ApiError::ContextWindowExceeded => {
            "Your message is too long for this model. Please make it shorter.".to_string()
        }
        ApiError::Timeout => {
            "Request took too long. Please try again.".to_string()
        }
        _ => {
            "An error occurred. Please try again later.".to_string()
        }
    }
}
```

use crate::errors::ApiError;
use std::time::Duration;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f32,
    /// Whether to add random jitter to delay
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Determine if an error should be retried
fn should_retry(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::RequestTimeout | ApiError::ProviderError(_) | ApiError::InternalError(_)
    )
}

/// Calculate next delay with exponential backoff and optional jitter
fn calculate_backoff(current: Duration, config: &RetryConfig) -> Duration {
    let mut next = Duration::from_secs_f32(current.as_secs_f32() * config.backoff_multiplier);

    // Cap at max delay
    if next > config.max_delay {
        next = config.max_delay;
    }

    // Add jitter if enabled
    if config.jitter {
        let jitter_ms = rand::random::<u64>() % 100;
        next = next.saturating_add(Duration::from_millis(jitter_ms));
    }

    next
}

/// Retry a future with exponential backoff
pub async fn retry_with_backoff<F, T>(mut f: F, config: &RetryConfig) -> Result<T, ApiError>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send>>,
{
    let mut retries = 0;
    let mut delay = config.initial_delay;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if retries >= config.max_retries || !should_retry(&e) {
                    return Err(e);
                }

                tracing::warn!(
                    attempt = retries + 1,
                    max_retries = config.max_retries,
                    delay_ms = delay.as_millis(),
                    error = %e,
                    "Retrying request after error"
                );

                tokio::time::sleep(delay).await;
                delay = calculate_backoff(delay, config);
                retries += 1;
            }
        }
    }
}

/// Simplified retry helper for closures that return Result
pub async fn retry_fn<F, T>(mut f: F, config: &RetryConfig) -> Result<T, ApiError>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send>>,
{
    retry_with_backoff(&mut f, config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_retry() {
        assert!(should_retry(&ApiError::RequestTimeout));
        assert!(should_retry(&ApiError::ProviderError("error".to_string())));
        assert!(!should_retry(&ApiError::InvalidRequest(
            "error".to_string()
        )));
    }

    #[test]
    fn test_backoff_calculation() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: false,
        };

        let delay1 = Duration::from_millis(100);
        let delay2 = calculate_backoff(delay1, &config);
        assert_eq!(delay2, Duration::from_millis(200));

        let delay3 = calculate_backoff(delay2, &config);
        assert_eq!(delay3, Duration::from_millis(400));
    }

    #[test]
    fn test_backoff_max_delay() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 10.0,
            jitter: false,
        };

        let delay = Duration::from_secs(1);
        let next_delay = calculate_backoff(delay, &config);
        // Would be 10s but capped at 5s
        assert_eq!(next_delay, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_retry_with_backoff_success() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let config = RetryConfig::default();
        let attempt = Arc::new(AtomicU32::new(0));

        let attempt_clone = attempt.clone();
        let result = retry_with_backoff(
            move || {
                let attempt_clone = attempt_clone.clone();
                Box::pin(async move {
                    let cur = attempt_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    if cur < 3 {
                        Err(ApiError::ProviderError("temp error".to_string()))
                    } else {
                        Ok("success".to_string())
                    }
                })
            },
            &config,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempt.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_max_retries_exceeded() {
        let config = RetryConfig {
            max_retries: 2,
            ..Default::default()
        };

        let result: Result<String, ApiError> = retry_with_backoff(
            || Box::pin(async { Err(ApiError::ProviderError("always fails".to_string())) }),
            &config,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_retry_no_retry_on_client_error() {
        let config = RetryConfig::default();

        let result: Result<String, ApiError> = retry_with_backoff(
            || Box::pin(async { Err(ApiError::InvalidRequest("bad request".to_string())) }),
            &config,
        )
        .await;

        assert!(result.is_err());
    }
}

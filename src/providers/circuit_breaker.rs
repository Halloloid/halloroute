use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    /// Normal operation - requests pass through
    Closed,
    /// Failing - requests are rejected
    Open,
    /// Testing recovery - one request allowed
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "Closed"),
            CircuitState::Open => write!(f, "Open"),
            CircuitState::HalfOpen => write!(f, "HalfOpen"),
        }
    }
}

/// Circuit breaker error
#[derive(Debug, Clone)]
pub enum CircuitBreakerError {
    /// Circuit is open, rejecting requests
    CircuitOpen,
    /// Underlying request failed
    RequestFailed(String),
}

impl std::fmt::Display for CircuitBreakerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitBreakerError::CircuitOpen => write!(f, "Circuit breaker is open"),
            CircuitBreakerError::RequestFailed(err) => write!(f, "Request failed: {}", err),
        }
    }
}

/// Configuration for circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Time to wait before half-open state
    pub recovery_timeout: Duration,
    /// Number of successes needed to close circuit from half-open
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            success_threshold: 2,
        }
    }
}

/// Circuit breaker for provider resilience
pub struct CircuitBreaker {
    provider_id: String,
    state: Arc<RwLock<CircuitState>>,
    failure_count: Arc<AtomicU32>,
    success_count: Arc<AtomicU32>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker for a provider
    pub fn new(provider_id: String) -> Self {
        Self::with_config(provider_id, CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with custom config
    pub fn with_config(provider_id: String, config: CircuitBreakerConfig) -> Self {
        CircuitBreaker {
            provider_id,
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            failure_count: Arc::new(AtomicU32::new(0)),
            success_count: Arc::new(AtomicU32::new(0)),
            last_failure_time: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// Execute a request through the circuit breaker
    pub async fn call<F, T>(&self, f: F) -> Result<T, CircuitBreakerError>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        // Check if we need to transition from Open to HalfOpen
        let current_state = *self.state.read().await;

        if current_state == CircuitState::Open {
            if let Some(last_failure) = *self.last_failure_time.read().await {
                if last_failure.elapsed() >= self.config.recovery_timeout {
                    tracing::info!(
                        provider_id = %self.provider_id,
                        "Circuit breaker transitioning to HalfOpen"
                    );
                    *self.state.write().await = CircuitState::HalfOpen;
                    self.success_count.store(0, Ordering::SeqCst);
                } else {
                    return Err(CircuitBreakerError::CircuitOpen);
                }
            } else {
                return Err(CircuitBreakerError::CircuitOpen);
            }
        }

        // Execute the request
        match f.await {
            Ok(result) => {
                self.record_success().await;
                Ok(result)
            }
            Err(err) => {
                self.record_failure().await;
                Err(CircuitBreakerError::RequestFailed(err))
            }
        }
    }

    /// Record a successful call
    pub async fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);

        let current_state = *self.state.read().await;
        match current_state {
            CircuitState::Closed => {
                // No action needed, continue normal operation
            }
            CircuitState::HalfOpen => {
                self.success_count.fetch_add(1, Ordering::SeqCst);
                if self.success_count.load(Ordering::SeqCst) >= self.config.success_threshold {
                    tracing::info!(
                        provider_id = %self.provider_id,
                        "Circuit breaker transitioning to Closed"
                    );
                    *self.state.write().await = CircuitState::Closed;
                    self.success_count.store(0, Ordering::SeqCst);
                }
            }
            CircuitState::Open => {
                // Should not happen, but reset if it does
                *self.state.write().await = CircuitState::Closed;
                self.success_count.store(0, Ordering::SeqCst);
            }
        }
    }

    /// Record a failed call
    pub async fn record_failure(&self) {
        let new_failure_count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure_time.write().await = Some(Instant::now());

        let current_state = *self.state.read().await;
        match current_state {
            CircuitState::Closed => {
                if new_failure_count >= self.config.failure_threshold {
                    tracing::warn!(
                        provider_id = %self.provider_id,
                        failures = new_failure_count,
                        threshold = self.config.failure_threshold,
                        "Circuit breaker transitioning to Open"
                    );
                    *self.state.write().await = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open goes back to open
                tracing::warn!(
                    provider_id = %self.provider_id,
                    "Circuit breaker transitioning back to Open from HalfOpen"
                );
                *self.state.write().await = CircuitState::Open;
                self.success_count.store(0, Ordering::SeqCst);
            }
            CircuitState::Open => {
                // Already open, just track the time
            }
        }
    }

    /// Get current circuit state
    pub async fn state(&self) -> CircuitState {
        let current = *self.state.read().await;
        if current == CircuitState::Open {
            if let Some(last_failure) = *self.last_failure_time.read().await {
                if last_failure.elapsed() >= self.config.recovery_timeout {
                    *self.state.write().await = CircuitState::HalfOpen;
                    return CircuitState::HalfOpen;
                }
            }
        }
        current
    }

    /// Reset circuit breaker to closed state
    pub async fn reset(&self) {
        tracing::info!(
            provider_id = %self.provider_id,
            "Circuit breaker reset to Closed"
        );
        *self.state.write().await = CircuitState::Closed;
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        *self.last_failure_time.write().await = None;
    }

    /// Get current metrics
    pub async fn metrics(&self) -> CircuitBreakerMetrics {
        CircuitBreakerMetrics {
            state: *self.state.read().await,
            failure_count: self.failure_count.load(Ordering::SeqCst),
            success_count: self.success_count.load(Ordering::SeqCst),
            last_failure_time: *self.last_failure_time.read().await,
        }
    }
}

/// Circuit breaker metrics
#[derive(Debug, Clone)]
pub struct CircuitBreakerMetrics {
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
    pub last_failure_time: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_to_open() {
        let cb = CircuitBreaker::with_config(
            "test_provider".to_string(),
            CircuitBreakerConfig {
                failure_threshold: 3,
                recovery_timeout: Duration::from_secs(1),
                success_threshold: 2,
            },
        );

        assert_eq!(cb.state().await, CircuitState::Closed);

        // Record 3 failures
        for _ in 0..3 {
            cb.record_failure().await;
        }

        assert_eq!(cb.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_open_to_half_open() {
        let cb = CircuitBreaker::with_config(
            "test_provider".to_string(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                recovery_timeout: Duration::from_millis(100),
                success_threshold: 1,
            },
        );

        // Open the circuit
        for _ in 0..2 {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, CircuitState::Open);

        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Next call should transition to HalfOpen
        let result = cb
            .call(async { Ok::<_, String>("success".to_string()) })
            .await;
        assert!(result.is_ok());
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_call_when_open() {
        let cb = CircuitBreaker::with_config(
            "test_provider".to_string(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                recovery_timeout: Duration::from_secs(10),
                success_threshold: 1,
            },
        );

        // Open the circuit
        for _ in 0..2 {
            cb.record_failure().await;
        }

        // Try to call through open circuit
        let result = cb
            .call(async { Ok::<_, String>("success".to_string()) })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::CircuitOpen)));
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_failure_reopens() {
        let cb = CircuitBreaker::with_config(
            "test_provider".to_string(),
            CircuitBreakerConfig {
                failure_threshold: 1,
                recovery_timeout: Duration::from_millis(50),
                success_threshold: 2,
            },
        );

        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);

        tokio::time::sleep(Duration::from_millis(70)).await;

        // Call will allow 1 probe in HalfOpen, but let's record a failure
        let result = cb
            .call(async { Err::<String, String>("probe failed".to_string()) })
            .await;
        assert!(result.is_err());
        assert_eq!(cb.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_manual_reset_and_metrics() {
        let cb = CircuitBreaker::new("test_provider".to_string());
        for _ in 0..5 {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, CircuitState::Open);

        let metrics = cb.metrics().await;
        assert_eq!(metrics.state, CircuitState::Open);
        assert_eq!(metrics.failure_count, 5);
        assert!(metrics.last_failure_time.is_some());

        cb.reset().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
        let reset_metrics = cb.metrics().await;
        assert_eq!(reset_metrics.failure_count, 0);
        assert_eq!(reset_metrics.success_count, 0);
        assert!(reset_metrics.last_failure_time.is_none());
    }
}

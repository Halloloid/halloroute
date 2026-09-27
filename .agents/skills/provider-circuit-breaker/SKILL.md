---
name: provider-circuit-breaker
description: Circuit breaker pattern for detecting failing providers, auto-recovery, fallback chains, and resilient request routing with exponential backoff
used_by: RustAgent Gateway
---

# Provider Circuit Breaker Pattern

Implements resilient provider routing with circuit breaker, retry logic, and fallback chains.

## Circuit Breaker States

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,      // Normal operation, allow requests
    Open,        // Provider failing, block requests
    HalfOpen,    // Testing if provider recovered
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerMetrics {
    pub state: CircuitState,
    pub failure_count: u32,
    pub success_count: u32,
    pub last_failure_time: Option<Instant>,
    pub consecutive_successes: u32,
}

pub struct CircuitBreaker {
    failure_threshold: u32,      // Open after N failures
    success_threshold: u32,      // Close after N successes in HalfOpen
    timeout: Duration,           // Time before HalfOpen retry
    metrics: Arc<Mutex<CircuitBreakerMetrics>>,
}
```

## Circuit Breaker Implementation

```rust
impl CircuitBreaker {
    pub fn new(failure_threshold: u32, success_threshold: u32, timeout: Duration) -> Self {
        CircuitBreaker {
            failure_threshold,
            success_threshold,
            timeout,
            metrics: Arc::new(Mutex::new(CircuitBreakerMetrics {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure_time: None,
                consecutive_successes: 0,
            })),
        }
    }

    pub async fn call<F, T>(&self, f: F) -> Result<T, CircuitBreakerError>
    where
        F: Fn() -> Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error>>>>>,
    {
        let mut metrics = self.metrics.lock().await;

        match metrics.state {
            CircuitState::Open => {
                if let Some(last_failure) = metrics.last_failure_time {
                    if last_failure.elapsed() > self.timeout {
                        metrics.state = CircuitState::HalfOpen;
                        metrics.success_count = 0;
                    } else {
                        return Err(CircuitBreakerError::CircuitOpen);
                    }
                } else {
                    return Err(CircuitBreakerError::CircuitOpen);
                }
            }
            _ => {}
        }

        drop(metrics);

        match f().await {
            Ok(result) => {
                let mut metrics = self.metrics.lock().await;
                metrics.failure_count = 0;
                metrics.consecutive_successes += 1;

                if metrics.state == CircuitState::HalfOpen
                    && metrics.consecutive_successes >= self.success_threshold
                {
                    metrics.state = CircuitState::Closed;
                }

                Ok(result)
            }
            Err(err) => {
                let mut metrics = self.metrics.lock().await;
                metrics.failure_count += 1;
                metrics.last_failure_time = Some(Instant::now());
                metrics.consecutive_successes = 0;

                if metrics.failure_count >= self.failure_threshold {
                    metrics.state = CircuitState::Open;
                }

                Err(CircuitBreakerError::RequestFailed(err.to_string()))
            }
        }
    }

    pub async fn get_state(&self) -> CircuitState {
        self.metrics.lock().await.state
    }
}
```

## Retry with Exponential Backoff

```rust
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    pub async fn execute<F, T>(&self, mut f: F) -> Result<T, Box<dyn std::error::Error>>
    where
        F: FnMut() -> Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error>>>>>,
    {
        let mut attempt = 0;
        let mut delay = self.initial_delay;

        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    attempt += 1;

                    if attempt > self.max_retries {
                        return Err(err);
                    }

                    // Exponential backoff with jitter
                    let jitter = Duration::from_millis(rand::random::<u64>() % 100);
                    let actual_delay = delay.saturating_add(jitter);
                    tokio::time::sleep(actual_delay.min(self.max_delay)).await;

                    delay = delay.saturating_mul(2);

                    tracing::warn!(
                        attempt = attempt,
                        next_delay_ms = actual_delay.as_millis(),
                        "Retrying after error: {}", err
                    );
                }
            }
        }
    }
}
```

## Fallback Chain

```rust
pub struct FallbackChain {
    providers: Vec<String>,
    circuit_breakers: HashMap<String, CircuitBreaker>,
}

impl FallbackChain {
    pub async fn execute_with_fallback<F>(
        &self,
        f: F,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>
    where
        F: Fn(&str) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, Box<dyn std::error::Error>>>>>,
    {
        let mut last_error = None;

        for provider_id in &self.providers {
            if let Some(cb) = self.circuit_breakers.get(provider_id) {
                if cb.get_state().await == CircuitState::Open {
                    tracing::debug!(provider_id = %provider_id, "Skipping open circuit");
                    continue;
                }
            }

            match f(provider_id).await {
                Ok(result) => {
                    tracing::info!(provider_id = %provider_id, "Request succeeded");
                    return Ok(result);
                }
                Err(err) => {
                    last_error = Some(err);
                    tracing::warn!(
                        provider_id = %provider_id,
                        error = %last_error.as_ref().unwrap(),
                        "Provider failed, trying next"
                    );
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "All providers exhausted".into()))
    }
}
```

## Usage in Router

```rust
pub async fn route_with_resilience(
    state: &AppState,
    model: &str,
    request: ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ApiError> {
    let chain = state.get_fallback_chain();
    let retry_policy = RetryPolicy {
        max_retries: 3,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(5),
    };

    let result = chain
        .execute_with_fallback(|provider_id| {
            Box::pin(async move {
                retry_policy
                    .execute(|| {
                        Box::pin(call_provider(provider_id, &request))
                    })
                    .await
            })
        })
        .await?;

    Ok(result)
}
```

## Key Metrics to Track

- **Failure rate** — percentage of failed requests per provider
- **Response time** — latency for success vs failure
- **Circuit state changes** — when providers flip between states
- **Retry attempts** — how many retries per request
- **Fallback activations** — when primary provider was skipped

## Configuration

```toml
[circuit_breaker]
failure_threshold = 5          # Open circuit after 5 failures
success_threshold = 3          # Close circuit after 3 successes in HalfOpen
timeout_secs = 30             # Try HalfOpen after 30 seconds

[retry]
max_retries = 3
initial_delay_ms = 100
max_delay_secs = 5
```

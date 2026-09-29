// RustAgent - Lightweight AI Gateway
pub mod api;
pub mod cache;
pub mod config;
pub mod errors;
pub mod http;
pub mod middleware;
pub mod providers;
pub mod quota;
pub mod routing;
pub mod state;
pub mod token;

pub use cache::{CacheMetrics, CacheMetricsSnapshot, RequestDeduplicator};
pub use errors::{ApiError, ApiResult};
pub use http::{retry_fn, retry_with_backoff, HttpClient, RetryConfig};
pub use middleware::{AuthError, BearerAuth, LoggingMiddleware};
pub use providers::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitState, Provider,
    ProviderHealth, ProviderRegistry,
};
pub use quota::QuotaManager;
pub use routing::{FallbackSelector, Router, RoutingStrategy};
pub use state::AppState;
pub use token::{ContextWindowManager, TokenCounter, TokenUsageStats};

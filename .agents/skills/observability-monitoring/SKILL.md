---
name: observability-monitoring
description: Structured logging with context, metrics export (Prometheus format), trace correlation IDs, health check strategies, dashboard backend API
used_by: RustAgent Gateway
---

# Observability & Monitoring

Production-ready logging, metrics, and health checks.

## Structured Logging

```rust
use tracing::{info, warn, error, debug, Span};
use uuid::Uuid;

pub struct RequestContext {
    pub request_id: String,
    pub provider_id: String,
    pub model: String,
    pub user_id: Option<String>,
}

impl RequestContext {
    pub fn new(provider_id: String, model: String) -> Self {
        RequestContext {
            request_id: Uuid::new_v4().to_string(),
            provider_id,
            model,
            user_id: None,
        }
    }

    pub fn span(&self) -> Span {
        tracing::info_span!(
            "request",
            request_id = %self.request_id,
            provider = %self.provider_id,
            model = %self.model,
        )
    }
}

pub async fn log_request_lifecycle(ctx: &RequestContext) {
    let _span = ctx.span().entered();
    
    info!("Request started");
    
    // Work happens here
    
    info!("Request completed");
}
```

## Metrics Collection

```rust
use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
use std::time::Instant;

pub struct MetricsCollector {
    // Counters
    total_requests: AtomicU64,
    successful_requests: AtomicU64,
    failed_requests: AtomicU64,
    
    // Latencies
    total_latency_ms: AtomicI64,
    min_latency_ms: AtomicI64,
    max_latency_ms: AtomicI64,
    
    // Providers
    provider_requests: dashmap::DashMap<String, AtomicU64>,
    provider_failures: dashmap::DashMap<String, AtomicU64>,
}

impl MetricsCollector {
    pub fn new() -> Arc<Self> {
        Arc::new(MetricsCollector {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            total_latency_ms: AtomicI64::new(0),
            min_latency_ms: AtomicI64::new(i64::MAX),
            max_latency_ms: AtomicI64::new(0),
            provider_requests: dashmap::DashMap::new(),
            provider_failures: dashmap::DashMap::new(),
        })
    }

    pub fn record_request(&self, provider_id: &str, latency_ms: i64, success: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        
        if success {
            self.successful_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_requests.fetch_add(1, Ordering::Relaxed);
            self.provider_failures
                .entry(provider_id.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(1, Ordering::Relaxed);
        }

        self.total_latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
        
        let mut min = self.min_latency_ms.load(Ordering::Relaxed);
        while latency_ms < min {
            match self.min_latency_ms.compare_exchange(
                min,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => min = actual,
            }
        }

        let mut max = self.max_latency_ms.load(Ordering::Relaxed);
        while latency_ms > max {
            match self.max_latency_ms.compare_exchange(
                max,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => max = actual,
            }
        }

        self.provider_requests
            .entry(provider_id.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_metrics(&self) -> MetricsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let total_latency = self.total_latency_ms.load(Ordering::Relaxed);

        MetricsSnapshot {
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            error_rate: if total > 0 { (failed as f64 / total as f64) * 100.0 } else { 0.0 },
            avg_latency_ms: if total > 0 { total_latency / total as i64 } else { 0 },
            min_latency_ms: self.min_latency_ms.load(Ordering::Relaxed),
            max_latency_ms: self.max_latency_ms.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MetricsSnapshot {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub error_rate: f64,
    pub avg_latency_ms: i64,
    pub min_latency_ms: i64,
    pub max_latency_ms: i64,
}
```

## Health Checks

```rust
pub struct HealthChecker {
    providers: Arc<DashMap<String, Provider>>,
    metrics: Arc<MetricsCollector>,
}

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub service: String,
    pub timestamp: String,
    pub providers: Vec<ProviderHealth>,
    pub metrics: MetricsSnapshot,
}

#[derive(Debug, Serialize)]
pub struct ProviderHealth {
    pub provider_id: String,
    pub status: String,
    pub last_check: String,
    pub error_rate: f64,
    pub avg_latency_ms: i64,
}

impl HealthChecker {
    pub async fn check(&self) -> HealthStatus {
        let mut provider_status = Vec::new();

        for entry in self.providers.iter() {
            let provider = entry.value();
            
            provider_status.push(ProviderHealth {
                provider_id: provider.id.clone(),
                status: if provider.is_available() { "healthy".to_string() } else { "down".to_string() },
                last_check: chrono::Utc::now().to_rfc3339(),
                error_rate: 0.0, // Would fetch from metrics
                avg_latency_ms: 0,
            });
        }

        HealthStatus {
            status: if provider_status.iter().any(|p| p.status == "healthy") {
                "healthy".to_string()
            } else {
                "degraded".to_string()
            },
            service: "RustAgent".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            providers: provider_status,
            metrics: self.metrics.get_metrics(),
        }
    }
}
```

## Prometheus Metrics Export

```rust
pub fn export_prometheus_metrics(metrics: &MetricsCollector) -> String {
    let snap = metrics.get_metrics();
    
    format!(
        r#"# HELP rustAgent_total_requests Total number of requests
# TYPE rustAgent_total_requests counter
rustAgent_total_requests {{}} {}

# HELP rustAgent_successful_requests Successful requests
# TYPE rustAgent_successful_requests counter
rustAgent_successful_requests {{}} {}

# HELP rustAgent_failed_requests Failed requests
# TYPE rustAgent_failed_requests counter
rustAgent_failed_requests {{}} {}

# HELP rustAgent_error_rate Error rate percentage
# TYPE rustAgent_error_rate gauge
rustAgent_error_rate {{}} {}

# HELP rustAgent_avg_latency_ms Average latency in milliseconds
# TYPE rustAgent_avg_latency_ms gauge
rustAgent_avg_latency_ms {{}} {}

# HELP rustAgent_min_latency_ms Minimum latency in milliseconds
# TYPE rustAgent_min_latency_ms gauge
rustAgent_min_latency_ms {{}} {}

# HELP rustAgent_max_latency_ms Maximum latency in milliseconds
# TYPE rustAgent_max_latency_ms gauge
rustAgent_max_latency_ms {{}} {}
"#,
        snap.total_requests,
        snap.successful_requests,
        snap.failed_requests,
        snap.error_rate,
        snap.avg_latency_ms,
        snap.min_latency_ms,
        snap.max_latency_ms,
    )
}
```

## Dashboard Backend API

```rust
pub async fn get_dashboard_stats(
    State(state): State<Arc<AppState>>,
) -> Json<DashboardStats> {
    let health = state.health_checker.check().await;
    
    Json(DashboardStats {
        overall_status: health.status,
        timestamp: health.timestamp,
        providers: health.providers,
        metrics: health.metrics,
        quota_status: get_quota_status(&state).await,
    })
}

#[derive(Debug, Serialize)]
pub struct DashboardStats {
    pub overall_status: String,
    pub timestamp: String,
    pub providers: Vec<ProviderHealth>,
    pub metrics: MetricsSnapshot,
    pub quota_status: Vec<QuotaStatus>,
}

#[derive(Debug, Serialize)]
pub struct QuotaStatus {
    pub provider_id: String,
    pub daily_remaining: Option<i64>,
    pub daily_percentage: f64,
    pub monthly_remaining: Option<i64>,
    pub monthly_percentage: f64,
}
```

## Tracing Setup

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "halloroute=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}
```

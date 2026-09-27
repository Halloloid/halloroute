---
name: performance-optimization
description: Memory-efficient patterns, connection pooling, buffer management, concurrency limits, and resource optimization for RustAgent
used_by: RustAgent Gateway
---

# Performance & Memory Optimization

Target: <100MB idle RAM, sub-100ms routing latency.

## Connection Pooling

```rust
use reqwest::Client;
use std::sync::Arc;

pub struct HttpClientPool {
    client: Arc<reqwest::Client>,
}

impl HttpClientPool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .http2_prior_knowledge()
            .build()
            .expect("Failed to build HTTP client");

        HttpClientPool {
            client: Arc::new(client),
        }
    }

    pub fn get_client(&self) -> Arc<reqwest::Client> {
        Arc::clone(&self.client)
    }
}
```

## Concurrency Limits

```rust
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct ConcurrencyLimiter {
    semaphore: Arc<Semaphore>,
}

impl ConcurrencyLimiter {
    pub fn new(max_concurrent: usize) -> Self {
        ConcurrencyLimiter {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    pub async fn acquire(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.semaphore.acquire().await.expect("Semaphore poisoned")
    }
}

// Usage
pub async fn rate_limited_request(
    limiter: &ConcurrencyLimiter,
    provider: &Provider,
) -> Result<Response, ApiError> {
    let _permit = limiter.acquire().await;
    call_provider(provider).await
}
```

## Buffer Management

```rust
use bytes::{Bytes, BytesMut};

pub struct BufferPool {
    buffers: Arc<tokio::sync::Mutex<Vec<BytesMut>>>,
    buffer_size: usize,
    max_buffers: usize,
}

impl BufferPool {
    pub fn new(buffer_size: usize, max_buffers: usize) -> Self {
        BufferPool {
            buffers: Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(max_buffers))),
            buffer_size,
            max_buffers,
        }
    }

    pub async fn get_buffer(&self) -> BytesMut {
        let mut buffers = self.buffers.lock().await;
        buffers.pop().unwrap_or_else(|| BytesMut::with_capacity(self.buffer_size))
    }

    pub async fn return_buffer(&self, mut buffer: BytesMut) {
        buffer.clear();
        let mut buffers = self.buffers.lock().await;
        if buffers.len() < self.max_buffers {
            buffers.push(buffer);
        }
    }
}
```

## Memory-Efficient Quota Tracker

```rust
use dashmap::DashMap;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct OptimizedQuotaTracker {
    // Use atomic integers instead of full entries for frequently accessed data
    daily_usage: Arc<DashMap<String, AtomicI64>>,
    monthly_usage: Arc<DashMap<String, AtomicI64>>,
}

impl OptimizedQuotaTracker {
    pub async fn add_tokens_atomic(&self, provider_id: &str, tokens: i64) {
        let key = format!("{}:{}", provider_id, chrono::Local::now().format("%Y-%m-%d"));
        
        // Atomic operation, no lock needed
        self.daily_usage
            .entry(key)
            .or_insert_with(|| AtomicI64::new(0))
            .fetch_add(tokens, Ordering::Relaxed);
    }

    pub async fn get_usage_atomic(&self, provider_id: &str) -> i64 {
        let key = format!("{}:{}", provider_id, chrono::Local::now().format("%Y-%m-%d"));
        
        self.daily_usage
            .get(&key)
            .map(|entry| entry.load(Ordering::Relaxed))
            .unwrap_or(0)
    }
}
```

## Lazy Initialization

```rust
use once_cell::sync::Lazy;

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("Failed to build HTTP client")
});

pub fn get_http_client() -> &'static reqwest::Client {
    &*HTTP_CLIENT
}
```

## Request Deduplication

```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn compute_request_hash(req: &ChatCompletionRequest) -> u64 {
    let mut hasher = DefaultHasher::new();
    
    // Hash only the immutable parts
    req.model.hash(&mut hasher);
    req.messages.iter().for_each(|m| m.content.hash(&mut hasher));
    req.temperature.hash(&mut hasher);
    
    hasher.finish()
}

pub struct RequestCache {
    cache: Arc<DashMap<u64, CachedResponse>>,
    ttl: std::time::Duration,
}

#[derive(Clone)]
pub struct CachedResponse {
    pub response: ChatCompletionResponse,
    pub created_at: std::time::Instant,
}

impl RequestCache {
    pub fn new(ttl: std::time::Duration) -> Self {
        RequestCache {
            cache: Arc::new(DashMap::new()),
            ttl,
        }
    }

    pub fn get(&self, hash: u64) -> Option<ChatCompletionResponse> {
        if let Some(entry) = self.cache.get(&hash) {
            if entry.value().created_at.elapsed() < self.ttl {
                return Some(entry.value().response.clone());
            }
        }
        None
    }

    pub fn insert(&self, hash: u64, response: ChatCompletionResponse) {
        self.cache.insert(
            hash,
            CachedResponse {
                response,
                created_at: std::time::Instant::now(),
            },
        );
    }
}
```

## Batch Operations

```rust
use tokio::sync::mpsc;

pub struct BatchProcessor {
    tx: mpsc::Sender<BatchItem>,
}

pub struct BatchItem {
    pub provider_id: String,
    pub tokens: i64,
}

impl BatchProcessor {
    pub fn new(batch_size: usize, flush_interval: std::time::Duration) -> (Self, mpsc::Receiver<BatchItem>) {
        let (tx, rx) = mpsc::channel(batch_size * 10);
        
        (BatchProcessor { tx }, rx)
    }

    pub async fn add(&self, item: BatchItem) -> Result<(), mpsc::error::SendError<BatchItem>> {
        self.tx.send(item).await
    }
}

pub async fn process_batches(
    mut rx: mpsc::Receiver<BatchItem>,
    tracker: Arc<OptimizedQuotaTracker>,
) {
    let mut batch: Vec<BatchItem> = Vec::with_capacity(100);
    
    while let Some(item) = rx.recv().await {
        batch.push(item);
        
        if batch.len() >= 100 {
            for item in batch.drain(..) {
                tracker.add_tokens_atomic(&item.provider_id, item.tokens).await;
            }
        }
    }
    
    // Flush remaining
    for item in batch {
        tracker.add_tokens_atomic(&item.provider_id, item.tokens).await;
    }
}
```

## Zero-Copy Response Streaming

```rust
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};

pub async fn stream_response_zero_copy(
    provider: &Provider,
    req: &ChatCompletionRequest,
) -> Result<impl Stream<Item = Result<Bytes, String>>, String> {
    let client = get_http_client();
    
    let response = client
        .post(&format!("{}/chat/completions", provider.api_endpoint))
        .json(&req)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // Direct byte streaming, no intermediate buffering
    Ok(response.bytes_stream().map_err(|e| e.to_string()))
}
```

## Graceful Shutdown

```rust
use tokio::sync::broadcast;

pub struct GracefulShutdown {
    shutdown_tx: broadcast::Sender<()>,
}

impl GracefulShutdown {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        GracefulShutdown { shutdown_tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub async fn wait_for_shutdown(&self) {
        let mut rx = self.subscribe();
        let _ = rx.recv().await;
    }
}

// Usage
pub async fn run_server_graceful(
    state: Arc<AppState>,
    shutdown: GracefulShutdown,
) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:20128").await?;
    let mut shutdown_rx = shutdown.subscribe();

    tokio::select! {
        result = axum::serve(listener, app).into_future() => result,
        _ = shutdown_rx.recv() => {
            tracing::info!("Shutting down gracefully");
            Ok(())
        }
    }
}
```

## Memory Profiling

```rust
pub struct MemoryStats {
    pub resident_set_size: u64,
    pub virtual_memory: u64,
    pub heap_allocated: u64,
}

#[cfg(target_os = "linux")]
pub fn get_memory_stats() -> MemoryStats {
    use std::fs;
    
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    
    let rss = status.lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0) * 1024;
    
    let vms = status.lines()
        .find(|l| l.starts_with("VmSize:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0) * 1024;

    MemoryStats {
        resident_set_size: rss,
        virtual_memory: vms,
        heap_allocated: 0,
    }
}

pub async fn monitor_memory(interval: std::time::Duration) {
    loop {
        let stats = get_memory_stats();
        tracing::info!(
            rss_mb = stats.resident_set_size / 1024 / 1024,
            vms_mb = stats.virtual_memory / 1024 / 1024,
            "Memory usage"
        );
        tokio::time::sleep(interval).await;
    }
}
```

## AppState Optimization

```rust
pub struct AppState {
    pub db_pool: Arc<DbPool>,
    pub providers: Arc<DashMap<String, Provider>>,
    pub http_client: Arc<reqwest::Client>,
    pub quota_tracker: Arc<OptimizedQuotaTracker>,
    pub concurrency_limiter: Arc<ConcurrencyLimiter>,
    pub request_cache: Arc<RequestCache>,
    pub buffer_pool: Arc<BufferPool>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            db_pool: Arc::new(DbPool::new()),
            providers: Arc::new(DashMap::new()),
            http_client: Arc::new(get_http_client().clone()),
            quota_tracker: Arc::new(OptimizedQuotaTracker::new()),
            concurrency_limiter: Arc::new(ConcurrencyLimiter::new(100)),
            request_cache: Arc::new(RequestCache::new(std::time::Duration::from_secs(60))),
            buffer_pool: Arc::new(BufferPool::new(4096, 50)),
        }
    }
}
```

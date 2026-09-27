---
name: request-caching
description: Request deduplication, response caching with TTL, content-addressed storage, and cache invalidation strategies for RustAgent
used_by: RustAgent Gateway
---

# Request Caching & Deduplication

Reduce redundant provider calls by caching identical requests.

## Content-Addressed Cache

```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct CacheEntry<T: Clone> {
    pub value: T,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub access_count: u64,
}

pub struct ContentAddressedCache<T: Clone> {
    entries: Arc<DashMap<u64, CacheEntry<T>>>,
    ttl: std::time::Duration,
    max_entries: usize,
}

impl<T: Clone> ContentAddressedCache<T> {
    pub fn new(ttl: std::time::Duration, max_entries: usize) -> Self {
        ContentAddressedCache {
            entries: Arc::new(DashMap::new()),
            ttl,
            max_entries,
        }
    }

    pub fn compute_hash<H: Hash>(value: &H) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    pub fn get<H: Hash>(&self, key: &H) -> Option<T> {
        let hash = Self::compute_hash(key);
        
        if let Some(mut entry) = self.entries.get_mut(&hash) {
            if entry.created_at.elapsed() < self.ttl {
                entry.last_accessed = Instant::now();
                entry.access_count += 1;
                return Some(entry.value.clone());
            } else {
                drop(entry);
                self.entries.remove(&hash);
            }
        }
        None
    }

    pub fn insert<H: Hash>(&self, key: &H, value: T) {
        let hash = Self::compute_hash(key);
        
        // Evict if at capacity (simple LRU)
        if self.entries.len() >= self.max_entries {
            if let Some((_, entry)) = self
                .entries
                .iter()
                .min_by_key(|e| e.value().last_accessed)
            {
                let evict_hash = *e.key();
                drop(entry);
                self.entries.remove(&evict_hash);
            }
        }

        self.entries.insert(
            hash,
            CacheEntry {
                value,
                created_at: Instant::now(),
                last_accessed: Instant::now(),
                access_count: 0,
            },
        );
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            capacity: self.max_entries,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub capacity: usize,
}
```

## Request Deduplication

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct CacheableRequest {
    pub model: String,
    pub messages_hash: u64,  // Hash of message content
    pub temperature: Option<u32>,  // Use u32 for hashability
    pub max_tokens: Option<i32>,
}

impl From<&ChatCompletionRequest> for CacheableRequest {
    fn from(req: &ChatCompletionRequest) -> Self {
        let mut hasher = DefaultHasher::new();
        for msg in &req.messages {
            msg.content.hash(&mut hasher);
        }
        let messages_hash = hasher.finish();

        let temp = req.temperature.map(|t| (t * 100.0) as u32);

        CacheableRequest {
            model: req.model.clone(),
            messages_hash,
            temperature: temp,
            max_tokens: req.max_tokens,
        }
    }
}

pub struct RequestDeduplicator {
    cache: ContentAddressedCache<ChatCompletionResponse>,
}

impl RequestDeduplicator {
    pub fn new(ttl: std::time::Duration) -> Self {
        RequestDeduplicator {
            cache: ContentAddressedCache::new(ttl, 1000),
        }
    }

    pub fn try_get(&self, req: &ChatCompletionRequest) -> Option<ChatCompletionResponse> {
        let cacheable = CacheableRequest::from(req);
        self.cache.get(&cacheable)
    }

    pub fn cache_response(&self, req: &ChatCompletionRequest, response: ChatCompletionResponse) {
        let cacheable = CacheableRequest::from(req);
        self.cache.insert(&cacheable, response);
    }

    pub fn get_stats(&self) -> CacheStats {
        self.cache.stats()
    }
}
```

## Streaming Response Caching

```rust
use futures::stream::Stream;
use std::pin::Pin;

#[derive(Clone)]
pub struct CachedStreamResponse {
    pub chunks: Vec<String>,
    pub created_at: Instant,
}

pub struct StreamCache {
    entries: Arc<DashMap<u64, CachedStreamResponse>>,
    ttl: std::time::Duration,
}

impl StreamCache {
    pub fn new(ttl: std::time::Duration) -> Self {
        StreamCache {
            entries: Arc::new(DashMap::new()),
            ttl,
        }
    }

    pub fn get_stream<H: Hash>(&self, key: &H) -> Option<impl Stream<Item = String>> {
        let hash = {
            let mut hasher = DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };

        if let Some(entry) = self.entries.get(&hash) {
            if entry.created_at.elapsed() < self.ttl {
                let chunks = entry.chunks.clone();
                return Some(futures::stream::iter(chunks));
            }
        }
        None
    }

    pub fn cache_stream<H: Hash>(&self, key: &H, chunks: Vec<String>) {
        let hash = {
            let mut hasher = DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };

        self.entries.insert(
            hash,
            CachedStreamResponse {
                chunks,
                created_at: Instant::now(),
            },
        );
    }
}
```

## Cache Invalidation Strategies

```rust
pub enum InvalidationStrategy {
    TTL(std::time::Duration),
    LRU(usize),
    LFU(usize),
    Manual,
}

pub struct CacheInvalidator {
    strategy: InvalidationStrategy,
}

impl CacheInvalidator {
    pub fn new(strategy: InvalidationStrategy) -> Self {
        CacheInvalidator { strategy }
    }

    pub fn should_invalidate(&self, entry: &CacheEntry<impl Clone>) -> bool {
        match self.strategy {
            InvalidationStrategy::TTL(ttl) => entry.created_at.elapsed() > ttl,
            InvalidationStrategy::LRU(_) => false, // Handled by DashMap eviction
            InvalidationStrategy::LFU(_) => false,
            InvalidationStrategy::Manual => false,
        }
    }

    pub async fn cleanup<T: Clone>(
        &self,
        cache: &ContentAddressedCache<T>,
    ) {
        match self.strategy {
            InvalidationStrategy::TTL(_) => {
                // TTL is checked on access
            }
            InvalidationStrategy::LRU(_) => {
                // LRU eviction happens on insert
            }
            InvalidationStrategy::LFU(_) => {
                // Would need to track access frequency
            }
            InvalidationStrategy::Manual => {
                cache.clear();
            }
        }
    }
}
```

## Cache Warming

```rust
pub struct CacheWarmer {
    cache: Arc<RequestDeduplicator>,
}

impl CacheWarmer {
    pub fn new(cache: Arc<RequestDeduplicator>) -> Self {
        CacheWarmer { cache }
    }

    pub async fn warm_common_requests(&self, state: &Arc<AppState>) {
        let common_models = vec!["gpt-4", "claude-3", "llama-70b"];
        
        for model in common_models {
            let req = ChatCompletionRequest {
                model: model.to_string(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                }],
                temperature: Some(0.7),
                max_tokens: Some(100),
                stream: Some(false),
            };

            // Pre-fetch from a provider to warm cache
            if let Ok(response) = call_provider(&state.providers.iter().next().unwrap().value(), &req).await {
                self.cache.cache_response(&req, response);
            }
        }
    }
}
```

## Cache Metrics

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub struct CacheMetrics {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
}

impl CacheMetrics {
    pub fn new() -> Self {
        CacheMetrics {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    pub fn get_stats(&self) -> CacheMetricsSnapshot {
        CacheMetricsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            hit_rate: self.hit_rate(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CacheMetricsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub hit_rate: f64,
}
```

## Usage in Handler

```rust
pub async fn chat_completions_cached(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    // Try cache first
    if let Some(cached) = state.request_deduplicator.try_get(&req) {
        state.cache_metrics.record_hit();
        tracing::debug!("Cache hit");
        return Ok(Json(cached));
    }

    state.cache_metrics.record_miss();

    // Proceed with normal flow
    let provider = select_provider(&state.providers, &req.model).await?;
    let response = call_provider(&provider, &req).await?;

    // Cache the response
    state.request_deduplicator.cache_response(&req, response.clone());

    Ok(Json(response))
}
```

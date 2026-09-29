use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse};

#[derive(Clone)]
pub struct CacheEntry<T: Clone> {
    pub value: T,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub access_count: u64,
}

pub struct ContentAddressedCache<T: Clone> {
    entries: Arc<DashMap<u64, CacheEntry<T>>>,
    ttl: Duration,
    max_entries: usize,
}

impl<T: Clone> ContentAddressedCache<T> {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
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

    pub fn insert<H: Hash>(&self, key: &H, value: T) -> bool {
        let hash = Self::compute_hash(key);
        let mut evicted = false;

        // Evict oldest accessed if at capacity (LRU)
        if self.entries.len() >= self.max_entries {
            if let Some(min_entry) = self.entries.iter().min_by_key(|e| e.value().last_accessed) {
                let evict_hash = *min_entry.key();
                drop(min_entry);
                self.entries.remove(&evict_hash);
                evicted = true;
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

        evicted
    }

    pub fn clear(&self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Cache key representation for a ChatCompletionRequest
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CacheableChatRequest {
    pub model: String,
    pub messages_hash: u64,
    pub temperature_bits: u32,
    pub max_tokens: Option<i32>,
}

impl From<&ChatCompletionRequest> for CacheableChatRequest {
    fn from(req: &ChatCompletionRequest) -> Self {
        let mut hasher = DefaultHasher::new();
        for msg in &req.messages {
            msg.role.hash(&mut hasher);
            msg.content.hash(&mut hasher);
        }
        let messages_hash = hasher.finish();

        let temperature_bits = req.temperature.unwrap_or(0.7).to_bits();

        CacheableChatRequest {
            model: req.model.clone(),
            messages_hash,
            temperature_bits,
            max_tokens: req.max_tokens,
        }
    }
}

/// Cache performance metrics
#[derive(Debug, Default)]
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

    pub fn get_stats(&self) -> CacheMetricsSnapshot {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        };

        CacheMetricsSnapshot {
            hits,
            misses,
            evictions: self.evictions.load(Ordering::Relaxed),
            hit_rate: (hit_rate * 100.0).round() / 100.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetricsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub hit_rate: f64,
}

/// Request deduplicator & response cache
pub struct RequestDeduplicator {
    cache: ContentAddressedCache<ChatCompletionResponse>,
    metrics: CacheMetrics,
}

impl RequestDeduplicator {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        RequestDeduplicator {
            cache: ContentAddressedCache::new(ttl, max_entries),
            metrics: CacheMetrics::new(),
        }
    }

    pub fn try_get(&self, req: &ChatCompletionRequest) -> Option<ChatCompletionResponse> {
        let cacheable = CacheableChatRequest::from(req);
        if let Some(cached) = self.cache.get(&cacheable) {
            self.metrics.record_hit();
            Some(cached)
        } else {
            self.metrics.record_miss();
            None
        }
    }

    pub fn cache_response(&self, req: &ChatCompletionRequest, response: ChatCompletionResponse) {
        let cacheable = CacheableChatRequest::from(req);
        let evicted = self.cache.insert(&cacheable, response);
        if evicted {
            self.metrics.record_eviction();
        }
    }

    pub fn clear(&self) {
        self.cache.clear();
    }

    pub fn get_metrics(&self) -> CacheMetricsSnapshot {
        self.metrics.get_stats()
    }

    pub fn entries_count(&self) -> usize {
        self.cache.len()
    }
}

// MemoryCache struct for backward compatibility and general key-value caching
pub struct MemoryCache {
    cache: ContentAddressedCache<String>,
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryCache {
    pub fn new() -> Self {
        MemoryCache {
            cache: ContentAddressedCache::new(Duration::from_secs(3600), 1000),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.cache.get(&key.to_string())
    }

    pub fn set(&self, key: String, value: String) {
        self.cache.insert(&key, value);
    }

    pub fn clear(&self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::v1::chat::{Choice, Message, Usage};

    #[test]
    fn test_content_addressed_cache_lru() {
        let cache = ContentAddressedCache::<String>::new(Duration::from_secs(60), 2);
        cache.insert(&"key1", "val1".to_string());
        cache.insert(&"key2", "val2".to_string());
        assert_eq!(cache.get(&"key1"), Some("val1".to_string()));

        // Insert 3rd item -> should evict key2 (since key1 was accessed)
        cache.insert(&"key3", "val3".to_string());
        assert_eq!(cache.get(&"key1"), Some("val1".to_string()));
        assert_eq!(cache.get(&"key3"), Some("val3".to_string()));
    }

    #[test]
    fn test_request_deduplicator() {
        let dedup = RequestDeduplicator::new(Duration::from_secs(60), 100);
        let req = ChatCompletionRequest {
            model: "llama-3-8b".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: "hello world".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: Some(false),
        };

        assert!(dedup.try_get(&req).is_none());

        let resp = ChatCompletionResponse {
            id: "test-id".to_string(),
            object: "chat.completion".to_string(),
            created: 123456789,
            model: "llama-3-8b".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: "Hi there!".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        };

        dedup.cache_response(&req, resp.clone());
        let cached = dedup.try_get(&req).expect("should hit cache");
        assert_eq!(cached.id, "test-id");
        assert_eq!(dedup.get_metrics().hits, 1);
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let cache = ContentAddressedCache::<String>::new(Duration::from_millis(10), 10);
        cache.insert(&"quick_key", "val".to_string());
        assert_eq!(cache.get(&"quick_key"), Some("val".to_string()));

        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(cache.get(&"quick_key"), None);
    }

    #[test]
    fn test_cacheable_request_differentiation() {
        let req1 = ChatCompletionRequest {
            model: "llama-3-8b".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: "ping".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: Some(false),
        };

        let mut req2 = req1.clone();
        req2.messages[0].content = "pong".to_string();

        let k1 = CacheableChatRequest::from(&req1);
        let k2 = CacheableChatRequest::from(&req2);
        assert_ne!(k1, k2);

        let mut req3 = req1.clone();
        req3.temperature = Some(0.2);
        let k3 = CacheableChatRequest::from(&req3);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_cache_metrics_calculation() {
        let metrics = CacheMetrics::new();
        assert_eq!(metrics.get_stats().hit_rate, 0.0);

        metrics.record_hit();
        metrics.record_miss();
        metrics.record_hit();
        metrics.record_hit();
        // 3 hits, 1 miss = 75%
        let stats = metrics.get_stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate, 75.0);
    }
}

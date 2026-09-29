pub mod memory;

pub use memory::{
    CacheEntry, CacheMetrics, CacheMetricsSnapshot, CacheableChatRequest, ContentAddressedCache,
    MemoryCache, RequestDeduplicator,
};

#[allow(dead_code)]
pub trait CacheLayer {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: String, value: String);
    fn clear(&self);
}

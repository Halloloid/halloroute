pub mod memory;

pub use memory::MemoryCache;

// CacheLayer trait stub for Phase 3+
#[allow(dead_code)]
pub trait CacheLayer {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: String, value: String);
    fn clear(&self);
}

//! Model pools: virtual model names that spread requests over several provider
//! models using a load-balancing strategy, falling back to the next member on failure.
pub mod exec;
pub mod store;

use crate::errors::{ApiError, ApiResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub use exec::execute_pool_chat;

pub const MAX_POOL_MEMBERS: usize = 50;
pub const MAX_WEIGHT: u32 = 100;
const MAX_ID_LEN: usize = 64;
/// A member that just failed is tried last for this long.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

/// How the next member is chosen. Remaining members are always kept as fallbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStrategy {
    /// Rotate evenly through the members
    RoundRobin,
    /// Smooth weighted rotation: a member with weight 3 is picked 3x as often as weight 1
    WeightedRoundRobin,
    /// Pick the member that was used least recently
    Lru,
    /// Always try members in listed order; later ones are only fallbacks
    Priority,
}

impl PoolStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PoolStrategy::RoundRobin => "round_robin",
            PoolStrategy::WeightedRoundRobin => "weighted_round_robin",
            PoolStrategy::Lru => "lru",
            PoolStrategy::Priority => "priority",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            PoolStrategy::RoundRobin => "Round robin",
            PoolStrategy::WeightedRoundRobin => "Weighted round robin",
            PoolStrategy::Lru => "Least recently used",
            PoolStrategy::Priority => "Priority (ordered fallback)",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "round_robin" => Ok(PoolStrategy::RoundRobin),
            "weighted_round_robin" => Ok(PoolStrategy::WeightedRoundRobin),
            "lru" => Ok(PoolStrategy::Lru),
            "priority" => Ok(PoolStrategy::Priority),
            other => Err(ApiError::InvalidRequest(format!(
                "Unknown strategy '{}'. Use one of: round_robin, weighted_round_robin, lru, priority",
                other
            ))),
        }
    }
}

fn default_weight() -> u32 {
    1
}

/// One provider model inside a pool. List order is the priority order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolMember {
    pub provider_id: String,
    pub model: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

/// A virtual model. Clients use `id` as the `model` value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPool {
    pub id: String,
    pub strategy: PoolStrategy,
    pub members: Vec<PoolMember>,
    #[serde(default)]
    pub collective_enabled: bool,
}

/// Pool ids are what clients send as `model`: lowercase letters, digits, `-`, `_`, `.`.
pub fn validate_pool_id(id: &str) -> ApiResult<()> {
    let ok = !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::InvalidRequest(format!(
            "Pool name must be 1-{} chars of lowercase letters, digits, '-', '_' or '.', starting with a letter or digit",
            MAX_ID_LEN
        )))
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A pool plus its runtime balancing state.
pub struct PoolEntry {
    pub pool: ModelPool,
    rr: AtomicUsize,
    tick: AtomicU64,
    wrr_current: Mutex<Vec<i64>>,
    last_used: Mutex<Vec<u64>>,
    failed_at: Mutex<Vec<Option<Instant>>>,
}

impl PoolEntry {
    pub fn new(pool: ModelPool) -> Self {
        let n = pool.members.len();
        PoolEntry {
            pool,
            rr: AtomicUsize::new(0),
            tick: AtomicU64::new(0),
            wrr_current: Mutex::new(vec![0; n]),
            last_used: Mutex::new(vec![0; n]),
            failed_at: Mutex::new(vec![None; n]),
        }
    }

    /// Member indices in the order they should be attempted for the next request.
    /// The first is the strategy's pick; the rest are fallbacks. Members that failed
    /// recently are moved to the back.
    pub fn order(&self) -> Vec<usize> {
        let members = &self.pool.members;
        let n = members.len();
        if n == 0 {
            return Vec::new();
        }

        let base: Vec<usize> = match self.pool.strategy {
            PoolStrategy::Priority => (0..n).collect(),
            PoolStrategy::RoundRobin => {
                let start = self.rr.fetch_add(1, Ordering::Relaxed) % n;
                (0..n).map(|i| (start + i) % n).collect()
            }
            PoolStrategy::WeightedRoundRobin => {
                let mut current = lock(&self.wrr_current);
                let total: i64 = members.iter().map(|m| m.weight.max(1) as i64).sum();
                for (i, m) in members.iter().enumerate() {
                    current[i] += m.weight.max(1) as i64;
                }
                let mut best = 0;
                for i in 1..n {
                    if current[i] > current[best] {
                        best = i;
                    }
                }
                current[best] -= total;
                let mut rest: Vec<usize> = (0..n).filter(|&i| i != best).collect();
                rest.sort_by(|a, b| current[*b].cmp(&current[*a]));
                let mut order = vec![best];
                order.extend(rest);
                order
            }
            PoolStrategy::Lru => {
                let last = lock(&self.last_used);
                let mut idx: Vec<usize> = (0..n).collect();
                idx.sort_by_key(|&i| last[i]);
                idx
            }
        };

        let failed = lock(&self.failed_at);
        let now = Instant::now();
        let (mut healthy, cooling): (Vec<usize>, Vec<usize>) = base
            .into_iter()
            .partition(|&i| !failed[i].is_some_and(|t| now.duration_since(t) < FAILURE_COOLDOWN));
        healthy.extend(cooling);
        healthy
    }

    /// Record that a member is being attempted (drives LRU).
    pub fn mark_used(&self, idx: usize) {
        let t = self.tick.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(slot) = lock(&self.last_used).get_mut(idx) {
            *slot = t;
        }
    }

    pub fn mark_failed(&self, idx: usize) {
        if let Some(slot) = lock(&self.failed_at).get_mut(idx) {
            *slot = Some(Instant::now());
        }
    }

    pub fn mark_ok(&self, idx: usize) {
        if let Some(slot) = lock(&self.failed_at).get_mut(idx) {
            *slot = None;
        }
    }
}

/// Runtime registry of pools, keyed by pool id (the virtual model name).
#[derive(Default)]
pub struct PoolRegistry {
    pools: DashMap<String, Arc<PoolEntry>>,
}

impl PoolRegistry {
    /// Insert or replace a pool (balancing state is reset).
    pub fn insert(&self, pool: ModelPool) {
        self.pools
            .insert(pool.id.clone(), Arc::new(PoolEntry::new(pool)));
    }

    pub fn remove(&self, id: &str) -> bool {
        self.pools.remove(id).is_some()
    }

    pub fn get(&self, id: &str) -> Option<Arc<PoolEntry>> {
        self.pools.get(id).map(|e| e.value().clone())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.pools.contains_key(id)
    }

    /// All pools sorted by id.
    pub fn list(&self) -> Vec<ModelPool> {
        let mut v: Vec<ModelPool> = self.pools.iter().map(|e| e.value().pool.clone()).collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(p: &str, m: &str, w: u32) -> PoolMember {
        PoolMember {
            provider_id: p.into(),
            model: m.into(),
            weight: w,
        }
    }

    fn entry(strategy: PoolStrategy, weights: &[u32]) -> PoolEntry {
        PoolEntry::new(ModelPool {
            id: "p".into(),
            strategy,
            members: weights
                .iter()
                .enumerate()
                .map(|(i, w)| member("prov", &format!("m{}", i), *w))
                .collect(),
            collective_enabled: false,
        })
    }

    #[test]
    fn priority_is_fixed_order() {
        let e = entry(PoolStrategy::Priority, &[1, 1, 1]);
        assert_eq!(e.order(), vec![0, 1, 2]);
        assert_eq!(e.order(), vec![0, 1, 2]);
    }

    #[test]
    fn round_robin_rotates_and_keeps_fallbacks() {
        let e = entry(PoolStrategy::RoundRobin, &[1, 1, 1]);
        assert_eq!(e.order(), vec![0, 1, 2]);
        assert_eq!(e.order(), vec![1, 2, 0]);
        assert_eq!(e.order(), vec![2, 0, 1]);
        assert_eq!(e.order(), vec![0, 1, 2]);
    }

    #[test]
    fn weighted_round_robin_respects_ratio() {
        let e = entry(PoolStrategy::WeightedRoundRobin, &[3, 1]);
        let mut counts = [0; 2];
        for _ in 0..40 {
            counts[e.order()[0]] += 1;
        }
        assert_eq!(counts, [30, 10]);
    }

    #[test]
    fn lru_prefers_least_recently_used() {
        let e = entry(PoolStrategy::Lru, &[1, 1, 1]);
        assert_eq!(e.order()[0], 0);
        e.mark_used(0);
        assert_eq!(e.order()[0], 1);
        e.mark_used(1);
        assert_eq!(e.order()[0], 2);
        e.mark_used(2);
        assert_eq!(e.order()[0], 0);
        e.mark_used(0);
        assert_eq!(e.order()[0], 1);
    }

    #[test]
    fn failed_member_is_tried_last_until_ok() {
        let e = entry(PoolStrategy::Priority, &[1, 1, 1]);
        e.mark_failed(0);
        assert_eq!(e.order(), vec![1, 2, 0]);
        e.mark_ok(0);
        assert_eq!(e.order(), vec![0, 1, 2]);
    }

    #[test]
    fn strategy_parse_roundtrip() {
        for s in [
            PoolStrategy::RoundRobin,
            PoolStrategy::WeightedRoundRobin,
            PoolStrategy::Lru,
            PoolStrategy::Priority,
        ] {
            assert_eq!(PoolStrategy::parse(s.as_str()).unwrap(), s);
        }
        assert!(PoolStrategy::parse("random").is_err());
    }

    #[test]
    fn pool_id_validation() {
        assert!(validate_pool_id("free-auto").is_ok());
        assert!(validate_pool_id("a.b_c-1").is_ok());
        assert!(validate_pool_id("").is_err());
        assert!(validate_pool_id("-bad").is_err());
        assert!(validate_pool_id("Has Space").is_err());
        assert!(validate_pool_id("UPPER").is_err());
        assert!(validate_pool_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn registry_insert_get_remove() {
        let r = PoolRegistry::default();
        r.insert(entry(PoolStrategy::Lru, &[1]).pool);
        assert!(r.contains("p"));
        assert_eq!(r.list().len(), 1);
        assert!(r.get("p").is_some());
        assert!(r.remove("p"));
        assert!(!r.remove("p"));
    }
}

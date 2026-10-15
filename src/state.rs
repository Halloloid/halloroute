use dashmap::DashMap;
use std::sync::Arc;

use crate::cache::RequestDeduplicator;
use crate::providers::Provider;
use crate::quota::DbPool;
use crate::token::{ContextWindowManager, TokenUsageStats};

#[derive(Clone)]
pub struct AppState {
    pub db_pool: DbPool,
    /// Thread-safe mutable provider registry for runtime add/remove/update
    pub providers: Arc<DashMap<String, Provider>>,
    pub request_deduplicator: Arc<RequestDeduplicator>,
    pub context_window_manager: ContextWindowManager,
    pub token_usage_stats: Arc<TokenUsageStats>,
}

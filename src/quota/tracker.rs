use chrono::Local;
use dashmap::DashMap;
use std::sync::Arc;

use crate::errors::ApiResult;

#[derive(Clone)]
pub struct DbPool {
    // Simple in-memory quota tracker
    // Key: "provider_id:date", Value: tokens_used
    quota_usage: Arc<DashMap<String, i64>>,
}

impl DbPool {
    pub fn new() -> Self {
        DbPool {
            quota_usage: Arc::new(DashMap::new()),
        }
    }
}

pub async fn init_pool() -> ApiResult<DbPool> {
    tracing::info!("Initializing in-memory quota tracker");
    Ok(DbPool::new())
}

pub async fn run_migrations(_pool: &DbPool) -> ApiResult<()> {
    tracing::info!("No migrations needed for in-memory storage");
    Ok(())
}

pub async fn get_today_quota_usage(pool: &DbPool, provider_id: &str) -> ApiResult<i64> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let key = format!("{}:{}", provider_id, today);

    let tokens = pool.quota_usage.get(&key).map(|v| *v).unwrap_or(0);
    Ok(tokens)
}

pub async fn increment_quota_usage(pool: &DbPool, provider_id: &str, tokens: i64) -> ApiResult<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let key = format!("{}:{}", provider_id, today);

    pool.quota_usage
        .entry(key)
        .and_modify(|v| *v += tokens)
        .or_insert(tokens);

    Ok(())
}

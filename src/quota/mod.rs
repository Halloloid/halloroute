pub mod tracker;

pub use tracker::{
    get_today_quota_usage, increment_quota_usage, init_pool, run_migrations, DbPool,
};

use crate::errors::ApiResult;
use crate::providers::Provider;

pub struct QuotaManager;

impl QuotaManager {
    pub async fn check_quota(
        pool: &DbPool,
        provider: &Provider,
        estimated_tokens: i64,
    ) -> ApiResult<bool> {
        let current_usage = tracker::get_today_quota_usage(pool, &provider.id).await?;

        // Check daily quota
        if let Some(daily_quota) = provider.free_quota_daily {
            if current_usage + estimated_tokens > daily_quota {
                tracing::warn!(
                    provider_id = %provider.id,
                    current = current_usage,
                    estimated = estimated_tokens,
                    limit = daily_quota,
                    "Daily quota would be exceeded"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn can_use_provider(pool: &DbPool, provider: &Provider) -> ApiResult<bool> {
        // Check if provider is healthy
        if !provider.is_available() {
            return Ok(false);
        }

        // Check if daily quota is not hit
        let current_usage = tracker::get_today_quota_usage(pool, &provider.id).await?;
        if let Some(daily_quota) = provider.free_quota_daily {
            if current_usage >= daily_quota {
                tracing::info!(
                    provider_id = %provider.id,
                    current = current_usage,
                    limit = daily_quota,
                    "Daily quota exhausted"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub async fn record_usage(pool: &DbPool, provider_id: &str, tokens_used: i64) -> ApiResult<()> {
        tracker::increment_quota_usage(pool, provider_id, tokens_used).await
    }
}

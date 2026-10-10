pub mod tracker;

pub use tracker::{
    create_memory_pool, get_today_quota_usage, increment_quota_usage, init_pool, run_migrations,
    start_quota_reset_tasks, DbPool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::ProviderHealth;

    #[tokio::test]
    async fn test_quota_manager_enforcement() {
        let pool = tracker::create_memory_pool().unwrap();
        let mut provider = Provider::new(
            "groq".into(),
            "Groq".into(),
            "https://api.groq.com".into(),
            "bearer".into(),
            Some(1000),
            None,
            30,
            vec!["llama3-8b".into()],
        );

        // Can use initially
        assert!(QuotaManager::can_use_provider(&pool, &provider)
            .await
            .unwrap());
        assert!(QuotaManager::check_quota(&pool, &provider, 500)
            .await
            .unwrap());

        // Consuming 800 tokens
        QuotaManager::record_usage(&pool, &provider.id, 800)
            .await
            .unwrap();

        // 800 + 300 = 1100 > 1000 limit -> check_quota returns false
        assert!(!QuotaManager::check_quota(&pool, &provider, 300)
            .await
            .unwrap());
        // 800 + 100 = 900 <= 1000 limit -> check_quota returns true
        assert!(QuotaManager::check_quota(&pool, &provider, 100)
            .await
            .unwrap());

        // Consuming another 200 tokens -> hits 1000 exact
        QuotaManager::record_usage(&pool, &provider.id, 200)
            .await
            .unwrap();
        assert!(!QuotaManager::can_use_provider(&pool, &provider)
            .await
            .unwrap());

        // Provider health check
        provider.health_status = ProviderHealth::Down;
        assert!(!QuotaManager::can_use_provider(&pool, &provider)
            .await
            .unwrap());
    }
}

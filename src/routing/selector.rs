use super::strategies::RoutingStrategy;
use crate::errors::{ApiError, ApiResult};
use crate::providers::{Provider, ProviderRegistry};
use crate::quota::{self, DbPool};

pub struct Router {
    registry: ProviderRegistry,
    strategy: RoutingStrategy,
}

impl Router {
    pub fn new(registry: ProviderRegistry, strategy: RoutingStrategy) -> Self {
        Router { registry, strategy }
    }

    pub async fn select_provider(
        &self,
        pool: &DbPool,
        model: Option<&str>,
    ) -> ApiResult<&Provider> {
        match self.strategy {
            RoutingStrategy::Auto => self.select_auto(pool, model).await,
            RoutingStrategy::Priority => self.select_priority(pool).await,
            RoutingStrategy::RoundRobin => self.select_round_robin(pool).await,
            RoutingStrategy::QuotaFirst => self.select_quota_first(pool).await,
            RoutingStrategy::LatencyFirst => self.select_latency_first(pool).await,
        }
    }

    async fn select_auto(&self, pool: &DbPool, model: Option<&str>) -> ApiResult<&Provider> {
        if let Some(model_name) = model {
            if let Some(provider) = self.registry.get_model_provider(model_name) {
                if quota::QuotaManager::can_use_provider(pool, provider).await? {
                    tracing::debug!(
                        provider_id = %provider.id,
                        model = model_name,
                        "Selected provider by model"
                    );
                    return Ok(provider);
                }
            }
        }

        self.select_round_robin(pool).await
    }

    async fn select_priority(&self, pool: &DbPool) -> ApiResult<&Provider> {
        for provider in self.registry.list_available_providers() {
            if quota::QuotaManager::can_use_provider(pool, provider).await? {
                return Ok(provider);
            }
        }
        Err(ApiError::AllProvidersUnavailable)
    }

    async fn select_round_robin(&self, pool: &DbPool) -> ApiResult<&Provider> {
        let available: Vec<&Provider> = self.registry.list_available_providers();

        if available.is_empty() {
            return Err(ApiError::AllProvidersUnavailable);
        }

        for provider in available {
            if quota::QuotaManager::can_use_provider(pool, provider).await? {
                return Ok(provider);
            }
        }

        Err(ApiError::AllProvidersUnavailable)
    }

    async fn select_quota_first(&self, pool: &DbPool) -> ApiResult<&Provider> {
        let mut best_provider: Option<&Provider> = None;
        let mut best_quota = -1i64;

        for provider in self.registry.list_available_providers() {
            if quota::QuotaManager::can_use_provider(pool, provider).await? {
                if let Ok(current_usage) =
                    quota::tracker::get_today_quota_usage(pool, &provider.id).await
                {
                    if let Some(daily_quota) = provider.free_quota_daily {
                        let remaining = daily_quota - current_usage;
                        if remaining > best_quota {
                            best_quota = remaining;
                            best_provider = Some(provider);
                        }
                    } else {
                        best_provider = Some(provider);
                        break;
                    }
                }
            }
        }

        best_provider.ok_or(ApiError::AllProvidersUnavailable)
    }

    async fn select_latency_first(&self, pool: &DbPool) -> ApiResult<&Provider> {
        self.select_round_robin(pool).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::ProviderHealth;
    use std::collections::HashMap;

    fn create_test_pool() -> DbPool {
        quota::tracker::create_memory_pool().expect("test pool")
    }

    fn sample_registry() -> ProviderRegistry {
        let mut map = HashMap::new();
        let mut p1 = Provider::new(
            "groq".into(),
            "Groq".into(),
            "https://api.groq.com/openai/v1".into(),
            "bearer".into(),
            Some(10_000),
            None,
            30,
            vec!["llama3-8b-8192".into()],
        );
        p1.health_status = ProviderHealth::Healthy;

        let mut p2 = Provider::new(
            "pollinations".into(),
            "Pollinations".into(),
            "https://text.pollinations.ai/openai".into(),
            "none".into(),
            Some(50_000),
            None,
            60,
            vec!["openai".into()],
        );
        p2.health_status = ProviderHealth::Healthy;

        let mut p3 = Provider::new(
            "broken".into(),
            "Broken".into(),
            "https://broken.ai/v1".into(),
            "none".into(),
            None,
            None,
            10,
            vec!["bad-model".into()],
        );
        p3.health_status = ProviderHealth::Down;

        map.insert(p1.id.clone(), p1);
        map.insert(p2.id.clone(), p2);
        map.insert(p3.id.clone(), p3);
        ProviderRegistry::new(map)
    }

    #[tokio::test]
    async fn test_router_select_auto_by_model() {
        let pool = create_test_pool();
        let router = Router::new(sample_registry(), RoutingStrategy::Auto);

        let provider = router
            .select_provider(&pool, Some("llama3-8b-8192"))
            .await
            .expect("should select groq");
        assert_eq!(provider.id, "groq");

        let provider2 = router
            .select_provider(&pool, Some("openai"))
            .await
            .expect("should select pollinations");
        assert_eq!(provider2.id, "pollinations");
    }

    #[tokio::test]
    async fn test_router_select_down_provider_fails() {
        let pool = create_test_pool();
        let router = Router::new(sample_registry(), RoutingStrategy::Auto);

        // bad-model is only on p3 which is Down -> falls back to available round robin
        let provider = router
            .select_provider(&pool, Some("bad-model"))
            .await
            .expect("falls back to available provider");
        assert!(provider.id == "groq" || provider.id == "pollinations");
    }

    #[tokio::test]
    async fn test_router_quota_first_strategy() {
        let pool = create_test_pool();
        let router = Router::new(sample_registry(), RoutingStrategy::QuotaFirst);

        // pollinations has 50k quota, groq has 10k quota -> pollinations wins
        let provider = router
            .select_provider(&pool, None)
            .await
            .expect("quota first selects highest remaining");
        assert_eq!(provider.id, "pollinations");
    }

    #[tokio::test]
    async fn test_router_all_providers_down() {
        let pool = create_test_pool();
        let mut map = HashMap::new();
        let mut p = Provider::new(
            "p1".into(),
            "P1".into(),
            "https://p1.ai".into(),
            "none".into(),
            None,
            None,
            10,
            vec![],
        );
        p.health_status = ProviderHealth::Down;
        map.insert(p.id.clone(), p);

        let router = Router::new(ProviderRegistry::new(map), RoutingStrategy::Priority);
        let err = router.select_provider(&pool, None).await;
        assert!(matches!(err, Err(ApiError::AllProvidersUnavailable)));
    }
}

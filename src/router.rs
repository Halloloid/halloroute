use crate::db::DbPool;
use crate::errors::{ApiError, ApiResult};
use crate::providers::{Provider, ProviderRegistry};
use crate::quota::QuotaManager;

#[allow(dead_code)]
#[derive(Debug, Clone)]
#[derive(Default)]
pub enum RoutingStrategy {
    #[default]
    Auto,
    Priority,
    RoundRobin,
    QuotaFirst,
    LatencyFirst,
}


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
                if QuotaManager::can_use_provider(pool, provider).await? {
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
            if QuotaManager::can_use_provider(pool, provider).await? {
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
            if QuotaManager::can_use_provider(pool, provider).await? {
                return Ok(provider);
            }
        }

        Err(ApiError::AllProvidersUnavailable)
    }

    async fn select_quota_first(&self, pool: &DbPool) -> ApiResult<&Provider> {
        let mut best_provider: Option<&Provider> = None;
        let mut best_quota = -1i64;

        for provider in self.registry.list_available_providers() {
            if QuotaManager::can_use_provider(pool, provider).await? {
                if let Ok(current_usage) =
                    crate::db::get_today_quota_usage(pool, &provider.id).await
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

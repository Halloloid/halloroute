use crate::errors::ApiError;
use crate::providers::Provider;
use crate::quota::DbPool;
use std::collections::HashMap;

/// Fallback chain selector for provider selection with multiple attempts
pub struct FallbackSelector {
    providers: HashMap<String, Provider>,
}

impl FallbackSelector {
    /// Create a new fallback selector
    pub fn new(providers: HashMap<String, Provider>) -> Self {
        FallbackSelector { providers }
    }

    /// Select a provider with fallback chain
    /// Tries: primary -> secondary -> tertiary -> available
    pub async fn select_with_fallback(
        &self,
        pool: &DbPool,
        primary: Option<&str>,
        model: Option<&str>,
    ) -> Result<Provider, ApiError> {
        // Try to find primary provider
        if let Some(primary_id) = primary {
            if let Some(provider) = self.providers.get(primary_id) {
                if self.is_provider_usable(pool, provider).await? {
                    tracing::info!(
                        provider_id = %provider.id,
                        reason = "primary",
                        "Selected provider"
                    );
                    return Ok(provider.clone());
                }
            }
        }

        // Try to find a provider that supports the model
        if let Some(model_name) = model {
            if let Some(provider) = self.find_provider_by_model(pool, model_name).await? {
                tracing::info!(
                    provider_id = %provider.id,
                    model = %model_name,
                    reason = "model_match",
                    "Selected provider"
                );
                return Ok(provider);
            }
        }

        // Try any healthy provider with available quota
        for (_, provider) in &self.providers {
            if self.is_provider_usable(pool, provider).await? {
                tracing::info!(
                    provider_id = %provider.id,
                    reason = "fallback",
                    "Selected provider"
                );
                return Ok(provider.clone());
            }
        }

        tracing::error!("No usable providers available");
        Err(ApiError::AllProvidersUnavailable)
    }

    /// Find a provider that supports a specific model
    async fn find_provider_by_model(
        &self,
        pool: &DbPool,
        model: &str,
    ) -> Result<Option<Provider>, ApiError> {
        for (_, provider) in &self.providers {
            // Check if provider supports model
            if provider.models.contains(&model.to_string()) {
                // Check if provider is usable
                if self.is_provider_usable(pool, provider).await? {
                    return Ok(Some(provider.clone()));
                }
            }
        }
        Ok(None)
    }

    /// Check if a provider is usable (healthy and has quota)
    async fn is_provider_usable(
        &self,
        pool: &DbPool,
        provider: &Provider,
    ) -> Result<bool, ApiError> {
        // Check health status
        if !provider.is_available() {
            tracing::debug!(
                provider_id = %provider.id,
                health_status = ?provider.health_status,
                "Provider not healthy"
            );
            return Ok(false);
        }

        // Check quota availability
        match crate::quota::QuotaManager::can_use_provider(pool, provider).await {
            Ok(can_use) => {
                if !can_use {
                    tracing::debug!(
                        provider_id = %provider.id,
                        "Provider quota exhausted"
                    );
                }
                Ok(can_use)
            }
            Err(e) => {
                tracing::error!(
                    provider_id = %provider.id,
                    error = %e,
                    "Error checking provider quota"
                );
                Err(e)
            }
        }
    }

    /// Get all healthy providers sorted by priority
    pub async fn get_healthy_providers(&self, pool: &DbPool) -> Result<Vec<Provider>, ApiError> {
        let mut healthy = Vec::new();

        for (_, provider) in &self.providers {
            if self.is_provider_usable(pool, provider).await? {
                healthy.push(provider.clone());
            }
        }

        Ok(healthy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{Provider, ProviderHealth};
    use crate::quota::tracker::create_memory_pool;
    use std::collections::HashMap;

    fn sample_provider(id: &str, models: Vec<&str>, health: ProviderHealth) -> Provider {
        let mut p = Provider::new(
            id.to_string(),
            id.to_uppercase(),
            format!("https://api.{}.com", id),
            "none".to_string(),
            Some(1000),
            None,
            100,
            models.into_iter().map(String::from).collect(),
        );
        p.health_status = health;
        p
    }

    #[tokio::test]
    async fn test_fallback_selector_primary_healthy() {
        let pool = create_memory_pool().unwrap();
        let mut map = HashMap::new();
        let p1 = sample_provider("groq", vec!["llama3"], ProviderHealth::Healthy);
        let p2 = sample_provider("together", vec!["llama3"], ProviderHealth::Healthy);
        map.insert(p1.id.clone(), p1);
        map.insert(p2.id.clone(), p2);

        let selector = FallbackSelector::new(map);
        let selected = selector
            .select_with_fallback(&pool, Some("groq"), Some("llama3"))
            .await
            .unwrap();
        assert_eq!(selected.id, "groq");
    }

    #[tokio::test]
    async fn test_fallback_selector_primary_down_falls_back_to_model() {
        let pool = create_memory_pool().unwrap();
        let mut map = HashMap::new();
        let p1 = sample_provider("groq", vec!["llama3"], ProviderHealth::Down);
        let p2 = sample_provider("together", vec!["llama3"], ProviderHealth::Healthy);
        map.insert(p1.id.clone(), p1);
        map.insert(p2.id.clone(), p2);

        let selector = FallbackSelector::new(map);
        let selected = selector
            .select_with_fallback(&pool, Some("groq"), Some("llama3"))
            .await
            .unwrap();
        assert_eq!(selected.id, "together");
    }

    #[tokio::test]
    async fn test_fallback_selector_primary_and_model_unavailable_falls_back_to_any() {
        let pool = create_memory_pool().unwrap();
        let mut map = HashMap::new();
        let p1 = sample_provider("groq", vec!["llama3"], ProviderHealth::Down);
        let p2 = sample_provider("pollinations", vec!["gpt-4"], ProviderHealth::Healthy);
        map.insert(p1.id.clone(), p1);
        map.insert(p2.id.clone(), p2);

        let selector = FallbackSelector::new(map);
        let selected = selector
            .select_with_fallback(&pool, Some("groq"), Some("llama3"))
            .await
            .unwrap();
        assert_eq!(selected.id, "pollinations");
    }

    #[tokio::test]
    async fn test_fallback_selector_all_down_errors() {
        let pool = create_memory_pool().unwrap();
        let mut map = HashMap::new();
        let p1 = sample_provider("groq", vec!["llama3"], ProviderHealth::Down);
        map.insert(p1.id.clone(), p1);

        let selector = FallbackSelector::new(map);
        let err = selector
            .select_with_fallback(&pool, Some("groq"), Some("llama3"))
            .await;
        assert!(matches!(err, Err(ApiError::AllProvidersUnavailable)));
    }

    #[tokio::test]
    async fn test_get_healthy_providers_list() {
        let pool = create_memory_pool().unwrap();
        let mut map = HashMap::new();
        let p1 = sample_provider("groq", vec!["llama3"], ProviderHealth::Healthy);
        let p2 = sample_provider("down_one", vec!["llama3"], ProviderHealth::Down);
        map.insert(p1.id.clone(), p1);
        map.insert(p2.id.clone(), p2);

        let selector = FallbackSelector::new(map);
        let healthy = selector.get_healthy_providers(&pool).await.unwrap();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, "groq");
    }
}

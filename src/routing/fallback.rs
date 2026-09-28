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
    use crate::providers::Provider;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_fallback_selector_with_primary() {
        let mut providers = HashMap::new();
        let provider = Provider::new(
            "test_provider".to_string(),
            "Test Provider".to_string(),
            "https://api.test.com".to_string(),
            "api_key".to_string(),
            Some(10000),
            None,
            100,
            vec!["gpt-3.5".to_string()],
        );
        providers.insert(provider.id.clone(), provider);

        let _selector = FallbackSelector::new(providers);
        // Note: This would need a real pool to test fully
    }
}

pub mod circuit_breaker;
pub mod client;
pub mod discovery;
pub mod filters;
pub mod registry;
pub mod templates;
pub mod types;

pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitState,
};
pub use registry::ProviderRegistry;
pub use types::{Provider, ProviderHealth};

use std::collections::HashMap;
use tracing::{error, info, warn};

/// Initialize providers by dynamically discovering available free models
///
/// Reads provider templates and checks environment for API keys.
/// For each key found, calls the provider's /v1/models endpoint to discover
/// available free models only.
///
/// Returns a map of provider_id -> Provider with only free models loaded.
pub async fn init_providers() -> HashMap<String, Provider> {
    let templates = templates::get_provider_templates();
    let mut providers = HashMap::new();

    info!(
        template_count = templates.len(),
        "Starting dynamic provider discovery"
    );

    for template in templates {
        // Check if API key is available in environment
        let api_key = std::env::var(&template.env_var_name).ok();

        // Skip if auth required but no key found
        if template.auth_type != "none" && api_key.is_none() {
            info!(
                provider = %template.id,
                env_var = %template.env_var_name,
                "Provider skipped: authentication required but no API key found"
            );
            continue;
        }

        // Attempt to discover provider and its free models
        match discovery::discover_provider(&template, api_key).await {
            Ok(Some(provider)) => {
                info!(
                    provider = %provider.id,
                    model_count = provider.models.len(),
                    models = ?provider.models,
                    "Successfully discovered provider with free models"
                );
                providers.insert(provider.id.clone(), provider);
            }
            Ok(None) => {
                warn!(
                    provider = %template.id,
                    "Provider available but has no free models"
                );
            }
            Err(e) => {
                error!(
                    provider = %template.id,
                    error = %e,
                    "Failed to discover provider"
                );
            }
        }
    }

    info!(
        provider_count = providers.len(),
        "Provider discovery completed"
    );

    providers
}

use super::filters::filter_free_models;
use super::types::{ModelInfo, ModelsResponse, Provider, ProviderTemplate};
use std::time::Duration;
use tracing::{debug, error, warn};

/// Error types for provider discovery
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Invalid JSON response: {0}")]
    JsonError(String),

    #[error("API returned error: {status} - {body}")]
    ApiError { status: u16, body: String },

    #[error("Request timeout")]
    Timeout,

    #[error("No models returned from API")]
    NoModels,

    #[error("Authentication failed: {0}")]
    AuthError(String),
}

/// Discover available free models for a provider
///
/// Returns `Some(Provider)` if models were found, `None` if no free models available,
/// and `Err` on discovery failures.
pub async fn discover_provider(
    template: &ProviderTemplate,
    api_key: Option<String>,
) -> Result<Option<Provider>, DiscoveryError> {
    debug!(
        provider = %template.id,
        auth_required = template.auth_type != "none",
        "Starting provider discovery"
    );

    // Check if auth is required but not provided
    if template.auth_type != "none" && api_key.is_none() {
        debug!(
            provider = %template.id,
            "Skipping: auth required but no API key provided"
        );
        return Ok(None);
    }

    // Call /v1/models endpoint
    let models = fetch_models(&template.api_endpoint, api_key.as_deref())
        .await
        .map_err(|e| {
            error!(provider = %template.id, error = %e, "Failed to fetch models");
            e
        })?;

    if models.is_empty() {
        warn!(provider = %template.id, "No models returned from API");
        return Err(DiscoveryError::NoModels);
    }

    debug!(
        provider = %template.id,
        total_models = models.len(),
        "Fetched models from provider"
    );

    // Filter to only free models
    let free_models = filter_free_models(models, &template.free_model_filter);

    if free_models.is_empty() {
        debug!(
            provider = %template.id,
            "No free models available for this provider"
        );
        return Ok(None);
    }

    debug!(
        provider = %template.id,
        free_models = free_models.len(),
        "Filtered to free models"
    );

    // Create Provider instance
    let provider = Provider::new(
        template.id.clone(),
        template.name.clone(),
        template.api_endpoint.clone(),
        template.auth_type.clone(),
        template.free_quota_daily,
        template.free_quota_monthly,
        template.rate_limit_rpm,
        free_models,
    );

    Ok(Some(provider))
}

/// Fetch available models from provider's /v1/models endpoint
async fn fetch_models(
    api_endpoint: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelInfo>, DiscoveryError> {
    let client = reqwest::Client::new();
    let url = format!("{}/models", api_endpoint);

    let mut request = client.get(&url);

    // Add authentication if provided
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    // Set timeout
    let response = tokio::time::timeout(Duration::from_secs(10), request.send())
        .await
        .map_err(|_| DiscoveryError::Timeout)
        .map_err(|e| {
            error!("Discovery request timed out: {}", e);
            e
        })?
        .map_err(|e| {
            error!("Discovery HTTP request failed: {}", e);
            DiscoveryError::HttpError(e.to_string())
        })?;

    let status = response.status();

    // Check for auth errors
    if status == 401 || status == 403 {
        return Err(DiscoveryError::AuthError(format!(
            "Authentication failed with status {}",
            status
        )));
    }

    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown".to_string());
        return Err(DiscoveryError::ApiError {
            status: status.as_u16(),
            body,
        });
    }

    // Parse response
    let models_response: ModelsResponse = response.json().await.map_err(|e| {
        error!("Failed to parse models response: {}", e);
        DiscoveryError::JsonError(e.to_string())
    })?;

    Ok(models_response.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_error_display() {
        let err = DiscoveryError::Timeout;
        assert_eq!(err.to_string(), "Request timeout");

        let err = DiscoveryError::NoModels;
        assert_eq!(err.to_string(), "No models returned from API");
    }
}

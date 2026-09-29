use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String, // "none", "api_key", "bearer"
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub models: Vec<String>,
    pub health_status: ProviderHealth,
    pub last_check: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderHealth {
    Healthy,
    Degraded,
    Down,
}

/// Free model detection strategy for a provider
#[derive(Debug, Clone)]
pub enum FreeModelFilter {
    /// All models are free (Groq free tier, Pollinations)
    All,
    /// No free models available
    None,
    /// Filter by glob patterns (e.g., "meta-llama/Llama-*-chat-hf")
    Patterns(Vec<String>),
    /// Only these specific model IDs are free
    ExplicitList(Vec<String>),
    /// Filter based on pricing API response (field name to check == 0)
    PricingAPI {
        field: String,
        zero_means_free: bool,
    },
}

/// Provider template with metadata for dynamic discovery
#[derive(Debug, Clone)]
pub struct ProviderTemplate {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,    // "none", "api_key", "bearer"
    pub env_var_name: String, // e.g., "GROQ_API_KEY"
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub free_model_filter: FreeModelFilter,
    pub discovery_timeout_secs: u64,
}

impl ProviderTemplate {
    pub fn new(
        id: &str,
        name: &str,
        api_endpoint: &str,
        auth_type: &str,
        env_var_name: &str,
        free_quota_daily: Option<i64>,
        free_quota_monthly: Option<i64>,
        rate_limit_rpm: i32,
        free_model_filter: FreeModelFilter,
    ) -> Self {
        ProviderTemplate {
            id: id.to_string(),
            name: name.to_string(),
            api_endpoint: api_endpoint.to_string(),
            auth_type: auth_type.to_string(),
            env_var_name: env_var_name.to_string(),
            free_quota_daily,
            free_quota_monthly,
            rate_limit_rpm,
            free_model_filter,
            discovery_timeout_secs: 10,
        }
    }
}

/// OpenAI-compatible models list response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelsResponse {
    pub data: Vec<ModelInfo>,
}

/// Model information from provider API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub owned_by: String,
    #[serde(default)]
    pub pricing: Option<serde_json::Value>,
}

impl Provider {
    pub fn new(
        id: String,
        name: String,
        api_endpoint: String,
        auth_type: String,
        free_quota_daily: Option<i64>,
        free_quota_monthly: Option<i64>,
        rate_limit_rpm: i32,
        models: Vec<String>,
    ) -> Self {
        Provider {
            id,
            name,
            api_endpoint,
            auth_type,
            free_quota_daily,
            free_quota_monthly,
            rate_limit_rpm,
            models,
            health_status: ProviderHealth::Healthy,
            last_check: None,
        }
    }

    pub fn is_available(&self) -> bool {
        self.health_status == ProviderHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation_and_availability() {
        let mut provider = Provider::new(
            "groq".into(),
            "Groq Cloud".into(),
            "https://api.groq.com/openai/v1".into(),
            "bearer".into(),
            Some(100_000),
            None,
            30,
            vec!["llama3-8b-8192".into(), "mixtral-8x7b-32768".into()],
        );

        assert_eq!(provider.id, "groq");
        assert!(provider.is_available());
        assert_eq!(provider.health_status, ProviderHealth::Healthy);

        provider.health_status = ProviderHealth::Degraded;
        assert!(!provider.is_available());

        provider.health_status = ProviderHealth::Down;
        assert!(!provider.is_available());
    }

    #[test]
    fn test_provider_template_builder() {
        let template = ProviderTemplate::new(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "bearer",
            "OPENROUTER_API_KEY",
            None,
            None,
            20,
            FreeModelFilter::PricingAPI {
                field: "pricing.prompt".into(),
                zero_means_free: true,
            },
        );

        assert_eq!(template.id, "openrouter");
        assert_eq!(template.env_var_name, "OPENROUTER_API_KEY");
        assert_eq!(template.discovery_timeout_secs, 10);
    }
}

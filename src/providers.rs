use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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

pub fn init_providers() -> HashMap<String, Provider> {
    let mut providers = HashMap::new();

    // OpenCode (Free Forever)
    providers.insert(
        "opencode".to_string(),
        Provider::new(
            "opencode".to_string(),
            "OpenCode".to_string(),
            "https://api.opencode.ai/v1".to_string(),
            "none".to_string(),
            None,
            None,
            600, // 10 RPM per free tier
            vec!["deepseek-v3.2".to_string(), "nemotron-3".to_string()],
        ),
    );

    // Groq (Free Tier)
    providers.insert(
        "groq".to_string(),
        Provider::new(
            "groq".to_string(),
            "Groq".to_string(),
            "https://api.groq.com/v1".to_string(),
            "api_key".to_string(),
            Some(14_400_000), // ~14.4M tokens/day free tier estimate
            None,
            30,
            vec![
                "mixtral-8x7b-32768".to_string(),
                "llama-3-70b".to_string(),
                "llama-3-8b".to_string(),
            ],
        ),
    );

    // Pollinations (No Key Needed)
    providers.insert(
        "pollinations".to_string(),
        Provider::new(
            "pollinations".to_string(),
            "Pollinations".to_string(),
            "https://api.pollinations.ai/v1".to_string(),
            "none".to_string(),
            None,
            None,
            100,
            vec![
                "gpt-4".to_string(),
                "claude-3".to_string(),
                "llama-70b".to_string(),
            ],
        ),
    );

    // Together AI (Free Tier)
    providers.insert(
        "together".to_string(),
        Provider::new(
            "together".to_string(),
            "Together AI".to_string(),
            "https://api.together.xyz/v1".to_string(),
            "api_key".to_string(),
            Some(1_000_000), // ~1M tokens/day free tier
            None,
            100,
            vec![
                "meta-llama/Llama-3-70b-chat-hf".to_string(),
                "meta-llama/Llama-3-8b-chat-hf".to_string(),
                "mistralai/Mistral-7B-Instruct-v0.1".to_string(),
            ],
        ),
    );

    // HuggingFace Inference (Free Tier)
    providers.insert(
        "huggingface".to_string(),
        Provider::new(
            "huggingface".to_string(),
            "HuggingFace".to_string(),
            "https://api-inference.huggingface.co/v1".to_string(),
            "api_key".to_string(),
            Some(100_000), // Limited free tier
            None,
            10,
            vec![
                "meta-llama/Llama-2-7b-chat".to_string(),
                "mistralai/Mistral-7B-Instruct-v0.1".to_string(),
            ],
        ),
    );

    providers
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegistry {
    pub providers: HashMap<String, Provider>,
}

impl ProviderRegistry {
    pub fn new(providers: HashMap<String, Provider>) -> Self {
        ProviderRegistry { providers }
    }

    pub fn get_provider(&self, id: &str) -> Option<&Provider> {
        self.providers.get(id)
    }

    pub fn list_providers(&self) -> Vec<&Provider> {
        self.providers.values().collect()
    }

    pub fn list_available_providers(&self) -> Vec<&Provider> {
        self.providers
            .values()
            .filter(|p| p.is_available())
            .collect()
    }

    pub fn list_models(&self) -> Vec<(String, String)> {
        // Returns (model_id, provider_id)
        let mut models = Vec::new();
        for provider in self.providers.values() {
            for model in &provider.models {
                models.push((model.clone(), provider.id.clone()));
            }
        }
        models
    }

    pub fn get_model_provider(&self, model: &str) -> Option<&Provider> {
        self.providers
            .values()
            .find(|p| p.models.iter().any(|m| m == model))
    }
}

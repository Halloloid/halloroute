use super::types::Provider;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    pub providers: HashMap<String, Provider>,
}

impl ProviderRegistry {
    pub fn new(providers: HashMap<String, Provider>) -> Self {
        ProviderRegistry { providers }
    }

    #[allow(dead_code)]
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

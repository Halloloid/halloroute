use super::types::Provider;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    pub providers: Arc<DashMap<String, Provider>>,
}

impl ProviderRegistry {
    pub fn new(providers: Arc<DashMap<String, Provider>>) -> Self {
        ProviderRegistry { providers }
    }

    #[allow(dead_code)]
    pub fn get_provider(&self, id: &str) -> Option<Provider> {
        self.providers.get(id).map(|r| r.value().clone())
    }

    pub fn list_providers(&self) -> Vec<Provider> {
        self.providers.iter().map(|r| r.value().clone()).collect()
    }

    pub fn list_available_providers(&self) -> Vec<Provider> {
        self.providers
            .iter()
            .map(|r| r.value().clone())
            .filter(|p| p.is_available())
            .collect()
    }

    pub fn list_models(&self) -> Vec<(String, String)> {
        // Returns (model_id, provider_id)
        let mut models = Vec::new();
        for r in self.providers.iter() {
            let provider = r.value();
            for model in &provider.models {
                models.push((model.clone(), provider.id.clone()));
            }
        }
        models
    }

    pub fn get_model_provider(&self, model: &str) -> Option<Provider> {
        for r in self.providers.iter() {
            let p = r.value();
            if p.models.iter().any(|m| m == model) {
                return Some(p.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::ProviderHealth;

    fn sample_registry() -> ProviderRegistry {
        let dashmap = Arc::new(DashMap::new());
        let mut p1 = Provider::new(
            "groq".into(),
            "Groq".into(),
            "https://api.groq.com/openai/v1".into(),
            "bearer".into(),
            Some(500_000),
            None,
            30,
            vec!["llama3-8b-8192".into(), "mixtral-8x7b-32768".into()],
        );
        p1.health_status = ProviderHealth::Healthy;

        let mut p2 = Provider::new(
            "pollinations".into(),
            "Pollinations".into(),
            "https://text.pollinations.ai/openai".into(),
            "none".into(),
            None,
            None,
            60,
            vec!["openai".into(), "mistral".into()],
        );
        p2.health_status = ProviderHealth::Down;

        dashmap.insert(p1.id.clone(), p1);
        dashmap.insert(p2.id.clone(), p2);
        ProviderRegistry::new(dashmap)
    }

    #[test]
    fn test_registry_lookups() {
        let registry = sample_registry();

        assert_eq!(registry.list_providers().len(), 2);
        assert_eq!(registry.list_available_providers().len(), 1);
        assert_eq!(registry.list_available_providers()[0].id, "groq");

        let p = registry.get_provider("groq");
        assert!(p.is_some());
        assert_eq!(p.unwrap().name, "Groq");

        let missing = registry.get_provider("non_existent");
        assert!(missing.is_none());

        let model_p = registry.get_model_provider("llama3-8b-8192");
        assert!(model_p.is_some());
        assert_eq!(model_p.unwrap().id, "groq");

        let missing_model = registry.get_model_provider("gpt-5-turbo");
        assert!(missing_model.is_none());

        let all_models = registry.list_models();
        assert_eq!(all_models.len(), 4);
    }
}

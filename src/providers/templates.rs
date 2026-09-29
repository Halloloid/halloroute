use super::types::{FreeModelFilter, ProviderTemplate};

/// Get all provider templates available for dynamic discovery
///
/// Templates are tried in order. No-auth providers (like Pollinations)
/// act as fallbacks when API keys are not available.
pub fn get_provider_templates() -> Vec<ProviderTemplate> {
    vec![
        // Pollinations: No auth required, all models free (try first as fallback)
        ProviderTemplate::new(
            "pollinations",
            "Pollinations",
            "https://api.pollinations.ai/v1",
            "none",
            "POLLINATIONS_API_KEY", // Not required, but checked if present
            None,
            None,
            100,
            FreeModelFilter::All,
        ),
        // Groq: All models are free on the free tier
        ProviderTemplate::new(
            "groq",
            "Groq",
            "https://api.groq.com/v1",
            "api_key",
            "GROQ_API_KEY",
            Some(14_400_000), // ~14.4M tokens/day free tier estimate
            None,
            30,
            FreeModelFilter::All,
        ),
        // Together AI: Specific free models matching patterns
        ProviderTemplate::new(
            "together",
            "Together AI",
            "https://api.together.xyz/v1",
            "api_key",
            "TOGETHER_API_KEY",
            Some(1_000_000), // ~1M tokens/day free tier
            None,
            100,
            FreeModelFilter::Patterns(vec![
                "meta-llama/Llama-*-chat-hf".to_string(),
                "meta-llama/Llama-*-instruct".to_string(),
                "mistralai/Mistral-*".to_string(),
                "NousResearch/Nous-*".to_string(),
            ]),
        ),
        // HuggingFace: Limited free tier with explicit models
        ProviderTemplate::new(
            "huggingface",
            "HuggingFace",
            "https://api-inference.huggingface.co/v1",
            "api_key",
            "HUGGINGFACE_API_KEY",
            Some(100_000), // Limited free tier
            None,
            10,
            FreeModelFilter::ExplicitList(vec![
                "meta-llama/Llama-2-7b-chat".to_string(),
                "mistralai/Mistral-7B-Instruct-v0.1".to_string(),
                "tiiuae/falcon-7b-instruct".to_string(),
            ]),
        ),
        // OpenRouter: Free models detected via pricing API
        ProviderTemplate::new(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "api_key",
            "OPENROUTER_API_KEY",
            None,
            None,
            200,
            FreeModelFilter::PricingAPI {
                field: "pricing.prompt".to_string(),
                zero_means_free: true,
            },
        ),
        // DeepInfra: Specific free models
        ProviderTemplate::new(
            "deepinfra",
            "DeepInfra",
            "https://api.deepinfra.com/v1",
            "api_key",
            "DEEPINFRA_API_KEY",
            None,
            None,
            100,
            FreeModelFilter::Patterns(vec![
                "meta-llama/Llama-2-*".to_string(),
                "mistralai/Mistral-*".to_string(),
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_templates_completeness() {
        let templates = get_provider_templates();
        assert_eq!(templates.len(), 6);

        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"pollinations"));
        assert!(ids.contains(&"groq"));
        assert!(ids.contains(&"together"));
        assert!(ids.contains(&"huggingface"));
        assert!(ids.contains(&"openrouter"));
        assert!(ids.contains(&"deepinfra"));

        for template in &templates {
            assert!(!template.name.is_empty());
            assert!(template.api_endpoint.starts_with("https://"));
            assert!(template.rate_limit_rpm > 0);
        }
    }
}

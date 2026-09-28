pub mod registry;
pub mod types;

pub use registry::ProviderRegistry;
pub use types::{Provider, ProviderHealth};

use std::collections::HashMap;

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

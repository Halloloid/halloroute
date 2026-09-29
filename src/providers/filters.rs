use super::types::{FreeModelFilter, ModelInfo};
use glob::Pattern;

/// Filter models to only include those available on the free tier
pub fn filter_free_models(models: Vec<ModelInfo>, filter: &FreeModelFilter) -> Vec<String> {
    match filter {
        FreeModelFilter::All => {
            // All models are free
            models.iter().map(|m| m.id.clone()).collect()
        }
        FreeModelFilter::None => {
            // No free models
            Vec::new()
        }
        FreeModelFilter::Patterns(patterns) => {
            // Filter by glob patterns
            models
                .iter()
                .filter(|model| {
                    patterns
                        .iter()
                        .any(|pattern| matches_pattern(&model.id, pattern))
                })
                .map(|m| m.id.clone())
                .collect()
        }
        FreeModelFilter::ExplicitList(free_ids) => {
            // Only include explicitly listed models
            models
                .iter()
                .filter(|model| free_ids.iter().any(|id| id == &model.id))
                .map(|m| m.id.clone())
                .collect()
        }
        FreeModelFilter::PricingAPI {
            field,
            zero_means_free,
        } => {
            // Filter by checking pricing field in response
            models
                .iter()
                .filter(|model| {
                    if let Some(pricing) = &model.pricing {
                        check_pricing_field(pricing, field, *zero_means_free)
                    } else {
                        false
                    }
                })
                .map(|m| m.id.clone())
                .collect()
        }
    }
}

/// Match a model ID against a glob pattern (e.g., "meta-llama/Llama-*-chat-hf")
fn matches_pattern(model_id: &str, pattern: &str) -> bool {
    // Convert glob pattern to regex-like matching
    // Handle common wildcards: * and ?
    if let Ok(glob_pattern) = Pattern::new(pattern) {
        glob_pattern.matches(model_id)
    } else {
        // Fallback: exact match if pattern is invalid
        model_id == pattern
    }
}

/// Check if a model's pricing field indicates it's free
fn check_pricing_field(
    pricing: &serde_json::Value,
    field_path: &str,
    zero_means_free: bool,
) -> bool {
    // Strip leading "pricing." if present, since `pricing` is already the pricing object
    let clean_path = field_path.strip_prefix("pricing.").unwrap_or(field_path);
    let parts: Vec<&str> = clean_path.split('.').collect();
    let mut current = pricing;

    for part in parts {
        match current.get(part) {
            Some(value) => current = value,
            None => {
                // Field not found, assume not free if looking for explicit 0
                return !zero_means_free;
            }
        }
    }

    // Check if the value indicates free tier
    match current {
        serde_json::Value::Number(n) => {
            if zero_means_free {
                // Price of 0 means free
                n.as_f64().map(|f| f == 0.0).unwrap_or(false)
            } else {
                // Non-zero price is acceptable (free tier detected by presence)
                true
            }
        }
        serde_json::Value::String(s) => {
            if zero_means_free {
                s == "0" || s == "0.0" || s.parse::<f64>().map(|f| f == 0.0).unwrap_or(false)
            } else {
                true
            }
        }
        serde_json::Value::Null => zero_means_free, // Null might mean free
        _ => !zero_means_free,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern_simple() {
        assert!(matches_pattern(
            "meta-llama/Llama-3-chat-hf",
            "meta-llama/Llama-*-chat-hf"
        ));
        assert!(matches_pattern(
            "meta-llama/Llama-70b-chat-hf",
            "meta-llama/Llama-*-chat-hf"
        ));
        assert!(!matches_pattern(
            "mistralai/Mistral-7B",
            "meta-llama/Llama-*-chat-hf"
        ));
    }

    #[test]
    fn test_filter_all() {
        let models = vec![
            ModelInfo {
                id: "model-1".to_string(),
                object: "model".to_string(),
                owned_by: "provider".to_string(),
                pricing: None,
            },
            ModelInfo {
                id: "model-2".to_string(),
                object: "model".to_string(),
                owned_by: "provider".to_string(),
                pricing: None,
            },
        ];

        let result = filter_free_models(models, &FreeModelFilter::All);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"model-1".to_string()));
        assert!(result.contains(&"model-2".to_string()));
    }

    #[test]
    fn test_filter_explicit_list() {
        let models = vec![
            ModelInfo {
                id: "gpt-4".to_string(),
                object: "model".to_string(),
                owned_by: "openai".to_string(),
                pricing: None,
            },
            ModelInfo {
                id: "llama-2".to_string(),
                object: "model".to_string(),
                owned_by: "meta".to_string(),
                pricing: None,
            },
        ];

        let filter = FreeModelFilter::ExplicitList(vec!["llama-2".to_string()]);
        let result = filter_free_models(models, &filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "llama-2");
    }

    #[test]
    fn test_filter_none() {
        let models = vec![ModelInfo {
            id: "model-1".to_string(),
            object: "model".to_string(),
            owned_by: "provider".to_string(),
            pricing: None,
        }];
        let result = filter_free_models(models, &FreeModelFilter::None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_pricing_api() {
        let models = vec![
            ModelInfo {
                id: "free-model".to_string(),
                object: "model".to_string(),
                owned_by: "openrouter".to_string(),
                pricing: Some(serde_json::json!({
                    "prompt": 0.0,
                    "completion": 0.0
                })),
            },
            ModelInfo {
                id: "free-model-str".to_string(),
                object: "model".to_string(),
                owned_by: "openrouter".to_string(),
                pricing: Some(serde_json::json!({
                    "prompt": "0",
                    "completion": "0"
                })),
            },
            ModelInfo {
                id: "paid-model".to_string(),
                object: "model".to_string(),
                owned_by: "openrouter".to_string(),
                pricing: Some(serde_json::json!({
                    "prompt": 0.005,
                    "completion": 0.015
                })),
            },
            ModelInfo {
                id: "unknown-pricing-no-field".to_string(),
                object: "model".to_string(),
                owned_by: "openrouter".to_string(),
                pricing: None,
            },
        ];

        let filter = FreeModelFilter::PricingAPI {
            field: "prompt".to_string(),
            zero_means_free: true,
        };

        let result = filter_free_models(models, &filter);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"free-model".to_string()));
        assert!(result.contains(&"free-model-str".to_string()));
        assert!(!result.contains(&"paid-model".to_string()));
        assert!(!result.contains(&"unknown-pricing-no-field".to_string()));
    }
}

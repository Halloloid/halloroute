use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::api::v1::chat::{ChatCompletionRequest, Message};

/// Fast token counter and estimator
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenCounter;

impl TokenCounter {
    /// Estimate tokens from text: average ~3.8-4.0 chars per token for English/code
    pub fn estimate_tokens(text: &str) -> i64 {
        if text.is_empty() {
            return 0;
        }
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();

        // Blend word-based (~1.3 tokens/word) and char-based (~4 chars/token) estimation
        let char_estimate = (char_count as f64 / 3.8).ceil() as i64;
        let word_estimate = (word_count as f64 * 1.3).ceil() as i64;

        char_estimate.max(word_estimate).max(1)
    }

    /// Count total estimated tokens for a slice of chat messages
    pub fn count_messages(messages: &[Message]) -> i64 {
        let mut total = 0i64;
        for msg in messages {
            // Role overhead (~1 token)
            total += 1;
            // Content estimation
            total += Self::estimate_tokens(&msg.content);
            // Formatting delimiter overhead per message (~3 tokens)
            total += 3;
        }
        // Conversation / system framing overhead
        total += 4;
        total
    }

    /// Count total estimated tokens for a complete ChatCompletionRequest
    pub fn count_request(req: &ChatCompletionRequest) -> i64 {
        let mut total = Self::count_messages(&req.messages);
        // Model identifier tokenization overhead
        total += Self::estimate_tokens(&req.model);
        // Parameter framing overhead
        total += 4;
        total
    }
}

/// Model capabilities and context window limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub model: String,
    pub context_window: i64,
    pub max_output_tokens: i64,
}

/// Context window manager to prevent exceeding model limits
#[derive(Debug, Clone)]
pub struct ContextWindowManager {
    models: HashMap<String, ModelCapabilities>,
}

impl Default for ContextWindowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextWindowManager {
    pub fn new() -> Self {
        let mut models = HashMap::new();

        // Common free models and their context window limits
        let known_models = vec![
            ("llama-3.3-70b-versatile", 131072, 8192),
            ("llama-3.1-8b-instant", 131072, 8192),
            ("llama-3-70b-8192", 8192, 8192),
            ("llama-3-8b-8192", 8192, 8192),
            ("mixtral-8x7b-32768", 32768, 4096),
            ("gemma2-9b-it", 8192, 4096),
            ("deepseek-chat", 65536, 4096),
            ("deepseek-coder", 65536, 4096),
            ("deepseek-r1-distill-llama-70b", 131072, 8192),
            ("qwen/qwen-2.5-72b-instruct", 32768, 4096),
        ];

        for (model, context_window, max_output) in known_models {
            models.insert(
                model.to_string(),
                ModelCapabilities {
                    model: model.to_string(),
                    context_window,
                    max_output_tokens: max_output,
                },
            );
        }

        ContextWindowManager { models }
    }

    pub fn get_capabilities(&self, model: &str) -> Option<&ModelCapabilities> {
        self.models.get(model)
    }

    pub fn fits_in_context(&self, model: &str, prompt_tokens: i64, requested_output: i64) -> bool {
        if let Some(caps) = self.get_capabilities(model) {
            prompt_tokens + requested_output <= caps.context_window
        } else {
            // Default assumption: 8192 context window if unknown
            prompt_tokens + requested_output <= 8192
        }
    }

    pub fn get_safe_output_tokens(&self, model: &str, prompt_tokens: i64) -> i64 {
        if let Some(caps) = self.get_capabilities(model) {
            let remaining = caps.context_window - prompt_tokens;
            remaining.clamp(1, caps.max_output_tokens)
        } else {
            (8192 - prompt_tokens).clamp(1, 2048)
        }
    }
}

/// Global atomic token usage stats
#[derive(Debug, Default)]
pub struct TokenUsageStats {
    pub total_input_tokens: AtomicI64,
    pub total_output_tokens: AtomicI64,
    pub total_requests: AtomicI64,
}

impl TokenUsageStats {
    pub fn new() -> Self {
        TokenUsageStats {
            total_input_tokens: AtomicI64::new(0),
            total_output_tokens: AtomicI64::new(0),
            total_requests: AtomicI64::new(0),
        }
    }

    pub fn record_usage(&self, input_tokens: i64, output_tokens: i64) {
        self.total_input_tokens
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens
            .fetch_add(output_tokens, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> TokenUsageSnapshot {
        let input = self.total_input_tokens.load(Ordering::Relaxed);
        let output = self.total_output_tokens.load(Ordering::Relaxed);
        let requests = self.total_requests.load(Ordering::Relaxed);
        let avg_tokens_per_request = if requests > 0 {
            (input + output) / requests
        } else {
            0
        };

        TokenUsageSnapshot {
            total_input_tokens: input,
            total_output_tokens: output,
            total_requests: requests,
            avg_tokens_per_request,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageSnapshot {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_requests: i64,
    pub avg_tokens_per_request: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_counter() {
        let text = "Hello world! This is a test message for token counting.";
        let tokens = TokenCounter::estimate_tokens(text);
        assert!(tokens > 0);

        let req = ChatCompletionRequest {
            model: "llama-3-8b".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: text.to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: Some(false),
        };

        let req_tokens = TokenCounter::count_request(&req);
        assert!(req_tokens > tokens);
    }

    #[test]
    fn test_context_window_manager() {
        let manager = ContextWindowManager::new();
        assert!(manager.fits_in_context("llama-3.1-8b-instant", 1000, 2000));
        assert!(!manager.fits_in_context("llama-3-8b-8192", 7000, 2000));
    }

    #[test]
    fn test_token_usage_stats() {
        let stats = TokenUsageStats::new();
        stats.record_usage(100, 50);
        stats.record_usage(200, 100);

        let snapshot = stats.get_stats();
        assert_eq!(snapshot.total_input_tokens, 300);
        assert_eq!(snapshot.total_output_tokens, 150);
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.avg_tokens_per_request, 225);
    }
}

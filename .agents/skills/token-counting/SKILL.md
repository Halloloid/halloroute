---
name: token-counting
description: Token counting and estimation for accurate quota tracking, provider-specific tokenizers, context window management
used_by: RustAgent Gateway
---

# Token Counting & Estimation

Accurate token counting for quota management across different providers.

## Basic Token Estimation

```rust
pub struct TokenCounter;

impl TokenCounter {
    /// Simple estimation: ~1.3 tokens per word (English average)
    pub fn estimate_tokens(text: &str) -> i64 {
        let word_count = text.split_whitespace().count() as f64;
        (word_count * 1.3).ceil() as i64
    }

    /// More accurate: count actual characters and apply ratio
    pub fn estimate_tokens_from_chars(text: &str) -> i64 {
        // Average ~4 chars per token
        ((text.len() as f64) / 4.0).ceil() as i64
    }

    pub fn count_messages(messages: &[Message]) -> i64 {
        let mut total = 0i64;
        
        for msg in messages {
            // Role typically 1 token
            total += 1;
            // Content tokens
            total += Self::estimate_tokens(&msg.content);
            // Formatting overhead per message
            total += 3;
        }
        
        // System message overhead
        total += 10;
        
        total
    }

    pub fn count_request(req: &ChatCompletionRequest) -> i64 {
        let mut total = 0i64;
        
        // Messages
        total += Self::count_messages(&req.messages);
        
        // Model specification
        total += 10;
        
        // Temperature, top_p, etc.
        total += 5;
        
        total
    }
}
```

## Provider-Specific Tokenizers

```rust
pub trait TokenizerProvider: Send + Sync {
    fn count_tokens(&self, text: &str) -> i64;
    fn encode(&self, text: &str) -> Vec<i32>;
    fn decode(&self, tokens: Vec<i32>) -> String;
}

/// OpenAI GPT-3/4 tokenizer (cl100k_base)
pub struct GPTTokenizer {
    // In production, use tiktoken-rs crate
}

impl TokenizerProvider for GPTTokenizer {
    fn count_tokens(&self, text: &str) -> i64 {
        // Approximate: GPT tokens are ~4 chars each
        ((text.len() as f64) / 4.0).ceil() as i64
    }

    fn encode(&self, text: &str) -> Vec<i32> {
        // Mock implementation
        vec![]
    }

    fn decode(&self, tokens: Vec<i32>) -> String {
        String::new()
    }
}

/// Claude tokenizer (different from OpenAI)
pub struct ClaudeTokenizer {
    // Claude has slightly different tokenization
}

impl TokenizerProvider for ClaudeTokenizer {
    fn count_tokens(&self, text: &str) -> i64 {
        // Claude tokens: ~3.5 chars each (slightly more efficient)
        ((text.len() as f64) / 3.5).ceil() as i64
    }

    fn encode(&self, text: &str) -> Vec<i32> {
        vec![]
    }

    fn decode(&self, tokens: Vec<i32>) -> String {
        String::new()
    }
}

pub struct TokenizerRegistry {
    tokenizers: std::collections::HashMap<String, Box<dyn TokenizerProvider>>,
}

impl TokenizerRegistry {
    pub fn new() -> Self {
        let mut tokenizers: std::collections::HashMap<String, Box<dyn TokenizerProvider>> = 
            std::collections::HashMap::new();
        
        tokenizers.insert("gpt-4".to_string(), Box::new(GPTTokenizer {}));
        tokenizers.insert("gpt-3.5".to_string(), Box::new(GPTTokenizer {}));
        tokenizers.insert("claude-3".to_string(), Box::new(ClaudeTokenizer {}));
        
        TokenizerRegistry { tokenizers }
    }

    pub fn get_tokenizer(&self, model: &str) -> Box<dyn TokenizerProvider> {
        self.tokenizers
            .get(model)
            .cloned()
            .unwrap_or_else(|| Box::new(GPTTokenizer {}))
    }

    pub fn count_tokens(&self, model: &str, text: &str) -> i64 {
        let tokenizer = self.get_tokenizer(model);
        tokenizer.count_tokens(text)
    }
}
```

## Context Window Management

```rust
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub model: String,
    pub context_window: i64,
    pub max_output_tokens: i64,
}

pub struct ContextWindowManager {
    models: std::collections::HashMap<String, ModelCapabilities>,
}

impl ContextWindowManager {
    pub fn new() -> Self {
        let mut models = std::collections::HashMap::new();
        
        models.insert(
            "gpt-4".to_string(),
            ModelCapabilities {
                model: "gpt-4".to_string(),
                context_window: 8192,
                max_output_tokens: 4096,
            },
        );
        
        models.insert(
            "gpt-4-turbo".to_string(),
            ModelCapabilities {
                model: "gpt-4-turbo".to_string(),
                context_window: 128000,
                max_output_tokens: 4096,
            },
        );
        
        models.insert(
            "claude-3-opus".to_string(),
            ModelCapabilities {
                model: "claude-3-opus".to_string(),
                context_window: 200000,
                max_output_tokens: 4096,
            },
        );

        ContextWindowManager { models }
    }

    pub fn get_capabilities(&self, model: &str) -> Option<ModelCapabilities> {
        self.models.get(model).cloned()
    }

    pub fn fits_in_context(
        &self,
        model: &str,
        prompt_tokens: i64,
        requested_output: i64,
    ) -> bool {
        if let Some(caps) = self.get_capabilities(model) {
            let total_needed = prompt_tokens + requested_output;
            total_needed <= caps.context_window
        } else {
            true // Assume fits if unknown
        }
    }

    pub fn get_safe_output_tokens(&self, model: &str, prompt_tokens: i64) -> i64 {
        if let Some(caps) = self.get_capabilities(model) {
            let remaining = caps.context_window - prompt_tokens;
            remaining.min(caps.max_output_tokens)
        } else {
            2000 // Default safe value
        }
    }
}
```

## Token Cost Calculation

```rust
#[derive(Debug, Clone)]
pub struct TokenCost {
    pub input_cost_per_1k: f64,
    pub output_cost_per_1k: f64,
}

pub struct CostCalculator {
    costs: std::collections::HashMap<String, TokenCost>,
}

impl CostCalculator {
    pub fn new() -> Self {
        let mut costs = std::collections::HashMap::new();
        
        // Free tier providers: $0
        costs.insert(
            "opencode".to_string(),
            TokenCost {
                input_cost_per_1k: 0.0,
                output_cost_per_1k: 0.0,
            },
        );
        
        costs.insert(
            "groq".to_string(),
            TokenCost {
                input_cost_per_1k: 0.0,
                output_cost_per_1k: 0.0,
            },
        );

        CostCalculator { costs }
    }

    pub fn calculate_cost(
        &self,
        provider: &str,
        input_tokens: i64,
        output_tokens: i64,
    ) -> f64 {
        if let Some(cost) = self.costs.get(provider) {
            let input_cost = (input_tokens as f64 / 1000.0) * cost.input_cost_per_1k;
            let output_cost = (output_tokens as f64 / 1000.0) * cost.output_cost_per_1k;
            input_cost + output_cost
        } else {
            0.0 // Unknown provider
        }
    }
}
```

## Token Usage Tracking

```rust
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
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
        self.total_input_tokens.fetch_add(input_tokens, Ordering::Relaxed);
        self.total_output_tokens.fetch_add(output_tokens, Ordering::Relaxed);
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> TokenUsageSnapshot {
        TokenUsageSnapshot {
            total_input_tokens: self.total_input_tokens.load(Ordering::Relaxed),
            total_output_tokens: self.total_output_tokens.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            avg_tokens_per_request: {
                let requests = self.total_requests.load(Ordering::Relaxed);
                if requests > 0 {
                    let total = self.total_input_tokens.load(Ordering::Relaxed)
                        + self.total_output_tokens.load(Ordering::Relaxed);
                    total / requests
                } else {
                    0
                }
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TokenUsageSnapshot {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_requests: i64,
    pub avg_tokens_per_request: i64,
}
```

## Provider-Specific Token Limits

```rust
#[derive(Debug, Clone)]
pub struct ProviderTokenLimits {
    pub provider_id: String,
    pub max_tokens_per_request: i64,
    pub max_tokens_per_minute: i64,
    pub max_tokens_per_day: i64,
}

pub struct TokenLimitManager {
    limits: std::collections::HashMap<String, ProviderTokenLimits>,
}

impl TokenLimitManager {
    pub fn new() -> Self {
        let mut limits = std::collections::HashMap::new();
        
        limits.insert(
            "opencode".to_string(),
            ProviderTokenLimits {
                provider_id: "opencode".to_string(),
                max_tokens_per_request: 2000,
                max_tokens_per_minute: 10000,
                max_tokens_per_day: i64::MAX, // Unlimited
            },
        );
        
        limits.insert(
            "groq".to_string(),
            ProviderTokenLimits {
                provider_id: "groq".to_string(),
                max_tokens_per_request: 4096,
                max_tokens_per_minute: 30000,
                max_tokens_per_day: 14_400_000, // ~14.4M free tier
            },
        );

        TokenLimitManager { limits }
    }

    pub fn check_token_limit(
        &self,
        provider_id: &str,
        requested_tokens: i64,
    ) -> Result<(), String> {
        if let Some(limit) = self.limits.get(provider_id) {
            if requested_tokens > limit.max_tokens_per_request {
                return Err(format!(
                    "Requested tokens {} exceeds max per request {}",
                    requested_tokens, limit.max_tokens_per_request
                ));
            }
            Ok(())
        } else {
            Ok(()) // Unknown provider, assume OK
        }
    }

    pub fn get_max_output_tokens(&self, provider_id: &str) -> i64 {
        self.limits
            .get(provider_id)
            .map(|l| l.max_tokens_per_request)
            .unwrap_or(2000)
    }
}
```

## Usage in Handler

```rust
pub async fn chat_completions_with_token_tracking(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, ApiError> {
    // Estimate input tokens
    let input_tokens = TokenCounter::count_request(&req);
    
    // Get provider
    let provider = select_provider(&state.providers, &req.model).await?;
    
    // Check context window
    let output_tokens = state.context_window_manager.get_safe_output_tokens(
        &provider.id,
        input_tokens,
    );
    
    if output_tokens <= 0 {
        return Err(ApiError::ContextWindowExceeded);
    }
    
    // Check token limits
    state.token_limit_manager.check_token_limit(&provider.id, input_tokens)?;
    
    // Call provider
    let response = call_provider(&provider, &req).await?;
    
    // Track usage
    let output_token_count = estimate_output_tokens(&response);
    state.token_usage_stats.record_usage(input_tokens, output_token_count);
    
    // Update quota
    state.quota_tracker.add_tokens(&provider.id, input_tokens + output_token_count).await;
    
    Ok(Json(response))
}
```

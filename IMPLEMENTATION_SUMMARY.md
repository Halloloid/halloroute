# Dynamic Provider Discovery Implementation Summary

## What Changed

RustAgent now discovers providers and their free models **dynamically at startup** instead of using hardcoded configurations. The system reads API keys from environment variables, calls each provider's `/v1/models` endpoint, and filters to free models only.

## New Architecture

```
Startup
  ├─ Load provider templates (metadata, no keys)
  ├─ For each template:
  │  ├─ Check env for API key
  │  ├─ Call /v1/models endpoint
  │  ├─ Filter free models using provider-specific rules
  │  └─ Create Provider with discovered models
  └─ Return providers map
```

## Files Added

### 1. `src/providers/templates.rs` (NEW)
Defines 6 provider templates with discovery metadata:

```rust
pub fn get_provider_templates() -> Vec<ProviderTemplate>
```

**Providers:**
- **Pollinations** — no auth, all models free (fallback)
- **Groq** — requires `GROQ_API_KEY`, all models free
- **Together AI** — requires `TOGETHER_API_KEY`, filtered by pattern
- **HuggingFace** — requires `HUGGINGFACE_API_KEY`, explicit list
- **OpenRouter** — requires `OPENROUTER_API_KEY`, pricing-based filter
- **DeepInfra** — requires `DEEPINFRA_API_KEY`, pattern filter

### 2. `src/providers/discovery.rs` (NEW)
Core discovery logic:

```rust
pub async fn discover_provider(
    template: &ProviderTemplate,
    api_key: Option<String>,
) -> Result<Option<Provider>, DiscoveryError>
```

- Validates auth requirements
- Calls provider's `/v1/models` endpoint
- Parses OpenAI-compatible response
- Returns `Ok(Some(Provider))` with free models, `Ok(None)` if no free models, or `Err`

### 3. `src/providers/filters.rs` (NEW)
Free model filtering logic:

```rust
pub fn filter_free_models(
    models: Vec<ModelInfo>,
    filter: &FreeModelFilter
) -> Vec<String>
```

Supports 5 detection strategies:
- **`All`** — every model is free
- **`None`** — no free models
- **`Patterns`** — glob pattern matching (e.g., `"meta-llama/Llama-*-chat-hf"`)
- **`ExplicitList`** — known free model IDs
- **`PricingAPI`** — check pricing field in response

## Files Modified

### 1. `src/providers/types.rs`
Added new types:

```rust
pub enum FreeModelFilter {
    All,
    None,
    Patterns(Vec<String>),
    ExplicitList(Vec<String>),
    PricingAPI { field: String, zero_means_free: bool },
}

pub struct ProviderTemplate {
    pub id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,
    pub env_var_name: String,
    pub free_quota_daily: Option<i64>,
    pub free_quota_monthly: Option<i64>,
    pub rate_limit_rpm: i32,
    pub free_model_filter: FreeModelFilter,
    pub discovery_timeout_secs: u64,
}

pub struct ModelsResponse {
    pub data: Vec<ModelInfo>,
}

pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: String,
    pub pricing: Option<serde_json::Value>,
}
```

### 2. `src/providers/mod.rs`
Refactored `init_providers()` to be async and use discovery:

```rust
pub async fn init_providers() -> HashMap<String, Provider>
```

Now:
- Loads templates
- Checks environment for API keys
- Calls `discover_provider()` for each template
- Logs all steps (debug/info/warn/error)
- Returns only providers with free models

### 3. `src/main.rs`
Updated to call async init:

```rust
let providers_map = providers::init_providers().await;
```

### 4. `Cargo.toml`
Added dependency:

```toml
glob = "0.3"  # For glob pattern matching
```

## Key Features

✅ **Dynamic Discovery** — No hardcoded models, reads from providers
✅ **API Key Support** — Checks environment for `{PROVIDER}_API_KEY`
✅ **Free Models Only** — Filters using provider-specific rules
✅ **Graceful Degradation** — Missing API keys skip provider, continue
✅ **Clear Logging** — Every discovery step logged (info/debug/warn/error)
✅ **Extensible** — Easy to add new providers or filters
✅ **OpenAI Compatible** — Uses standard `/v1/models` response format
✅ **Error Handling** — Distinguishes between auth errors, timeouts, API errors

## Usage

### With API Keys
```bash
export GROQ_API_KEY="gsk_..."
export TOGETHER_API_KEY="..."
cargo run
```

### Minimal (No API Keys)
```bash
cargo run  # Uses Pollinations (no auth required)
```

## Expected Logs

### With Groq key:
```
INFO rust_agent: Starting RustAgent Gateway
INFO rust_agent::providers: Starting dynamic provider discovery
DEBUG rust_agent::providers::discovery: Starting provider discovery provider="pollinations"
INFO rust_agent::providers: Successfully discovered provider with free models
  provider="pollinations" model_count=12
DEBUG rust_agent::providers::discovery: Starting provider discovery provider="groq"
INFO rust_agent::providers: Successfully discovered provider with free models
  provider="groq" model_count=3 models=["mixtral-8x7b-32768", ...]
INFO rust_agent::providers: Provider discovery completed provider_count=2
```

### Without API keys:
```
INFO rust_agent::providers: Provider skipped: authentication required but no API key found
  provider="groq" env_var="GROQ_API_KEY"
INFO rust_agent::providers: Successfully discovered provider with free models
  provider="pollinations" model_count=12
INFO rust_agent::providers: Provider discovery completed provider_count=1
```

## Adding a New Provider

1. Add template to `templates.rs`:
```rust
ProviderTemplate::new(
    "provider_id",
    "Provider Name",
    "https://api.provider.com/v1",
    "api_key",  // or "none"
    "PROVIDER_API_KEY",
    Some(quota),
    None,
    100,  // RPM
    FreeModelFilter::Patterns(vec!["model-*".to_string()]),
)
```

2. Set environment variable (if auth required):
```bash
export PROVIDER_API_KEY="..."
```

3. Run discovery:
```bash
RUST_LOG=debug cargo run
```

## Testing

Unit tests in `filters.rs` cover:
- Glob pattern matching
- Explicit list filtering
- Pricing API navigation

Run tests:
```bash
cargo test --lib filters
```

## Benefits Over Hardcoded Approach

| Aspect | Before | After |
|--------|--------|-------|
| Model Lists | Hardcoded in code | Discovered from APIs |
| Adding Provider | Modify code + redeploy | Add template + set env var |
| Outdated Models | Requires new release | Auto-updated next startup |
| Missing API Key | Error/crash | Skip provider, continue |
| Free vs Paid | Manual filtering | Automatic detection |
| Provider Downtime | Breaking | Graceful (skip provider) |

## Future Enhancements

Phase 3+:
- **Parallel Discovery** — discover multiple providers concurrently
- **Persistent Cache** — SQLite storage with TTL
- **Periodic Refresh** — re-discover every 24 hours
- **Circuit Breaker** — skip failing providers automatically
- **Model Metadata** — track pricing, latency, quota per model

## Files Overview

```
src/providers/
├── mod.rs                 # Main init_providers() - MODIFIED
├── types.rs              # Core types - MODIFIED (added 4 new types)
├── registry.rs           # Unchanged
├── templates.rs          # Provider templates - NEW
├── discovery.rs          # Discovery logic - NEW
└── filters.rs            # Model filtering - NEW

Root:
└── DYNAMIC_DISCOVERY.md  # Detailed docs - NEW
```

## Validation Checklist

- [x] Types added: `ProviderTemplate`, `FreeModelFilter`, `ModelsResponse`, `ModelInfo`
- [x] Templates defined for 6 providers
- [x] Discovery logic handles auth requirements
- [x] Filtering supports 5 strategies
- [x] Error types defined and handled
- [x] Logging at all discovery steps
- [x] Async init integrated into main.rs
- [x] Glob dependency added to Cargo.toml
- [x] Documentation written
- [x] No breaking changes to existing code

## Next Steps

1. **Test startup** — Run with/without API keys, check logs
2. **Add test providers** — Verify filter logic with real responses
3. **Implement periodic refresh** (Phase 3) — Background discovery task
4. **Add circuit breaker** (Phase 3) — Resilience for failing providers
5. **Performance optimization** — Parallel discovery, caching

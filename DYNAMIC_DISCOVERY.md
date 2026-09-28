# Dynamic Provider Discovery System

## Overview

RustAgent now uses **dynamic provider discovery** instead of hardcoded provider configurations. This system:

- Reads environment variables for API keys
- Calls each provider's `/v1/models` endpoint
- Filters to **free models only** using provider-specific rules
- Creates `Provider` instances with discovered models
- Gracefully handles missing API keys and API errors

## Architecture

```
Startup
  ↓
Read Provider Templates (templates.rs)
  ↓
For each template:
  ├─ Check env for API key (e.g., GROQ_API_KEY)
  ├─ Skip if auth required but no key
  └─ If key found or no auth required:
      ├─ Call GET /v1/models endpoint
      ├─ Parse response (OpenAI-compatible format)
      ├─ Filter free models (filters.rs)
      ├─ Create Provider instance
      └─ Add to registry
  ↓
Return providers map (only with free models)
```

## New Modules

### `types.rs` — Core Data Structures

**`Provider`** (existing, unchanged)
- `id`, `name`, `api_endpoint`, `auth_type`
- `models` — list of free model IDs
- `health_status`, `last_check`

**`ProviderTemplate`** (new)
- Metadata for a provider (no API key, no models yet)
- `id` — unique provider identifier
- `name` — display name
- `api_endpoint` — base URL (e.g., `https://api.groq.com/v1`)
- `auth_type` — "none", "api_key", or "bearer"
- `env_var_name` — where to look for API key (e.g., "GROQ_API_KEY")
- `free_quota_daily`, `free_quota_monthly` — known free tier limits
- `rate_limit_rpm` — requests per minute
- `free_model_filter` — strategy to detect free models
- `discovery_timeout_secs` — HTTP request timeout

**`FreeModelFilter`** (new)
Enum with 5 strategies:

1. **`All`** — Every model is free
   - Groq (all models free on free tier)
   - Pollinations (all free, no auth)

2. **`None`** — No free models
   - OpenAI paid tier (no free models)

3. **`Patterns(Vec<String>)`** — Glob patterns
   - Filter models by name patterns
   - Example: `"meta-llama/Llama-*-chat-hf"`
   - Used by Together AI, DeepInfra

4. **`ExplicitList(Vec<String>)`** — Known free model IDs
   - Only these specific models are free
   - Used by HuggingFace (limited free tier)

5. **`PricingAPI { field, zero_means_free }`** — Detect via pricing API
   - Check a field in response (e.g., `pricing.prompt`)
   - If field == 0, model is free
   - Used by OpenRouter

**`ModelsResponse`** (new)
- OpenAI-compatible models list response
- Contains `data: Vec<ModelInfo>`

**`ModelInfo`** (new)
- Individual model info from API
- `id` — model identifier
- `object` — "model" (OpenAI standard)
- `owned_by` — provider name
- `pricing` — optional pricing object

### `templates.rs` — Provider Templates

Defines 6 provider templates with discovery metadata:

```rust
pub fn get_provider_templates() -> Vec<ProviderTemplate>
```

**Providers included:**

| Provider | Auth | Free Models | Filter Type |
|----------|------|-------------|------------|
| **Pollinations** | None | All | `All` |
| **Groq** | API Key | All | `All` |
| **Together AI** | API Key | Llama, Mistral | `Patterns` |
| **HuggingFace** | API Key | Llama-2, Mistral | `ExplicitList` |
| **OpenRouter** | API Key | Price == 0 | `PricingAPI` |
| **DeepInfra** | API Key | Llama-2, Mistral | `Patterns` |

### `discovery.rs` — Discovery Logic

**`discover_provider(template, api_key) -> Result<Option<Provider>, DiscoveryError>`**

Main discovery function:

1. Check if auth required but key missing → `Ok(None)` (skip)
2. Call `fetch_models()` to get `/v1/models` list
3. Filter to free models using template's `FreeModelFilter`
4. Create `Provider` with discovered free models
5. Return `Ok(Some(provider))` or error

**`fetch_models(endpoint, api_key) -> Result<Vec<ModelInfo>, DiscoveryError>`**

HTTP client logic:

- Constructs `GET {endpoint}/models`
- Adds `Authorization: Bearer {api_key}` if provided
- Sets 10-second timeout
- Parses OpenAI-compatible response
- Handles 401/403 auth errors
- Returns error details for debugging

**Error Types:**

```rust
pub enum DiscoveryError {
    HttpError(String),      // Network failure
    JsonError(String),      // Parse failure
    ApiError { status, body },  // HTTP error
    Timeout,                // Request timeout
    NoModels,               // API returned empty list
    AuthError(String),      // 401/403
}
```

### `filters.rs` — Free Model Filtering

**`filter_free_models(models, filter) -> Vec<String>`**

Filters model list by strategy:

- **`All`** — Return all models
- **`None`** — Return empty list
- **`Patterns(patterns)`** — Match against glob patterns
  - Uses `glob` crate for glob pattern matching
  - Example: `"meta-llama/Llama-*-chat-hf"` matches `"meta-llama/Llama-3-70b-chat-hf"`
- **`ExplicitList(ids)`** — Exact match against known list
- **`PricingAPI { field, zero_means_free }`** — Check pricing object
  - Supports nested fields: `"pricing.prompt"`
  - If `zero_means_free=true`, then `field == 0` means free
  - If field missing, assumes free

**`matches_pattern(model_id, pattern) -> bool`**

Uses glob crate to match model IDs:
```rust
"meta-llama/Llama-3-70b-chat-hf" matches "meta-llama/Llama-*-chat-hf" ✓
"mistralai/Mistral-7B" matches "meta-llama/*" ✗
```

**`check_pricing_field(pricing, field_path, zero_means_free) -> bool`**

Navigates nested JSON to check pricing:
```rust
check_pricing_field({"pricing": {"prompt": 0}}, "pricing.prompt", true)
// → true (price is 0, free model)
```

### `mod.rs` — Main Init Function

**`pub async fn init_providers() -> HashMap<String, Provider>`**

Async initialization routine:

1. Load provider templates
2. Log discovery start
3. For each template:
   - Check environment for API key
   - Skip if auth required but no key (log info)
   - Call `discover_provider()` async
   - On success: add to providers map (log info)
   - On `Ok(None)`: no free models available (log warn)
   - On error: log error, continue to next provider
4. Return map of discovered providers

**Key features:**
- All discovery requests run sequentially (clean startup logs)
- Providers can fail independently (one API down ≠ total failure)
- Logging at every step (debug/info/warn/error)
- Returns `HashMap<String, Provider>` with only free models

## Configuration

### Environment Variables

Set these to enable provider discovery:

```bash
# Groq (all models free on free tier)
export GROQ_API_KEY="gsk_..."

# Together AI
export TOGETHER_API_KEY="..."

# HuggingFace
export HUGGINGFACE_API_KEY="hf_..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."

# DeepInfra
export DEEPINFRA_API_KEY="..."

# Pollinations (optional, works without key)
export POLLINATIONS_API_KEY="..."
```

### No Configuration Required

These providers work without API keys:
- **Pollinations** — uses `https://api.pollinations.ai/v1`

### Fallback Behavior

If no API keys are provided:
- Pollinations is attempted first (no auth required)
- If Pollinations fails, RustAgent starts with no providers
- All other providers require API keys

## Usage Example

### Startup with Groq API Key

```bash
$ export GROQ_API_KEY="gsk_xxx"
$ cargo run
```

Logs:
```
INFO rust_agent: Starting RustAgent Gateway
INFO rust_agent: Database initialized
INFO rust_agent::providers: Starting dynamic provider discovery
  template_count = 6
DEBUG rust_agent::providers::discovery: Starting provider discovery
  provider = "pollinations", auth_required = false
DEBUG rust_agent::providers::discovery: Fetched 12 models from provider
INFO rust_agent::providers: Successfully discovered provider with free models
  provider = "pollinations", model_count = 12
  models = ["gpt-4", "claude-3", "llama-70b", ...]
DEBUG rust_agent::providers::discovery: Starting provider discovery
  provider = "groq", auth_required = true
DEBUG rust_agent::providers::discovery: Fetched 3 models from provider
INFO rust_agent::providers: Successfully discovered provider with free models
  provider = "groq", model_count = 3
  models = ["mixtral-8x7b-32768", "llama-3-70b", "llama-3-8b"]
INFO rust_agent::providers: Provider discovery completed
  provider_count = 2
```

### Startup without API Keys

```bash
$ cargo run
```

Logs:
```
INFO rust_agent: Starting RustAgent Gateway
INFO rust_agent: Database initialized
INFO rust_agent::providers: Starting dynamic provider discovery
  template_count = 6
INFO rust_agent::providers: Provider skipped: authentication required but no API key found
  provider = "groq", env_var = "GROQ_API_KEY"
INFO rust_agent::providers: Provider skipped: authentication required but no API key found
  provider = "together", env_var = "TOGETHER_API_KEY"
...
DEBUG rust_agent::providers::discovery: Starting provider discovery
  provider = "pollinations", auth_required = false
INFO rust_agent::providers: Successfully discovered provider with free models
  provider = "pollinations", model_count = 12
  models = ["gpt-4", "claude-3", "llama-70b", ...]
INFO rust_agent::providers: Provider discovery completed
  provider_count = 1
```

## Adding a New Provider

To add support for a new provider:

1. **Add to templates.rs:**
   ```rust
   pub fn get_provider_templates() -> Vec<ProviderTemplate> {
       vec![
           // ... existing providers ...
           ProviderTemplate::new(
               "my_provider",
               "My Provider",
               "https://api.myprovider.com/v1",
               "api_key",  // or "none" or "bearer"
               "MY_PROVIDER_API_KEY",
               Some(500_000),  // daily quota if known
               None,
               100,  // RPM
               FreeModelFilter::Patterns(vec![
                   "my-model-*".to_string(),
               ]),
           ),
       ]
   }
   ```

2. **Define the free model filter:**
   - Use `All` if all models are free
   - Use `Patterns` with glob patterns if specific models are free
   - Use `ExplicitList` if only certain models are known to be free
   - Use `PricingAPI` if provider has pricing info in `/v1/models` response

3. **Set environment variable (if auth required):**
   ```bash
   export MY_PROVIDER_API_KEY="..."
   ```

4. **Test discovery:**
   ```bash
   RUST_LOG=debug cargo run
   ```
   Look for logs showing model discovery for your provider.

## Limitations & Future Work

### Current Limitations
- Discovery runs synchronously (providers discovered one at a time)
- No caching of discovered models (re-discovered on every startup)
- No periodic refresh (24-hour re-discovery not yet implemented)
- No fallback for discovery failures (single provider down stops discovery)

### Phase 3 Improvements
- **Parallel discovery** — fetch from multiple providers concurrently
- **Persistent model cache** — SQLite storage with TTL
- **Periodic refresh** — background task to re-discover every 24 hours
- **Circuit breaker** — skip providers failing consistently
- **Model metadata** — track pricing, latency, quota per model

## Testing

### Unit Tests

Run filter tests:
```bash
cargo test --lib filters
```

Tests cover:
- Glob pattern matching
- Explicit list filtering
- Pricing API field navigation

### Integration Test (Manual)

With Groq API key:
```bash
export GROQ_API_KEY="gsk_..."
export RUST_LOG=debug
cargo run
curl http://localhost:20128/v1/models
```

Expected response includes Groq models:
```json
{
  "data": [
    {"id": "mixtral-8x7b-32768", ...},
    {"id": "llama-3-70b", ...},
    ...
  ]
}
```

## Troubleshooting

### "Provider skipped: authentication required but no API key found"

**Cause:** Template requires auth but env var not set.

**Fix:** Set the environment variable:
```bash
export GROQ_API_KEY="your-key-here"
```

### "Failed to discover provider: HTTP request failed"

**Cause:** Network error or provider API down.

**Solution:** 
- Check internet connection
- Verify provider status page
- Check firewall rules
- Increase timeout if network is slow

### "Failed to discover provider: Authentication failed"

**Cause:** Invalid API key.

**Fix:** Verify API key is correct and has permissions.

### No providers discovered

**Solution:**
- Ensure at least one API key is set
- Or rely on Pollinations (no auth required)
- Check logs: `RUST_LOG=debug cargo run`

## Files Changed

- **`src/providers/types.rs`** — Added `ProviderTemplate`, `FreeModelFilter`, `ModelsResponse`, `ModelInfo`
- **`src/providers/templates.rs`** — New file with 6 provider templates
- **`src/providers/discovery.rs`** — New file with discovery logic and HTTP client
- **`src/providers/filters.rs`** — New file with free model filtering
- **`src/providers/mod.rs`** — Refactored `init_providers()` to be async and use discovery
- **`src/main.rs`** — Updated to call `init_providers().await`
- **`Cargo.toml`** — Added `glob = "0.3"` dependency

## Summary

This system replaces hardcoded provider lists with a flexible, extensible discovery mechanism that:

✅ Reads API keys from environment
✅ Discovers models dynamically at startup
✅ Filters to free models only
✅ Supports 6+ providers out-of-the-box
✅ Handles missing auth gracefully
✅ Logs all discovery steps clearly
✅ Easy to add new providers
✅ No external dependencies (SQLite only)

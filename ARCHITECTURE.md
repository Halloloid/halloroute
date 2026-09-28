# RustAgent - Modular Architecture

## Overview

RustAgent is refactored into a clean, modular Rust architecture optimized for maintainability, testing, and future feature expansion. Each module has a single responsibility and clear boundaries.

---

## Module Structure

```
src/
├── main.rs                          # Entry point (minimal, delegates to lib)
├── lib.rs                           # Library root with public re-exports
├── server.rs                        # Axum application setup & listener
├── state.rs                         # Shared application state (AppState)
│
├── api/                             # HTTP API layer
│   ├── mod.rs                       # Route registration (public routes() fn)
│   ├── v1/                          # OpenAI-compatible v1 endpoints
│   │   ├── mod.rs                   # Sub-route registration
│   │   ├── chat.rs                  # POST /v1/chat/completions
│   │   ├── models.rs                # GET /v1/models
│   │   └── health.rs                # GET /health
│   └── admin/                       # Admin management endpoints
│       ├── mod.rs                   # Sub-route registration
│       ├── providers.rs             # GET /admin/providers, POST /admin/providers/:id/{enable,disable}
│       └── quota.rs                 # GET /admin/quota
│
├── errors/                          # Error handling
│   └── mod.rs                       # ApiError enum + IntoResponse impl
│
├── providers/                       # Provider registry & types
│   ├── mod.rs                       # Public API: init_providers(), Provider re-exports
│   ├── types.rs                     # Provider, ProviderHealth struct definitions
│   └── registry.rs                  # ProviderRegistry logic (model lookup, availability filtering)
│
├── routing/                         # Smart request routing
│   ├── mod.rs                       # Public API: Router, RoutingStrategy re-exports
│   ├── strategies.rs                # RoutingStrategy enum (Auto, Priority, RoundRobin, QuotaFirst, LatencyFirst)
│   └── selector.rs                  # Provider selection implementations
│
├── quota/                           # Quota management & tracking
│   ├── mod.rs                       # Public API: QuotaManager, pool initialization
│   └── tracker.rs                   # DbPool (DashMap-based in-memory quota tracker)
│
├── config/                          # Configuration
│   └── mod.rs                       # Config, ServerConfig, DatabaseConfig structs + env loading
│
├── middleware/                      # HTTP middleware (Phase 2+)
│   ├── mod.rs                       # Middleware registration
│   ├── logging.rs                   # Request/response logging (stub)
│   └── auth.rs                      # API key authentication (stub)
│
├── http/                            # HTTP client utilities (Phase 2+)
│   ├── mod.rs                       # HttpClient exports
│   └── client.rs                    # Reqwest pooling & setup (stub)
│
└── cache/                           # Request caching (Phase 3+)
    ├── mod.rs                       # CacheLayer trait & exports
    └── memory.rs                    # In-memory cache impl (stub)
```

---

## Module Responsibilities

### `api/` — HTTP API Layer
**Purpose**: Handle HTTP requests/responses, route dispatch, request validation.

- **v1/chat.rs** — Chat completions endpoint
  - Validates ChatCompletionRequest
  - Calls router to select provider
  - Checks quota
  - Forwards to provider
  - Returns OpenAI-compatible response

- **v1/models.rs** — Model list endpoint
  - Returns all available models from all providers

- **v1/health.rs** — Health check
  - Returns `{ "status": "ok" }`

- **admin/providers.rs** — Provider management
  - List all providers with status
  - Enable/disable providers

- **admin/quota.rs** — Quota tracking
  - Get quota usage per provider per day

**Dependencies**: `state`, `errors`, `providers`, `quota`, `routing`

---

### `providers/` — Provider Registry
**Purpose**: Define provider types, initialize provider pool, provide provider lookup.

- **types.rs** — Data structures
  - `Provider` — provider config, models, auth, quotas, health
  - `ProviderHealth` — Healthy, Degraded, Down enum

- **registry.rs** — Provider lookup logic
  - `ProviderRegistry` — collection of providers with methods:
    - `get_provider(id)` — lookup by ID
    - `list_providers()` — all providers
    - `list_available_providers()` — healthy providers only
    - `get_model_provider(model_name)` — find provider for specific model
    - `list_models()` — all available models

- **mod.rs** — Initialization & re-exports
  - `init_providers()` — creates HashMap with 5 free-tier providers
  - Public re-exports of Provider, ProviderHealth

**Dependencies**: None (fundamental type definitions)

---

### `routing/` — Smart Provider Selection
**Purpose**: Select best provider based on strategy, quota, availability.

- **strategies.rs** — Routing strategies
  - `RoutingStrategy` enum: Auto (default), Priority, RoundRobin, QuotaFirst, LatencyFirst

- **selector.rs** — Selection implementation
  - `Router::select_provider()` — dispatches to strategy implementation
  - `select_auto()` — by model name, fallback to round-robin
  - `select_priority()` — first available provider
  - `select_round_robin()` — cycle through available providers
  - `select_quota_first()` — provider with most remaining quota
  - `select_latency_first()` — placeholder (current: round-robin)

- **mod.rs** — Router initialization & re-exports
  - `Router::new(registry, strategy)`
  - Public re-exports of Router, RoutingStrategy

**Dependencies**: `providers`, `quota`, `errors`

---

### `quota/` — Quota Tracking
**Purpose**: Track API usage per provider per day, enforce limits, prevent overages.

- **tracker.rs** — In-memory storage
  - `DbPool` — DashMap<"provider_id:YYYY-MM-DD", tokens_used>
  - `init_pool()` — create new DashMap
  - `get_today_quota_usage(provider_id)` — today's total tokens
  - `increment_quota_usage(provider_id, tokens)` — record usage

- **mod.rs** — Quota manager & re-exports
  - `QuotaManager::check_quota()` — validate request fits in limit
  - `QuotaManager::can_use_provider()` — check health + quota availability
  - `QuotaManager::record_usage()` — log token usage after request

**Dependencies**: `errors`, `providers`

---

### `errors/` — Error Handling
**Purpose**: Define error types, map to HTTP responses.

- **mod.rs** — Error types & responses
  - `ApiError` enum: DatabaseError, ProviderError, InvalidRequest, ProviderNotFound, QuotaExceeded, AllProvidersUnavailable, RequestTimeout, InternalError
  - `IntoResponse` impl — maps errors to HTTP status + JSON body
  - `ApiResult<T>` — type alias for `Result<T, ApiError>`

**Dependencies**: None

---

### `config/` — Configuration
**Purpose**: Load settings from environment, provide defaults.

- **mod.rs** — Configuration
  - `Config` — top-level config
  - `ServerConfig` — host, port, workers
  - `DatabaseConfig` — DB path
  - `Config::from_env()` — load from `RUST_AGENT_*` env vars with defaults

**Dependencies**: None

---

### `state/` — Shared Application State
**Purpose**: Define the shared state passed to all handlers.

- **state.rs** — AppState
  - `AppState` — holds `db_pool` (DbPool) and `providers` (HashMap<String, Provider>)
  - Wrapped in `Arc<AppState>` for thread-safe sharing

**Dependencies**: `quota`, `providers`

---

### `middleware/` — HTTP Middleware (Phase 2+)
**Purpose**: Cross-cutting concerns like logging, authentication, rate limiting.

- **logging.rs** — Request/response logging (stub)
- **auth.rs** — API key validation (stub)

**Dependencies**: Will depend on `errors`, `state`

---

### `http/` — HTTP Client (Phase 2+)
**Purpose**: Centralized HTTP client setup with pooling, timeouts, retries.

- **client.rs** — Reqwest client factory (stub)
  - Connection pooling
  - Keepalive configuration
  - Timeout defaults

**Dependencies**: Will depend on `config`, `errors`

---

### `cache/` — Request Caching (Phase 3+)
**Purpose**: Cache responses for duplicate requests within TTL.

- **memory.rs** — In-memory cache (stub)
  - Content-addressed storage (key = hash of request + model)
  - TTL-based expiration
  - LRU eviction

**Dependencies**: Will depend on `errors`

---

## Data Flow Example: Chat Completions Request

```
Client Request
    ↓
POST /v1/chat/completions
    ↓
api::v1::chat_completions()
    ├─ Validates ChatCompletionRequest
    ├─ Calls state.db_pool (quota tracker)
    ├─ Creates Router with default strategy
    ├─ Router::select_provider() 
    │   ├─ Calls providers registry to find model
    │   ├─ Checks QuotaManager::can_use_provider()
    │   └─ Returns selected Provider
    ├─ QuotaManager::check_quota() — verify tokens fit
    ├─ Makes HTTP request to provider
    ├─ QuotaManager::record_usage() — log tokens
    └─ Returns OpenAI-compatible ChatCompletionResponse
        ↓
    Client Response
```

---

## Dependency Graph (Clean Layers)

```
┌─────────────────────────────────────────────┐
│            HTTP API Layer                   │
│  (api/ - handles requests/responses)        │
└──────────────┬──────────────────────────────┘
               │
     ┌─────────┼─────────┐
     ↓         ↓         ↓
┌────────────┐ ┌──────────────┐ ┌──────────────┐
│  Routing   │ │   Quota      │ │  Errors      │
│  (routing/)│ │  (quota/)    │ │  (errors/)   │
└────────────┘ └──────────────┘ └──────────────┘
     │              │
     └──────┬───────┘
            ↓
    ┌──────────────────┐
    │   Providers      │
    │  (providers/)    │
    └──────────────────┘
            ↓
    ┌──────────────────┐
    │    Config        │
    │   (config/)      │
    └──────────────────┘
```

---

## Adding New Features

### Phase 2: Circuit Breaker + Streaming
1. Update `providers/types.rs` — add `CircuitBreakerState` to Provider
2. Create `providers/circuit_breaker.rs` — track failures, auto-recovery
3. Update `routing/selector.rs` — check circuit breaker status before routing
4. Create `api/v1/chat_streaming.rs` — SSE response handler
5. Update `http/client.rs` — implement actual HTTP pooling

### Phase 3: Request Caching
1. Implement `cache/memory.rs` — in-memory cache with TTL
2. Create `cache/key.rs` — content-addressed caching
3. Update `api/v1/chat.rs` — check cache before calling provider

### Phase 4: Dashboard
1. Create `api/dashboard/` — static file serving
2. Create `api/dashboard/api.rs` — data endpoints (provider status, quota history)

### Phase 5: Observability
1. Update `middleware/logging.rs` — structured logging with correlation IDs
2. Create `observability/metrics.rs` — Prometheus-compatible metrics export
3. Create `observability/traces.rs` — distributed tracing setup

---

## Module Public APIs

### `providers`
```rust
pub struct Provider { /* ... */ }
pub enum ProviderHealth { Healthy, Degraded, Down }
pub struct ProviderRegistry { /* ... */ }

pub fn init_providers() -> HashMap<String, Provider>
```

### `routing`
```rust
pub enum RoutingStrategy { Auto, Priority, RoundRobin, QuotaFirst, LatencyFirst }
pub struct Router { /* ... */ }

impl Router {
    pub fn new(registry: ProviderRegistry, strategy: RoutingStrategy) -> Self
    pub async fn select_provider(&self, pool: &DbPool, model: Option<&str>) -> ApiResult<&Provider>
}
```

### `quota`
```rust
pub struct QuotaManager;
pub struct DbPool { /* ... */ }

impl QuotaManager {
    pub async fn check_quota(pool: &DbPool, provider: &Provider, tokens: i64) -> ApiResult<bool>
    pub async fn can_use_provider(pool: &DbPool, provider: &Provider) -> ApiResult<bool>
    pub async fn record_usage(pool: &DbPool, provider_id: &str, tokens: i64) -> ApiResult<()>
}

pub async fn init_pool() -> ApiResult<DbPool>
pub async fn run_migrations(pool: &DbPool) -> ApiResult<()>
```

### `errors`
```rust
pub enum ApiError { /* ... */ }
pub type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError { /* ... */ }
```

### `api`
```rust
pub fn routes() -> Router<Arc<AppState>>
pub async fn health_check() -> Json<HealthResponse>
pub async fn chat_completions(...) -> ApiResult<HttpResponse>
pub async fn list_models(...) -> ApiResult<Json<ModelsResponse>>
pub async fn get_providers(...) -> ApiResult<Json<...>>
pub async fn get_quota_status(...) -> ApiResult<Json<...>>
pub async fn disable_provider(...) -> ApiResult<Json<...>>
pub async fn enable_provider(...) -> ApiResult<Json<...>>
```

---

## Testing Strategy

Each module can be tested independently:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Test providers registry
    #[test]
    fn test_provider_lookup() { /* ... */ }

    // Test routing strategies
    #[tokio::test]
    async fn test_auto_routing() { /* ... */ }

    // Test quota tracking
    #[tokio::test]
    async fn test_quota_enforcement() { /* ... */ }

    // Test error responses
    #[test]
    fn test_error_to_http_response() { /* ... */ }

    // Integration tests
    #[tokio::test]
    async fn test_chat_completions_endpoint() { /* ... */ }
}
```

---

## Performance Considerations

1. **In-Memory State** — DashMap allows lock-free concurrent access to quota tracker
2. **Provider Registry** — HashMap lookup is O(1) for provider selection
3. **Routing** — Strategies are optimized for minimal iteration over providers
4. **Future: HTTP Pooling** — Connection reuse will reduce latency per request

---

## Maintainability Benefits

1. **Clear Separation of Concerns** — each module has one job
2. **Easy Testing** — modules can be tested independently
3. **Easy Feature Addition** — new features fit naturally into existing structure
4. **Easy Debugging** — clear data flow makes issues easier to trace
5. **Easy Onboarding** — new developers can understand module responsibilities quickly

---

## Next Steps: Phase 2

The modular architecture is now ready for Phase 2 implementation:

- [ ] Add circuit breaker for provider health tracking
- [ ] Implement streaming responses with SSE
- [ ] Add API key authentication middleware
- [ ] Implement actual provider HTTP clients
- [ ] Add retry/backoff logic
- [ ] Create fallback chains for resilience

Refer to the skills in `.agents/skills/` for implementation patterns.

# RustAgent Skills Library

Complete skill set for building an optimized, production-ready AI gateway.

## Skills Overview

### 1. **rust-axum-patterns** ✅
- Axum web framework setup with shared state
- Request extractors and error handling
- Streaming responses (SSE)
- Route organization and middleware
- **Use when:** Setting up HTTP endpoints, request handling

### 2. **provider-circuit-breaker** ✅
- Circuit breaker state machine (Closed/Open/HalfOpen)
- Exponential backoff retry logic
- Fallback chains (A → B → C routing)
- Provider failure detection and auto-recovery
- **Use when:** Implementing resilient provider routing

### 3. **quota-tracking-patterns** ✅
- Daily/weekly/monthly quota tracking
- Race-free concurrent updates with DashMap
- Rate limiting (per-minute RPM limits)
- Alert thresholds (warning/critical)
- Background reset jobs
- **Use when:** Managing free-tier quotas

### 4. **axum-streaming** ✅
- OpenAI-compatible SSE streaming format
- Provider streaming integration
- Error recovery in streams
- Client disconnection handling
- Backpressure management
- **Use when:** Building `/v1/chat/completions` streaming

### 5. **performance-optimization** ✅
- Connection pooling (HTTP client reuse)
- Concurrency limiters (semaphores)
- Buffer pooling and reuse
- Memory-efficient quota tracking (atomics)
- Request deduplication
- Graceful shutdown handling
- **Use when:** Optimizing for <100MB RAM, sub-100ms latency

### 6. **request-caching** ✅
- Content-addressed cache with TTL
- Request deduplication
- Streaming response caching
- LRU/LFU/TTL invalidation strategies
- Cache warming
- Hit rate metrics
- **Use when:** Reducing redundant provider calls

### 7. **token-counting** ✅
- Token estimation (word count, char-based)
- Provider-specific tokenizers (GPT, Claude)
- Context window management
- Token cost calculation
- Usage statistics tracking
- Provider token limits
- **Use when:** Accurate quota management, context validation

### 8. **provider-integration** ✅
- Provider client trait interface
- OpenCode implementation
- Groq implementation
- Provider factory pattern
- Request/response normalization
- Error handling and retries
- **Use when:** Integrating new AI providers

### 9. **observability-monitoring** ✅
- Structured logging with correlation IDs
- Metrics collection (counters, latencies, rates)
- Health checks (liveness/readiness)
- Prometheus metrics export
- Dashboard backend API
- Tracing setup
- **Use when:** Production monitoring, debugging

### 10. **error-handling-recovery** ✅
- Comprehensive error types
- IntoResponse trait implementation
- Retry logic with exponential backoff
- Graceful degradation strategies
- Timeout handling
- Error recovery manager
- User-friendly error messages
- **Use when:** Building robust error handling

### 11. **configuration-management** ✅
- Environment-based config loading
- TOML/JSON configuration files
- API key management (secure)
- Feature flags (runtime toggleable)
- Runtime configuration updates
- Configuration validation
- **Use when:** Managing environment-specific settings

---

## Quick Reference: Which Skill to Use

| Task | Skill(s) |
|------|----------|
| Build HTTP endpoints | rust-axum-patterns |
| Handle provider failures | provider-circuit-breaker, error-handling-recovery |
| Track free-tier quotas | quota-tracking-patterns, token-counting |
| Stream responses | axum-streaming |
| Optimize memory/latency | performance-optimization, request-caching |
| Add new AI provider | provider-integration |
| Monitor production | observability-monitoring |
| Manage secrets/config | configuration-management |
| Token estimation | token-counting |
| Request caching | request-caching |

---

## Development Phases & Skill Usage

### Phase 1: MVP (Week 1-2)
- ✅ rust-axum-patterns — HTTP server
- ✅ provider-integration — single provider
- ✅ quota-tracking-patterns — basic tracking
- ✅ configuration-management — env config
- ✅ error-handling-recovery — basic errors

### Phase 2: Resilience (Week 3-4)
- ✅ provider-circuit-breaker — fallback logic
- ✅ error-handling-recovery — retry strategies
- ✅ axum-streaming — streaming responses
- ✅ observability-monitoring — request logging

### Phase 3: Optimization (Week 5-6)
- ✅ performance-optimization — memory/concurrency
- ✅ request-caching — deduplication
- ✅ token-counting — accurate estimation
- ✅ quota-tracking-patterns — alert thresholds

### Phase 4: Observability (Week 7-8)
- ✅ observability-monitoring — metrics/dashboard
- ✅ configuration-management — runtime flags
- ✅ error-handling-recovery — user-friendly errors

### Phase 5: Production (Week 9+)
- All skills combined
- Docker optimization
- Documentation

---

## How to Use Skills

1. **Identify your task** — Check the quick reference table
2. **Open the skill file** — `.agents/skills/<skillname>/SKILL.md`
3. **Reference the patterns** — Copy-paste code examples
4. **Adapt to your context** — Modify for your specific needs
5. **Follow the structure** — Use the organization shown in the skill

---

## Skill Dependencies

```
rust-axum-patterns (foundation)
  ├─ provider-integration (needs handlers)
  │   ├─ provider-circuit-breaker (fallback)
  │   └─ error-handling-recovery (errors)
  ├─ quota-tracking-patterns (tracking)
  │   └─ token-counting (token math)
  ├─ axum-streaming (streaming)
  ├─ performance-optimization (efficiency)
  │   └─ request-caching (memory)
  ├─ observability-monitoring (logging)
  └─ configuration-management (config)
```

---

## Key Architectural Decisions

1. **Axum over Actix** — Lighter, composable, streaming first-class
2. **DashMap for state** — Lock-free concurrent hashmaps
3. **In-memory for Phase 1** — SQLite added in Phase 3
4. **Circuit breaker** — Resilience via state machine, not just retries
5. **Token counting** — Estimated in Phase 1, accurate in Phase 3
6. **Feature flags** — Runtime toggleable, not compile-time

---

## Common Patterns Across Skills

- **Arc<T>** — Shared ownership across async tasks
- **DashMap** — Lock-free concurrent access
- **Tokio** — Async runtime for all operations
- **Serde** — JSON serialization everywhere
- **Thiserror** — Custom error types
- **Tracing** — Structured logging with context

---

## Testing Strategy by Skill

| Skill | Test Approach |
|-------|---------------|
| rust-axum-patterns | Integration tests with test server |
| provider-circuit-breaker | Unit tests for state transitions |
| quota-tracking-patterns | Unit tests for quota math |
| axum-streaming | Mock provider streams |
| performance-optimization | Benchmark tests, memory profiling |
| request-caching | Unit tests for cache hits/misses |
| token-counting | Unit tests with known token counts |
| provider-integration | Mock HTTP responses |
| observability-monitoring | Unit tests for metric recording |
| error-handling-recovery | Unit tests for error types |
| configuration-management | Unit tests for parsing/validation |

---

## Performance Targets

| Metric | Target | Skill |
|--------|--------|-------|
| Idle RAM | <100MB | performance-optimization |
| Routing latency | <100ms | provider-circuit-breaker |
| Cache hit rate | >50% | request-caching |
| Error recovery time | <5s | provider-circuit-breaker |
| P95 latency | <500ms | performance-optimization |
| Throughput | >100 req/s | axum-streaming, performance-optimization |

---

## Next Steps

1. **Start Phase 1** — Use skills in order: axum-patterns → provider-integration → quota-tracking → configuration-management → error-handling
2. **Reference as needed** — Don't memorize, copy-paste and adapt
3. **Build incrementally** — Test each skill in isolation
4. **Progress to Phase 2** — Add circuit-breaker, streaming
5. **Optimize in Phase 3** — Performance, caching, token-counting

---

**All skills are production-ready patterns. Use them as your implementation playbook.**

# RustAgent — Optimized Free AI Gateway

A lightweight, memory-efficient Rust-based AI gateway that routes requests to free-tier AI providers only. Built as an alternative to OmniRoute with a focus on minimal resource usage for background service deployment.

## Project Vision

- **Memory-efficient** — target <100MB idle RAM (vs OmniRoute's ~1GB+ in Node.js)
- **Free models only** — no paid tiers, ever
- **Production-ready fallback** — intelligent routing with circuit breakers & auto-retry
- **OpenAI-compatible API** — drop-in replacement for existing clients
- **Zero external dependencies** — SQLite only, runs standalone

## Architecture

```
Client Apps (Claude Code, Cursor, Codex)
           ↓
    RustAgent Gateway (HTTP Server)
           ↓
    Smart Router (Load Balance, Fallback, Quota)
           ↓
    Free Provider Pool
    ├─ OpenCode (DeepSeek V3.2, Nemotron 3)
    ├─ Groq (multi-model, free RPM)
    ├─ Pollinations (no key needed)
    ├─ Kilo Code (free tier)
    └─ Other free providers...
```

## Tech Stack

- **Language**: Rust 1.70+
- **HTTP Server**: Axum 0.7 (lightweight, composable)
- **Async Runtime**: Tokio 1.35
- **Concurrency**: DashMap (lock-free state)
- **Database**: In-memory (Phase 1-2), SQLite in Phase 3+
- **Serialization**: Serde + serde_json
- **HTTP Client**: Reqwest 0.12
- **Logging**: Tracing + tracing-subscriber
- **Config**: Environment variables + Serde
- **Frontend**: HTMX + Askama SSR templates (Phase 4, ultra-low memory, zero Node.js runtime)

## Core Components

### 1. Provider Registry (`providers/`)
- **types.rs** — Provider, ProviderHealth structs
- **registry.rs** — ProviderRegistry with model lookup
- **mod.rs** — init_providers() with 5 free providers (OpenCode, Groq, Pollinations, Together, HuggingFace)
- Per-provider: API endpoints, quota limits, rate limits, auth type
- Health status tracking (Healthy, Degraded, Down)

### 2. Router/Dispatcher (`routing/`)
- **strategies.rs** — RoutingStrategy enum (Auto, Priority, RoundRobin, QuotaFirst, LatencyFirst)
- **selector.rs** — Provider selection logic by strategy
- **mod.rs** — Router initialization
- Strategies: model-based routing, availability filtering, quota-aware selection
- Phase 2: circuit breaker, retry with backoff, fallback chains

### 3. Quota Manager (`quota/`)
- **tracker.rs** — DbPool (DashMap-based in-memory quota tracker)
- **mod.rs** — QuotaManager with check/record methods
- Tracks daily usage per provider in memory ("provider_id:YYYY-MM-DD" → tokens)
- Phase 3: Migrate to SQLite with automatic daily/monthly reset timers

### 4. HTTP API (`api/`)
- **v1/chat.rs** — POST /v1/chat/completions (OpenAI-compatible)
- **v1/models.rs** — GET /v1/models (list available models)
- **v1/health.rs** — GET /health (health check)
- **admin/providers.rs** — GET /admin/providers, POST /admin/providers/:id/{enable,disable}
- **admin/quota.rs** — GET /admin/quota (quota status per provider)
- Phase 2: Add streaming with SSE
- Phase 3: Add /v1/embeddings

### 5. Error Handling (`errors/`)
- **mod.rs** — ApiError enum with IntoResponse impl
- Error types: DatabaseError, ProviderError, InvalidRequest, QuotaExceeded, AllProvidersUnavailable, etc.
- HTTP response mapping: 400/401/429/503/504 status codes

### 6. Supporting Modules
- **state.rs** — AppState (db_pool + providers HashMap)
- **config/** — Config loading from environment
- **middleware/** — Stubs for logging, auth (Phase 2+)
- **http/** — HTTP client factory stubs (Phase 2+)
- **cache/** — Request caching stubs (Phase 3+)

### 7. Dashboard (Phase 4)
- Ultra-low memory SSR UI built with **Askama** templates + **HTMX** (zero Node.js dependencies)
- Real-time updates via Server-Sent Events (`hx-ext="sse"`)
- Provider status cards with live circuit breaker health indicators
- Quota usage tracking and history
- Recent requests log
- Manual provider enable/disable

## Implementation Phases

| Phase | Status | Scope | Completion |
|-------|--------|-------|------------|
| **Phase 1** | ✅ Complete | HTTP server (Axum), dynamic provider discovery, quota tracking, modular architecture | 100% |
| **Phase 2** | ✅ Complete | Circuit breaker, streaming (SSE), API key auth, real provider clients, retry/fallback | 100% |
| **Phase 3** | 📋 Planned | Request caching with TTL, SQLite quota persistence, token counting, embeddings endpoint | 0% |
| **Phase 4** | 📋 Planned | Dashboard UI (Askama + HTMX with SSE), admin API enhancements, monitoring | 0% |
| **Phase 5** | 📋 Planned | Docker image, production hardening, comprehensive docs, performance tuning | 0% |

## Key Constraints

1. Free models only — reject any paid tiers
2. Low memory footprint — <100MB idle
3. Stateless (mostly) — horizontal scaling ready
4. OpenAI-compatible — drop-in client replacement
5. SQLite only — no external services

## Success Metrics

- ✅ <100MB idle RAM
- ✅ Sub-100ms routing latency
- ✅ 99.9% uptime with auto-fallback
- ✅ Automatic quota tracking
- ✅ Works with Claude Code, Cursor, OpenCode, etc.

## Current Status: Phase 2 ✅ Complete

### What's Built (Phases 1 & 2)
- ✅ Axum HTTP server running on `127.0.0.1:20128`
- ✅ 7 working endpoints (health, chat, models, admin providers, admin quota)
- ✅ **Dynamic Provider Discovery**: 6 providers dynamically discovered via API keys with strict free-model filtering
- ✅ **Circuit Breaker State Machine**: `Closed` / `HalfOpen` / `Open` tracking with failure thresholds and auto-recovery
- ✅ **Resilient HTTP Client**: Connection reuse, timeouts, and exponential retry backoff with jitter
- ✅ **Multi-Tier Fallback Routing**: Automatic failover across providers if primary or quota is exhausted
- ✅ **OpenAI SSE Streaming**: Real-time chunked delta streaming on `/v1/chat/completions` (`stream: true`)
- ✅ **Bearer Authentication**: API key extraction and validation via `RUSTAGENT_API_KEY`
- ✅ **In-Memory Quota Manager**: Lock-free quota tracking with daily resets
- ✅ **10-Module Clean Architecture**: Modular, extensible codebase with 0 errors/warnings

### Modular Architecture
```
src/
├── api/              # HTTP endpoints (v1 + admin, SSE streaming)
├── providers/        # Provider registry, discovery, circuit breaker, HTTP client
├── routing/          # Smart selection strategies & multi-tier fallback chains
├── quota/            # Quota tracking and enforcement
├── errors/           # Error types and HTTP responses
├── config/           # Configuration loading from environment
├── state/            # Shared application state
├── middleware/       # HTTP middleware (auth, logging)
├── http/             # HTTP client factory & retry policy
├── cache/            # Request caching layer (Phase 3)
├── lib.rs            # Library root with re-exports
├── main.rs           # Minimal entry point
└── server.rs         # Axum app setup
```

### Documentation
- **ARCHITECTURE.md** — Detailed module responsibilities, data flow, testing strategy
- **DYNAMIC_DISCOVERY.md** — Free model discovery rules and filtering mechanics
- **REFACTORING_SUMMARY.md** — Architecture refactoring history
- **11 Skills** in `.agents/skills/` — Production-ready implementation patterns

### Providers Discovery (Dynamic)
Providers dynamically discover and import **strictly free models** based on user-provided API keys:

1. **Groq** — Free tier models (requires `GROQ_API_KEY`)
2. **OpenRouter** — Auto-detects `:free` models via pricing API (requires `OPENROUTER_API_KEY`)
3. **Together AI** — Free tier models by pattern (requires `TOGETHER_API_KEY`)
4. **HuggingFace** — Free tier models (requires `HUGGINGFACE_API_KEY`)
5. **DeepInfra** — Free tier models by pattern (requires `DEEPINFRA_API_KEY`)
6. **Pollinations** — Free models, zero key required (universal fallback)

### Ready for Phase 3
The foundation is set for:
- Request caching with TTL and LRU eviction (prompt hashing)
- SQLite database persistence for quotas and stats
- Pre-request token counting & estimation
- `/v1/embeddings` endpoint support

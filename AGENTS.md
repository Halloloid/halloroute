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
- **HTTP Server**: Actix-web or Axum
- **Database**: SQLite + rusqlite
- **Async Runtime**: Tokio
- **Config**: Serde + TOML/JSON
- **Frontend**: React/SvelteKit (lightweight, optional)

## Core Components

### 1. Provider Registry
- Hardcoded/config-based free-tier only providers
- Per-provider: API endpoints, quota limits, rate limits, auth
- Automatic health checks (circuit breaker)
- Live quota tracking

### 2. Router/Dispatcher
- Strategies: `auto`, `priority`, `round-robin`, `quota-first`, `latency-first`
- Resilience: circuit breaker, retry with backoff, fallback chains
- Request deduplication

### 3. Quota Manager
- Track usage per provider in SQLite
- Automatic reset timers (daily, weekly, monthly)
- Graceful degradation when quotas hit

### 4. HTTP Server
- OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/embeddings`, `/v1/models`)
- Admin endpoints for provider management
- Streaming responses (SSE)

### 5. Dashboard
- Provider status cards
- Quota usage tracking
- Recent requests log
- Manual provider enable/disable

## Implementation Phases

| Phase | Scope | Target |
|-------|-------|--------|
| **Phase 1** | HTTP server, single provider, quota tracking | Week 1-2 |
| **Phase 2** | Provider registry, router, fallback logic | Week 3-4 |
| **Phase 3** | Caching, compression, token counting | Week 5-6 |
| **Phase 4** | Dashboard, admin API | Week 7-8 |
| **Phase 5** | Docker, polish, docs | Week 9+ |

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

## Current Status

- Project initialized
- Architecture designed
- Ready for Phase 1 implementation

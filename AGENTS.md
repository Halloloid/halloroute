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
    HalloRoute Gateway (HTTP Server)
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

### Dashboard Completed Pages & Architecture

HalloRoute features an ultra-low memory SSR dashboard built with **Askama templates**, **HTMX**, and **Server-Sent Events (SSE)**, requiring zero Node.js runtime and consuming <30MB RAM.

1. **Overview Dashboard (`/dashboard` or `/dashboard/tab/overview`)**
   - **Role**: Real-time operational control room.
   - **Features**: Live metrics strip (requests, cache hit rate, active providers, live RSS memory usage via OS APIs updating every 3s via SSE), recent event stream log, active provider health cards, and interactive LLM playground.

2. **Provider Fleet (`/dashboard/tab/providers`)**
   - **Role**: Upstream provider management.
   - **Features**: Displays configured free-tier provider endpoints, rate-limit meters, health status, and controls to toggle or reset circuit breakers.

3. **Logs & Playground (`/dashboard/tab/logs`)**
   - **Role**: Telemetry inspection and model testing.
   - **Features**: Live request logs table with status filtering, and an interactive prompt playground to test chat completion routing directly through the gateway.

4. **Quotas & Analytics (`/dashboard/tab/quotas`)**
   - **Role**: Quota burn rate and token analytics tracking.
   - **Features**: Tracks daily/monthly token consumption per provider, utilization percentage progress bars, midnight UTC reset timers, and historical request logs with free-tier cost breakdown ($0.00).

5. **Client Setup Hub (`/dashboard/tab/client-setup`)**
   - **Role**: Client tool onboarding.
   - **Features**: Pre-built copy-paste configuration snippets for **Claude Code, Cursor, Aider, OpenCode, Python SDK, and Node.js SDK** with a dynamic Gateway Key selector that automatically injects selected production/development keys (`hr-live_...`) into snippets and step-by-step export instructions.

## Implementation Phases

| Phase | Status | Scope | Completion |
|-------|--------|-------|------------|
| **Phase 1** | ✅ Complete | HTTP server (Axum), dynamic provider discovery, quota tracking, modular architecture | 100% |
| **Phase 2** | ✅ Complete | Circuit breaker, streaming (SSE), API key auth, real provider clients, retry/fallback | 100% |
| **Phase 3** | ✅ Complete | Request caching with TTL, SQLite quota persistence, token counting, embeddings endpoint | 100% |
| **Phase 4** | ✅ Complete | Dashboard UI (Askama + HTMX with SSE), admin API enhancements, monitoring & playground | 100% |
| **Phase 5** | ✅ Complete | **Gateway API Key Generation, Dynamic UI Provider Manager, Client Setup Hub, Collective Intelligence**<br>• 5.1: Gateway API Key Management Backend & Database ✅<br>• 5.2: Gateway API Key Admin Endpoints & UI Management ✅<br>• 5.3: Dynamic Provider Manager & Live Connection Ping ✅<br>• 5.4: Client Setup Hub ✅<br>• 5.5: Collective Intelligence ✅ | 100% |
| **Phase 6** | 📋 Planned | **Docker image, production hardening, comprehensive docs, performance tuning**<br>• 6.1: Dockerization & Container Optimization<br>• 6.2: Production Hardening & Rate Limiting / Security Headers<br>• 6.3: Comprehensive Documentation & Final Polish | 0% |

### Phase 5 Subphase Details

#### 5.1: Gateway API Key Management Backend & Database ✅ COMPLETE
- **Scope**:
  - SQLite schema for gateway keys (`id`, `key_hash`, `prefix`, `name`, `created_at`, `revoked_at`).
  - Secure key generation utility (`hr-live_...` prefix + random bytes, SHA-256 hash storage).
  - Repository methods: create, list, validate (by hash lookup), revoke.
  - Update `src/middleware/auth.rs` to check bearer tokens against both env var and DB-backed keys.
- **Files**: New `src/gateway_keys/` module, update `src/quota/tracker.rs` (DB pool), `src/middleware/auth.rs`, `src/state.rs`
- **Verification**: Unit tests for key gen/validation/revocation; integration test for auth middleware with dynamic keys.
- **Status**: Implemented with 4 comprehensive tests, 0 errors/warnings. Key generation uses secure random + SHA-256, validation includes revocation checks.

#### 5.2: Gateway API Key Admin Endpoints & UI Management
- **Scope**:
  - REST endpoints: `POST /admin/gateway-keys`, `GET /admin/gateway-keys`, `DELETE /admin/gateway-keys/:id`.
  - Dashboard tab/template for listing active keys (masked), creating new keys (one-time display), and revoking.
  - HTMX-powered forms for inline key management without page reload.
- **Files**: New `src/api/admin/gateway_keys.rs`, Askama templates in `src/dashboard/templates/`, update `src/server.rs` routes.
- **Verification**: Admin API route tests; dashboard rendering tests.
- **Status**: Implemented with dashboard tabs (Overview, Providers, Logs & Playground, Quotas & Analytics), Askama SSR templates, HTMX integration, and all 72 test cases passing.

#### 5.3: Dynamic Provider Manager & Live Connection Ping ✅ COMPLETE
- **Scope**:
  - SQLite persistence for user-added provider configs (endpoint URL, API key, model list).
  - Live ping endpoint: `POST /admin/providers/ping` — sends test request to provider `/v1/models` with supplied key, returns discovered models filtered by free-tier criteria.
  - Admin CRUD: `POST /admin/providers/dynamic`, `PUT /admin/providers/dynamic/:id`, `DELETE /admin/providers/dynamic/:id`.
  - Runtime registration: dynamically inject validated providers into `ProviderRegistry` and circuit breaker pool.
- **Files**: `src/providers/dynamic.rs`, `src/api/admin/dynamic_providers.rs`, modal `templates/tabs/provider_form.html` (opened from the existing Providers tab via `＋ CONNECT NEW PROVIDER`), `static/provider-modal.css`, `tests/dynamic_providers_test.rs`.
- **Verification**: `cargo test` passes (CRUD/filter/key round-trip tests + request validation unit tests). Modal JS not browser-tested.
- **Status**: ✅ Implemented. See `PHASE_5_3_COMPLETE.md` for details and known limitations (plaintext keys, unauthenticated `/admin/*`).

#### 5.4: Client Setup Hub ✅ COMPLETE
- **Status**: Implemented; see `PHASE_5_4_COMPLETE.md`. Claude Code is shown as unsupported (gateway has no `/v1/messages`). Also added `/admin/gateway-keys` routes and routed `stream: true` to the SSE handler. JS not browser-tested.
- **Scope**:
  - Dedicated "Client Setup" dashboard tab/page.
  - Pre-built config snippets for: Claude Code, Cursor, Codex, OpenCode, Aider, Python SDK, Node.js SDK.
  - Each snippet includes base URL (`http://127.0.0.1:20128/v1`), placeholder for gateway key, and copy-to-clipboard button.
  - Step-by-step instructions per client tool.
- **Files**: New Askama template(s) in `src/dashboard/templates/setup/`, static asset references if needed.
- **Verification**: Route existence test; content assertion tests for expected config strings.

#### 5.5: Collective Intelligence ✅ COMPLETE
- **Status**: Implemented with bounded specialist execution, aggregation, independent critique, capability filtering, tool-call validation/continuation, quota-aware pool reuse, correlation logging, and OpenAI-compatible JSON/SSE responses.
- **Specification**: See `PHASE_5_5_COLLECTIVE_INTELLIGENCE.md`.
- **Scope**: Add an opt-in orchestration layer above existing pools with bounded specialist roles, parallel analysis, aggregation, critique, synthesis, capability filtering, controlled tool execution, quota budgets, cancellation, observability, and OpenAI-compatible streaming.
- **Constraint**: Existing pool behavior and normal client requests remain unchanged. Workspace mutations are delegated to the connected coding client as the single writer.

---

### Phase 6 Subphase Details

#### 6.1: Dockerization & Container Optimization
- **Scope**:
  - Multi-stage `Dockerfile`: builder stage (Rust compile with `--release`), runtime stage (Alpine or distroless, static binary only).
  - Target image size <50MB; ensure <100MB RAM at idle inside container.
  - `docker-compose.yml` with volume mount for SQLite data directory.
  - `.dockerignore` excluding `target/`, `.git/`, `tests/`, etc.
- **Files**: Root-level `Dockerfile`, `docker-compose.yml`, `.dockerignore`.
- **Verification**: Build succeeds; container starts and responds to `/health`; image size check.

#### 6.2: Production Hardening & Rate Limiting / Security Headers
- **Scope**:
  - Request body size limit middleware (e.g., 10MB max).
  - Per-IP rate limiting on public endpoints (`/v1/*`) using token bucket or sliding window.
  - Security headers: `X-Content-Type-Options`, `X-Frame-Options`, `Strict-Transport-Security`, `Content-Security-Policy`.
  - Structured JSON logging enhancement for production log aggregation.
  - Optional Prometheus metrics endpoint (`/metrics`) exposing request counts, latencies, cache hits, quota usage.
- **Files**: Extend `src/middleware/`, possibly new `src/metrics/` module, update `src/server.rs`.
- **Verification**: Middleware unit tests; header presence assertions; load test for rate limiter behavior.

#### 6.3: Comprehensive Documentation & Final Polish
- **Scope**:
  - Rewrite/update `README.md`: quickstart, architecture overview, env vars reference, client setup guide, troubleshooting.
  - Inline doc comments (`///`) across all public modules and functions.
  - CHANGELOG.md summarizing phases 1–6 features.
  - Run full test suite; fix any flaky or failing tests introduced during development.
  - Clippy lint pass (`cargo clippy -- -D warnings`).
- **Files**: `README.md`, `CHANGELOG.md`, source files for doc comments.
- **Verification**: All tests green; zero clippy warnings; README renders correctly on GitHub/GitLab.

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

## Zed Agent Streaming Issue — Resolved

Zed agent requests stopped after messages such as “I’ll list the directory” because HalloRoute always sent the final SSE chunk with `finish_reason: "stop"`. Zed therefore treated tool-call turns as completed instead of executing the requested tool. HalloRoute now preserves the provider’s finish reason, detects streamed `tool_calls`, emits `finish_reason: "tool_calls"`, and logs the result for diagnosis. Provider requests also correctly use the actual `Bearer` API token.

## Current Status: Phase 5 Foundation ✅ Complete (Architectural Fixes + Gateway Key Backend)

### Critical Improvements Completed (Pre-Phase 5.2)

**Fix #1: Mutable Providers Architecture** ✅
- Changed `AppState.providers` from immutable `HashMap<String, Provider>` to thread-safe `Arc<DashMap<String, Provider>>`
- Enables runtime add/remove/update of providers (blocking requirement for Phase 5.3: Dynamic Provider Manager)
- Updated all call sites: `ProviderRegistry`, `Router`, `FallbackSelector`, and dashboard handlers
- All tests refactored to use `DashMap` for consistency
- **Impact**: Phase 5.3 (Dynamic Provider CRUD) can now proceed without architectural blocking

**Fix #2: Authentication Enforcement** ✅ **SECURITY CRITICAL**
- Added `BearerAuth` extractor to all v1 API endpoints: `/v1/chat/completions`, `/v1/embeddings`, `/v1/chat/completions` (streaming)
- All endpoints now validate API keys via `validate_api_key_async()` which checks:
  - Static `RUSTAGENT_API_KEY` env var (fast path)
  - Dynamic gateway keys from database (DB-backed)
  - Revocation status for gateway keys
- Admin/dashboard playground calls use internal routing (bypass auth as intended)
- **Impact**: Gateway is now secure by default; unauthenticated access is blocked

### What's Built (Phases 1, 2, 3, 4 & 5.1 + Foundation Fixes)
- ✅ **Axum HTTP Server**: Running on `127.0.0.1:20128` with graceful shutdown (`Ctrl+C` & `SIGTERM`)
- ✅ **Request Caching & Deduplication Layer** (`src/cache/`): In-memory TTL content-addressed cache, prompt hashing, LRU eviction on capacity, cache hit rate metrics, `/admin/cache` & `/admin/cache/clear` endpoints
- ✅ **SQLite Quota & Stats Persistence** (`src/quota/`): Bundled SQLite storage (`rusqlite`) with WAL mode + in-memory fast path, request logs, and automated background daily midnight reset timers
- ✅ **Accurate Token Estimation & Context Window Management** (`src/token/`): `TokenCounter` estimating messages/prompts, `ContextWindowManager` enforcing model context window boundaries, and global atomic token metrics
- ✅ **Embeddings Endpoint** (`src/api/v1/embeddings.rs`): Full OpenAI-compatible `POST /v1/embeddings` endpoint routed to free models with quota tracking
- ✅ **Dynamic Provider Discovery**: 6 providers dynamically discovered via API keys with strict free-model filtering
- ✅ **Circuit Breaker State Machine**: `Closed` / `HalfOpen` / `Open` tracking with failure thresholds and auto-recovery
- ✅ **Multi-Tier Fallback Routing**: Automatic failover across providers if primary or quota is exhausted
- ✅ **OpenAI SSE Streaming**: Real-time chunked delta streaming on `/v1/chat/completions` (`stream: true`)
- ✅ **Bearer Authentication**: API key extraction and validation via `RUSTAGENT_API_KEY` + dynamic gateway keys, now **enforced on all v1 endpoints**
- ✅ **Gateway API Key System** (`src/gateway_keys/`): Secure key generation with `hr-live_` prefix, SHA-256 hash storage, create/list/validate/revoke operations, SQLite persistence
- ✅ **Mutable Runtime Provider Registry**: `Arc<DashMap>` for dynamic provider add/remove/update at runtime
- ✅ **12-Module Clean Architecture**: Modular, extensible codebase with 0 errors/warnings

### Modular Architecture
```
src/
├── api/              # HTTP endpoints (v1 + admin, SSE streaming, embeddings)
├── cache/            # Request deduplication, TTL response caching & LRU eviction
├── config/           # Configuration loading from environment
├── errors/           # Error types and HTTP responses
├── gateway_keys/     # Gateway API key management (SHA-256 hash, revocation) ⭐ NEW
├── http/             # HTTP client factory & retry policy with jitter
├── middleware/       # HTTP middleware (auth with dynamic keys, logging)
├── providers/        # Provider registry, discovery, circuit breaker, HTTP client
├── quota/            # SQLite persistent quota tracking with WAL mode & reset jobs
├── routing/          # Smart selection strategies & multi-tier fallback chains
├── state/            # Shared application state
├── token/            # Fast token counting, context window management & stats
├── lib.rs            # Library root with re-exports
├── main.rs           # Minimal entry point
└── server.rs         # Axum app setup with graceful shutdown
```

### Ready for Phase 5: Gateway Keys, Dynamic Providers & Client Integration
Phase 5 implements the complete gateway management layer:
- **Gateway API Key Generation & Management**:
  - In-memory fast path + SQLite-persisted gateway keys (`hr-live_...`) with hashing, prefixes, and revocation.
  - Endpoints and UI to create, list, and revoke gateway keys.
  - Authentication middleware updated to validate bearer tokens against generated keys.
- **Dynamic Provider Management in UI**:
  - UI modal to add custom OpenAI-compatible endpoints or connect provider API keys (Groq, OpenRouter, Together, DeepInfra, HuggingFace, Pollinations).
  - **Live Connection Ping & Key Validation**: Actively ping provider endpoints with the provided key to verify credentials and fetch models *before* saving.
  - Strict free-tier model filter applied to discovered models before registering into the runtime pool.
  - SQLite persistence for user-added provider keys across restarts.
  - Provider deletion & reconfiguration via Admin API and Dashboard.
- **Client Setup & Integration Hub**:
  - Dedicated "API Keys & Setup" tab in dashboard.
  - Ready-to-copy setup instructions and config snippets for Claude Code, Cursor, Codex, OpenCode, Aider, and SDKs.

## Phase 5.5: Collective Intelligence — Implementation Guide

The complete implementation specification is in `PHASE_5_5_COLLECTIVE_INTELLIGENCE.md`. Agents implementing this phase **must read that document first** and use it as the source of truth for scope, workflow, constraints, testing, and acceptance criteria.

### GAN Mode (Phase 5.5 extension)

In addition to the original parallel-specialist mode, `collective` now supports a `gan` mode inspired by the ECC GAN harness (`gan-planner`, `gan-generator`, `gan-evaluator` agents from `github.com/affaan-m/ecc`). Instead of Claude sub-agents, each GAN role is a focused system-prompt injection routed through the existing HalloRoute model pool. The models YOU configure in the pool do the work — not a hardcoded provider.

**GAN workflow:**
```
Round 0 — Planner:    user request → PlanArtifact { spec, rubric }
Round 1…N — Loop:     PlanArtifact + [prev impl] + [evaluator feedback]
                            → GeneratorArtifact { implementation }
                            → EvaluatorArtifact { score 0-10, report }
              If score >= pass_threshold OR max_rounds exhausted → stop
Round N+1 — Synthesis: best GeneratorArtifact + final EvaluatorArtifact
                            → one coherent OpenAI-compatible response
```

**Module layout** (`src/collective/`):
- `mod.rs`     — public entry points, pool validation, mode dispatch
- `types.rs`   — `CollectiveConfig`, `CollectiveMode`, `CollectiveRole`, artifacts
- `tools.rs`   — tool-call state machine and validation
- `agents.rs`  — `run_planner`, `run_generator`, `run_evaluator` (GAN agents)
- `parallel.rs`— original fan-out → aggregate → critique → synthesize
- `gan.rs`     — GAN orchestration loop

**Enabling GAN mode:**
```json
{ "model": "first-hallo", "collective": true,
  "collective_config": {
    "mode": "gan",
    "max_rounds": 3,
    "pass_threshold": 7.5,
    "max_tokens_total": 16000,
    "timeout_ms": 180000
  },
  "messages": [{"role": "user", "content": "Add OAuth2 login"}]
}
```

### Core architectural rule

Collective Intelligence is an opt-in orchestration layer **above** the existing model pools. Do not change ordinary pool behavior: a normal pool request still selects one member, uses fallback, records quota, and returns one OpenAI-compatible response. Collective mode may call multiple specialists, but it must reuse the existing provider client, pool selection, authentication, quota, retry, and streaming infrastructure.

### Required implementation order

1. Define and validate the gateway-only collective request configuration and server-side limits.
2. Add workflow types, role definitions, correlation IDs, budgets, and explicit workflow states.
3. Implement bounded orchestration over existing pool execution, with parallel independent analysis and graceful member failure.
4. Add context compaction, artifact aggregation, conflict detection, critique, and final synthesis.
5. Add provider/pool capability filtering for tools, streaming, structured output, and context limits.
6. Preserve tool-call state, tool results, streamed deltas, and provider finish reasons; use a single controlled writer for workspace tools.
7. Add lifecycle logging, cancellation, quota accounting, cache-key separation, and dashboard-safe summaries.
8. Add mock-provider, unit, integration, and comparative quality tests before enabling additional roles or tool execution.

### Non-negotiable safeguards

- Collective mode must be explicitly enabled; never activate it for existing clients implicitly.
- Enforce maximum roles, concurrency, rounds, tokens, wall-clock time, tool iterations, and workspace actions.
- Do not allow concurrent agents to mutate the workspace in the first implementation.
- Do not send collective-only fields to upstream providers.
- Do not select providers that lack required capabilities.
- Do not log prompts, source content, API keys, or authorization headers by default.
- Do not present partial or proposed work as completed work.
- Client disconnects and workflow deadlines must cancel internal provider and tool work.
- The final external response must remain valid OpenAI-compatible JSON/SSE and emit `[DONE]` exactly once.

### Definition of done

Phase 5.5 is complete only when:

- ordinary pool and non-collective requests remain behaviorally compatible;
- at least three specialist roles can execute through existing pools;
- aggregation identifies duplicate and conflicting findings;
- critique and synthesis include evidence and explicit uncertainty;
- Zed-style tool calls preserve `tool_calls` and `finish_reason: "tool_calls"`;
- provider failures and partial workflows degrade predictably;
- lifecycle logs trace one workflow without secrets;
- quota, token, latency, and failure metrics are measurable;
- all new tests and the existing full test suite pass;
- `PHASE_5_5_COLLECTIVE_INTELLIGENCE.md` is updated when design decisions change.

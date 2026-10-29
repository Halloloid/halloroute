# HalloRoute — Free AI Gateway

> **Use every free AI model through one API. Never pay. Never hit a wall.**

HalloRoute is an AI gateway that unifies all the free-tier AI providers — Groq, Together, HuggingFace, Pollinations, OpenCode, and more — behind a single OpenAI-compatible endpoint. It picks the fastest healthy provider, automatically falls back when one goes down, tracks quotas so you never waste a request, and caches duplicate calls so providers aren't hit twice.

Just point Claude Code, Cursor, Codex, Aider, or any OpenAI-compatible client at `http://127.0.0.1:20128/v1` and start building. HalloRoute takes care of everything else — routing, retries, failover, streaming, and quota management — silently, in the background, using less RAM than a single browser tab.

---

## Why HalloRoute?

| | HalloRoute | Typical Node.js gateways |
|---|---|---|
| **Idle RAM** | ~30–50 MB | 500 MB – 1 GB+ |
| **Startup time** | < 1 second | 5–15 seconds |
| **External services** | None (SQLite bundled) | Redis, Postgres, etc. |
| **Cost** | $0 forever | Often paid tiers |
| **Runtime** | Single static binary | Node.js + npm ecosystem |

### What makes it different

- **Truly free** — only routes to free-tier AI providers. No paid plans, no surprise bills.
- **Runs anywhere quietly** — uses so little memory you can leave it running 24/7 on your dev machine without noticing.
- **Self-healing** — if a provider goes down or hits its quota, HalloRoute automatically switches to the next one. No manual intervention.
- **Drop-in compatible** — speaks the OpenAI API, so every tool that works with OpenAI works with HalloRoute out of the box.
- **Zero setup infrastructure** — no Docker, no databases to install, no Redis. Just run the binary.

---

## For Developers

HalloRoute is a full-featured AI gateway — not just a proxy. Here's everything it does under the hood when a request comes in.

### Smart Routing

Every request goes through a routing engine that decides which provider should handle it. You can set the model to `"auto"` and the router picks the best available provider, or you can request a specific model and HalloRoute finds the provider that serves it. Five routing strategies are available — automatic, priority-based, round-robin, quota-first (prefer providers with the most remaining quota), and latency-first (prefer the fastest responder). The router filters out any provider that is currently unhealthy or over its daily quota before making a selection.

### Circuit Breaker

Each provider is wrapped in a circuit breaker that tracks its health in real time. When a provider starts failing (timeouts, 5xx errors, rate limits), the circuit breaker transitions through three states: **Closed** (healthy, all requests pass through), **Open** (too many failures, requests are blocked and routed elsewhere), and **Half-Open** (after a cooldown period, a single test request is allowed through to check if the provider has recovered). This prevents your requests from piling up against a broken provider and gives failing services time to recover.

### Multi-Tier Fallback

If the selected provider fails — network error, rate limit, quota exhausted, server error — HalloRoute doesn't return an error. It automatically retries the request against the next best provider in the pool, and then the next, until one succeeds or all providers have been tried. This happens transparently within the same HTTP request, so your client sees a single successful response even if the first two providers were down.

### Request Caching & Deduplication

Identical requests within a configurable TTL window are served from an in-memory cache instead of hitting the provider again. The cache is content-addressed (requests are hashed by their full content — model, messages, parameters), uses LRU eviction when capacity is reached, and tracks hit rates so you can monitor how effectively it's reducing provider load. If two clients send the exact same prompt at the same time, only one request goes to the provider.

### Quota Tracking

HalloRoute monitors how many tokens each provider has consumed today and compares that against the provider's known free-tier limit. Quota data is persisted to SQLite so it survives restarts, and a background job automatically resets daily counters at midnight. When a provider approaches or exceeds its daily limit, the router stops sending requests to it and favors providers with remaining capacity.

### Real-Time SSE Streaming

When a client sends `"stream": true`, HalloRoute opens a streaming connection to the provider and forwards Server-Sent Events (SSE) chunks back to the client in real time — the same `data: {"choices":[{"delta":{"content":"..."}}]}` format that OpenAI uses. This means you get token-by-token output in Claude Code, Cursor, and any other tool that supports streaming completions.

### Token Counting & Context Window Management

Before forwarding a request, HalloRoute estimates the token count of the messages and checks whether they fit within the target model's context window. If a prompt is too large, it returns a clear error instead of letting the provider reject it with a cryptic message. Token counts are also recorded for quota tracking accuracy.

### Embeddings

A full OpenAI-compatible embeddings endpoint (`POST /v1/embeddings`) is available. Requests are routed to free-tier embedding models with the same quota tracking, fallback, and circuit breaker protections as chat completions.

### Gateway API Keys

HalloRoute generates its own API keys (prefixed `hr-live_`) that you hand out to clients. Keys are SHA-256 hashed before storage — the raw key is shown once at creation time and never stored. Keys can be revoked instantly, and the auth middleware validates every incoming request against both the static env-var key and any database-backed gateway keys.

### Dynamic Provider Discovery

At startup, HalloRoute probes each configured provider's API to discover what models are available, then filters the list to only free-tier models. If a provider's key is invalid or the provider is unreachable, it's marked as down but kept in the registry for automatic recovery later. This means you never accidentally route to a paid model.

### Live Dashboard

Open `http://127.0.0.1:20128/` in a browser and you get a real-time dashboard showing provider health cards (with circuit breaker state), quota usage bars, recent request logs, cache statistics, and a built-in chat playground where you can test completions directly. The dashboard updates live via Server-Sent Events — no polling, no page refreshes. It's server-rendered with zero JavaScript framework overhead.

### OpenAI-Compatible API Surface

The full API surface — `/v1/chat/completions`, `/v1/models`, `/v1/embeddings` — follows the OpenAI specification, so any client library or tool that speaks OpenAI works with HalloRoute without modification. Just change the base URL.

---

### Tech Stack

| Component | Technology | Purpose |
|-----------|-----------|---------|
| Language | Rust 1.70+ | Memory safety, low footprint |
| HTTP Server | Axum 0.7 | Lightweight async web framework |
| Async Runtime | Tokio 1.35 | High-performance async I/O |
| Database | SQLite (bundled via rusqlite) | Zero-dependency persistence |
| Concurrency | DashMap | Lock-free concurrent state |
| HTTP Client | Reqwest 0.12 | Provider API calls with streaming |
| Templates | Askama + HTMX | SSR dashboard, zero JS runtime |
| Auth | SHA-256 hashed gateway keys | Secure API key management |
| Logging | tracing + tracing-subscriber | Structured, filterable logs |

### Architecture

```
Client Apps (Claude Code, Cursor, Codex, Aider, SDKs)
                    ↓
          HalloRoute Gateway (Axum HTTP Server)
                    ↓
          ┌─────────────────────────┐
          │  Smart Router           │
          │  • Model-based routing  │
          │  • Circuit breaker      │
          │  • Quota-aware fallback │
          │  • Request caching      │
          └─────────┬───────────────┘
                    ↓
          Free Provider Pool
          ├─ OpenCode    (DeepSeek, Nemotron — no key needed)
          ├─ Groq        (Llama, Mixtral — free RPM tier)
          ├─ Pollinations (GPT-4, Claude 3 — no key needed)
          ├─ Together AI  (Llama 3, Mistral — free tier)
          ├─ HuggingFace  (Llama, Mistral — free tier)
          └─ + your own providers via dashboard
                    ↓
          SQLite (quota tracking, request logs, gateway keys)
```

### Project Structure

```
src/
├── api/              # HTTP endpoints (v1 + admin, SSE streaming, embeddings)
├── cache/            # Request deduplication, TTL response caching & LRU eviction
├── config/           # Configuration loading from environment
├── dashboard/        # Askama + HTMX SSR dashboard
├── errors/           # Error types and HTTP responses
├── gateway_keys/     # Gateway API key management (SHA-256 hash, revocation)
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

---

## Getting Started

### Prerequisites

- **Rust 1.70+** — install from [rustup.rs](https://rustup.rs/)
- That's it. SQLite is bundled automatically.

### Step 1 — Clone the repo

```bash
git clone https://github.com/your-username/rustAgent.git
cd rustAgent
```

### Step 2 — Configure environment

```bash
cp .env.example .env
```

Edit `.env` with your provider API keys (optional — some providers work without keys):

```bash
# Required for authenticated requests to HalloRoute itself
RUSTAGENT_API_KEY=your-secret-key

# Provider keys (add whichever you have — all optional)
GROQ_API_KEY=gsk_...
TOGETHER_API_KEY=...
HUGGINGFACE_API_KEY=hf_...

# Server settings (defaults shown)
RUSTAGENT_HOST=127.0.0.1
RUSTAGENT_PORT=20128
RUSTAGENT_DB_PATH=halloroute.db
RUST_LOG=halloroute=debug
```

### Step 3 — Build and run

```bash
cargo run --release
```

You should see:

```
Starting RustAgent Gateway
Initializing SQLite quota database
Database & quota background reset jobs initialized
Loaded 5 providers
HTTP server listening on 127.0.0.1:20128
```

### Step 4 — Verify it works

```bash
# Health check
curl http://127.0.0.1:20128/health

# List available models
curl http://127.0.0.1:20128/v1/models \
  -H "Authorization: Bearer your-secret-key"

# Send a chat request
curl -X POST http://127.0.0.1:20128/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Step 5 — Point your tools at HalloRoute

Use `http://127.0.0.1:20128/v1` as the base URL in any OpenAI-compatible client.

**Claude Code:**
```bash
claude config set apiUrl http://127.0.0.1:20128/v1
```

**Cursor:** Set the API base URL in Settings → Models → OpenAI Base URL.

**Python (OpenAI SDK):**
```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:20128/v1",
    api_key="your-secret-key",
)
```

---

## API Reference

### Chat Completions

```
POST /v1/chat/completions
```

OpenAI-compatible. Supports `stream: true` for SSE streaming.

### Models

```
GET /v1/models
```

Returns all models available across connected providers.

### Embeddings

```
POST /v1/embeddings
```

OpenAI-compatible embeddings endpoint.

### Health

```
GET /health
```

Returns gateway health status.

### Admin

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/admin/providers` | GET | List all providers with health status |
| `/admin/quota` | GET | Current quota usage per provider |
| `/admin/cache` | GET | Cache hit rate and stats |
| `/admin/cache/clear` | POST | Clear the response cache |
| `/admin/providers/:id/enable` | POST | Re-enable a provider |
| `/admin/providers/:id/disable` | POST | Temporarily disable a provider |

### Dashboard

Open `http://127.0.0.1:20128/` in your browser for a live dashboard with provider status, quota tracking, request logs, and a chat playground.

---

## License

MIT — See LICENSE file.

---

## Contributing

1. Fork the repo
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Make changes and add tests: `cargo test`
4. Open a PR

---

<!-- Phase 5/6 content: client setup hub, Docker instructions, and production deployment guide will be added here. -->

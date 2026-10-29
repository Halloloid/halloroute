# HalloRoute — Free AI Gateway

**One gateway, many free AI providers.** HalloRoute sits between your apps and free AI APIs, automatically picking the best available provider so you never hit a quota wall or pay a cent.

Point Claude Code, Cursor, Codex, or any OpenAI-compatible client at `http://127.0.0.1:20128/v1` and start coding — HalloRoute handles the rest.

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

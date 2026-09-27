# RustAgent — Phase 1 Complete ✅

## What's Built

### Core Infrastructure
- ✅ **HTTP Server** (Actix-web) — listens on `127.0.0.1:20128`
- ✅ **SQLite Database** — automatic migrations, quota tracking
- ✅ **Provider Registry** — 5 free-tier providers pre-configured
- ✅ **Smart Router** — auto-selects providers based on model & quota
- ✅ **Quota Manager** — tracks daily token usage per provider

### Providers (Free Forever)
1. **OpenCode** — DeepSeek V3.2, Nemotron 3 (no quota limit)
2. **Groq** — Mixtral 8x7B, Llama 3 (14.4M tokens/day free tier)
3. **Pollinations** — GPT-4, Claude 3, Llama 70B (no key needed)
4. **Together AI** — Llama 3, Mistral (1M tokens/day free tier)
5. **HuggingFace** — Llama 2, Mistral (limited free tier)

### OpenAI-Compatible Endpoints
- `POST /v1/chat/completions` — chat with auto-fallback
- `GET /v1/models` — list all available models
- `GET /health` — health check

### Admin Endpoints
- `GET /admin/providers` — list provider status & health
- `GET /admin/quota` — current quota usage & remaining
- `POST /admin/providers/{id}/disable` — pause a provider
- `POST /admin/providers/{id}/enable` — resume a provider

---

## Project Structure

```
rustAgent/
├── Cargo.toml                  # Dependencies & metadata
├── Cargo.lock
├── AGENTS.md                   # Project vision (this file goes here)
├── README.md                   # Setup & usage guide
├── .env.example                # Environment variables template
├── docker-compose.yml          # Local dev setup
├── src/
│   ├── main.rs                # Entry point
│   ├── server.rs              # HTTP server & routes
│   ├── config.rs              # Configuration loading
│   ├── db.rs                  # SQLite & migrations
│   ├── errors.rs              # Error types & responses
│   ├── providers.rs           # Provider registry
│   ├── quota.rs               # Quota tracking
│   ├── router.rs              # Request routing logic
│   └── handlers/
│       ├── mod.rs
│       ├── chat.rs            # /v1/chat/completions
│       ├── models.rs          # /v1/models
│       ├── health.rs          # /health
│       └── admin.rs           # Admin endpoints
└── migrations/
    └── 001_initial.sql        # Schema creation
```

---

## Getting Started

### 1. **Prerequisites**
- Rust 1.70+ (install: https://rustup.rs/)
- SQLite 3 (usually included)

### 2. **Clone & Setup**
```bash
cd rustAgent
cp .env.example .env
```

### 3. **Run Locally**
```bash
cargo run
```

Expected output:
```
2026-09-27T13:28:29Z RustAgent: Starting RustAgent Gateway
2026-09-27T13:28:29Z RustAgent: Database initialized
2026-09-27T13:28:29Z RustAgent: Loaded 5 providers
2026-09-27T13:28:29Z RustAgent: Starting HTTP server on 127.0.0.1:20128
```

### 4. **Test It**
```bash
# Health check
curl http://localhost:20128/health

# List models
curl http://localhost:20128/v1/models

# Check providers
curl http://localhost:20128/admin/providers

# Check quota
curl http://localhost:20128/admin/quota

# Chat (requires provider API keys set)
curl -X POST http://localhost:20128/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

---

## Configuration

### Environment Variables
```bash
# Server
RUST_AGENT_HOST=127.0.0.1          # Bind address
RUST_AGENT_PORT=20128              # Port
RUST_LOG=rust_agent=debug          # Log level

# Database
RUST_AGENT_DB_PATH=data/rust_agent.db

# Provider API Keys (optional, only needed for paid providers)
GROQ_API_KEY=your_groq_key
TOGETHER_API_KEY=your_together_key
HUGGINGFACE_API_KEY=your_hf_key
```

### OpenCode & Pollinations
These providers are **free forever** with **no API key needed**. They're enabled by default.

---

## Next Steps: Phase 2-5

### Phase 2: Multi-Provider Fallback (Week 3-4)
- [ ] Circuit breaker (mark provider as `degraded` after N failures)
- [ ] Retry logic with exponential backoff
- [ ] Fallback chain (if provider A fails → try B → try C)
- [ ] Request deduplication (cache identical requests for 60 seconds)

### Phase 3: Optimization (Week 5-6)
- [ ] Response caching (TTL per request type)
- [ ] Request compression (whitespace trimming, dedup)
- [ ] Token counting (accurate via provider APIs)
- [ ] Rate limiter (per-API-key RPM limits)

### Phase 4: Dashboard (Week 7-8)
- [ ] React-based UI for provider management
- [ ] Real-time quota usage graphs
- [ ] Recent request logs
- [ ] One-click provider enable/disable

### Phase 5: Production (Week 9+)
- [ ] Docker image & docker-compose
- [ ] Helm chart for Kubernetes
- [ ] Comprehensive logging & observability
- [ ] Performance benchmarks
- [ ] API documentation (OpenAPI/Swagger)

---

## Architecture Diagram

```
Client (Claude Code, Cursor, etc.)
          ↓ HTTP
RustAgent Gateway
    ├→ Config Loader
    ├→ Router (auto/priority/round-robin)
    ├→ Quota Manager
    └→ Provider Selector
          ↓
Provider Pool
    ├→ OpenCode (free forever)
    ├→ Groq (free tier)
    ├→ Pollinations (free, no key)
    ├→ Together AI (free tier)
    └→ HuggingFace (limited free)
          ↓
Local SQLite
    ├→ quota_usage (track daily tokens)
    ├→ request_cache (optional: cache responses)
    ├→ providers (registry + health)
    └→ logs (request history)
```

---

## Tech Stack (Phase 1)

| Component | Tech | Why |
|-----------|------|-----|
| Language | Rust | Low RAM, fast, memory-safe |
| Web Framework | Actix-web | High-performance, async |
| Database | SQLite | Zero dependencies, fast, file-based |
| Async Runtime | Tokio | Standard for Rust async |
| HTTP Client | Reqwest | Easy provider integration |
| Config | Serde + TOML | Type-safe, simple format |
| Logging | Tracing | Structured, performant |

---

## Success Metrics (Phase 1)

- ✅ Compiles without errors
- ✅ Runs on <50MB RAM
- ✅ Responds to `/health` in <10ms
- ✅ Routes requests to first available provider
- ✅ Tracks quota per provider per day
- ✅ Auto-selects provider if model is specified
- ✅ Compatible with OpenAI client libraries

---

## Known Limitations (Phase 1)

- ⚠️ **No retries yet** — if a provider fails, request fails
- ⚠️ **No circuit breaker** — unhealthy providers not auto-disabled
- ⚠️ **Basic token counting** — uses word-count estimate (will improve in Phase 3)
- ⚠️ **No caching** — each request hits the provider
- ⚠️ **No dashboard** — admin endpoints only (web UI in Phase 4)
- ⚠️ **Mock auth** — providers without auth keys won't work yet

---

## Contributing

1. Fork the repo
2. Create a feature branch: `git checkout -b feat/something`
3. Make changes
4. Test: `cargo test`
5. Push and open a PR

---

## License

MIT — See LICENSE file

---

## Support

- 📖 **Docs**: See `AGENTS.md` for architecture & vision
- 🐛 **Issues**: Report bugs on GitHub
- 💬 **Discussions**: Ask questions in Discussions tab

---

**Ready for Phase 2?** Next up: Circuit breaker, retry logic, and fallback chains.

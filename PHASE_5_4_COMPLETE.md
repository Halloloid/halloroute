# Phase 5.4 — Client Setup Hub

Dashboard tab at `/dashboard/tab/client-setup` (sidebar: **Client Setup Hub**).

## What it does

- **Model selector** populated from models of currently healthy providers (no hardcoded model names).
- **Generate key** form: `POST /admin/gateway-keys`. The full key is displayed once and injected into all snippets for the page session. Only prefixes of existing active keys are listed (full keys are not recoverable). Without a freshly generated key, snippets use the placeholder `YOUR_HR_LIVE_KEY`.
- **Per-client cards** with step-by-step instructions, a live-rendered config snippet, and COPY CONFIG (clipboard API with `execCommand` fallback and visible failure message).

| Client | Status on page | Notes |
|---|---|---|
| Aider | compatible | `OPENAI_API_BASE`, `OPENAI_API_KEY`, `--model openai/<model>` |
| OpenCode | compatible | `opencode.json` with `@ai-sdk/openai-compatible` provider |
| Python SDK | compatible | `openai` package |
| Node SDK | compatible | `openai` package, ES modules |
| Codex CLI | caveat | `~/.codex/config.toml`; needs a version supporting `wire_api = "chat"` |
| Cursor | caveat | Cursor calls models from its own servers, so a public HTTPS URL is required |
| Claude Code | **unsupported** | Needs Anthropic `/v1/messages`, which the gateway does not implement; no config offered |

The previous hardcoded CONNECTED/READY badges, progress bars and invented model names were removed.

## Backend changes made for this phase

- `src/api/admin/gateway_keys.rs` (new): `POST /admin/gateway-keys` (201, name trimmed/validated), `GET /admin/gateway-keys`, `DELETE /admin/gateway-keys/:id` (204; 400 if unknown/already revoked). These were described in Phase 5.2 but were never routed.
- `src/api/v1/chat.rs`: `POST /v1/chat/completions` now dispatches to the SSE handler when `stream: true` (previously the streaming handler existed but was not routed, so `stream: true` returned plain JSON, which breaks streaming clients such as Aider/OpenCode).
- `src/api/v1/streaming.rs`: stream now ends with `data: [DONE]`.
- `src/dashboard/mod.rs`: `ClientSetupTabTemplate` gains `models` and `base_url`; only active keys are listed.
- `templates/tabs/client_setup.html`, `static/client-setup.css`: rewritten/extended.

## Tests (`cargo test`: all passing)

- `tests/dashboard_test.rs`: 7 tests — all tabs return 200; 7 clients with step lists; 6 copy buttons; model options from providers; no fake models/badges; snippet templates (Aider prefix, Codex TOML, OpenCode provider, SDKs); Claude Code marked unsupported; only active key prefixes rendered and raw key never rendered.
- `tests/api_routes_test.rs`: gateway key create/list/revoke flow and blank-name rejection.
- `tests/e2e_provider_flow_test.rs`: `stream: true` returns `text/event-stream` ending in `[DONE]`.
- Unit tests for key-name validation.

## Known limitations

- Snippet rendering, copy, and key generation are client-side JS and were **not tested in a browser**.
- Streaming is still simulated: the upstream call is non-streaming and then chunked.
- `/admin/*` (including key creation) is unauthenticated — address in Phase 6.2.
- If `RUSTAGENT_API_KEY` is unset, the gateway accepts any bearer token (pre-existing behavior).
- Base URL is the constant `GATEWAY_BASE_URL` (`127.0.0.1:20128`), matching the hardcoded bind address.
- Model `"auto"` is not advertised: the router forwards the literal model name upstream.

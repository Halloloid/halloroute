# Phase 5.3 — Dynamic Provider Manager & Live Connection Ping

Providers can be connected at runtime from the **Providers** tab
(`＋ CONNECT NEW PROVIDER`) or via the admin API. There is no separate tab.

## UI (Providers tab)

- `＋ CONNECT NEW PROVIDER` opens a modal (`templates/tabs/provider_form.html`,
  styles in `static/provider-modal.css`).
- **TEST CONNECTION** pings `<base>/models` with the entered key and lists the free models found.
- **CONNECT** validates again server-side, saves to SQLite, and registers the provider
  in the live `AppState.providers` map (no restart).
- Dashboard-added provider cards additionally show **Edit** and **Remove**.
  Built-in/env-discovered providers are never editable or removable from here.

## API

| Method | Path | Notes |
|---|---|---|
| POST | `/admin/providers/ping` | Always HTTP 200; `{success, models, error}`. Optional `provider_id` reuses the stored key when editing. |
| POST | `/admin/providers/dynamic` | 201 on success. 400 on bad id/URL/auth/limits, duplicate id, no free models, or unreachable upstream. |
| GET | `/admin/providers/dynamic` | List (API keys are never serialized). |
| GET | `/admin/providers/dynamic/:id` | |
| PUT | `/admin/providers/dynamic/:id` | Blank `api_key` keeps the stored key. |
| DELETE | `/admin/providers/dynamic/:id` | 204. 404 for non-dynamic ids. |

`free_model_filter`: `all`, `patterns:glob1,glob2`, or `explicit:id1,id2`.

## Persistence / runtime

- Table `dynamic_providers` (schema in `src/quota/tracker.rs`, shared by the file DB and the in-memory test pool).
- `main.rs` loads active rows into the provider map at startup.
- `Provider.api_key` carries the stored key; `Provider::resolve_api_key()` (stored key, then
  `<ID>_API_KEY` env) is used by chat and embeddings upstream calls. The key is `skip_serializing`.

## Known limitations

- Keys are stored in plaintext in SQLite.
- `/admin/*` endpoints have no authentication (pre-existing; relevant now that they can store keys and make outbound requests). Worth addressing in Phase 6.2.
- Discovery uses `reqwest` without SSRF restrictions on the base URL.
- The streaming endpoint still wraps a non-streaming upstream call (pre-existing).
- The modal JS has no automated tests; it was not exercised in a browser.

## Tests

`cargo test` — 63 unit + integration tests pass, including
`tests/dynamic_providers_test.rs` (10 tests: CRUD, filters, key round-trip/no-leak) and
validation unit tests in `src/api/admin/dynamic_providers.rs`.

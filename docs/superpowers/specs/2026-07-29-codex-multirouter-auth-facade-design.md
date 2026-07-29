# Codex MultiRouter Authentication Facade Design

Date: 2026-07-29
Status: Approved direction, pending implementation plan

## Problem

CCSwitchMulti currently projects every Codex MultiRouter through one local proxy provider that carries `experimental_bearer_token = "PROXY_MANAGED"`. That is correct when CCSM owns the upstream credential, but it conflicts with routes whose explicit authentication source is `native_codex_auth`: those routes require the caller's real Codex Desktop authorization, while the facade causes Codex to send the placeholder instead.

The design must support four route authentication sources without changing task identity or leaking credentials:

- Codex Desktop current ChatGPT login.
- A fixed CCSM-managed ChatGPT OAuth account.
- The ordered CCSM OAuth account pool.
- A third-party provider's own API key or authorization settings.

## Goals

- Keep one stable Codex-side MultiRouter provider ID: `codex_model_router_v2`.
- Treat the provider `name` as a capability declaration, not the user-visible Router title.
- Preserve OpenAI Codex capabilities for official and CCSM-managed ChatGPT OAuth routes.
- Use HTTP Responses plus SSE for all MultiRouter traffic; do not require an upstream Responses WebSocket proxy.
- Make credential ownership explicit and deterministic for every request.
- Preserve existing route bindings during migration and never rewrite `auth.json`.
- Keep Desktop authorization out of third-party upstream requests.

## Non-goals

- Do not add Responses-over-WebSocket forwarding to CCSM in this change.
- Do not merge standalone `cc-switch-official` task history into MultiRouter history.
- Do not automatically change a fixed managed account into Desktop current login or an account pool.
- Do not implement reset-credit redemption; OpenAI remains responsible for that behavior.

## Terminology

- Provider ID: the key selected by `model_provider`; it is configuration and task identity.
- Provider `name`: Codex capability classification. Current Codex uses exact `name == "OpenAI"` checks for OpenAI-specific behavior.
- Facade: the single provider table written into live `config.toml` so Codex sends requests to the CCSM loopback proxy.
- Route auth source: the request-local owner of the real upstream credential.

## Decision

MultiRouter always projects this stable identity:

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
wire_api = "responses"
supports_websockets = false
```

`name = "OpenAI"` applies to Desktop OAuth and CCSM-managed OAuth alike. It does not mean OpenAI owns the credential. It tells Codex to retain the OpenAI Responses capability path, including remote compaction and OpenAI tool metadata. Credential ownership remains controlled by the facade auth mode and the selected route.

The user-visible Router name stays in the CCSM database and UI. It is not written into the Codex provider `name` field.

## Facade Modes

### Native or Mixed Facade

Use this mode when any enabled route uses `native_codex_auth`, or when an enabled account pool may select the Desktop current-login account.

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = true
wire_api = "responses"
supports_websockets = false
http_headers = { "x-cc-switch-proxy-mode" = "router" }
```

The facade must not contain `experimental_bearer_token`. Codex sends its real current-login Authorization to CCSM. The separate proxy-mode header identifies local Router traffic without occupying the upstream Authorization channel, and CCSM removes that header before forwarding.

Request behavior:

| Route source | Outbound credential behavior |
| --- | --- |
| `native_codex_auth` | Preserve the incoming Codex Authorization and account identity. |
| `managed_codex_oauth` | Discard incoming Authorization and inject the selected CCSM OAuth account. |
| `account_pool`, Desktop candidate | Preserve incoming Codex Authorization. |
| `account_pool`, managed candidate | Discard incoming Authorization and inject the selected CCSM OAuth account. |
| `provider_config` | Discard incoming Authorization and inject only the target provider credential. |

### Fully Managed Facade

Use this mode when no enabled route can use Desktop current-login authentication.

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = false
experimental_bearer_token = "PROXY_MANAGED"
wire_api = "responses"
supports_websockets = false
```

Codex sends only the local placeholder. CCSM must replace it before every managed OAuth or third-party upstream request. A placeholder reaching a managed upstream remains a hard error.

## Facade Classification

Classification is derived from enabled routes and the persisted Router-level official-auth policy:

1. Any `native_codex_auth` route selects Native or Mixed.
2. An `account_pool` route selects Native or Mixed when the policy enables the Desktop account; otherwise it selects Fully Managed.
3. Fixed `managed_codex_oauth` and `provider_config` routes do not require Native or Mixed.
4. Ambiguous legacy configurations are not silently reclassified. The current facade remains active until the user confirms and saves a Router authentication policy.

The classifier must be a shared backend function used by config projection, status reporting, migration preview, and tests. Frontend inference is display-only and must not become the execution truth.

## Standalone Official Takeover

The upstream CC Switch v3.17.0 behavior remains separate and unchanged:

```toml
model_provider = "cc-switch-official"

[model_providers.cc-switch-official]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = true
wire_api = "responses"
supports_websockets = false
```

It always uses the Codex current login and does not participate in CCSM account scheduling.

## Capability and Protocol Boundary

The Codex-side facade advertises native OpenAI Responses capability. The selected effective route remains authoritative for the real upstream protocol:

- Official and managed ChatGPT OAuth routes use native Responses.
- OpenAI-compatible Chat routes use the existing Responses-to-Chat conversion.
- Messages and Anthropic routes use the existing conversion paths.
- `/responses/compact` is resolved by the compact request's own model and converted according to the effective route.

For non-OpenAI routes, CCSM must remove unsupported OpenAI-only metadata, tools, authentication, and fingerprint headers before forwarding. Route catalog capabilities remain the source for image, tool, reasoning, and modality availability.

## WebSocket Boundary

`wire_api = "responses"` describes the application protocol. `supports_websockets = false` selects HTTP POST plus SSE as its transport. MultiRouter keeps WebSocket disabled because each HTTP request provides a complete model field that can be routed independently, and because a persistent socket would require frame-level model, account, retry, and failover state.

No `426` fallback is required for the custom MultiRouter facade. `426` remains relevant only to designs that retain the built-in `openai` provider with `openai_base_url`.

## Security Boundary

- Never write `PROXY_MANAGED` or a CCSM-managed OAuth token into Codex `auth.json`.
- Never forward the local proxy-mode header upstream.
- Never forward Desktop Authorization to `provider_config` or managed-account routes.
- External Agent API requests cannot borrow Desktop current-login credentials.
- Logs and diagnostics record auth strategy and account ID only; they never record bearer values.
- Native authorization is allowed only for local Codex-originated requests and only after the effective route explicitly selects it.

## Migration

- Preserve `codex_model_router_v2` as the task provider ID.
- Preserve each route's exact auth source and managed account ID.
- Routers with an explicit native route regenerate as Native or Mixed and remove the placeholder from the facade.
- Routers with only managed routes retain the placeholder and regenerate as Fully Managed.
- Legacy ambiguous Routers show a migration choice and do not change live behavior until saved.
- Changing facade mode marks Codex restart as required. Existing sessions are not claimed to hot-reload provider authentication.
- Migration never inserts, removes, or replaces Codex login records.

## UI

The Router workspace exposes one official authentication policy:

- Desktop current login.
- Fixed CCSM OAuth account.
- OAuth account pool.

The UI displays the generated facade class as read-only status: `Desktop/Mixed` or `Fully managed`. It does not expose `name`, `requires_openai_auth`, or `experimental_bearer_token` as independent user switches because invalid combinations must not be constructible.

Changing the policy or enabling the Desktop account in a pool shows a restart-required state after save.

## Error Handling

- Native or Mixed without a usable incoming Codex Authorization returns an actionable Desktop-login error.
- Fully Managed with an unresolved placeholder returns an internal auth-resolution error before network forwarding.
- A missing fixed managed account returns an account-specific re-login error without falling back to Desktop.
- An unavailable Desktop pool entry is skipped only when another explicitly enabled managed candidate exists; otherwise the request fails with the Desktop-login reason.
- Authentication failures do not silently fail over from an official account to a third-party route unless the Router explicitly enables that fallback.

## Verification

Implementation acceptance requires:

1. Config projection tests for both facade modes and exact `name = "OpenAI"`.
2. A Codex auth harness proving Native or Mixed sends the real caller bearer and Fully Managed sends the placeholder.
3. End-to-end forwarder tests covering native, fixed managed OAuth, pool-native, pool-managed, and provider-config routes.
4. Credential-leak tests proving Desktop Authorization cannot reach a third-party or External Agent API upstream.
5. HTTP/SSE and `/responses/compact` routing tests across official, Chat, Messages, and Anthropic routes.
6. Migration tests for explicit native, fixed account, pool with and without Desktop, and ambiguous legacy Routers.
7. Frontend tests for policy persistence, facade status, account ordering, reserve thresholds, and restart-required state.
8. A real request using the installed Codex 0.146 family after rebuilding CCSM, with proxy logs proving route, auth strategy, transport, and upstream status without logging credentials.

## Rejected Alternatives

- Two separate Router provider IDs: rejected because it splits task history and duplicates catalogs and UI state.
- Built-in `openai` plus `openai_base_url` for all modes: rejected because the built-in provider cannot be safely overridden with a managed placeholder or forced WebSocket disablement.
- Always use `PROXY_MANAGED`: rejected because it cannot carry Desktop current-login Authorization to a native route.
- Always use Desktop Authorization: rejected because fully managed users must work without a Codex Desktop login and third-party routes must not receive that credential.

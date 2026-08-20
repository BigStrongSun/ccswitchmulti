# Usage log route names and status filters

## Root cause

`proxy_request_logs.provider_id` stores request-local Codex route identities such as `codex-multirouter::route::router-codex-official` (and, in older rows, repeated `::route::` segments). Those identities are intentionally not rows in `providers`, so the usage query's exact `(provider_id, app_type)` join missed and displayed the opaque ID as `provider_name`. The parent Router's `codexRouting.routes` contains the route id and `targetProviderId`; the target provider row contains the canonical display name.

The live read-only database confirmed this shape on 2026-08-21. It contained large groups for `router-codex-official`, the DeepSeek route, and the Qwen route, plus legacy nested route IDs. It also contained status codes `201`, `204`, `400`, `401`, `422`, `424`, `429`, `500`, `502`, `503`, `504`, `520`, and `521`, so the fixed UI list was incomplete.

## Fix

- `provider_name_coalesce()` now resolves a dynamic route suffix against valid Router route arrays (`codexRouting.routes`, `codexModelRoutes`, or `modelRoutes`) and returns the target Provider name, then route label/name, then the parent Router name for removed/unknown routes. The persisted provider ID and forwarding behavior are unchanged.
- `LogFilters` now accepts `statusGroup: "other"`. The request-log query treats it as `status_code NOT IN (200, 400, 401, 429, 500)`; existing exact status-code filters remain unchanged.
- The request-log UI exposes the localized `Other`/`其他` option and sends the explicit status group.

## Verification

- Rust regression test covers direct, nested, and removed route IDs, target-name filtering, `Other` (`201/204/502`), and exact `500` filtering.
- `cargo test --manifest-path src-tauri/Cargo.toml services::usage_stats::tests --lib`: 40 passed.
- `pnpm vitest run tests/components/RequestLogTable.test.tsx`: 3 passed.
- `pnpm typecheck`, Prettier checks, and `cargo fmt --check` passed.
- No prompt contents, token values, credentials, or live configuration secrets were read.

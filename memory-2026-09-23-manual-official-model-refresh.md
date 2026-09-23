# 2026-09-23 Manual official model catalog refresh

- The authority chain is OpenAI Codex `models.json` -> CCSM's independent
  `codex-official-models-cache.json` snapshot -> generated
  `~/.codex/cc-switch-model-catalog.json` and takeover-period
  `models_cache.json`. Generated files are projections, never an authority for
  newly released model capabilities.
- The Tauri command `refresh_codex_official_model_catalog` has no arguments.
  It returns camelCase `source`, `fetchedAt`, `modelCount`,
  `usedStaleCache`, `projectionApplied`, `projectionReason`, and
  `refreshError`. Successful network refreshes report
  `source=openai_codex_models_json`; a failed request reports `stale_cache`
  only when a prior independent snapshot exists, otherwise `unavailable` and
  `usedStaleCache=false`.
- Manual refresh bypasses both the six-hour TTL and automatic failure cooldown.
  Concurrent manual callers wait for the existing single-flight request using
  a pre-enabled Tokio Notify waiter, avoiding a check-to-wait missed wakeup.
  A failed request never rewrites `fetched_at` or projects stale data.
- Projection runs only after a fresh atomic cache save and only when Codex
  takeover is enabled and live `config.toml` points at the CCSM-owned catalog.
  Disabled takeover or a user-owned catalog produces a clear reason and does
  not modify generated outputs.
- `cc-switch-model-catalog.json` and `models_cache.json` are separate files,
  so no filesystem-wide atomic rename is claimed. Their publisher snapshots
  catalog, cache, and backup, uses atomic per-file writes, and compensates by
  restoring all three on cache-sync failure. A command failure reports
  `projection_outputs_unconfirmed`, never that no generated file changed.
- A successful public snapshot followed by projection failure is still fresh
  upstream state; a later explicit refresh intentionally performs another
  forced upstream request rather than treating that snapshot as an applied
  projection.
- UI entry is Status -> Link status -> "Force refresh official model catalog".
  It displays the above result fields and tells the user to refresh the Codex
  model picker. This source change is not installed, deployed, or runtime-UI
  verified.

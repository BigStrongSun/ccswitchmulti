# Codex capacity replay investigation (2026-11-17)

## Goal and Scope

- Restore bounded same-provider replay for an explicit capacity rejection before semantic output, including the normal Responses progress lifecycle.
- Preserve the client-owned native transport retry policy, ten-attempt capacity budget, and prohibition on replay after text, reasoning, or tool output.
- Do not replay the user's historical task, change credentials/configuration, restart live services, or publish a release in this source-fix batch.
- Acceptance: realistic generic-type/explicit-code errors recover; empty progress does not consume the semantic-output gate; malformed/nonempty/unknown events fail closed; existing capacity, transport-owner, and terminal regressions pass.

## Incident Evidence

- Main checkout: `C:\Users\sunda\Documents\LLMservice\cc-switch`, branch `main`, starting HEAD `6640e2778`. Existing unrelated changes: `memory.md`, `codex_history_migration.rs`, `commands/protocol_compatibility.rs`, and `docs/provider-settings-layout-preview.html`. Preserve them; do not create another worktree.
- Running installed executable: `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe`, version `3.20.4-13`. Read-only live DB query found Codex `capacity_retry_enabled=1`, `max_retries=6`. Managed Codex config projects request retries 2 and stream retries 10.
- Screenshot matched thread `01a1547d-50cd-7e21-8cf7-e6c4b1e9512e`, turn `01a1e826-1704-7a30-baf6-eda3ab75e530`, in the retained rollout dated November 16. The failing request was November 17 at 15:00 China time, not the rollout filename's start time.
- Router trace `5aed6dd1-1acc-411b-b828-a2f06d88e02d`: native Responses, `gpt-6.1-terra`, official upstream; sent 15:00:42.763, HTTP 200 at 15:00:46.898, response ready 15:00:47.009.
- At 15:00:56.676 router recorded `upstream_sse_error`, `event=error`, `error_type=service_unavailable_error`, message hash `b4ae2cef08093d6d`, `attempt=0`. Error journal saw 104428 response bytes. Neither byte count nor HTTP 200 proves semantic output or successful completion.
- At 15:00:56.686 rollout `task_complete.error` was `server_overloaded` with the screenshot message. Codex local SQLite log independently records the same turn error at 07:00:56 UTC. No new assistant/tool item was persisted for the final failed sampling request. CCSM usage row records stream failure with zero tokens at 15:00:57, distinct from the preceding successful tool-call sampling.
- Full SSE prefix and original error code/message were not retained in these incident logs. Exact prefix and payload are therefore not reconstructed as observed facts. No new scoped capture was armed and no real inference request was replayed.

## Root Causes in Source

1. `is_capacity_error_payload` read `error.type.or_else(error.code)`. A generic `service_unavailable_error` masks an explicit `server_is_overloaded` or `slow_down` code. Official Codex checks the code, maps these to `ServerOverloaded`, and marks that error non-retryable. Consequently the client's `stream_max_retries=10` does not rescue a capacity error missed by CCSM.
2. Native SSE replay treated every non-comment event except `response.created` as semantic. A normal empty `response.in_progress` or `response.queued` therefore permanently disabled capacity replay before any text/reasoning/tool output. Existing positive tests skipped the progress lifecycle and used the overload marker as the type, missing both gaps.

These are independently reproducible implementation defects consistent with the incident. The missing raw prefix prevents claiming that both were definitely exercised by that one historical response.

## Fix and Verification

- Inspect both error type and code independently; explicit quota/auth/policy/parameter rejection wins over capacity markers and wording. A generic service-unavailable type alone is still insufficient for SSE replay.
- Only matching `response.in_progress`/`response.queued` payloads with nonempty response ID, matching status, explicit empty output array and no error are nonsemantic. Preserve their bytes; do not clear an already-set semantic-output gate.
- Native Failed terminals now log a fixed `capacity_retry_decision` reason with booleans and attempt count. Reasons distinguish non-capacity, disabled, semantic output, absent reconnect factory, retry started and budget exhausted; no raw error text/header is added.

- RED stage 1: existing compiled library's `native_capacity_` group: 9 passed / 2 failed. Empty in-progress (split across chunks) and empty queued cases expected one reconnect but observed zero. Existing nonempty/unknown/post-output negative cases passed.
- RED stage 2: `cargo test --manifest-path src-tauri/Cargo.toml --lib capacity -- --nocapture`: 24 passed / 4 failed. The additional failures were the independent type/code classifier test and native `event:error` recovery with generic type plus explicit overload code.
- GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib streaming_retry -- --nocapture`: 61 passed / 0 failed. Includes split-chunk progress, repeated lifecycle events, explicit code, malformed/nonempty/unknown progress, post-output prohibition, transport owner, quota/auth/policy negatives and ten-reconnect exhaustion.
- Reused that freshly built library executable (`src-tauri/target/debug/deps/cc_switch_lib-a31fc3a47a7b98a6.exe`) for `proxy::forwarder::tests --quiet`: 183/183; `capacity --quiet`: 29/29. Capacity overlaps the other suites; these are not 273 unique tests. Final rustfmt changed only test whitespace after compilation.
- `rustfmt --edition 2021 --check` and `git diff --check` passed. Changed text strictly decodes as UTF-8, without BOM or U+FFFD. Three library-test warnings came from unchanged/out-of-scope files (`protocol_compatibility.rs`, `runner_tests.rs`, `session_usage_codex.rs`), not this fix.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` passed (exit 0), checking final formatted production source. No frontend/UI behavior changed; no full release gate, installer, production inference or Desktop E2E was run.
- Independent Luna read-only review found no blocker. Unrecognized progress shapes remain conservative, and raw incident prefix reconstruction remains unavailable.
- Test temp/output ownership: `.local-archives/20261117-capacity/`; reuse existing `src-tauri/target` rather than creating a worktree or cross-project Cargo cache.
- Source fix and local commit are not installation acceptance. Installed version remains unchanged until an explicit build/install verification batch completes.
- The repository's post-commit hook starts an asynchronous release build on every commit. This source-only batch uses a command-local `core.hooksPath=.local-archives/20261117-capacity/no-hooks` override when committing, without changing Git configuration or invoking a release/install pipeline.

## Search and Sources

- Built-in Web searched and opened official OpenAI [error handling](https://developers.openai.com/api/docs/guides/error-codes) and [Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events). The latter shows created/in-progress with empty output before output-item events.
- No deferred `tool_search` or Matrix tool was exposed in this session. Discovered the fixed Matrix bridge's tools via MCP `tools/list`, then independently called `tools/call search` with a public documentation query. It returned `fetch failed`; external dual-search agreement is not established. The configured bridge path is current, not the deprecated `.codex/tools` path.
- Local official-source cross-check: `C:\Users\sunda\Documents\LLMservice\codex-source-latest\codex-rs\codex-api\src\sse\responses.rs` (code-based overload mapping and ignored in-progress) and `codex-rs\protocol\src\error.rs` (`ServerOverloaded` non-retryable). These local sources explain the failure mechanism; no claim that this checkout is identical to the installed Desktop backend.
- No credentials, cookies, private prompts, or ACPs material sent to either search service. Diagnostic DB queries are read-only; do not copy raw HTTP-header logs into this document.

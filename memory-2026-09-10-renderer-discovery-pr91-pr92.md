# 2026-09-10 Codex renderer client discovery performance and PR #91/#92 integration

## Root cause

- `build_model_picker_unlock_script` previously called `patchReactAppServerClients()` on every 1.5-second renderer heartbeat. The function synchronously enumerated every DOM element and then breadth-first traversed up to 140,000 objects from React internals.
- After the native history refresh completed, `installAppServerPatch()` invoked the same traversal a second time in the same heartbeat. This made the compatibility layer itself a periodic main-thread long-task source.
- The installed runtime observed during diagnosis was still `3.20.1-3` on `127.0.0.1:15721`; source validation must not be reported as installed-runtime acceptance.

## Correct ownership boundary

- The cached `app-server-manager-signals-*` module is the primary discovery path. Its signal getter can replace its request client while the imported module object remains stable, so the lightweight heartbeat must call the getter again and idempotently patch the current client.
- React traversal remains a compatibility fallback because Codex bundles may not expose the expected module shape. It is no longer synchronous or unconditional: DOM discovery and object traversal both yield after at most 4 ms or 800 visited items, only one discovery promise may run, and module failures use a 30-second full-scan backoff.
- `MutationObserver` only queues added nodes. The interval drains those nodes at most every 5 seconds and only when module discovery did not find a client. The callback itself never walks the subtree.
- History manager objects found during the initial fallback scan are retained in a bounded set. Native refresh completion schedules lineage hydration directly from those managers, eliminating the old second full traversal. Deferred or failed hydration retries are limited to once per 30 seconds.
- Scheduler version `2` clears the old interval and disconnects the previous observer when the new script is injected into a renderer that already has compatibility state.

## Pull-request audit and integration

- PR #91 (`4b43a379`) reuses the existing final-boundary OpenCode Go identity policy for direct deep-probe requests. It derives a stable identity from the candidate-run nonce and transport branch, preserves explicit session headers, and does not affect unrelated endpoints.
- PR #92 (`f4ac7ef0`) projects reasoning declarations from the persisted route's real target Provider and copies them only for aliases explicitly saved by that route. This avoids using temporary wizard collision aliases as the capability source.
- Both exact PR heads were merged into the isolated integration branch with dedicated merge commits before the renderer performance change. They were not treated as verified merely because GitHub reported them mergeable; focused tests were run locally.

## TDD and validation evidence

- RED: `codex_app_compatibility_heartbeat_does_not_repeat_full_react_scan` failed on the old script at the missing scheduler-version assertion.
- GREEN: the renderer scheduling test and executable QuickJS replacement-client test passed. The generated complete compatibility script also parses successfully in QuickJS.
- Pre-change baseline: Codex workspace 79/79, Codex Desktop 43/43, OpenCode Go 6/6.
- After PR integration: Codex workspace 81/81, OpenCode Go 8/8, protocol compatibility runner 35/35.
- Final concentrated gate: Codex workspace 81/81, Codex Desktop 46/46, OpenCode Go 8/8, protocol compatibility runner 35/35, TypeScript, renderer production build, `cargo check --lib --no-default-features`, CI-scope Clippy with `-D warnings`, rustfmt, focused Prettier, diff check, and strict UTF-8/no-BOM/no-U+FFFD validation all passed. Existing Browserslist/bundle-size notices remained warnings only.

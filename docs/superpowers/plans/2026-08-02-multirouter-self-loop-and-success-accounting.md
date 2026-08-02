# MultiRouter Self-loop and Success Accounting Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ensure Codex MultiRouter candidates always use their referenced target provider for network forwarding, and reject any local-proxy self-loop as a failed request before an upstream HTTP attempt can be counted successful.

**Architecture:** Keep route ordering and parent attribution in the retry layer, but materialize every resolved route against its persisted target provider before `forward()` treats it as resolved. Add one shared self-loop validation boundary after effective base URL extraction, apply it to normal, raw, and passthrough forwarding paths, and preserve the existing request accounting path so rejected recursion increments failure rather than success.

**Tech Stack:** Rust, Tokio, Axum/Reqwest, SQLite-backed provider router, Cargo tests.

---

### Task 1: Capture the route-materialization regression

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`

- [ ] Add a regression test showing an expanded route with `targetProviderId` still carries the parent router's local base URL before target materialization.
- [ ] Add a test for the forwarding-candidate builder that expects the effective candidate to inherit the target provider's external base URL and credentials while retaining parent attribution.
- [ ] Run the targeted tests and verify they fail for the missing target materialization, not for test setup.
- [ ] Commit the RED tests with a detailed diagnostic message.

### Task 2: Make retry candidates effective providers

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`
- Reuse: `src-tauri/src/proxy/providers/codex.rs`

- [ ] Change candidate construction to resolve referenced target providers through the router and call the existing `materialize_codex_routed_provider_from_target` contract before retry/account-pool expansion.
- [ ] Return an explicit configuration error when a route references a missing target provider.
- [ ] Keep route order, parent provider identity, model override, protocol metadata, and account-pool behavior unchanged.
- [ ] Run the focused route tests until GREEN.

### Task 3: Reject every effective local-proxy self-loop as failure

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`

- [ ] Add tests for matched-route and already-resolved-route local proxy URLs, not only route misses.
- [ ] Centralize the validation so normal, raw passthrough, and unknown-endpoint forwarding reject a local effective upstream.
- [ ] Emit a deterministic `route_error` reason and an actionable `InvalidRequest`/configuration failure.
- [ ] Verify the error returns before a response shell can increment `success_requests`; assert the existing retry failure path increments failure accounting.

### Task 4: Document and verify the root fix

**Files:**
- Modify: `memory.md`
- Modify: `docs/superpowers/plans/2026-08-02-multirouter-self-loop-and-success-accounting.md`

- [ ] Run `cargo fmt --check` and focused tests for all forwarding variants.
- [ ] Run the relevant Rust library suite, documenting any unrelated live-port conflict separately.
- [ ] Update project memory with the root cause, invariant, tests, and runtime validation evidence.
- [ ] Commit implementation and documentation with a detailed message ending in the required attribution.

### Task 5: Build, deploy, and validate the running CCSM

**Files:**
- Build output only; do not commit generated installers unless the repository release workflow requires it.

- [ ] Build the Windows release artifact using the repository's documented release/export workflow.
- [ ] Back up the installed executable, replace it with the new build, and restart CCSM.
- [ ] Send a uniquely identified routed Codex request through `127.0.0.1:15721` without exposing stored credentials.
- [ ] Verify `codex-router.log` records an external effective upstream URL, request volume remains bounded, and the request reaches a terminal completion or explicit failure.
- [ ] Verify no new recursive nested errors or false-success burst appears in `proxy_request_logs`.

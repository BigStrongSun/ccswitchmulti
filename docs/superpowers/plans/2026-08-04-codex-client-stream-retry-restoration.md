# Codex Client Stream Retry Restoration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore Codex-owned five-attempt recovery for Responses streams that disconnect after semantic output, without making CCSM replay semantic events itself.

**Architecture:** Keep the existing two-layer boundary. CCSM retries only before semantic output; after that boundary, it exposes an incomplete stream and Codex owns sampling retry through the generated provider's `stream_max_retries = 5`. Preserve request-stage retries, Response Grace, keepalives, and Zstd behavior.

**Tech Stack:** Rust 1.85, Tauri 2, TOML projection through `toml_edit`, Cargo unit tests, Markdown documentation.

## Global Constraints

- Do not add a proxy-side post-semantic replay or event-deduplication state machine.
- Keep `CODEX_MANAGED_REQUEST_MAX_RETRIES` at `2`.
- Set the managed Codex stream retry budget explicitly to `5`.
- Do not bump the application version or publish/install a release in this change.
- Every commit message must end with `本次提交由BigStrongsSun完成`.

---

### Task 1: Lock the managed retry contract with TDD

**Files:**
- Modify: `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consumes: `CODEX_MANAGED_REQUEST_MAX_RETRIES: u64` and `CODEX_MANAGED_STREAM_MAX_RETRIES: u64`.
- Produces: managed provider TOML with request retry budget 2 and stream retry budget 5.

- [ ] **Step 1: Write the failing test**

Rename `managed_codex_retry_budget_allows_pre_stream_but_not_in_flight_stream_retry` to `managed_codex_retry_budget_preserves_codex_stream_recovery` and assert literal values:

```rust
assert_eq!(CODEX_MANAGED_REQUEST_MAX_RETRIES, 2);
assert_eq!(CODEX_MANAGED_STREAM_MAX_RETRIES, 5);
```

- [ ] **Step 2: Run the test to verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests::managed_codex_retry_budget_preserves_codex_stream_recovery --lib
```

Expected: FAIL because the current stream retry constant is 0.

- [ ] **Step 3: Implement the minimal production change**

Set `CODEX_MANAGED_STREAM_MAX_RETRIES` to 5 and rewrite its comment to describe the two-layer ownership boundary: CCSM retries only before semantic output; Codex owns retry after semantic output.

- [ ] **Step 4: Run focused tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests::managed_codex_retry_budget_preserves_codex_stream_recovery --lib
cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml services::proxy::tests --lib
```

Expected: all selected tests PASS.

- [ ] **Step 5: Commit code and tests**

Stage `src-tauri/src/codex_config.rs` and commit the restored client-owned retry contract with the required signature.

### Task 2: Correct the reconnect documentation and project memory

**Files:**
- Modify: `docs/guides/codex-reconnect-mechanism-zh.md`
- Modify: `docs/superpowers/plans/2026-08-03-codex-native-sse-recovery.md`
- Modify: `memory.md`

**Interfaces:**
- Consumes: the code contract from Task 1 and official Codex default retry behavior.
- Produces: one consistent explanation of proxy-owned pre-semantic recovery and Codex-owned post-semantic recovery.

- [ ] **Step 1: Rewrite the guide's retry table and diagrams**

Change `stream_max_retries` from 0 to 5, distinguish proxy-transparent retry from Codex sampling retry, and remove claims that all post-item retries necessarily corrupt history.

- [ ] **Step 2: Mark the earlier plan as superseded at this boundary**

Add a note explaining that its `stream_max_retries=0` decision was superseded after current official source and the `v3.16.5-22` regression comparison were verified.

- [ ] **Step 3: Correct project memory**

Replace the old “must remain 0” conclusion with the exact regression: `72c8ca22` disabled the official recovery mechanism after `v3.16.5-22`; the current fix restores 5 while retaining proxy safety boundaries.

- [ ] **Step 4: Review documentation consistency**

Run:

```powershell
rg -n "stream_max_retries=0|stream_max_retries` \| `0|不自动重发|不能简单调大" docs/guides/codex-reconnect-mechanism-zh.md memory.md
git diff --check
```

Expected: no live guidance claims that managed Codex stream retry remains disabled; diff check passes.

- [ ] **Step 5: Commit documentation and memory**

Stage the three documentation files and commit with the required signature.

### Task 3: Verify the combined recovery boundary

**Files:**
- Test: `src-tauri/src/proxy/providers/streaming_retry.rs`
- Test: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/services/proxy.rs`

**Interfaces:**
- Consumes: final code and documentation from Tasks 1 and 2.
- Produces: fresh verification evidence and a clean Git worktree.

- [ ] **Step 1: Run retry and configuration suites**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::streaming_retry::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml services::proxy::tests --lib
```

- [ ] **Step 2: Run compile and formatting checks**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
```

- [ ] **Step 3: Inspect final state**

```powershell
git status --short
git log -4 --oneline
```

Expected: verification commands pass and only intentional changes are committed.


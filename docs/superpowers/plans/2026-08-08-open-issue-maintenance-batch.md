# Open Issue Maintenance Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve issues `#35`, `#34`, `#6`, and `#3` with isolated root-cause fixes, regression evidence, and auditable local commits.

**Architecture:** Keep each compatibility rule at the narrowest existing ownership boundary: Claude profile persistence in `claude_desktop_config`, LM Studio request shaping in the Codex forwarder/native Responses boundary, proxy error classification in the Responses error builder, and unsigned-build guidance in the README. Each code change follows a focused RED/GREEN cycle and preserves existing behavior outside its explicit scope.

**Tech Stack:** Rust, serde_json, Cargo tests, rustfmt, Markdown, Git.

## Global Constraints

- Do not overwrite explicit user values with compatibility defaults.
- Do not change HTTP retry behavior while fixing issue `#6` diagnostics.
- Do not apply LM Studio-specific request fields to unrelated providers.
- Do not claim a local commit is released or close a release-bound issue prematurely.
- Every repository modification must be committed locally, and every commit message must end with `本次提交由BigStrongsSun完成`.

---

### Task 1: Preserve Claude Desktop User Profile Fields (`#35`)

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs:974`
- Test: `src-tauri/src/claude_desktop_config.rs:1488`

**Interfaces:**
- Consumes: existing `read_json_or_empty`, generated profile `serde_json::Value`, and `ClaudeDesktopPaths::profile_path`.
- Produces: an ownership-aware merged profile where newly generated keys win and existing unknown keys survive.

- [ ] **Step 1: Write the failing regression test**

Create an existing profile with `autoModeEnabled`, `toolSearchEnabled`, and a stale `inferenceGatewayBaseUrl`, then apply a provider and assert that user extras survive while the managed base URL is replaced.

- [ ] **Step 2: Run the test and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib claude_desktop_apply_preserves_user_profile_extra_fields -- --exact --nocapture`

Expected: FAIL because the current full-file write removes the user extras.

- [ ] **Step 3: Implement the ownership-aware merge**

Read the existing profile immediately before the deployment writes, merge existing object keys only when absent from the generated object, and pass the merged value to `write_json_file`. Preserve rollback behavior and propagate malformed/read errors consistently with the existing transaction.

- [ ] **Step 4: Verify GREEN and neighboring tests**

Run the focused test, then `cargo test --manifest-path src-tauri/Cargo.toml --lib claude_desktop_config::tests -- --nocapture`.

- [ ] **Step 5: Format, inspect, and commit**

Run `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `git diff --check`, inspect the staged diff, then commit only this task.

### Task 2: Normalize LM Studio Responses Text Format (`#34`)

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs:2503`
- Test: `src-tauri/src/proxy/forwarder.rs:8524`

**Interfaces:**
- Consumes: effective `Provider`, Codex app/endpoint flags, and the already normalized native Responses request body.
- Produces: an LM Studio-only request normalization that inserts `text.format.type = "text"` when absent.

- [ ] **Step 1: Write failing scope and payload tests**

Add tests proving that the effective LM Studio provider is selected for the compatibility rule, that a missing format is added, that an explicit format remains unchanged, and that an unrelated provider is untouched.

- [ ] **Step 2: Run the tests and verify RED**

Run the new focused Cargo test filter and confirm failure is caused by the absent normalization helper/behavior.

- [ ] **Step 3: Implement the minimal native Responses rule**

Add a pure provider-scope predicate and a pure request mutator. Invoke it after standard Responses normalization and before final body filtering. Do not alter Chat/Anthropic conversion paths or official OAuth requests.

- [ ] **Step 4: Verify GREEN and neighboring passthrough tests**

Run the new tests plus the existing Codex Responses passthrough normalizer tests.

- [ ] **Step 5: Format, inspect, and commit**

Run Rust formatting/checks and `git diff --check`, then commit only the LM Studio compatibility change.

### Task 3: Classify Codex Upstream Send Failures (`#6`)

**Files:**
- Modify: `src-tauri/src/proxy/handlers.rs:4196`
- Test: `src-tauri/src/proxy/handlers.rs:6502`

**Interfaces:**
- Consumes: `ProxyError`, provider name, request model, and endpoint.
- Produces: stage-accurate error JSON/messages while retaining existing status mapping and diagnostic fields.

- [ ] **Step 1: Replace the misleading expectation with a RED regression**

For an official OpenAI Codex `ForwardFailed` error, assert that the message identifies upstream connection/send failure and excludes `CC Switch local proxy failed`. Add or retain a separate test showing a genuine local/internal failure keeps local-proxy wording.

- [ ] **Step 2: Run the test and verify RED**

Run the focused handler test and confirm it fails on the existing generic message.

- [ ] **Step 3: Implement stage-aware message selection**

Classify `ForwardFailed` as upstream forwarding/connectivity failure in the Codex proxy error builder, with official-provider wording where applicable. Preserve error code/status mapping, structured provider/model/endpoint fields, and retry semantics.

- [ ] **Step 4: Verify GREEN and neighboring error tests**

Run all focused `codex_proxy_*error*` tests and the proxy error mapper tests.

- [ ] **Step 5: Format, inspect, and commit**

Run Rust formatting/checks and `git diff --check`, then commit only the diagnostics change.

### Task 4: Document Unsigned macOS Installation (`#3`)

**Files:**
- Modify: `README.md:109`

**Interfaces:**
- Consumes: current release asset naming and unsigned/notarization workflow evidence.
- Produces: Chinese and English app-scoped installation guidance without weakening Gatekeeper globally.

- [ ] **Step 1: Reconcile the stale draft PR with the current README**

Port only still-correct content, use current asset names, and remove any stale signed/notarized claim.

- [ ] **Step 2: Validate the documentation**

Run targeted `rg` assertions for `CCSwitchMulti_<version>_aarch64.dmg`, `com.apple.quarantine`, `Open Anyway`, and absence of global Gatekeeper-disable commands. Run `git diff --check`.

- [ ] **Step 3: Inspect and commit**

Commit only the README documentation change. Do not merge the stale draft PR or close the issue until the GitHub-visible documentation boundary is satisfied.

### Task 5: Batch Verification, Memory, and Issue State

**Files:**
- Modify: `memory.md`

**Interfaces:**
- Consumes: all four task commits and fresh verification output.
- Produces: an auditable project-memory entry and a precise list of GitHub issues eligible for closure.

- [ ] **Step 1: Run fresh batch verification**

Run the affected Rust test modules, `cargo check --manifest-path src-tauri/Cargo.toml --lib`, `cargo fmt --manifest-path src-tauri/Cargo.toml --check`, and `git diff --check`.

- [ ] **Step 2: Update project memory**

Record root causes, implementation commits, RED/GREEN evidence, external-search evidence, and any remaining push/release/runtime boundary.

- [ ] **Step 3: Commit the memory update**

Inspect and commit only `memory.md` with a detailed message.

- [ ] **Step 4: Re-read GitHub state before mutations**

Fetch issues `#35`, `#34`, `#6`, and `#3` plus PR `#4`. Close only items whose repository-visible acceptance boundary is actually satisfied; otherwise report the exact remaining remote action.

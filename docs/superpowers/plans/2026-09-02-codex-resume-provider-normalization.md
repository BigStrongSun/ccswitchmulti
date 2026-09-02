# Codex Resume Provider Normalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure Codex Desktop cold-resumes historical tasks through the currently active CCSwitchMulti Provider instead of replaying a stale persisted Provider ID.

**Architecture:** Extend the existing renderer compatibility payload with the active `model_provider` read from live `config.toml`. Normalize only `thread/resume` requests at the renderer request boundary, supporting both direct app-server calls and `send-cli-request-for-host` wrappers; retain the existing `thread/list` normalization and leave all other methods untouched.

**Tech Stack:** Rust, embedded JavaScript, QuickJS unit tests, TOML.

**Spec:** Root-cause evidence for task `01a04e4f-55be-7463-98ca-d5d8fb1cd158`: its persisted Provider is `openai`, while contemporaneous router logs prove the actual Qwen traffic entered `codex-multirouter`; current live Provider is `codex_model_router_v2`.

## Global Constraints

- Do not edit rollout JSONL or `state_5.sqlite`.
- Do not restart or terminate the Codex Desktop/app-server hosting the current task.
- Preserve all non-`thread/resume` request parameters.
- If live `model_provider` is absent or blank, do not invent a Provider override.
- Use UTF-8 without BOM and update `memory.md` after verification.

---

### Task 1: Normalize cold-resume Provider at the renderer boundary

**Files:**
- Modify: `src-tauri/src/codex_desktop.rs`
- Modify: `memory.md`
- Test: inline Rust/QuickJS tests in `src-tauri/src/codex_desktop.rs`

**Interfaces:**
- Consumes: live top-level TOML `model_provider` and renderer request `(method, params)`.
- Produces: `CodexModelCatalogProjection.model_provider: Option<String>` serialized as `modelProvider`, plus JavaScript `normalizeAppServerRequestParams(method, params)` behavior.

- [x] **Step 1: Write the failing behavioral test**

Add a QuickJS test that supplies `modelProvider: "codex_model_router_v2"` and asserts literal outputs for direct and host-wrapped `thread/resume`, unchanged unrelated methods, and the existing `thread/list` behavior. Add a second case proving a missing active Provider leaves the historical value untouched.

- [x] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_app_request_normalization -- --nocapture`

Expected: FAIL because the request-normalization core/helper and Provider payload field do not yet exist.

- [x] **Step 3: Implement the minimal behavior**

Add the active Provider to `CodexModelCatalogProjection`, read it from live TOML beside the active model, extract the request normalization JavaScript into an executable core, and set only `thread/resume.modelProvider` (direct or wrapped) when the payload has a nonblank current Provider.

- [x] **Step 4: Run focused and regression verification**

Run:

```text
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_app_request_normalization -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_desktop::tests -- --nocapture
cargo check --manifest-path src-tauri/Cargo.toml --lib --no-default-features
cargo fmt --manifest-path src-tauri/Cargo.toml --check
git diff --check
```

Expected: all commands pass with no new warnings attributable to this change.

- [x] **Step 5: Record the root cause and commit**

Update `memory.md` with the persisted-label-versus-actual-router distinction, resume lifecycle boundary, and verification results. Commit only the plan, production change, tests, and memory using a detailed message ending with `本次提交由BigStrongsSun完成`.

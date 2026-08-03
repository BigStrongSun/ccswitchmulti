# Codex Native SSE Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep managed Codex HTTP/SSE turns alive through bounded upstream failures before any persistable output, without replaying a completed item or tool invocation.

**Architecture:** Add a native Responses SSE state machine next to the existing Anthropic conversion retry wrapper. It forwards exactly one protocol scaffold (`response.created` and keepalive comments), reconnects at most five times only before semantic output, and treats text/reasoning deltas, output items, tools, terminal events, and malformed blocks as a no-replay boundary. The existing Responses WebSocket switch remains disabled until a separate end-to-end relay can prove upgrade, header preservation, reconnect, and HTTP fallback against the official service.

**Tech Stack:** Rust, Axum, Hyper/Reqwest streaming, Tokio, SSE, Cargo tests.

## Global Constraints

- Keep `stream_max_retries = 0` in managed Codex configuration: Codex must not replay a turn after CCSM has emitted semantic output.
- Retry only the exact same upstream provider request; do not fail over a live stream.
- Never log, persist, or add test fixtures containing bearer tokens, account identifiers, or attestation values.
- Do not enable `supports_websockets` until real official WebSocket integration verification exists.

---

### Task 1: Native Responses SSE recovery wrapper

**Files:**
- Modify: `src-tauri/src/proxy/providers/streaming_retry.rs`
- Test: `src-tauri/src/proxy/providers/streaming_retry.rs`

**Interfaces:**
- Produces: `create_resilient_responses_sse_stream(initial, reconnector) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send`.
- Consumes: `ByteStream`, `StreamReconnector`, and `RESPONSES_STREAM_MAX_RETRIES` from the same module.

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test(start_paused = true)]
async fn native_responses_reconnects_after_created_before_semantic_output() {
    // first attempt sends response.created then transport error;
    // retry sends response.created, a delta and response.completed.
    // Assert exactly one response.created, the delta/completed exist, and calls == 1.
}

#[tokio::test(start_paused = true)]
async fn native_responses_never_reconnects_after_output_item_done() {
    // first attempt sends created + output_item.done then transport error.
    // Assert calls == 0 and the transport error is visible downstream.
}
```

- [ ] **Step 2: Run the targeted test and verify it fails because the native wrapper does not exist**

Run: `cargo test --lib native_responses_reconnects_after_created_before_semantic_output`

Expected: compile failure for missing `create_resilient_responses_sse_stream`.

- [ ] **Step 3: Implement the minimal native event scanner and wrapper**

```rust
pub fn create_resilient_responses_sse_stream(
    initial: ByteStream,
    reconnector: Option<StreamReconnector>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
```

Parse complete SSE blocks. Before semantic output, suppress duplicate `response.created` after reconnect and emit `: ping\n\n` every ten seconds during upstream silence. Retry only transport errors or an unterminated EOF; forward explicit `response.failed`/`error` unchanged. Mark every event other than `response.created` and comments as semantic, including malformed blocks, so any unexpected protocol evolution fails safely rather than replaying.

- [ ] **Step 4: Run the wrapper tests and verify they pass**

Run: `cargo test --lib streaming_retry`

Expected: all existing retry tests plus both native tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/proxy/providers/streaming_retry.rs
git commit -m "fix(codex): recover native SSE before semantic output"
```

### Task 2: Route the managed native Codex path through the wrapper

**Files:**
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: `src-tauri/src/proxy/handlers.rs`

**Interfaces:**
- Consumes: `ForwardResult.stream_reconnect` and `create_resilient_responses_sse_stream`.
- Produces: managed raw Codex streaming response with retry wrapper only for a successful SSE response.

- [ ] **Step 1: Write a failing focused route-selection test**

```rust
#[test]
fn codex_native_stream_uses_recovery_only_for_sse_success() {
    assert!(should_wrap_native_codex_responses_stream(true, StatusCode::OK, true));
    assert!(!should_wrap_native_codex_responses_stream(false, StatusCode::OK, true));
    assert!(!should_wrap_native_codex_responses_stream(true, StatusCode::BAD_GATEWAY, true));
    assert!(!should_wrap_native_codex_responses_stream(true, StatusCode::OK, false));
}
```

- [ ] **Step 2: Run it and verify it fails for the missing selector**

Run: `cargo test --lib codex_native_stream_uses_recovery_only_for_sse_success`

Expected: compile failure for missing selector.

- [ ] **Step 3: Implement selector and response-body replacement**

Take `result.stream_reconnect` in `handle_raw_openai_passthrough`. When the raw request is streaming, status is successful, and content type is SSE, replace only the response body with the native wrapper while preserving status and non-entity headers. Send all other responses through the unchanged processor.

- [ ] **Step 4: Run focused handler and streaming suites**

Run: `cargo test --lib codex_native_stream_uses_recovery_only_for_sse_success streaming_retry`

Expected: all selected tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/proxy/handlers.rs src-tauri/src/proxy/providers/streaming_retry.rs
git commit -m "fix(codex): wire safe retry into native Responses streams"
```

### Task 3: Preserve the WebSocket safety boundary

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/codex_config.rs`
- Modify: `memory.md`

**Interfaces:**
- Produces: an explicit test proving managed router remains HTTP/SSE until an official Responses WS relay exists.

- [ ] **Step 1: Add an assertion that the managed provider still has `supports_websockets = false`**

Use the existing managed-provider projection test and assert the false value for a clean config.

- [ ] **Step 2: Run the test and verify the currently enforced boundary**

Run: `cargo test --lib managed_codex`

Expected: all managed Codex projection tests pass.

- [ ] **Step 3: Record the protocol boundary and verification evidence in `memory.md`**

Record that the raw native wrapper eliminates pre-semantic stalls but cannot resume a semantic SSE stream, and that WS enablement remains a separate protocol/integration deliverable.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/codex_config.rs memory.md
git commit -m "docs(codex): lock websocket recovery boundary"
```

### Task 4: Verify and release-gate

**Files:**
- Verify only: Rust source and build metadata.

- [ ] **Step 1: Run focused regression suites**

Run: `cargo test --lib streaming_retry && cargo test --lib managed_codex_retry_budget && cargo test --lib hyper_client::tests`

- [ ] **Step 2: Run static validation**

Run: `cargo fmt --check && cargo check --lib && git diff --check`

- [ ] **Step 3: Check the worktree and document any unrelated running build processes instead of terminating them**

Run: `git status --short`


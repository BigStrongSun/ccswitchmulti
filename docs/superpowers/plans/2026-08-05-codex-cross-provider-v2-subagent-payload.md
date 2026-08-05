# Codex Cross-Provider V2 Subagent Payload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver readable Multi-Agent V2 tasks from an official Codex parent to Qwen and DeepSeek children without modifying reserved collaboration schemas.

**Architecture:** Restore the mixed-router policy marker at route materialization, apply a narrowly scoped `agents.*` schema rewrite at the official parent boundary, then project plaintext agent messages to ordinary user input at every third-party child boundary. Opaque ciphertext fails closed.

**Tech Stack:** Rust, serde_json, Axum proxy pipeline, Cargo unit tests, Codex Responses and Responses Lite protocols.

## Global Constraints

- Never rewrite `collaboration.*`.
- Never decrypt or globally delete `encrypted_content`.
- Do not persist collaboration plaintext in CCSwitchMulti logs or storage.
- Keep pure-official Multi-Agent V2 encryption unchanged.
- Every production change follows a failing regression test and a separate local commit whose message ends with `本次提交由BigStrongsSun完成`.

---

### Task 1: Stage A policy and schema boundary

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/providers/openai_compat.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`

**Interfaces:**
- Produces: `codex_multirouter_needs_plaintext_v2_collaboration(&Provider) -> bool`
- Produces: `make_codex_v2_agents_messages_plaintext(&mut Value) -> usize`

- [ ] Write failing tests proving mixed-router state survives resolved-provider materialization.
- [ ] Run the focused provider test and confirm the policy marker is absent.
- [ ] Implement the request-local marker and materialization whitelist.
- [ ] Run the focused provider test and confirm it passes.
- [ ] Write failing schema tests covering flat/nested `agents`, Lite tools, and untouched `collaboration`.
- [ ] Run the focused OpenAI compatibility tests and confirm `encrypted` remains on `agents.*`.
- [ ] Implement the three-function `agents.*` rewrite.
- [ ] Add the forwarder gate requiring Codex, official effective route, and the propagated mixed-router policy.
- [ ] Run focused provider, compatibility, and forwarder tests.
- [ ] Commit Stage A.

### Task 2: Stage B third-party input projection

**Files:**
- Create: `src-tauri/src/proxy/providers/codex_multi_agent.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`

**Interfaces:**
- Produces: `project_codex_agent_messages_for_third_party(&mut Value) -> Result<usize, ProxyError>`

- [ ] Write failing tests for plaintext `agent_message` to `role=user` projection.
- [ ] Write a failing test recovering clearly plaintext legacy `encrypted_content`.
- [ ] Write a failing test rejecting opaque base64/Fernet-like encrypted content.
- [ ] Run the focused module tests and confirm all three fail for the missing behavior.
- [ ] Implement the minimal projection and opaque-content classifier.
- [ ] Invoke it after effective route resolution and before Chat/Anthropic/native Responses conversion, only for non-official Codex providers.
- [ ] Run focused module and forwarder tests.
- [ ] Commit Stage B.

### Task 3: Verification, memory, and package

**Files:**
- Modify: `memory.md`
- Modify release/version files only if packaging requires a new build identifier.

**Interfaces:**
- Consumes: Stage A and Stage B behavior.
- Produces: verified Windows installer artifacts and reproducible runtime evidence.

- [ ] Run rustfmt and `git diff --check`.
- [ ] Run focused tests, `cargo check --lib`, and the relevant Rust regression target.
- [ ] Inspect diffs for plaintext logging or persistence.
- [ ] Update `memory.md` with the corrected root cause, code boundaries, tests, and remaining runtime uncertainty.
- [ ] Commit verification knowledge.
- [ ] Build the Windows installer/package with the repository's existing release workflow.
- [ ] Install or stage the build, fully restart CCSwitchMulti and Codex app-server, then run unique-nonce OpenAI-to-Qwen and OpenAI-to-DeepSeek child tasks that execute a read-only tool.
- [ ] Inspect both child rollouts for readable `Payload:` text and no opaque task block.
- [ ] Report artifact paths, test evidence, and any remaining uncertainty.


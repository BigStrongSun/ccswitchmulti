# Cross-Provider Reasoning Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow a Codex conversation to switch from Qwen Chat Completions to official OpenAI Responses without replaying a synthetic reasoning ID as nonexistent `store: false` server state.

**Architecture:** Extend the existing official Responses request-boundary normalizer. Plain third-party reasoning remains inline and readable but loses its synthetic ID; encrypted official reasoning and all unrelated item classes keep their current behavior.

**Tech Stack:** Rust, serde_json, Cargo unit tests, CCSwitchMulti local proxy, Codex Desktop.

## Global Constraints

- Do not enable `store: true` or persist provider response items in CCSwitchMulti.
- Do not delete readable third-party reasoning summaries.
- Do not modify IDs or encrypted content on official encrypted reasoning items.
- Do not log prompt, response, reasoning, credentials, or cookies.
- Use a failing regression before production changes and commit each modification.

---

### Task 1: Lock the plain-reasoning replay contract

**Files:**
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Consumes: `normalize_replayed_item_ids_for_openai(body: &mut Value) -> usize`
- Produces: A regression contract for synthetic plain reasoning and encrypted official reasoning.

- [ ] **Step 1: Write the failing test**

Add `openai_request_inlines_synthetic_plain_reasoning_without_id`. Its literal fixture contains `id: "rs_resp_chatcmpl-b4d3bcf7f34003ac"`, a readable `summary`, and no encrypted content. Assert that normalization reports one change, removes `id`, and preserves `summary`.

- [ ] **Step 2: Run the single test to verify RED**

Run:

```powershell
cargo test openai_request_inlines_synthetic_plain_reasoning_without_id --lib
```

Expected: FAIL because the existing prefix check preserves the synthetic `rs_*` ID.

- [ ] **Step 3: Commit the RED test**

Stage only `transform_codex_chat.rs` and commit with the project-required attribution footer.

### Task 2: Project plain reasoning as inline official input

**Files:**
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Consumes: `normalize_replayed_item_ids_for_openai(body: &mut Value) -> usize`
- Produces: The same function with plain-reasoning ID removal and unchanged encrypted-reasoning handling.

- [ ] **Step 1: Implement the minimal branch**

Before prefix normalization, detect `type == "reasoning"` without non-empty `encrypted_content`. Remove `id`, increment the changed count only when an ID existed, and continue to the next item. Leave every other item branch unchanged.

- [ ] **Step 2: Run focused GREEN verification**

Run:

```powershell
cargo test openai_request_inlines_synthetic_plain_reasoning_without_id --lib
cargo test openai_request_does_not_rewrite_encrypted_reasoning_item_ids --lib
cargo test normalize_replayed_item_ids_for_openai --lib
```

Expected: all matching tests pass.

- [ ] **Step 3: Run module verification**

Run:

```powershell
cargo test transform_codex_chat --lib
cargo fmt --check
git diff --check
```

Expected: zero failures and no formatting errors.

- [ ] **Step 4: Commit the implementation**

Stage only `transform_codex_chat.rs` and commit with the project-required attribution footer.

### Task 3: Verify the complete runtime boundary

**Files:**
- Modify after validation: `memory.md`

**Interfaces:**
- Consumes: Installed CCSwitchMulti proxy on `127.0.0.1:15721` and the existing Qwen/official MultiRouter routes.
- Produces: Runtime evidence for both switch directions and a durable project note.

- [ ] **Step 1: Run broader Rust verification**

Run `cargo check --lib` followed by the full library test suite with one test thread if the live proxy must remain bound. Record any pre-existing or environmental failures separately.

- [ ] **Step 2: Build and install the tested binary**

Use the repository's existing local release/export workflow. Verify the file version and that the process listening on port 15721 is the new binary before acceptance probes.

- [ ] **Step 3: Exercise the real switch matrix**

Create disposable Codex conversations and run Qwen→official, official→Qwen, and Qwen→Qwen sequences. Each turn must reach a normal task completion. Inspect `codex-router.log` for route, effective endpoint, HTTP status, and absence of the prior item-not-found error.

- [ ] **Step 4: Update project memory**

Append the root cause, exact invariant, commits, automated results, installed version, and live acceptance evidence to `memory.md`. Do not record sensitive request content or credentials.

- [ ] **Step 5: Commit the evidence**

Stage only `memory.md` and any intentional version/release files, then commit with the project-required attribution footer.

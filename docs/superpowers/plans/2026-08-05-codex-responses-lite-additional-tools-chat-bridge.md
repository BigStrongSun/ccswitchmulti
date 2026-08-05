# Codex Responses Lite `additional_tools` Chat Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` and execute each checkbox in order. Apply `superpowers:test-driven-development` for every behavior change and `superpowers:verification-before-completion` before any completion claim.

**Goal:** Preserve Responses Lite tools while preventing invalid non-assistant `content:null` messages when CCSwitchMulti converts Codex `/responses` requests to third-party Chat Completions.

**Architecture:** Extend the existing `CodexToolContext` collection boundary to consume `input[].type=additional_tools` recursively and reuse `add_response_tool` for normalization and deduplication. Treat `additional_tools` as structural metadata during message projection. Add a role-aware content normalization boundary that emits strings for non-assistant messages while preserving legal assistant tool-call `content:null`.

**Tech Stack:** Rust, `serde_json::Value`, Cargo unit tests, Git worktrees, GitHub CLI.

## Global constraints

- Do not disable Responses Lite or rewrite session JSONL/SQLite.
- Do not add Qwen/provider-specific handling or network retry behavior.
- Do not globally remove null content; assistant tool-call messages may legally use it.
- Reuse `CodexToolContext::add_response_tool` for top-level, additional, and tool-search-provided tools.
- Keep the upstream PR branch based directly on `upstream/main` and free of fork-only commits.
- Every commit message must end with `本次提交由BigStrongsSun完成`.

---

### Task 1: Lock the broken conversion contract with RED tests

**Files:**
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Input: Responses request containing top-level tools, `input[].additional_tools`, messages, and tool search output.
- Output: Chat request containing normalized `messages`, `tools`, and `tool_choice`.

- [ ] **Step 1: Add a Responses Lite regression test**

Construct a request whose first input item is `additional_tools` with one function and whose second item is a developer message with base instructions. Call the real `responses_to_chat_completions_with_reasoning_text_only_and_cache` conversion and assert that no generated message has null content and that the function appears once in top-level `tools`.

- [ ] **Step 2: Add tool-shape and deduplication tests**

Cover a custom tool, namespace-style function name, and a duplicate definition shared by top-level `tools` and `additional_tools`. Assert existing flattening/custom conversion behavior and one emitted Chat tool per logical name.

- [ ] **Step 3: Add role-aware null-content tests**

Cover missing and explicit-null content for developer/system/user messages. Assert every emitted non-assistant message has string content. Add an assistant synthetic tool-call case and assert its `content:null` is preserved.

- [ ] **Step 4: Run focused tests and capture RED evidence**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform_codex_chat::tests::responses_lite_additional_tools --lib -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform_codex_chat::tests::responses_non_assistant_null_content --lib -- --nocapture
```

Expected: failures demonstrate that `additional_tools` becomes a null system message, its tools are absent, or non-assistant null content survives. The assistant preservation assertion may already pass.

- [ ] **Step 5: Commit RED tests**

Stage only `transform_codex_chat.rs`, inspect the staged diff, and commit the failing regression tests with a detailed message and the required signature.

### Task 2: Implement the root fix and reach GREEN

**Files:**
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Extend: `build_codex_tool_context_from_request` to collect `additional_tools.tools`.
- Extend: input-item dispatch to consume `additional_tools` without creating a message.
- Harden: `responses_message_item_to_chat_message` content conversion by Chat role.

- [ ] **Step 1: Add recursive additional-tool collection**

Add a narrowly named recursive collector over `body.input`. For each object with `type=additional_tools`, iterate its `tools` array and call `CodexToolContext::add_response_tool`. Invoke it after top-level tools and before the existing `tool_search_output` collector so all sources retain stable request order and shared deduplication.

- [ ] **Step 2: Consume `additional_tools` structurally**

Add an explicit input-item match arm that produces no Chat message and does not flush or reorder pending reasoning, media, or tool-call state.

- [ ] **Step 3: Normalize message content by output role**

In the real message conversion path, retain `Value::Null` only for assistant messages. For system/developer-to-system and user messages, convert missing or null content to `Value::String(String::new())`. Leave existing non-null structured/string conversion intact.

- [ ] **Step 4: Run all new tests to verify GREEN**

Run the focused test filters from Task 1 and confirm all assertions pass.

- [ ] **Step 5: Run adjacent regression suites**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform_codex_chat::tests --lib
```

Confirm system collapse, tool search, hosted tools, custom tools, media conversion, and existing Responses-to-Chat tests remain green.

- [ ] **Step 6: Commit implementation**

Stage only the production/test file, inspect the staged diff, and commit the root fix with a detailed message and the required signature.

### Task 3: Record project knowledge and verify the fork branch

**Files:**
- Modify: `memory.md`

- [ ] **Step 1: Record the resolved incident contract**

Document the affected Codex and CCSwitchMulti versions, the `additional_tools` protocol shape, why it was absent from persisted session history, the exact faulty fallback chain, the two-layer fix, and the assistant-null exception. Mark the bug fixed only after tests pass.

- [ ] **Step 2: Run compile, format, and diff checks**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
```

- [ ] **Step 3: Run the library suite where resources permit**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

If an existing environment-dependent test fails, capture its exact name/output and independently prove the changed module passes; do not call the full suite green.

- [ ] **Step 4: Commit project memory**

Stage `memory.md`, inspect the staged diff, and commit the verified record with the required signature.

### Task 4: Prepare a clean upstream-based worktree and branch

**Files:**
- Inspect: `.gitignore`
- Create: isolated Git worktree outside the primary checkout

- [ ] **Step 1: Verify worktree prerequisites**

Run `git rev-parse --git-dir`, `git rev-parse --git-common-dir`, inspect existing worktrees, and verify the chosen worktree location cannot be accidentally tracked. Fetch `upstream` and record the current `upstream/main` commit.

- [ ] **Step 2: Create the clean branch**

Create `bigstrongsun/fix-responses-lite-additional-tools` from `upstream/main` in an isolated worktree. Confirm its merge-base is exactly the fetched upstream head.

- [ ] **Step 3: Replay the TDD change cleanly**

Apply the RED tests first, run them and capture failure, commit them. Apply the production fix, run focused and adjacent suites, then commit GREEN. Include only source/tests and an upstream-suitable explanation; exclude fork release and unrelated docs/history.

- [ ] **Step 4: Verify the clean diff**

Run:

```powershell
git log --oneline upstream/main..HEAD
git diff --stat upstream/main...HEAD
git diff --check upstream/main...HEAD
```

Expected: only the intentional TDD and implementation commits/files appear.

### Task 5: Create the upstream Issue and ready PR

**Files:**
- No repository file changes required beyond Task 4.

- [ ] **Step 1: Verify GitHub identity and remotes**

Run `gh --version`, `gh auth status`, and `git remote -v`. Do not print or transmit credentials.

- [ ] **Step 2: Create a sanitized upstream Issue**

Against `farion1231/cc-switch`, report the minimal `additional_tools` input, invalid `system/content:null` Chat output, affected `v3.19.1` and current-main commit, source-level root cause, and expected conversion. Exclude session text, request bodies from real users, private URLs, tokens, and machine-identifying logs.

- [ ] **Step 3: Push the clean branch to the fork**

Push `bigstrongsun/fix-responses-lite-additional-tools` to `BigStrongSun/ccswitchmulti` and verify the remote branch commit.

- [ ] **Step 4: Open a ready-for-review upstream PR**

Create a non-draft PR from the fork branch to `farion1231/cc-switch:main`, link the Issue, explain tool preservation and the role-aware null defense, list RED-to-GREEN evidence and verification commands, and ensure the diff contains no fork-only commits.

- [ ] **Step 5: Inspect final GitHub state**

Read back the Issue and PR, verify links, base/head branches, draft state, changed files, commits, and checks. Report any pending CI without claiming it passed.

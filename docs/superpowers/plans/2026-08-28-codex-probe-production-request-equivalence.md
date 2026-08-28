# Codex Probe Production Request Equivalence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ordinary third-party Codex protocol probes prepare and validate the same Provider-controlled upstream request as the production forwarder.

**Architecture:** Compile the complete effective Provider into one secret-bearing, redacted request policy in `proxy/providers/codex_request.rs`. Both forwarder and probe call that policy for URL, authentication, Provider headers/body overrides, model/reasoning mapping and wire-body conversion, while production-only session/history/transport concerns remain in `forwarder.rs`.

**Tech Stack:** Rust, Tauri backend, reqwest/http, serde_json, existing Codex adapter/transformers, tokio fixtures.

**Spec:** `docs/superpowers/specs/2026-08-28-codex-probe-production-request-equivalence-design.md`

## Global Constraints

- Only ordinary third-party OpenAI Responses and Chat Completions Providers are in scope.
- Probe prompts and tools remain fixed and contain no user data.
- Managed OAuth and official Providers remain excluded from active probing.
- No production code is written before its regression test is observed failing for the intended reason.
- Secrets never appear in Debug, serialization, progress events, profile keys or logs.
- All commits end with `本次提交由BigStrongsSun完成`.
- Do not push, publish, install or modify the main checkout.

---

### Task 1: Freeze the shared request contract with RED tests

**Files:**
- Create: `src-tauri/src/proxy/providers/codex_request.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Modify: `src-tauri/src/protocol_compatibility/provider_tests.rs`
- Modify: `src-tauri/src/protocol_compatibility/runner_tests.rs`

**Interfaces:**
- Produces: `CodexRequestTransport`, `CodexThirdPartyRequestPolicy`, `PreparedCodexRequest`, `CODEX_REQUEST_PREPARER_VERSION`.
- Consumes: `Provider`, `CodexAdapter`, existing Chat/Responses mapping functions.

- [ ] **Step 1: Write a compile-contract test**

Add a Provider fixture containing model catalog mapping, custom UA, local header/body overrides and reasoning capability. Assert that the compiled candidate retains a non-empty request-policy fingerprint, has redacted Debug output, and rejects managed/official Providers.

- [ ] **Step 2: Write final-request literal tests**

For Chat and Responses, prepare a fixed logical request and assert literal URL, headers and body fields: upstream model, mapped reasoning effort/thinking field, output budget, custom UA, accepted custom header, rejected Authorization override and nested body override.

- [ ] **Step 3: Run RED**

Run:

```powershell
$env:CARGO_TARGET_DIR='C:\Users\sunda\Documents\LLMservice\cc-switch\.worktrees\probe-production-request-equivalence\target-protocol-probe'
cargo test --manifest-path src-tauri/Cargo.toml --lib protocol_compatibility::provider_tests --no-default-features
```

Expected: compilation/test failure because the shared policy and candidate fingerprint do not exist.

- [ ] **Step 4: Commit RED tests and design**

Commit only the design/plan and tests, recording the expected RED failure in the message.

### Task 2: Implement the shared Provider request policy

**Files:**
- Create: `src-tauri/src/proxy/providers/codex_request.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`

**Interfaces:**
- `CodexThirdPartyRequestPolicy::compile(provider: &Provider) -> Result<Self, ProxyError>`
- `CodexThirdPartyRequestPolicy::prepare(&self, transport: CodexRequestTransport, logical_body: Value, options: CodexRequestOptions) -> Result<PreparedCodexRequest, ProxyError>`
- `CodexThirdPartyRequestPolicy::fingerprint(&self) -> &str`
- `CodexThirdPartyRequestPolicy::credential_fingerprint(&self) -> &str`

- [ ] **Step 1: Implement secret-safe policy compilation**

Use `CodexAdapter::extract_base_url`, `extract_auth` and `get_auth_headers`. Store the complete effective Provider privately, implement custom redacted `Debug`, and derive stable hashes without serializing secret values into output objects.

- [ ] **Step 2: Implement URL and body preparation**

Use the adapter/full-URL production rules for endpoint generation. Chat preparation must call the existing upstream-model mapper, reasoning resolver and `responses_to_chat_completions_with_reasoning_text_only_and_cache`; Responses preparation must call native effort and model mapping. Apply canonical filtering and Provider body override after protocol conversion.

- [ ] **Step 3: Implement Provider header policy**

Generate adapter auth headers, content type, custom UA and allowed local header overrides; protected credential/transport headers must not be overridden.

- [ ] **Step 4: Make forwarder call the shared functions**

Replace the ordinary third-party Codex fragments for URL, model/reasoning/wire-body preparation and final Provider header/body policy with calls to the shared module. Leave managed OAuth, official, Anthropic, history, hosted-tool loops, retries and transport in `forwarder.rs`.

- [ ] **Step 5: Run GREEN and focused production tests**

Run provider request tests plus existing `forwarder`/Codex provider tests. Expected: all pass without changing literal expectations.

- [ ] **Step 6: Commit the shared production boundary**

Commit the preparer and forwarder extraction with the required Chinese trailer.

### Task 3: Route protocol probe traffic through the shared policy

**Files:**
- Modify: `src-tauri/src/protocol_compatibility/mod.rs`
- Modify: `src-tauri/src/protocol_compatibility/provider.rs`
- Modify: `src-tauri/src/protocol_compatibility/runner.rs`
- Delete: `src-tauri/src/protocol_compatibility/endpoint.rs` if no remaining callers exist
- Modify: `src-tauri/src/protocol_compatibility/provider_tests.rs`
- Modify: `src-tauri/src/protocol_compatibility/runner_tests.rs`

**Interfaces:**
- `ProbeCandidate` privately owns `CodexThirdPartyRequestPolicy`.
- `ProbeTargetKey.request_policy_fingerprint: String` invalidates stale records.

- [ ] **Step 1: Write the probe-vs-production contract RED test**

Prepare the same fixed logical request through the production-facing policy entry and the runner-facing candidate entry. Compare URL, header multimap, body and fingerprint. Mutate custom UA, body override and reasoning policy independently and assert each changes the target key.

- [ ] **Step 2: Run RED**

Expected: runner still calls `build_probe_url`, manually converts Chat and sends only Bearer/content-type.

- [ ] **Step 3: Replace compressed candidate credentials**

Compile `ProbeCandidate` from the complete effective Provider policy. Implement `PartialEq`, `Eq` and `Debug` using public identity plus fingerprints, never Provider/secrets.

- [ ] **Step 4: Replace runner wire construction**

Build the fixed logical probe request, call `candidate.prepare_request(transport, logical)`, then construct reqwest from returned URL/headers/body. Remove direct Bearer, endpoint and Chat conversion code.

- [ ] **Step 5: Version profile identity**

Add request-policy fingerprint to `ProbeTargetKey`, storage/lease material and profile tests; bump the profile/preparer version so existing independent-runner records do not silently survive.

- [ ] **Step 6: Run GREEN and commit**

Run all protocol compatibility tests and commit with the required trailer.

### Task 4: Reuse production terminal semantics

**Files:**
- Modify: `src-tauri/src/protocol_compatibility/runner.rs`
- Modify: `src-tauri/src/protocol_compatibility/runner_tests.rs`
- Modify only if necessary: `src-tauri/src/proxy/providers/codex_terminal.rs`

**Interfaces:**
- Consumes: `classify_chat_terminal`, `classify_native_responses_terminal`, `ChatTerminalEvidence`, `NativeResponsesEvidence`.
- Produces: a probe-local boolean/result mapping that treats only production `Complete` disposition as stage success.

- [ ] **Step 1: Add Chat RED cases**

Add SSE fixtures for `[DONE]` with missing/unknown `finish_reason`, missing final output and incomplete tool arguments. Assert streaming/continuation does not pass.

- [ ] **Step 2: Add Responses RED cases**

Add `response.completed` fixtures with missing status, `failed`/`incomplete`, missing final output and incomplete tool calls. Assert the stage fails with redacted `invalid_response`.

- [ ] **Step 3: Run RED**

Expected: current `saw_done`/event-name checks incorrectly pass at least the scaffold-only cases.

- [ ] **Step 4: Build terminal evidence and call production classifiers**

Derive evidence from captured payloads without response-body logging. Treat only production complete dispositions as successful; preserve current reasoning/tool observations separately.

- [ ] **Step 5: Run GREEN and commit**

Run `protocol_compatibility` and `codex_terminal` tests, then commit with the required trailer.

### Task 5: Full verification and durable project memory

**Files:**
- Modify: `memory.md`
- Modify if results require clarification: design and plan documents

**Interfaces:**
- Produces: repeatable validation evidence and current architectural memory.

- [ ] **Step 1: Run focused tests**

Run protocol compatibility, Codex terminal, forwarder, handlers and Provider tests with the isolated `CARGO_TARGET_DIR`.

- [ ] **Step 2: Run complete backend verification**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features
cargo check --manifest-path src-tauri/Cargo.toml --tests --no-default-features
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
git diff --check
```

- [ ] **Step 3: Verify encoding and secret safety**

Strictly decode all changed text as UTF-8, assert no BOM/U+FFFD, and scan test/debug output and changed files for fixture secrets outside intentional test inputs.

- [ ] **Step 4: Update `memory.md`**

Record the root cause, new shared ownership boundary, profile invalidation behavior, exact verification results and explicit exclusions. Remove or supersede any stale statement that the independent runner remains active.

- [ ] **Step 5: Commit verification/memory**

Commit the final documentation with the required trailer. Do not push or install.

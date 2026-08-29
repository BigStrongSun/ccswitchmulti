# Codex Protocol Compatibility Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make automatic Chat/Responses probing, reasoning presentation, tool continuation, terminal handling, and Provider persistence match real Codex Desktop and CLI behavior across Qwen, DeepSeek, Kimi, GLM, xAI, and ordinary OpenAI-compatible gateways.

**Architecture:** Keep transport capability, reasoning field semantics, and Codex presentation quality as separate facts. Reuse the production request preparer and terminal classifier for probes, choose the strongest complete transport first, use Codex-visible reasoning quality only as a capability-tie breaker, and commit Provider plus probe receipts atomically.

**Tech Stack:** Rust/Tauri, reqwest/SSE, SQLite, React/TypeScript, Vitest.

**Spec:** Existing protocol-compatibility design documents plus the repository memory entries for production-request equivalence and raw-versus-summary semantics.

## Global Constraints

- Never relabel raw `reasoning_text` as a model-generated reasoning summary.
- Never infer compatibility by model brand; probe both maintained transports for every effective Provider/model target.
- Availability, authentication, quota, 429, network, and 5xx failures are not protocol incompatibility.
- Only a complete terminal, complete tool call, and consumed tool-result continuation can produce `Verified` readiness.
- Preserve unrelated dirty-worktree changes and do not push, publish, install, or restart without separate authorization.

---

### Task 1: Codex-visible transport selection

**Files:**
- Modify: `src-tauri/src/protocol_compatibility/selection.rs`
- Test: `src-tauri/src/protocol_compatibility/selection_tests.rs`
- Test: `src-tauri/src/protocol_compatibility/runner_tests.rs`

**Interfaces:**
- Consumes: `TransportProbeAssessment`, `ReasoningSemantic`, `TransportKind`.
- Produces: deterministic `TransportSelection` ordered by transport capability, then Codex-visible reasoning quality, then native Responses preference.

- [x] Replace the old same-capability tests with literal fixtures proving `Summary > Readable > Opaque/None`, equal reasoning prefers Responses, and stronger transport capability always wins.
- [x] Run the focused tests and confirm they fail only because the selector currently ignores reasoning semantics.
- [x] Add the minimal presentation-rank tuple to the selector.
- [x] Run focused selection and runner tests and confirm they pass.

### Task 2: Real Codex terminal, reasoning, and continuation matrix

**Files:**
- Modify/Test: `src-tauri/src/protocol_compatibility/runner_tests.rs`
- Modify/Test as required: `src-tauri/src/proxy/providers/codex_terminal.rs`
- Modify/Test as required: `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- Modify/Test as required: `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- Modify/Test as required: `src-tauri/src/proxy/providers/streaming_retry.rs`
- Modify/Test as required: `src-tauri/src/proxy/handlers.rs`

**Interfaces:**
- Consumes: production JSON/SSE terminal classification and Chat/Responses reasoning/tool events.
- Produces: complete, incomplete, failed, unsupported, or unavailable probe outcomes matching production behavior.

- [x] Add literal RED fixtures for missing/unknown finish reason, length cutoff, reasoning-only pseudo-terminal, half tool calls, EOF, failed/incomplete Responses, and tool-result continuation that ignores the supplied result.
- [x] Trace every failing fixture to the production classifier or bridge boundary before changing code.
- [x] Apply one root fix per confirmed failing boundary and rerun its focused regression.
- [x] Run the complete protocol, terminal, streaming, transform, retry, handler, and forwarder suites.

### Task 3: Provider preflight and atomic persistence

**Files:**
- Modify/Test as required: `src-tauri/src/commands/protocol_compatibility.rs`
- Modify/Test as required: `src-tauri/src/services/provider/mod.rs`
- Modify/Test as required: `src-tauri/src/codex_multirouter/mutation.rs`
- Modify/Test as required: `src-tauri/src/store.rs`
- Modify/Test as required: `src-tauri/src/provider.rs`

**Interfaces:**
- Consumes: prepared effective Provider/model targets and probe receipts.
- Produces: atomic Provider/MultiRouter/Universal persistence or an actionable, non-destructive failure.

- [x] Verify receipts cover request-policy fingerprint, corpus/budget version, target identity, and expiry.
- [x] Add RED tests for 521/unavailable classification, stale receipt rejection, partial MultiRouter writes, and Universal partial commits.
- [x] Fix only confirmed transaction or classification gaps.
- [x] Re-run Provider, MultiRouter, store, and protocol suites.

### Task 4: Probe progress and advanced contract UI

**Files:**
- Modify/Test as required: `src/components/providers/forms/CodexFormFields.tsx`
- Modify/Test as required: `src/components/providers/forms/CodexProtocolProbeProgressDialog.tsx`
- Modify/Test as required: `src/components/providers/forms/CodexProtocolAdvancedSettings.tsx`
- Modify/Test as required: `src/components/providers/forms/ProviderForm.tsx`
- Modify/Test as required: `src/lib/api/protocol-compatibility.ts`
- Modify/Test as required: corresponding Vitest files.

**Interfaces:**
- Consumes: backend stage progress, selected transport, readiness, reasoning semantic/source, and failure category.
- Produces: ordinary automatic flow plus guarded advanced override without frontend protocol inference.

- [x] Verify Provider-scoped model selection, running-stage visibility, disabled-step explanation, retry, and raw-versus-summary wording with component tests.
- [x] Add RED tests for any missing user-visible state or invalid advanced request.
- [x] Implement the smallest UI changes and run focused/full Vitest plus typecheck.

### Task 5: Live Provider and release-grade verification

**Files:**
- Update: `memory.md`

**Interfaces:**
- Consumes: configured local Providers without exposing credentials.
- Produces: redacted Chat/Responses probe evidence and a locally committed, reviewable source state.

- [x] Run real dual-transport probes for each usable configured Provider/model and compare the selected result with captured response shape.
- [x] Run complete Rust library tests, check, clippy, rustfmt, Vitest, typecheck, renderer build, and Windows application build.
- [x] Strictly decode changed text as UTF-8, reject BOM/U+FFFD, and run `git diff --check`.
- [x] Update `memory.md` with root causes, fixed/remaining boundaries, search channels, and exact verification evidence.
- [x] Stage only task-owned files and create a local commit whose message ends with `本次提交由BigStrongsSun完成`.

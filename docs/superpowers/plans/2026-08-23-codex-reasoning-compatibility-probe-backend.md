# Codex Protocol Compatibility Probe Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** During Provider save, automatically run one evidence-driven protocol compatibility probe that verifies each third-party Codex model's transport, streaming, tool and replay behavior, then derives a safe Chat-to-Responses reasoning projection only when the response evidence supports it. Reasoning-strength/effort discovery remains a separate capability chain.

**Architecture:** Keep passive `reasoning_capabilities` discovery independent. Replace the current pair of minimal `model_fetch` HTTP probe paths with one `protocol_compatibility` domain: it first compiles an unsaved `ProbeCandidate`, performs endpoint negotiation plus the multi-stage upstream transaction using production transformers, and atomically persists the Provider plus a redacted target-bound protocol profile. The reasoning projection is one validated subprofile. Runtime observes only structural shape for drift and invalidates mismatched profiles; it never starts a second billed probe. Manual override is a separate, revision-checked record; no frontend component is in scope.

**Tech Stack:** Rust 2021, Tauri 2 commands, reqwest, serde/serde_json, rusqlite, existing Chat/Responses transformers, tokio test fixtures.

**Spec:** `docs/superpowers/specs/2026-08-23-codex-reasoning-compatibility-probe-backend-design.md`

## Global Constraints

- Do not implement React/UI changes, installer changes, release changes, or Desktop config mutation in this branch.
- Never persist or log API keys, headers, prompt text, reasoning text, tool arguments, tool results, or unredacted upstream bodies.
- Provider save is the sole default authorization for one bounded active probe; re-test is explicit only after a failed/expired profile. Runtime request handling never starts an active probe.
- `reasoning_capabilities` continues to resolve effort/thinking parameters from passive metadata and explicit configuration only; this probe must never create or overwrite those capability values.
- A probe profile is keyed to provider, route, requested/effective model, transport, endpoint fingerprint, and auth kind; any mismatch invalidates it.
- Manual override wins only after semantic validation; it cannot expose opaque or encrypted data as raw reasoning.
- Official OAuth, native Responses, encrypted content, V2 subagent message projection, and tool-call ordering must remain unchanged.
- Follow RED → GREEN → focused regression → local commit. Every commit message ends with `本次提交由BigStrongsSun完成`.

---

### Task 1: Establish the profile and evidence domain as pure Rust types

**Files:**
- Create: `src-tauri/src/protocol_compatibility/mod.rs`
- Create: `src-tauri/src/protocol_compatibility/types.rs`
- Create: `src-tauri/src/protocol_compatibility/redaction.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: module tests in `src-tauri/src/protocol_compatibility/types.rs` and `redaction.rs`

**Interfaces:**

```rust
pub enum ReasoningSemantic { Readable, Summary, Opaque, None }
pub enum ReasoningSource { ReasoningContent, Reasoning, ReasoningDetails, ThinkTags, NativeResponses, None }
pub enum HistoryReplay { ChatReasoningContent, Omit, NativeOnly }
pub struct ProbeTargetKey { /* canonical, hashable route identity */ }
pub struct ProbeCandidate { /* unsaved effective provider + route + credentials held in memory */ }
pub enum ProbeReadiness { Verified, Partial, Unverified }
pub struct VerifiedProtocolCompatibilityProfile { /* endpoint, transport, stream, tool, replay and optional reasoning subprofile */ }
pub struct VerifiedReasoningProfile { /* spec section 4.2 */ }
pub struct ProtocolCompatibilityRecord { /* profile plus redacted structural evidence */ }
pub struct ManualReasoningOverride { /* profile subset + revision + reason */ }
pub struct ReasoningProjection { semantic: ReasoningSemantic, replay: HistoryReplay }
```

- [ ] **Step 1: Write failing pure-type tests** for target-key determinism, candidate compilation without a persisted provider ID, endpoint credential stripping, endpoint fingerprint changes, profile expiry, readiness transitions, and validation rejection of `Opaque -> Readable`, `Summary -> raw`, missing source, and invalid native replay.
- [ ] **Step 2: Write failing redaction tests** that feed representative Chat JSON/SSE records containing bearer headers, `reasoning_content`, `content`, tool arguments and tool outputs; assert the output contains only allowlisted JSON paths, counts, lengths, status, event names, and SHA-256 values.
- [ ] **Step 3: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::types --lib
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::redaction --lib
```

- [ ] **Step 4: Implement immutable types, validation, canonical endpoint hashing, and redaction.** Do not import `Provider` or open a database in these files.
- [ ] **Step 5: Run GREEN tests and format.**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
```

- [ ] **Step 6: Commit.**

```powershell
git add src-tauri/src/protocol_compatibility src-tauri/src/lib.rs
git commit -m "feat(codex): define protocol compatibility probe domain" -m "本次提交由BigStrongsSun完成"
```

### Task 2: Persist protocol profiles and reasoning overrides without reusing health logs

**Files:**
- Create: `src-tauri/src/database/dao/protocol_compatibility.rs`
- Modify: `src-tauri/src/database/dao/mod.rs`
- Modify: `src-tauri/src/database/schema.rs`
- Modify: `src-tauri/src/database/mod.rs`
- Modify: `src-tauri/src/database/backup.rs`
- Test: `src-tauri/src/database/tests.rs` and DAO module tests

**Interfaces:**

```rust
Database::save_protocol_compatibility_result(&ProtocolCompatibilityRecord) -> Result<(), AppError>
Database::get_protocol_compatibility_result(&ProbeTargetKey) -> Result<Option<ProtocolCompatibilityRecord>, AppError>
Database::save_reasoning_manual_override(&ManualReasoningOverride, expected_revision: i64) -> Result<i64, AppError>
Database::clear_reasoning_manual_override(&ProbeTargetKey, expected_revision: i64) -> Result<i64, AppError>
Database::prune_protocol_compatibility_results(now: i64) -> Result<u64, AppError>
```

- [ ] **Step 1: Add failing migration tests** from schema v16 asserting both new tables, indexes, backup inclusion, 30-day verified retention, 7-day failure retention, and no automatic deletion of manual overrides.
- [ ] **Step 2: Add failing DAO tests** for exact target lookup, replacement only for the same target, endpoint mismatch miss, revision conflict, transactional override update, and serialized-evidence redaction.
- [ ] **Step 3: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
cargo test --manifest-path src-tauri/Cargo.toml schema_migration --lib
```

- [ ] **Step 4: Bump `SCHEMA_VERSION` and implement the two dedicated tables/DAO.** Use the existing migration savepoint convention. Add both tables to backup export/import, startup pruning, and database tests; do not alter `stream_check_logs`.
- [ ] **Step 5: Run targeted migration/DAO tests and commit.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
cargo test --manifest-path src-tauri/Cargo.toml schema_migration --lib
git add src-tauri/src/database
git commit -m "feat(codex): persist protocol compatibility profiles" -m "本次提交由BigStrongsSun完成"
```

### Task 3: Capture and classify real upstream response shapes

**Files:**
- Create: `src-tauri/src/protocol_compatibility/capture.rs`
- Create: `src-tauri/src/protocol_compatibility/classify.rs`
- Modify: `src-tauri/src/protocol_compatibility/mod.rs`
- Test: module tests in `capture.rs` and `classify.rs`

**Interfaces:**

```rust
pub async fn capture_transport_probe(...) -> Result<CapturedProbeExchange, ProbeError>
pub fn classify_reasoning_shape(exchange: &CapturedProbeExchange) -> ClassifiedReasoningShape
```

- [ ] **Step 1: Write fixtures and failing classifier tests** for streaming Qwen/vLLM `delta.reasoning_content`, `delta.reasoning` string/object, `reasoning_details`, `<think>` split across UTF-8 chunks, summary-only fields, empty fields, summary+raw mixed fields, opaque/encrypted fields, and no reasoning. Add the captured Qwen shape: nonempty `delta.reasoning_content`, then nonempty ordinary `delta.content` before a `tool_calls` delta. Assert `pre_tool_visible_content=Present`, while reasoning source/semantic are derived only from the reasoning field.
- [ ] **Step 2: Add failing capture tests** using a local `axum` fixture upstream to prove SSE framing, data-only events, `[DONE]`, non-streaming JSON, HTTP failure, timeout, and that capture never returns raw body text in `Debug` or persisted evidence.
- [ ] **Step 3: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::capture --lib
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::classify --lib
```

- [ ] **Step 4: Implement byte-safe SSE capture and semantic classifier.** Classification may produce `Readable` only from verified readable reasoning paths; mixed or opaque data is `Summary`/`Opaque`, never raw. Capture ordinary nonempty `content` that precedes a tool call only as `PreToolVisibleContent::{Absent,Present}` plus redacted structural evidence; it must not be offered to `ReasoningSource`. Keep raw bytes in a local non-serializable buffer that is dropped after the probe.
- [ ] **Step 5: Run GREEN tests and commit.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::capture --lib
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::classify --lib
git add src-tauri/src/protocol_compatibility
git commit -m "feat(codex): classify protocol response shapes" -m "本次提交由BigStrongsSun完成"
```

### Task 4: Implement forced-tool and history-replay compatibility transactions

**Files:**
- Create: `src-tauri/src/protocol_compatibility/runner.rs`
- Modify: `src-tauri/src/protocol_compatibility/mod.rs`
- Test: `runner.rs` module tests

**Interfaces:**

```rust
pub async fn run_protocol_compatibility_probe(candidate: ProbeCandidate, client: &Client) -> ProtocolCompatibilityRecord
```

- [ ] **Step 1: Write failing local-upstream tests** for endpoint/transport fallback plus all four response outcomes: full verified readable profile; summary-only profile; first-turn raw succeeds but forced-tool continuation fails; tool choice unsupported. Add a Qwen-style tool round where `reasoning_content` and ordinary `content` precede the forced call; assert the run records `PreToolVisibleContent::Present` without changing the selected reasoning semantic or replay policy. Fixtures must assert the tool is named `ccsm_protocol_compatibility_probe`, takes a fixed `nonce`, and returns fixed JSON without touching filesystem/network.
- [ ] **Step 2: Add tests proving the runner constructs the second request through production `responses_to_chat_completions_with_reasoning*` code, preserves tool call order, and records `tool_turn_verified=false` when the second upstream request is rejected or ends before a continuation message.
- [ ] **Step 3: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::runner --lib
```

- [ ] **Step 4: Implement endpoint negotiation plus the four-stage transaction.** Reuse the legacy URL derivation rules but replace the duplicate minimal HTTP request functions; use short bounded timeouts; report unsupported tool forcing separately from failed transport; call the same pure conversion functions that the proxy uses; persist only the final profile/evidence after all stages complete.
- [ ] **Step 5: Run runner and Chat-transformer regressions; commit.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility::runner --lib
cargo test --manifest-path src-tauri/Cargo.toml transform_codex_chat --lib
git add src-tauri/src/protocol_compatibility
git commit -m "feat(codex): verify protocol tool-turn replay" -m "本次提交由BigStrongsSun完成"
```

### Task 5: Resolve a request-local projection and replace global raw behavior

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/providers/codex_chat_common.rs`
- Modify: `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: module tests in all four provider files and handler tests

**Interfaces:**

```rust
pub fn resolve_reasoning_projection(provider: &Provider, request: &Value, db: &Database) -> ReasoningProjection
pub fn create_responses_sse_stream_from_chat_with_context_and_projection(..., projection: ReasoningProjection) -> ...
pub fn chat_completion_to_response_with_context_and_projection(..., projection: ReasoningProjection) -> Result<Value, ProxyError>
```

- [ ] **Step 1: Write failing tests** showing a verified Qwen target emits `response.reasoning_text.delta` and final `content:[reasoning_text]`; a verified summary target emits summary SSE and final summary; unknown target never emits raw; original `outputFormat` alone does not select semantic output. Add a Qwen-style reasoning-plus-pre-tool-content fixture: ordinary pre-tool `content` emits only `response.output_text.*`; its presence does not change the selected reasoning SSE family or replay input.
- [ ] **Step 2: Add failed-path tests** proving raw/summary Responses history both replay as Chat `reasoning_content` only when `HistoryReplay::ChatReasoningContent`; opaque and official encrypted content remain untouched; V2 commentary/tool-call merge remains ordered.
- [ ] **Step 3: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml streaming_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml transform_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml codex_chat_common --lib
```

- [ ] **Step 4: Thread immutable projection through the handler and both conversion paths.** Replace `d25ebe31`'s unconditional `reasoning_text` emission. `extract_reasoning_field_text` remains a source extractor only; it cannot decide semantic type. Do not access SQLite inside the stream state machine; resolve once at request setup.
- [ ] **Step 5: Run focused bridge regressions and commit.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml streaming_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml transform_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml openai_compat --lib
git add src-tauri/src/proxy
git commit -m "fix(codex): project reasoning from verified compatibility evidence" -m "本次提交由BigStrongsSun完成"
```

### Task 6: Run preflight on Provider save and observe runtime drift

**Files:**
- Create: `src-tauri/src/commands/protocol_compatibility.rs`
- Create: `src-tauri/src/protocol_compatibility/runtime_observer.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: command, provider-service, observer and handler tests

**Interfaces:**

```rust
pub async fn probe_provider_candidate(candidate: ProbeCandidate, client: &Client) -> ProtocolCompatibilityRecord
pub fn observe_protocol_profile_shape(profile: &VerifiedProtocolCompatibilityProfile, shape: &ObservedResponseShape) -> ProfileObservation
probe_codex_protocol_compatibility(request) -> ProtocolCompatibilityResult
get_codex_protocol_compatibility(target) -> ProtocolCompatibilityInspection
plan_codex_reasoning_override(request) -> OverridePlan
apply_codex_reasoning_override(request) -> CompatibilityInspection
clear_codex_reasoning_override(request) -> CompatibilityInspection
```

- [ ] **Step 1: Write failing provider-save tests** proving `ProviderService::add` and `ProviderService::update` compile the unsaved effective route/model candidate before live config publication; a verified preflight persists Provider plus profile atomically; an unavailable/unsupported upstream saves the Provider as `Partial/Unverified` with no automatic raw projection; and endpoint, credential, route, transport or effective-model changes invalidate the old profile.
- [ ] **Step 2: Write failing observer tests** for matching Qwen structural shape, a changed reasoning field path, a changed event order, and `PreToolVisibleContent` drift. Assert no raw body is accepted by the observer and mismatch invalidates the existing profile without issuing upstream traffic.
- [ ] **Step 3: Write failing command tests** for missing candidate, concurrent probe rejection, redacted result serialization, no raw evidence in errors, revision conflict, invalid override, and manual override precedence.
- [ ] **Step 4: Run RED tests.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
cargo test --manifest-path src-tauri/Cargo.toml provider_service --lib
cargo test --manifest-path src-tauri/Cargo.toml handlers --lib
```

- [ ] **Step 5: Implement the save boundary.** Build `ProbeCandidate` from the in-memory Provider draft using the same MultiRouter route/model resolution used by production requests; run one bounded active probe before publication. A verified result commits the Provider, profile and live projection together. A failed/unsupported result commits the Provider only with `Partial/Unverified`; never make raw projection selectable from that state. Preserve the existing `add/update` return contract until a later frontend task consumes the richer inspection command.
- [ ] **Step 6: Implement `runtime_observer`.** At the Chat→Responses boundary, pass only field paths, event kinds/order, counts, lengths, hashes and `PreToolVisibleContent` to it. On mismatch, atomically expire the profile and select the safe fallback for subsequent requests; it must not call `reqwest`, inspect body text, or re-probe.
- [ ] **Step 7: Implement thin Tauri commands over the domain service.** `probe` is an explicit re-test command; `get` is read-only; override `plan/apply/clear` use the database revision transaction. Do not add TypeScript API wrappers or UI imports.
- [ ] **Step 8: Run command/domain tests and commit.**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
cargo check --manifest-path src-tauri/Cargo.toml --lib
git add src-tauri/src/protocol_compatibility src-tauri/src/services/provider src-tauri/src/commands src-tauri/src/proxy/handlers.rs src-tauri/src/lib.rs
git commit -m "feat(codex): preflight provider protocol compatibility" -m "本次提交由BigStrongsSun完成"
```

### Task 7: Privacy, migration and cross-boundary regression gate

**Files:**
- Modify: targeted tests only
- Modify: `memory.md` after real results exist

- [ ] **Step 1: Add end-to-end fixture tests** that run Chat SSE capture → profile classification → projection → Responses-to-Chat replay, asserting readable Qwen yields multi-delta raw reasoning, summary gateway retains summary, a Qwen tool round can record `pre_tool_visible_content=Present` without becoming raw reasoning, and every persisted row lacks test prompt/reasoning/tool text.
- [ ] **Step 2: Add regression fixtures** for official OAuth encrypted reasoning, native third-party Responses normalization, V2 `message.encrypted` stripping, and commentary + tool-call merging.
- [ ] **Step 3: Run verification.**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml protocol_compatibility --lib
cargo test --manifest-path src-tauri/Cargo.toml streaming_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml transform_codex_chat --lib
cargo test --manifest-path src-tauri/Cargo.toml openai_compat --lib
cargo test --manifest-path src-tauri/Cargo.toml forwarder --lib
cargo test --manifest-path src-tauri/Cargo.toml handlers --lib
git diff --check
```

- [ ] **Step 4: Record actual test results in root `memory.md`; validate UTF-8 no-BOM/no U+FFFD; commit.**

```powershell
git add memory.md src-tauri/src
git commit -m "test(codex): verify protocol compatibility probe boundaries" -m "本次提交由BigStrongsSun完成"
```

## Deferred Frontend Slice

Only after Tasks 1-7 pass, add a model-row status card that calls `get`, an explicit **re-test** button for failed or expired profiles, result/failure-stage display, and an advanced editor that calls plan/apply/clear. The ordinary save flow remains the sole default probe trigger. The UI must not contain its own classifier or raw/summary default logic.

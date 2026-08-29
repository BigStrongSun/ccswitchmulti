# Codex Protocol Evidence and User-Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist Chat and Responses probe evidence independently, preserve it across an explicit advanced protocol override, and make probe/production User-Agent policy identical.

**Architecture:** Keep the existing Provider Set selection receipt and executable profile path unchanged, while adding transport-specific diagnostic observations keyed by exact request identity. Treat selector output as the automatic recommendation and Provider manual metadata as the final user choice. Route both probe and production headers through one deterministic third-party UA policy.

**Tech Stack:** Rust/Tauri, SQLite/rusqlite, React/TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-08-30-codex-protocol-evidence-and-user-agent-design.md`

## Global Constraints

- Do not let observations drive automatic Provider Set grouping or runtime transport selection.
- Do not delete or rewrite the unselected protocol observation when the user overrides the final protocol.
- Partial/Unverified branches remain diagnostic only.
- Preserve existing Provider Set atomicity and the legacy-only dynamic transport boundary.
- Preserve unrelated dirty-worktree changes; stage only task-owned hunks.

---

### Task 1: Transport-specific observation schema and DAO

**Files:**
- Modify: `src-tauri/src/database/mod.rs`
- Modify: `src-tauri/src/database/schema.rs`
- Modify: `src-tauri/src/database/dao/protocol_compatibility.rs`
- Test: `src-tauri/src/database/tests.rs`

**Interfaces:**
- Produces: `save_protocol_probe_observations(&[ProtocolCompatibilityRecord])`, `get_protocol_probe_observation(&ProbeTargetKey)`, and provider-scoped listing/deletion.

- [ ] Write RED migration and round-trip tests proving v18→v19 creates the observation table and Chat/Responses rows cannot overwrite each other.
- [ ] Run the focused database tests and verify failure is the missing v19 schema/API.
- [ ] Add the v19 table, migration, exact-target DAO, and logical-source deletion.
- [ ] Rerun the focused database tests to GREEN.

### Task 2: One probe transaction emits two persisted observations

**Files:**
- Modify: `src-tauri/src/commands/protocol_compatibility.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/protocol-compatibility.ts`
- Test: command module tests.

**Interfaces:**
- Consumes: one full `ProtocolCompatibilityProbeResult` and its `ProbeCandidate`.
- Produces: one selection receipt plus two branch-specific observations, each with an exact transport target and branch readiness.

- [ ] Write RED tests for Responses-recommended/Chat-Verified and Responses-Verified/Chat-Partial results.
- [ ] Verify RED fails because only the selected target is materialized.
- [ ] Implement observation materialization, atomic persistence, listing command, and preflight response field.
- [ ] Verify Provider Set planner still sees exactly one selection record per model.
- [ ] Rerun command, protocol, Provider Set, and DAO tests to GREEN.

### Task 3: Deterministic probe/production User-Agent policy

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex_request.rs`
- Create: `src-tauri/src/proxy/providers/codex_request_user_agent_tests.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`

**Interfaces:**
- Produces: `effective_third_party_user_agent` used by both `prepare_headers` and `apply_provider_header_policy`.

- [ ] Write RED tests proving no-custom-UA requests carry the CCSwitchMulti product UA, custom UA wins, and changing UA changes the candidate request fingerprint.
- [ ] Run the focused tests and verify the missing default/parity failures.
- [ ] Add the shared priority resolver and bump the request preparer version.
- [ ] Rerun request-policy, probe equivalence, and forwarder tests to GREEN.

### Task 4: Advanced final protocol override without evidence loss

**Files:**
- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/CodexProtocolAdvancedSettings.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/lib/api/protocol-compatibility.ts`
- Test: `tests/components/CodexFormFields.test.tsx`
- Test: `tests/components/AddProviderDialog.test.tsx`
- Test: `tests/components/EditProviderDialog.test.tsx`
- Test: `src/components/providers/forms/CodexProtocolAdvancedSettings.test.tsx`

**Interfaces:**
- Consumes: automatic recommendation, two observations, manual/auto mode, final `apiFormat`, receipt IDs.
- Produces: a manual Chat/Responses Provider payload that retains current receipts and never rewrites the recommendation.

- [ ] Write RED tests for “probe recommends Responses → switch manual → choose Chat → submit keeps receipt and Chat final choice”.
- [ ] Write RED identity tests proving final protocol/mode changes retain evidence while UA/URL/header/body/model changes invalidate it.
- [ ] Remove final protocol/model-row protocol from probe identity, keep request-affecting fields, and submit nonempty receipts in either mode.
- [ ] Make advanced wording distinguish automatic recommendation from final user override.
- [ ] Rerun focused component tests and typecheck to GREEN.

### Task 5: Verification, memory, and selective commit

**Files:**
- Update: `memory.md`
- Update: this plan's checkboxes with current evidence.

- [ ] Run focused Rust suites for database, protocol compatibility, Provider Set, request policy, proxy/forwarder, and Provider commands.
- [ ] Run full Rust library, check, clippy, and rustfmt.
- [ ] Run focused and full Vitest, typecheck, format check, and renderer build.
- [ ] Strictly decode changed text as UTF-8 without BOM/U+FFFD and run `git diff --check`.
- [ ] Rebuild an exact staged candidate tree so unrelated dirty-worktree changes are excluded from verification.
- [ ] Update `memory.md`, stage only owned files/hunks, and commit locally with the required attribution line.

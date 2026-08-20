# Codex MultiRouter Provider SSOT Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Provider and provider model entries the only authoritative source for MultiRouter upstream protocol, connection, authentication material, and capabilities.

**Architecture:** Add a versioned declarative route schema and a shared Rust compiler/resolver. Provider mutations atomically rebuild dependent projections; runtime materialization always uses the current target Provider and canonical model, while route state contains only routing policy and secret-free auth references.

**Tech Stack:** Rust/Tauri, rusqlite, serde/serde_json, TypeScript/React, Vitest, Cargo tests.

**Spec:** `docs/superpowers/specs/2026-08-20-codex-multirouter-provider-ssot-design.md`

## Global Constraints

- Use TDD for every behavior change and observe RED before production edits.
- Route v2 must not persist API keys, tokens, Base URLs, inherited protocol, or model capabilities.
- Runtime, catalog projection, diagnostics, and migration must share one Rust compiler.
- Preserve v1 read compatibility and explicit historical behavior through previewed migration.
- All repository text remains UTF-8 without BOM; use `apply_patch` for edits.
- Each independently testable phase ends in a local Git commit whose message ends with `本次提交由BigStrongsSun完成`.

---

### Task 1: Define and validate schema v2 contracts

**Files:**
- Modify: `src/types.ts`
- Create: `src-tauri/src/codex_multirouter/mod.rs`
- Create: `src-tauri/src/codex_multirouter/schema.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/codex_multirouter/schema.rs`

**Interfaces:**
- Produce `CodexRoutingConfigV2`, `CodexRoutingRouteV2`, `CodexModelSelection`, `CodexRouteAuthPolicy`, and tolerant `CodexRoutingDocument::parse(&Value)`.
- Produce `validate_v2(plan, providers) -> Result<(), Vec<CodexRoutingValidationIssue>>` with stable issue codes.

- [ ] Write Rust tests for `all`, `include`, alias target validation, duplicate route IDs, missing target Provider, and rejection of secret/connection/capability fields.
- [ ] Run `cargo test --lib codex_multirouter::schema::tests --no-fail-fast` and verify the new tests fail because the module/types do not exist.
- [ ] Implement serde contracts and validation; retain a tolerant `Legacy(Value)` parse result without mutating it.
- [ ] Add matching TypeScript v2 types while retaining v1 compatibility types behind a union.
- [ ] Re-run schema tests and `pnpm typecheck`; commit schema contracts.

### Task 2: Build the single compiler and dependency fingerprint

**Files:**
- Create: `src-tauri/src/codex_multirouter/compiler.rs`
- Modify: `src-tauri/src/codex_multirouter/mod.rs`
- Test: `src-tauri/src/codex_multirouter/compiler.rs`

**Interfaces:**
- Consume a v2 plan plus `HashMap<String, Provider>`.
- Produce `CompiledCodexRoutingPlan { routes, visible_models, model_catalog, dependency_fingerprint, warnings }`.
- Produce `CompiledCodexModel { visible_model, canonical_model, target_provider_id, route_id, api_format, api_format_source, capability_summary }`.

- [ ] Write RED tests for Provider protocol inheritance, model-level protocol override, `all` auto-inclusion, `include` stability, deterministic aliases, exact collision handling, per-model modalities/reasoning/cache, and fingerprint changes.
- [ ] Implement canonical JSON normalization and SHA-256 fingerprinting over only referenced Provider/model fields.
- [ ] Implement deterministic compilation and catalog projection without reading route v1 snapshots.
- [ ] Verify compiler tests, idempotent output under map ordering changes, and no secret values in compiled/debug serialization; commit compiler.

### Task 3: Switch runtime resolution and materialization to the compiler

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: existing Rust tests in those modules

**Interfaces:**
- Add a state-aware resolver that loads current target Providers, compiles/fingerprint-checks the plan, and returns `ResolvedCodexRoute`.
- Materialization starts from the current target Provider, applies the selected canonical model and secret-free auth policy, then resolves protocol/capabilities from model > Provider.

- [ ] Write RED runtime tests proving a Provider Chat↔Responses edit changes the next request without plan mutation and mixed models on one Provider use different protocols.
- [ ] Add RED tests for exact > prefix > default, aliases, Desktop OAuth, managed OAuth, account pool, text/image conversion, reasoning, and cache resolution.
- [ ] Replace v2 route snapshot reads with compiled resolution; keep the existing v1 path read-only behind the schema discriminator.
- [ ] Remove route capability/protocol precedence from v2 materialization while preserving v1 behavior.
- [ ] Run focused proxy/handler/forwarder tests and commit runtime SSOT.

### Task 4: Add projection storage, diagnostics, and read-back proof

**Files:**
- Create: `src-tauri/src/codex_multirouter/projection.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src/lib/api/providers.ts`

**Interfaces:**
- Produce projection status with fingerprint, generated timestamp, pending state, warnings, and secret-free resolved route summaries.
- Add commands to inspect and retry projection generation; neither command returns credentials.

- [ ] Write RED tests for fingerprint mismatch rebuild, catalog/cache atomic write, pending status on injected file failure, retry, and DB/resolver/file read-back agreement.
- [ ] Reuse the existing atomic catalog/cache writers and persist projection metadata separately from authoritative route declarations.
- [ ] Add diagnostic command/API types and ensure redaction tests cover nested auth/config fields.
- [ ] Run codex config, projection, command, and TypeScript tests; commit projection/diagnostics.

### Task 5: Centralize Provider mutations and dependent plan updates

**Files:**
- Create: `src-tauri/src/services/provider/codex_multirouter.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/database/dao/providers.rs`
- Modify: `src/lib/query/mutations.ts`

**Interfaces:**
- Add one backend mutation coordinator for update, rename, refresh, plan save, and delete.
- Return affected plan IDs, removed spawn candidates, projection status, and warnings.

- [ ] Write RED transaction tests for Provider update, rename rewiring, multiple dependent plans, rollback on projection DB failure, and optimistic revision conflict.
- [ ] Add DAO transaction helpers that accept an existing rusqlite transaction rather than nesting per-provider saves.
- [ ] Move dependent plan compilation/persistence into the backend coordinator.
- [ ] Remove frontend loops that call `syncCodexMultiRouterPlanWithProviders` and save plans one by one.
- [ ] Verify mutation hook tests and backend transaction tests; commit centralized mutations.

### Task 6: Implement cascade deletion and empty-plan safety

**Files:**
- Modify: `src-tauri/src/services/provider/codex_multirouter.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Test: provider service and takeover tests

**Interfaces:**
- Deleting a referenced Provider removes all matching routes in the same transaction.
- Empty plans are retained disabled with no default route/catalog; active takeover restoration is a precondition to commit.

- [ ] Write RED tests for one/many plans, partial route removal, spawn-candidate pruning, default-route reassignment, and last-route removal.
- [ ] Add failure-injected tests proving official config restore failure leaves Provider/routes unchanged.
- [ ] Implement backup-first restore coordination and transactional cascade; expose affected-plan details to the UI.
- [ ] Run deletion, takeover, provider service, and catalog tests; commit lifecycle integrity.

### Task 7: Implement previewed, idempotent v1→v2 migration

**Files:**
- Create: `src-tauri/src/codex_multirouter/migration.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src/lib/api/providers.ts`
- Test: `src-tauri/src/codex_multirouter/migration.rs`

**Interfaces:**
- `preview_codex_multirouter_migration(provider_id, expected_revision)` returns redacted diff, warnings, `planToken`, and generated Provider summaries.
- `apply_codex_multirouter_migration(provider_id, expected_revision, plan_token)` verifies the exact preview and applies atomically.

- [ ] Write RED tests for equal-value inheritance, differing-value preservation, DeepSeek model protocol split, Qwen stale protocol, inline credentials extraction, conflicting-route Provider clones, alias classification, all/include selection, and idempotence.
- [ ] Implement canonical plan hashing and expiring in-memory plan tokens bound to provider/revision/diff.
- [ ] Implement migration-generated Provider cloning without including secrets in preview/log output.
- [ ] Add conflict/error codes for stale revision, stale token, missing target, and ambiguous legacy state.
- [ ] Run migration/security tests and commit migration.

### Task 8: Replace route editor and wizard persistence with v2 policy UI

**Files:**
- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/codex/CodexMultiRouterWizard.tsx`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx`
- Test: their existing Vitest suites

**Interfaces:**
- Route editor exposes model selection, aliases, prefixes, enabled/order/label, target Provider, and auth policy only.
- Provider/model links open the authoritative editor for protocol, endpoint, credentials, and capabilities.

- [ ] Write RED component tests that forbidden inherited fields are absent, `all/include` persists correctly, aliases validate against canonical models, and auth policies contain no secrets.
- [ ] Add migration preview/apply UI shown before first edit/enable of v1 plans, including warnings and generated Provider summaries.
- [ ] Convert wizard and workspace plan creation to schema v2; remove TS route snapshot builders/sync helpers once unused.
- [ ] Run focused component/lib tests and typecheck; commit v2 UI.

### Task 9: Documentation, complete verification, and Mac canary

**Files:**
- Modify: `README.md`
- Modify: `docs/guides/codex-multirouter-guide-zh.md`
- Modify: `memory.md`
- Add the release note only when a release version is selected after acceptance.

- [ ] Document ownership, migration, projection diagnostics, cascade deletion, and restart requirements.
- [ ] Strictly decode all changed text as UTF-8 and verify no BOM/U+FFFD was introduced.
- [ ] Run full frontend tests, `pnpm typecheck`, full `cargo test --lib`, `cargo fmt --check`, Prettier check, and `git diff --check`.
- [ ] Build an installable artifact and deploy transactionally to the affected Mac with rollback retained.
- [ ] On the Mac, change Qwen Chat→Responses and Responses→Chat without recreating the route; verify matching `route_resolved`, `request_prepared`, `effective_endpoint`, conversion flags, upstream status, and response completion for the same trace.
- [ ] Update `memory.md` with final architecture, migration, test counts, canary evidence, unresolved limitations, and commit IDs; commit acceptance documentation.

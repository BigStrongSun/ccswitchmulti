# Codex Mixed-Protocol Provider Set Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Every production change follows RED → GREEN → focused regression. Do not use subagents in this dirty shared worktree.

**Goal:** Persist every automatically probed Codex logical source as either one protocol-homogeneous Provider or a stable MultiRouter facade with two protocol-homogeneous leaves, while atomically preserving profiles, dependent routers, Universal sources, active state, Provider save validation, and wizard behavior.

**Architecture:** Add a backend-owned Provider Set planner and mutation layer. The frontend submits one logical Provider draft and matching probe receipt identities; it never writes per-model protocols or saves leaves itself. Mixed Verified selections become a schema-v2 facade at the original ID plus deterministic Chat/Responses leaves. Uniform selections remain one ordinary Provider. Partial/Failed/stale selections produce a zero-write Blocked plan. Request-time per-model protocol resolution remains only for explicitly detected legacy mixed data.

**Tech Stack:** Rust/Tauri, SQLite transaction DAO, serde/serde_json, React/TypeScript, Vitest, existing schema-v2 MultiRouter compiler and protocol compatibility profiles.

**Spec:** `docs/superpowers/specs/2026-08-29-codex-mixed-protocol-provider-split-design.md`

## Global constraints

- Work on current `main`; do not switch branches or reset unrelated dirty changes.
- Stage only task-owned hunks. Existing protocol hardening changes remain intact and unstaged unless a task explicitly owns them.
- Do not infer protocol by model name, URL, provider name, or HTTP 200.
- Do not persist model-level `apiFormat` on new or edited ordinary Providers.
- Partial/Failed/Unavailable/stale probe facts never become executable profiles or groups.
- All key commits end with `本次提交由BigStrongsSun完成`.
- Do not push, publish, install, restart, or replace the current application without separate authorization.

---

### Task 1: Provider Set domain model and pure planner

**Files:**

- Create: `src-tauri/src/codex_multirouter/provider_set.rs`
- Modify: `src-tauri/src/codex_multirouter/mod.rs`
- Reuse: `src-tauri/src/protocol_compatibility/mod.rs`
- Reuse: `src-tauri/src/provider.rs`

**RED tests:**

- Same model with both branches Verified enters only selector-selected transport.
- All selected Responses returns `Single(Responses)` and clears model-level protocol fields.
- All selected Chat returns `Single(Chat)` and clears model-level protocol fields.
- Mixed Verified returns original-ID facade plus deterministic Responses/Chat leaves.
- A Partial, Failed, Unavailable, stale, missing, duplicate, or selection-less enabled model returns `Blocked`.
- Disabled models do not participate in grouping.
- Existing unowned deterministic leaf ID returns `codex_provider_set_leaf_id_conflict`.
- Existing owned leaves are reused only when marker parent/transport/version match.
- Facade plus leaves restore the original logical draft and authoritative source catalog.

**GREEN implementation:**

- Add `CodexProviderSetPlan`, preview, blocked model, generated marker, prepared mutation, and deterministic-ID helpers.
- Normalize model identity using the same public/upstream model functions as probe compilation.
- Validate one current Verified record and one selected transport per enabled model.
- Build protocol-homogeneous catalog copies and top-level protocol/TOML values.
- Build schema-v2 facade routes without Router→Router.
- Compute a secret-safe canonical digest over normalized draft, selected records, revisions, generated IDs, and plan.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_multirouter::provider_set --no-default-features
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

**Commit:** domain model and planner only.

---

### Task 2: Atomic Provider Set DAO and rollback semantics

**Files:**

- Modify: `src-tauri/src/database/dao/providers.rs`
- Modify as needed: `src-tauri/src/database/dao/protocol_compatibility.rs`
- Modify as needed: `src-tauri/src/database/dao/universal_providers.rs`
- Test in the same Rust modules.

**RED tests:**

- Upserting source then failing second leaf rolls back source, first leaf, profiles, settings cleanup, and `is_current`.
- Mixed commit stores only Verified profiles and binds each profile to the correct leaf target.
- Split→uniform deletes only owned leaves and their executable profiles.
- Current source stays current after split; current owned leaf collapses to source on uniform plan.
- Dependent Router update and Universal definition are in the same transaction.
- An injected Universal child failure rolls back all applications and the definition.

**GREEN implementation:**

- Generalize `apply_provider_set_with_protocol_profiles_setting_cleanup_and_universal_upsert` to accept upserts, owned deletes, dependent Router replacements, setting cleanup, current-state transition, and optional Universal mutation as one transaction input.
- Validate every profile belongs to one Provider in the committed set.
- Validate deletes carry matching generated ownership metadata.
- Keep live/settings publication outside the transaction as a derived, retryable projection.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib database::dao::providers --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib database::dao::universal_providers --no-default-features
```

**Commit:** DAO transaction and rollback coverage only.

---

### Task 3: Dependent MultiRouter expansion and split→uniform folding

**Files:**

- Modify: `src-tauri/src/codex_multirouter/provider_set.rs`
- Modify: `src-tauri/src/codex_multirouter/mutation.rs`
- Modify as needed: `src-tauri/src/codex_multirouter/compiler.rs`
- Test: existing module test blocks.

**RED tests:**

- `mode=all` route targeting source expands to two leaf routes on split.
- `mode=include` partitions canonical models and aliases; empty partitions are omitted.
- Route enabled/priority/fallback/match/user label survive expansion.
- Deterministic generated route IDs update `defaultRouteId` when unambiguous.
- Ambiguous default mapping blocks the whole Provider Set mutation.
- A Router targeting an existing facade is flattened to leaves; persisted Router→Router is rejected.
- split→uniform folds only generated leaf routes back to original source and removes generated duplicates.

**GREEN implementation:**

- Add pure dependency-rewrite helpers driven by before/after Provider Set shape.
- Extend prepared mutation to carry dependent Router revisions and replacements.
- Revalidate dependency revisions immediately before DAO commit.
- Reuse existing projection finalization after the database transaction.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_multirouter::mutation --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_multirouter::compiler --no-default-features
```

**Commit:** dependency migration and folding.

---

### Task 4: Provider prepare/commit commands and save validation

**Files:**

- Modify: `src-tauri/src/commands/protocol_compatibility.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/protocol-compatibility.ts`
- Modify: `src/lib/api/providers.ts`
- Modify: `src/types.ts`

**RED tests:**

- Ordinary auto save with no current receipt returns `codex_provider_set_probe_required` and performs zero writes.
- A stale receipt/digest returns `codex_provider_set_probe_stale`.
- Uniform Verified auto save commits Single without confirmation.
- Mixed Verified prepare returns Split preview but does not write until explicit split intent.
- Mixed commit with changed draft/revision/digest fails without writing.
- Partial/Failed record returns structured Blocked models and does not preserve an old protocol as if successful.
- Advanced manual mode permits one whole-Provider protocol and rejects model-level mixed protocol.
- New add and edit use the same prepare/commit path.

**GREEN implementation:**

- Add serializable prepare request/preview, commit intent, commit request/outcome commands.
- Make ordinary `add_provider`/`update_provider` delegate to Provider Set validation for Codex.
- Replace `resolve_automatic_probe_outcome` fallback-on-probe-error behavior with fail-closed normal mode; keep explicit manual bypass only for advanced single protocol.
- Consume one-shot receipt only during successful prepare/commit matching and rebind draft identity safely.
- Return `committed_with_projection_error` when DB commit succeeded but live projection needs retry.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib commands::provider --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib commands::protocol_compatibility --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib services::provider --no-default-features
pnpm vitest run src/lib/codexProtocolSettings.test.ts
```

**Commit:** backend command boundary and TypeScript contracts.

---

### Task 5: Universal Provider integration

**Files:**

- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src/components/universal/UniversalProviderFormModal.tsx`
- Modify: `src/components/universal/UniversalProviderPanel.tsx`
- Test: `tests/components/UniversalProviderFormModal.test.tsx`
- Test: `tests/components/UniversalProviderPanel.test.tsx`

**RED tests:**

- Universal preview exposes the Codex child Single/Split/Blocked result.
- Split Codex child and Claude/Gemini children commit in one transaction.
- Any blocked Codex model or child validation failure yields zero writes.
- Universal source identity remains stable; generated Codex leaves record the Codex parent, not a new Universal source.
- Editing a split Universal restores the logical Codex source catalog.

**GREEN implementation:**

- Prepare all Universal child mutations before writing.
- Feed the Codex child through Provider Set planner/commit.
- Remove sequential child-save behavior from Universal UI/backend.
- Reuse the same Split/Blocked confirmation state returned by backend.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib universal --no-default-features
pnpm vitest run tests/components/UniversalProviderFormModal.test.tsx tests/components/UniversalProviderPanel.test.tsx
```

**Commit:** Universal atomic Provider Set integration.

---

### Task 6: Provider UI ordinary/advanced save flow

**Files:**

- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/CodexProtocolProbeProgressDialog.tsx`
- Create: `src/components/providers/forms/CodexProviderSetPreviewDialog.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/components/providers/AddProviderDialog.tsx`
- Modify as needed: Provider edit/list components.
- Test: `tests/components/CodexFormFields.test.tsx`
- Test: `tests/components/AddProviderDialog.test.tsx`
- Create: matching preview dialog test.

**RED tests:**

- Auto save with stale/no probe opens deep-probe progress and does not call add/update.
- Single preview saves directly.
- Split preview lists Responses and Chat groups and exposes only `返回调整模型` and `确认按协议拆分`.
- Confirm calls one backend commit, never two `onSubmit` calls.
- Editing model/auth/URL/overrides after preview invalidates digest and confirmation.
- Blocked preview lists model/stage/category and only offers retry/return.
- Advanced mode selects one whole Provider protocol and cannot persist mixed model rows.
- Split facade appears as one logical source and opens an editable restored draft.

**GREEN implementation:**

- Remove all frontend writes to `modelCatalog.models[].apiFormat` from automatic results.
- Remove `buildSplitCodexProviderData`, `codexProviderSplit`, and sequential `onSubmit` behavior.
- Add one save coordinator that calls probe → prepare → optional confirm → commit.
- Reuse backend records/progress without compiling grouping semantics in TypeScript.
- Keep reasoning projection/tool schema/history replay controls under advanced mode and separate from Provider Set plan.

**Focused verification:**

```powershell
pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/AddProviderDialog.test.tsx src/components/providers/forms/CodexProtocolProbeProgressDialog.test.tsx src/components/providers/forms/CodexProviderSetPreviewDialog.test.tsx
pnpm typecheck
```

**Commit:** Provider save UX and split confirmation.

---

### Task 7: Configuration wizard reuse and batch transaction

**Files:**

- Modify: `src/components/codex/CodexMultiRouterWizard.tsx`
- Modify: `src/lib/codexMultiRouterWizard.ts`
- Modify: `src/lib/api/providers.ts`
- Test: `tests/components/CodexMultiRouterWizard.test.tsx`
- Test: `src/components/codex/CodexMultiRouterWizard.test.tsx`
- Test: `tests/lib/codexMultiRouterWizard.test.ts`
- Test: `src/lib/codexMultiRouterWizard.test.ts`

**RED tests:**

- Wizard never uses `model_fetch` reachability to choose protocol.
- Each logical source uses the same deep-probe progress and backend prepare result as Provider settings.
- Split source remains one logical source in source selection but final Router plan references leaves.
- Blocked source prevents final save and exposes targeted retry.
- Final save is one backend batch commit, not a `providersApi.update` loop.
- Batch failure leaves all sources and final Router unchanged.
- Success page still exposes history repair, Subagent, reasoning effort, and model ordering entries.

**GREEN implementation:**

- Delete protocol inference from shallow model fetch.
- Reuse Provider source configuration/readiness and Provider Set preview components.
- Add backend batch prepare/commit for all source Provider Sets plus final Router.
- Flatten split sources before final Router persistence and reject Router→Router.
- Preserve existing wizard navigation while splitting overloaded pages where required.

**Focused verification:**

```powershell
pnpm vitest run tests/components/CodexMultiRouterWizard.test.tsx src/components/codex/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts src/lib/codexMultiRouterWizard.test.ts
pnpm typecheck
```

**Commit:** wizard Provider Set integration and batch save.

---

### Task 8: Legacy mixed-data compatibility boundary

**Files:**

- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`
- Modify as needed: `src-tauri/src/services/provider/mod.rs`
- Modify as needed: Provider diagnostics/UI files.

**RED tests:**

- New/edited ordinary Provider with mixed model-level protocols is rejected before persistence.
- Normal Single or split leaf never uses request-time model-level transport resolution.
- Legacy mixed ordinary Provider may dynamically select transport only with an unexpired matching Verified profile.
- Missing/stale/mismatched profile falls back to top-level protocol and records migration-required diagnostics.
- No model-name inference occurs.
- Successful Provider Set migration removes all model-level protocol fields and disables fallback.

**GREEN implementation:**

- Add an explicit legacy-mixed detector and migration diagnostic.
- Gate `apply_detected_codex_transport_to_effective_provider` and compiled-model transport overrides behind that detector plus valid profile.
- Keep top-level protocol authoritative for every normalized Provider/leaf.
- Surface migration-required state to Provider settings.

**Focused verification:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::codex --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::forwarder --no-default-features
```

**Commit:** legacy-only dynamic protocol boundary.

---

### Task 9: Full verification, desktop QA, memory, and final commits

**Files:**

- Update: `memory.md`
- Update plan checkboxes with actual evidence.

**Requirement-by-requirement verification:**

- Map every explicit spec requirement to a focused test or desktop action.
- Run all protocol compatibility, Provider service/commands, DAO, MultiRouter, Universal, proxy, and forwarder focused suites.
- Run full Rust library, check, clippy, rustfmt.
- Run focused and full Vitest, typecheck, renderer build.
- Build the Windows application without installing or replacing the live app.
- Launch a development/test instance and use desktop interaction only for visual acceptance of Single, Split, Blocked, invalidated confirmation, split→uniform, wizard batch save, and success-page entries.
- Strictly decode all changed text as UTF-8, reject BOM and U+FFFD, then run `git diff --check`.
- Audit `git status`, staged diff, commit ancestry, and task-owned files before every commit.
- Update `memory.md` with root cause, architecture, migrations, exact test counts, desktop evidence, search channels, and remaining uncertainty.

**Commands:**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features
cargo check --manifest-path src-tauri/Cargo.toml --tests --no-default-features
cargo clippy --manifest-path src-tauri/Cargo.toml --tests --no-default-features -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --check
pnpm vitest run
pnpm typecheck
pnpm build:renderer
pnpm tauri build --debug
git diff --check
```

**Completion rule:** Do not claim completion until current source, tests, Windows build, and real Tauri UI evidence prove every spec item. Do not mark a queued build or a standalone Vite page as desktop acceptance.

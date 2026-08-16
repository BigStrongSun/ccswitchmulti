# Codex Preset Reasoning Capabilities Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make built-in CCSwitchMulti providers expose accurate per-model reasoning choices to Codex while allowing validated overrides for custom providers.

**Architecture:** Store a canonical reasoning capability on each `modelCatalog.models[]` row. Resolve that capability once from the effective provider and upstream model, then project it into catalog/inline model metadata and use it for Chat request conversion. Built-in presets ship maintained capabilities; custom providers edit the same schema.

**Tech Stack:** TypeScript/React, Rust/serde_json, Tauri, Vitest, Rust unit tests.

## Global Constraints

- Built-in presets are maintained compatibility adapters and must work without user edits.
- Unknown third-party models must not inherit GPT reasoning efforts.
- Catalog metadata and outbound conversion must use the same resolved capability.
- MultiRouter resolves against the materialized target provider and upstream model.
- Preserve existing providers through legacy `codexChatReasoning` fallback for one compatibility cycle.
- Do not modify the pre-existing untracked `codex_config_diff.txt`, `wizard_diff.txt`, or `workspacepage_diff.txt` files.

---

### Task 1: Shared schema and preset capabilities

**Files:**

- Modify: `src/types.ts`
- Modify: `src/config/codexProviderPresets.ts`
- Test: `src/config/codexProviderPresets.test.ts`

**Interfaces:**

- Produces `CodexModelReasoningCapability`, `CodexReasoningEffort`, and `CodexCatalogModel.reasoning`.
- Built-in catalog rows consume the schema directly.

- [ ] Add failing preset snapshot tests for DeepSeek V4, Grok 4.5, GLM-5.2, Step models, and boolean-only/no-effort models.
- [ ] Run the focused Vitest file and confirm missing `reasoning` assertions fail.
- [ ] Add the shared TypeScript schema and a preset helper that validates literal capability declarations.
- [ ] Declare per-model capabilities in maintained presets, including separate Step model rows.
- [ ] Run focused tests and `pnpm typecheck`.
- [ ] Commit the schema and preset capability increment.

### Task 2: Backend resolver and catalog projection

**Files:**

- Create: `src-tauri/src/proxy/providers/codex_reasoning.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Test: Rust unit tests colocated in the module and `codex_config.rs`.

**Interfaces:**

- Produces `resolve_codex_model_reasoning_capability(provider, model)`.
- Produces normalized supported/default/disable/upstream mapping data.
- Catalog generation consumes the JSON capability already present on each model row.

- [ ] Add failing resolver tests for exact model lookup, upstream alias lookup, absent capability, invalid defaults, and legacy fallback.
- [ ] Add failing catalog tests proving Grok lacks `none`, GLM defaults to `max`, Step models differ, and unknown models expose no inherited GPT efforts.
- [ ] Run focused Rust tests and confirm expected failures.
- [ ] Implement parsing, validation, source tagging, and legacy fallback in the focused resolver module.
- [ ] Replace DeepSeek-only catalog mutation with general capability projection and strip template reasoning fields when unresolved.
- [ ] Ensure inline TOML aliases derive only from projected catalog entries.
- [ ] Run focused and neighboring Rust tests.
- [ ] Commit the resolver/catalog increment.

### Task 3: Request conversion and MultiRouter preservation

**Files:**

- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src/lib/codexMultiRouterSync.ts`
- Modify: `src/lib/codexMultiRouterWizard.ts`
- Test: colocated Rust tests and existing MultiRouter Vitest suites.

**Interfaces:**

- Consumes the Task 2 resolver using the effective routed provider and mapped upstream model.
- Converts each visible effort through the capability `effortMap` and parameter format.

- [ ] Add failing request tests for GLM medium→high, Step 2603 high/low only, boolean-only thinking, and invalid effort rejection.
- [ ] Add failing MultiRouter tests showing a routed model keeps its `reasoning` object through sync/wizard generation.
- [ ] Run focused tests and confirm failures.
- [ ] Materialize capability metadata with routes and resolve after upstream model mapping.
- [ ] Build `CodexChatReasoningConfig` from the resolved capability before heuristic legacy inference.
- [ ] Make conversion mapping data-driven and reject an invalid declared effort instead of silently guessing.
- [ ] Run focused Rust and frontend MultiRouter tests.
- [ ] Commit the conversion/MultiRouter increment.

### Task 4: User editing and validation

**Files:**

- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/hooks/useCodexConfigState.ts`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Test: relevant Provider form Vitest files.

**Interfaces:**

- Edits `CodexCatalogModel.reasoning` for custom providers.
- Displays built-in values as maintained defaults and marks edited rows as user overrides.

- [ ] Add failing UI tests for rendering per-model efforts, changing defaults, validation, and restoring a preset value.
- [ ] Run focused UI tests and confirm failures.
- [ ] Add a compact per-row reasoning editor in the existing advanced catalog section.
- [ ] Validate default membership, `none`/disable consistency, and mapping completeness before save.
- [ ] Preserve reasoning objects in form normalization and model fetch merges.
- [ ] Run focused UI tests and typecheck.
- [ ] Commit the editing increment.

### Task 5: Verification and knowledge update

**Files:**

- Modify: `memory.md`

**Interfaces:**

- Records final architecture, commands, test counts, and any remaining evidence limits.

- [ ] Run focused preset, Provider form, MultiRouter, resolver, catalog, and conversion suites.
- [ ] Run the full Rust library suite, frontend suite excluding `.worktrees/**`, `cargo check --lib`, `pnpm typecheck`, changed-file formatting checks, and `git diff --check`.
- [ ] Audit every design completion criterion against current files and test evidence.
- [ ] Update `memory.md` with implementation facts and verification evidence.
- [ ] Commit the final knowledge/verification increment with the required attribution footer.

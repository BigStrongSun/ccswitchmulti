# Codex Provider Menu Projection And Routing Entry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hide the legacy route editor, force maintained presets to project their model catalog, keep an explained opt-out only for custom providers, and preserve all stored routing compatibility.

**Architecture:** Keep `codexRouting` parsing, state, persistence, backend resolution, and the MultiRouter workspace unchanged. Restrict the generic form to catalog/menu controls by disabling its legacy editor presentation, and change only the new-provider initialization path for `codexLocalModelMapping`.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, Rust/Tauri compatibility tests.

## Global Constraints

- Existing `codexRouting` object and array-shaped legacy data must remain readable and writable.
- MultiRouter workspace remains the sole visible route editor.
- Explicit `codexLocalModelMapping: false` values remain false only for custom providers.
- Maintained presets and MultiRouter always project their catalog.
- New custom Codex providers default to `codexLocalModelMapping: true`.
- The custom-provider switch is the final control inside the collapsed advanced section and explains its exact effect.

---

### Task 1: Lock The Generic Form Behavior

**Files:**

- Modify: `tests/components/CodexFormFields.test.tsx`
- Modify: `tests/components/ProviderForm.codexPreset.test.tsx`

**Interfaces:**

- Consumes: `CodexFormFields`, `ProviderForm`
- Produces: regression expectations for hidden route controls and default menu projection

- [ ] **Step 1: Replace the route-editor visibility test**

Assert that `Codex 多模型路由` and `添加路由` are absent even when a routing callback and saved route configuration are supplied.

- [ ] **Step 2: Add the new-provider default test**

Render a new Codex Provider and assert the mocked `takeoverEnabled` prop is `true`. Keep the existing explicit-false save test unchanged.

- [ ] **Step 3: Run the focused tests and verify RED**

Run: `pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexPreset.test.tsx`

Expected: the hidden-entry and new-default assertions fail against the current implementation.

### Task 2: Implement The Presentation And Default Changes

**Files:**

- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`

**Interfaces:**

- Consumes: existing `codexRouting`, `onCodexRoutingChange`, and `meta.codexLocalModelMapping`
- Produces: no generic route editor UI; new Provider default `true`; explicit stored values unchanged

- [ ] **Step 1: Hide the legacy editor presentation**

Make `canEditRouting` false in the generic form with a source comment directing route editing to `CodexRouterWorkspacePage`. Do not remove route state synchronization or dialog code so historical values remain lossless.

- [ ] **Step 2: Default new providers to menu projection**

Update `codexLocalModelMappingFromInitialData` to return `true` only when `initialData` is absent, preserve explicit booleans, and retain the legacy catalog fallback for stored providers. Change both custom-template resets and preset application to keep the new-provider default enabled.

- [ ] **Step 3: Keep the advanced section collapsed for the default alone**

Remove `takeoverEnabled` from `hasAnyAdvancedValue`; the switch remains physically inside `CollapsibleContent`, while unrelated actual advanced values may still expand the section.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run: `pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexPreset.test.tsx`

Expected: both suites pass.

### Task 3: Record And Verify Compatibility

**Files:**

- Modify: `memory.md`

**Interfaces:**

- Consumes: final behavior and test evidence
- Produces: repository-local maintenance guidance

- [ ] **Step 1: Record the product boundary**

Document that generic route UI is hidden, history compatibility remains, new menu projection defaults on, and MultiRouter owns route editing.

- [ ] **Step 2: Run broader frontend verification**

Run: `pnpm typecheck`

Run: `pnpm format:check`

Run: `pnpm vitest run tests/components/useCodexConfigState.test.ts tests/integration/App.test.tsx tests/lib/codexMultiRouterSync.test.ts`

Expected: all commands exit successfully.

- [ ] **Step 3: Inspect the final diff and commit**

Stage only the planned files and commit with the required attribution footer `本次提交由BigStrongsSun完成`.

### Task 4: Restrict And Explain The Projection Opt-Out

**Files:**

- Modify: `tests/components/CodexFormFields.test.tsx`
- Modify: `tests/components/ProviderForm.codexPreset.test.tsx`
- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/ja.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`

**Interfaces:**

- Consumes: stable `codexPresetId`, `codexTakeoverEnabled`, existing advanced collapsible
- Produces: `allowModelMenuProjectionToggle` presentation boundary and forced maintained-preset save semantics

- [ ] **Step 1: Add failing component and form tests**

Assert that the custom toggle renders after request overrides with complete explanatory copy, maintained presets pass no editable toggle and save `codexLocalModelMapping: true`, and switching to a custom source defaults the toggle to enabled.

- [ ] **Step 2: Verify RED**

Run: `pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexPreset.test.tsx --exclude "**/.worktrees/**"`

Expected: fail because the toggle is not last, maintained preset editability is not distinguished, and custom selection currently resets the state to false.

- [ ] **Step 3: Implement the minimal behavior**

Derive maintained-preset identity from the selected preset or saved stable `codexPresetId`; force its projection state and saved metadata to true. Pass an explicit custom-only presentation prop to `CodexFormFields`, move the panel to the end of `CollapsibleContent`, and replace the hints in all four locales with the approved control/not-control/opt-out explanation.

- [ ] **Step 4: Verify GREEN and compatibility**

Run the focused tests, `pnpm typecheck`, changed-file Prettier checks, MultiRouter state/sync tests, and isolated App integration suite. Record the final boundary in `memory.md` and commit with the required attribution footer.

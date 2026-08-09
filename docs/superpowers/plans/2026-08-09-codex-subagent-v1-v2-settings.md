# Codex Sub-Agent V1/V2 Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-MultiRouter Sub-Agent V1/V2 selector, preserve both configurations, make V2 automatically expose DeepSeek Flash/Pro roles, and deliver an installed Windows build with live V1/V2 proof.

**Architecture:** `settingsConfig.codexRouting.subagentVersion` is the only persisted protocol selector. Frontend readers normalize missing or invalid values to V2; Rust uses the same default to project catalog metadata, Codex feature flags, and CCSM-owned role files. Existing `modelCatalog.spawnAgentModels` remains the V1 direct-override order, while V2 roles are derived from the complete routable catalog.

**Tech Stack:** React 19, TypeScript, Vitest/Testing Library, Rust, serde_json, toml_edit, Tauri 2, pnpm, Cargo, PowerShell.

## Global Constraints

- New and legacy plans default to V2; switching versions never deletes `spawnAgentModels`.
- One MultiRouter session uses exactly one protocol version.
- V1 must set model metadata to V1 and `[features.multi_agent_v2].enabled=false`.
- V2 must set model metadata to V2, enable its feature policy, and retain the non-reserved `agents` namespace for mixed routes.
- V2 managed roles use the complete routable catalog and never overwrite user role files.
- Qwen behavior is out of scope.
- Every RED and GREEN stage is a separate local commit whose final message line is `本次提交由BigStrongsSun完成`.
- Do not push, create a PR, or publish a GitHub Release.

---

### Task 1: Shared Version Contract And Migration

**Files:**
- Modify: `src/types.ts`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx`
- Modify: `src/lib/codexMultiRouterWizard.ts`
- Test: `src/components/codex/CodexRouterWorkspacePage.test.ts`
- Test: `src/lib/codexMultiRouterWizard.test.ts`

**Interfaces:**
- Produces: `type CodexSubagentVersion = "v1" | "v2"`.
- Produces: `normalizeCodexSubagentVersion(value: unknown): CodexSubagentVersion`.
- Produces: `readCodexRouting(provider)` with normalized `subagentVersion`.
- Consumes: existing `CodexRoutingConfig`, `buildCodexMultiRouterWizardPlan`, and `modelCatalog.spawnAgentModels`.

- [ ] **Step 1: Write failing migration tests**

```ts
expect(readCodexRouting(planWithoutVersion)?.subagentVersion).toBe("v2");
expect(readCodexRouting(planWithInvalidVersion)?.subagentVersion).toBe("v2");
expect(readCodexRouting(planWithV1)?.subagentVersion).toBe("v1");
expect(buildCodexMultiRouterWizardPlan(sources, { subagentVersion: "v1" })
  .settingsConfig.codexRouting.subagentVersion).toBe("v1");
expect(saved.settingsConfig.modelCatalog.spawnAgentModels).toEqual(["deepseek-v4-pro"]);
```

- [ ] **Step 2: Run RED tests**

Run: `pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts src/lib/codexMultiRouterWizard.test.ts --exclude ".worktrees/**"`

Expected: FAIL because `subagentVersion` is absent or not normalized.

- [ ] **Step 3: Commit RED**

Stage only the two test files and commit with the required attribution footer.

- [ ] **Step 4: Implement the shared contract**

```ts
export type CodexSubagentVersion = "v1" | "v2";

export function normalizeCodexSubagentVersion(value: unknown): CodexSubagentVersion {
  return value === "v1" ? "v1" : "v2";
}
```

Extend `CodexRoutingConfig`, normalize object and legacy-array routing, add the builder option, and persist it without touching `spawnAgentModels`.

- [ ] **Step 5: Run GREEN tests and typecheck**

Run the RED command again, then `pnpm typecheck`.

Expected: all selected tests pass and TypeScript exits 0.

- [ ] **Step 6: Commit GREEN**

Stage Task 1 production files and commit with the required attribution footer.

### Task 2: Rust Protocol And Feature Projection

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Test: inline `#[cfg(test)]` tests in `src-tauri/src/codex_config.rs`

**Interfaces:**
- Produces: Rust enum `CodexSubagentVersion { V1, V2 }` or an equivalent private typed parser.
- Produces: `codex_subagent_version(settings: &Value) -> CodexSubagentVersion`.
- Changes: `apply_codex_multi_agent_transport_policy` and `ensure_codex_multi_agent_reserved_schema_compatible` consume the selected version.

- [ ] **Step 1: Write failing projection tests**

```rust
assert_eq!(codex_subagent_version(&json!({})), CodexSubagentVersion::V2);
assert_eq!(codex_subagent_version(&json!({"codexRouting":{"subagentVersion":"v1"}})), CodexSubagentVersion::V1);
assert!(models.iter().all(|m| m["multi_agent_version"] == "v1"));
assert_eq!(multi_agent_v2["enabled"].as_bool(), Some(false));
assert!(v2_models.iter().all(|m| m["multi_agent_version"] == "v2"));
assert_eq!(v2_feature["enabled"].as_bool(), Some(true));
```

Cover V1 mixed routes, V2 mixed routes, missing/invalid default, official-only routes, and preservation of a custom non-reserved V2 namespace.

- [ ] **Step 2: Run RED tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_version -- --nocapture`

Expected: FAIL because the parser and V1 projection do not exist.

- [ ] **Step 3: Commit RED**

Commit only the failing Rust tests.

- [ ] **Step 4: Implement typed projection**

Parse `settings.codexRouting.subagentVersion`, default invalid/missing to V2, write both `multi_agent_version` and `multiAgentVersion` across the active catalog, and make the TOML policy explicit:

```rust
match version {
    CodexSubagentVersion::V1 => multi_agent_v2["enabled"] = toml_edit::value(false),
    CodexSubagentVersion::V2 => {
        multi_agent_v2["enabled"] = toml_edit::value(true);
        multi_agent_v2["hide_spawn_agent_metadata"] = toml_edit::value(true);
    }
}
```

Apply the `agents` namespace replacement only for V2 mixed delivery.

- [ ] **Step 5: Run GREEN and regressions**

Run the RED command, then targeted existing multi-agent policy tests and `cargo fmt --manifest-path src-tauri/Cargo.toml --check`.

- [ ] **Step 6: Commit GREEN**

Commit the Rust implementation with the required footer.

### Task 3: Version-Aware Managed Role Lifecycle

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Test: inline managed-agent tests in the same file

**Interfaces:**
- Changes: `sync_codex_managed_agent_files(specs, version)` generates roles only for V2.
- Preserves: CCSM ownership marker and `ccswitch-<role>` collision behavior.

- [ ] **Step 1: Write failing role lifecycle tests**

```rust
sync_codex_managed_agent_files(&specs, CodexSubagentVersion::V1)?;
assert!(!managed_flash.exists());
assert!(!managed_pro.exists());
assert!(user_flash.exists());

sync_codex_managed_agent_files(&specs_with_pro_sixth, CodexSubagentVersion::V2)?;
assert!(managed_flash.exists());
assert!(managed_pro.exists());
```

Also prove changing `spawnAgentModels` does not change the V2 role set.

- [ ] **Step 2: Run RED and commit**

Run: `cargo test --manifest-path src-tauri/Cargo.toml managed_agent_files -- --nocapture`

Expected: FAIL because sync is not version-aware. Commit the failing tests.

- [ ] **Step 3: Implement V1 prune and V2 generation**

Thread the selected version into sync. For V1, desired managed roles are empty and stale pruning removes only files bearing the CCSM marker. For V2, derive roles from all routeable specs.

- [ ] **Step 4: Run GREEN and commit**

Run all managed-agent tests and Rust formatting, then commit production changes.

### Task 4: Wizard V1/V2 Navigation And Settings

**Files:**
- Modify: `src/components/codex/CodexMultiRouterWizard.tsx`
- Test: `src/components/codex/CodexMultiRouterWizard.test.tsx`

**Interfaces:**
- Consumes: `CodexSubagentVersion`, `normalizeCodexSubagentVersion`, builder option from Task 1.
- Produces: wizard steps `subagentV1` and `subagentV2` and draft version state.

- [ ] **Step 1: Write failing UI behavior tests**

Render the real wizard and assert both navigation buttons, V1/V2 difference copy, V2 default, V1 selection, preserved direct overrides, and save payload:

```ts
expect(screen.getByRole("button", { name: /Sub-Agent V1/ })).toBeInTheDocument();
expect(screen.getByRole("button", { name: /Sub-Agent V2/ })).toBeInTheDocument();
await user.click(screen.getByRole("button", { name: /启用 V1/ }));
expect(screen.getByText(/当前使用 V1/)).toBeInTheDocument();
expect(saved.settingsConfig.codexRouting.subagentVersion).toBe("v1");
```

- [ ] **Step 2: Run RED and commit**

Run: `pnpm vitest run src/components/codex/CodexMultiRouterWizard.test.tsx --exclude ".worktrees/**"`

Expected: FAIL because steps and controls do not exist. Commit tests.

- [ ] **Step 3: Implement wizard flow**

Add both keys to the step union, step metadata/rules/status transitions, initialize the draft from normalized routing, render comparison copy and role/direct-override content, and pass the selected version to preview/save.

- [ ] **Step 4: Run GREEN, typecheck, and commit**

Run the RED command and `pnpm typecheck`, then commit production changes.

### Task 5: MultiRouter Workspace Sub-Agent Settings

**Files:**
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx`
- Test: `src/components/codex/CodexRouterWorkspacePage.test.ts`

**Interfaces:**
- Consumes: normalized routing and existing direct-override editor.
- Produces: `Sub-Agent 设置` section with a V1/V2 selector, V1 editor, and V2 managed-role preview.

- [ ] **Step 1: Write failing workspace tests**

Assert V1/V2 controls, legacy V2 display, selector save, preserved `spawnAgentModels`, V1 editor visibility, V2 role preview, and refresh persistence using the real component helpers.

- [ ] **Step 2: Run RED and commit**

Run: `pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts --exclude ".worktrees/**"`

Expected: FAIL because the full settings section does not exist. Commit tests.

- [ ] **Step 3: Implement the workspace section**

Replace the old advanced disclosure with a full-width settings section. Save version changes through the existing provider update path; update only `codexRouting.subagentVersion` and preserve routes/catalog/auth. Keep the existing editor under the V1 panel and show deterministic role cards under V2.

- [ ] **Step 4: Run GREEN, typecheck, and commit**

Run the RED command and `pnpm typecheck`, then commit production changes.

### Task 6: Full Regression And Versioned Windows Build

**Files:**
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `memory.md`

**Interfaces:**
- Produces: version `3.19.1-15` and local installer/export metadata.

- [ ] **Step 1: Run full pre-version verification**

Run:

```powershell
pnpm vitest run --exclude ".worktrees/**" --no-file-parallelism
pnpm typecheck
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml --check
git diff --check
```

Expected: zero failures. Record any pre-existing format-only exclusions explicitly.

- [ ] **Step 2: Update version files and commit**

Set all four version sources to `3.19.1-15`, regenerate Cargo.lock through Cargo, verify equality, and commit with the required footer.

- [ ] **Step 3: Build the complete Windows package**

Run: `pnpm tauri build`

Expected: exit 0 with NSIS installer and updater artifacts under `src-tauri/target/release/bundle`.

- [ ] **Step 4: Export and hash artifacts**

Run the repository local export script against the successful build. Record absolute paths, sizes, file/product versions, SHA-256 hashes, and non-empty signatures in `memory.md`.

### Task 7: Install And Live V1/V2 Acceptance

**Files:**
- Modify: `memory.md`
- Runtime: installed CCSwitchMulti, Codex Desktop/app-server, generated config/catalog/agents files, rollout JSONL, router logs

**Interfaces:**
- Consumes: Task 6 installer.
- Produces: installed `3.19.1-15` and live evidence for UI, V1, V2 Flash, and V2 Pro.

- [ ] **Step 1: Capture pre-install state and install**

Record running executable versions, process start times, active provider/config hashes, then stop CCSM safely, run the NSIS installer, and verify the installed executable reports `3.19.1-15`.

- [ ] **Step 2: Restart CCSM and Codex app-server**

Ensure old processes are gone, start the installed CCSM hidden, restart Codex Desktop/app-server, and confirm new process start times and healthy local proxy endpoints.

- [ ] **Step 3: Verify UI persistence**

Use the installed UI to inspect both wizard navigation entries and workspace settings; switch V1/V2, save, refresh, and restart. Confirm SQLite/provider JSON preserves both `subagentVersion` and `spawnAgentModels`.

- [ ] **Step 4: Run V1 canary**

Select V1, start a new Codex session, use a direct override child with a unique nonce and one read-only command, follow up once, then verify rollout protocol/model/provider/task content and router HTTP 200.

- [ ] **Step 5: Run V2 automatic Flash canary**

Select V2 and start a new session. Ask for a long-context scan without naming a model; verify parent chooses `deepseek-flash`, child performs a read-only tool call, returns final, handles follow-up, and routes through Responses with HTTP 200.

- [ ] **Step 6: Run V2 automatic Pro canary**

Ask for complex cross-module diagnosis without naming a model; verify `deepseek-pro`, role/model/provider fields, real read-only tool use, final, follow-up, Chat bridge route, and HTTP 200.

- [ ] **Step 7: Record final evidence and commit**

Update `memory.md` with exact versions, artifact hashes, session ids, rollout facts, router evidence, tests, limitations, and commit hashes. Run `git diff --check`, commit the evidence, and leave the worktree clean.

### Task 8: Completion Audit

**Files:**
- Read: approved design, this plan, git history, built artifacts, installed runtime evidence

- [ ] **Step 1: Audit every explicit requirement**

Map design requirements to source, tests, artifacts, UI state, rollout evidence, and logs. Treat missing or indirect evidence as incomplete.

- [ ] **Step 2: Run final freshness checks**

Re-run targeted V1/V2 tests, version equality checks, artifact hashes, installed executable version, process health, and `git status --short`.

- [ ] **Step 3: Complete the active goal only if all evidence is present**

Call the goal completion tool only after every requirement is proven and no required work remains.

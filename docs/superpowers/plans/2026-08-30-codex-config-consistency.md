# Codex Config Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline execution) to implement this plan task-by-task with verification checkpoints.

**Goal:** 修复 Codex V2 Common Config 同步分裂，并在 CCSwitchMulti 启动后检测 Codex live 配置漂移，交由用户明确决定是否写回。

**Architecture:** 后端新增纯对账/指纹模块和两个 Tauri command；启动恢复链路完成后发出 `codex-config-consistency` 事件。Codex Router 的所有同步入口收敛到“Common Config → V2 projection → live/backup”单一路径，前端以可去重的 hook 驱动确认弹窗。

**Tech Stack:** Rust/Tauri 2、SQLite settings、`toml_edit`、SHA-256、React 18、TypeScript、Vitest/Testing Library、现有 shadcn Dialog。

**Spec:** `docs/superpowers/specs/2026-08-30-codex-config-consistency-design.md`

## Global Constraints

- 不修改 Codex rollout JSONL、SQLite history 或 session projection。
- 代理接管期间 live 是 CCSM 有意生成的配置，不报告为外部漂移。
- 对账结果和日志不得包含 API key、OAuth token、Cookie 或完整配置正文。
- 所有 Codex 写入必须复用既有写锁、写前/写后重读和原子替换。
- Windows 文本保持 UTF-8 无 BOM；不清理工作树中与本任务无关的既有修改。
- 每个任务先写失败测试并观察失败，再写最小实现；每个任务独立提交，提交说明末尾包含“本次提交由BigStrongsSun完成”。

---

### Task 1: 提取 Codex Router 有效配置构造并修复 Common Config 同步

**Files:**
- Modify: `src-tauri/src/services/provider/live.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/services/provider/live.rs` (Rust unit tests)
- Test: `src-tauri/src/services/provider/mod.rs` (Rust unit tests)

**Interfaces:**
- Produce `pub(crate) fn build_codex_live_config_for_provider(db: &Database, provider: &Provider) -> Result<String, AppError>` in `codex_config.rs`; it returns the exact normalized `config.toml` text the normal Codex live writer would generate from effective settings, catalog projection and unified-session policy, without writing files.
- Produce `fn sync_codex_router_provider(state: &AppState, provider: &Provider) -> Result<(), AppError>` in `services/provider/live.rs`; it is the single Router branch used by both current-provider sync functions.

- [ ] **Step 1: Write the failing regression tests**

Add tests that seed an in-memory DB with a schema-v2 Router and `common_config_codex = "model_reasoning_effort = \\\"medium\\\"\\n"`, set the stored Router config to `high`, run the relevant sync helper, and assert the generated live/backup TOML contains `medium`. Add a second assertion that the expected-config helper applies V2 projection fields without dropping the common field.

- [ ] **Step 2: Run the focused Rust tests and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml services::provider::live::tests::codex_multirouter -- --nocapture`.
Expected: FAIL because the V2 branch returns after `ensure_codex_multirouter_projection` and the new helper does not yet exist.

- [ ] **Step 3: Implement the pure expected-config builder**

In `codex_config.rs`, build effective settings through `build_effective_settings_with_common_config`, apply `codex_multirouter::projection::build_projection_artifact` plus `apply_projection_owned_settings` for schema-v2, resolve the existing catalog tool profile/provider context, call the existing preparation pipeline, and apply unified-session injection using the same category/settings predicate as the live writer. Normalize and validate the returned TOML before returning it.

- [ ] **Step 4: Route both V2 sync branches through the shared writer**

Remove the early V2 returns in `ProviderService::sync_current_provider_for_app` and `sync_current_provider_for_app_respecting_takeover`. Call `sync_codex_router_provider`; in non-takeover mode write the expected config through the existing atomic Codex writer, while takeover mode updates `proxy_live_backup` and then refreshes the proxy-active projection. Keep MCP projection behavior unchanged.

- [ ] **Step 5: Run the focused tests and verify GREEN**

Run `cargo test --manifest-path src-tauri/Cargo.toml services::provider::live::tests::codex_multirouter -- --nocapture` and `cargo test --manifest-path src-tauri/Cargo.toml services::provider::tests:: -- --nocapture`.
Expected: all focused tests pass and no test writes outside its temporary Codex directory.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/codex_config.rs src-tauri/src/services/provider/live.rs src-tauri/src/services/provider/mod.rs
git commit -m "fix: keep Codex Router Common Config synchronized" -m "Unify V2 Router live and takeover-backup projection around effective Common Config settings and add regressions for the split-brain write path. 本次提交由BigStrongsSun完成"
```

### Task 2: Add backend semantic reconciliation and explicit resolution commands

**Files:**
- Create: `src-tauri/src/codex_config_consistency.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/database/dao/settings.rs`
- Test: `src-tauri/src/codex_config_consistency.rs`

**Interfaces:**
- Produce `CodexConfigConsistencyReport { state, provider_id, expected_fingerprint, actual_fingerprint, changed_keys, reason }` with camelCase serde names.
- Produce `pub fn inspect(state: &AppState) -> Result<CodexConfigConsistencyReport, AppError>`.
- Produce `pub fn resolve(state: &AppState, expected_fingerprint: String, action: CodexConfigConsistencyAction) -> Result<CodexConfigConsistencyReport, AppError>` where actions serialize as `apply_ccsm`, `keep_codex`, `later`.
- Export Tauri commands `inspect_codex_config_consistency` and `resolve_codex_config_consistency` from `commands/mod.rs` and register them in `lib.rs`.
- Persist acknowledgement metadata through `Database::set_setting` under the exact keys `codex_config_consistency:last_action`, `codex_config_consistency:last_actual_fingerprint`, and `codex_config_consistency:last_provider_id`.

- [ ] **Step 1: Write failing semantic-diff and resolution tests**

Cover: comments/whitespace/key-order-only changes return `consistent`; a changed value returns `external_drift` with only key paths; takeover returns `not_applicable`; invalid TOML returns `unavailable`; `apply_ccsm` rejects a stale expected fingerprint, creates one timestamped drift backup before atomic write, and succeeds with a fresh fingerprint; `keep_codex` records only fingerprints/action in settings; `later` records nothing.

- [ ] **Step 2: Run the focused tests and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_config_consistency -- --nocapture`.
Expected: FAIL because the module, report, commands and semantic normalizer do not exist.

- [ ] **Step 3: Implement owned-config normalization and fingerprints**

Parse expected and actual `config.toml` with `toml_edit`, recursively canonicalize tables/arrays, normalize CCSM-owned catalog paths to a sentinel, and exclude only explicitly non-owned runtime metadata. Hash canonical JSON with SHA-256. Generate sorted changed key paths without including values.

- [ ] **Step 4: Implement inspect/resolve with compare-and-swap**

Resolve the current operable Codex provider. Skip when no provider, proxy takeover, or missing live file. For `apply_ccsm`, re-read live bytes under the Codex write lock, compare its fingerprint to the command argument, write `config.toml.ccsm-drift-<UTC timestamp>.bak`, call `build_codex_live_config_for_provider`, and atomically replace the file. Persist only resolution metadata for `keep_codex`; leave settings untouched for `later`.

- [ ] **Step 5: Run the focused tests and verify GREEN**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_config_consistency -- --nocapture` and `cargo test --manifest-path src-tauri/Cargo.toml codex_config -- --nocapture`.
Expected: all reconciliation and existing Codex writer tests pass with no secret text in serialized reports.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/codex_config_consistency.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src-tauri/src/database
git commit -m "feat: reconcile external Codex config changes safely" -m "Add semantic live-config drift reports, compare-and-swap resolution actions, fingerprint acknowledgements, and recoverable drift backups. 本次提交由BigStrongsSun完成"
```

### Task 3: Run reconciliation after startup recovery and publish the event

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/codex_config_consistency.rs`
- Test: `src-tauri/src/codex_config_consistency.rs` (startup sequencing helper tests)

**Interfaces:**
- Produce `pub(crate) async fn reconcile_after_startup(app: &tauri::AppHandle)` that obtains `AppState`, calls `inspect`, persists the latest report fingerprint, and emits `codex-config-consistency` only for `external_drift`.

- [ ] **Step 1: Write the failing startup-order test**

Add a testable sequencing helper that records markers and asserts reconciliation runs after `initialize_common_config_snippets` and `restore_proxy_state_on_startup`; assert no event is emitted for `consistent`, `not_applicable`, or `unavailable`.

- [ ] **Step 2: Run the focused test and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_config_consistency::tests::startup -- --nocapture`.
Expected: FAIL because the startup helper and event emission do not exist.

- [ ] **Step 3: Wire the helper at the end of the existing startup async task**

Call `reconcile_after_startup` only after crash recovery, common-config initialization and proxy restoration. Log failures without aborting startup. Keep silent-start behavior unchanged.

- [ ] **Step 4: Run startup and command tests**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_config_consistency -- --nocapture`.
Expected: PASS; event payload is the same report shape exposed by the command.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/codex_config_consistency.rs
git commit -m "feat: publish Codex config drift after startup recovery" -m "Run reconciliation only after CCSM startup writers settle and notify the frontend without blocking launch. 本次提交由BigStrongsSun完成"
```

### Task 4: Add frontend API, deduplicated hook and confirmation dialog

**Files:**
- Create: `src/lib/api/codexConfigConsistency.ts`
- Create: `src/hooks/useCodexConfigConsistency.ts`
- Create: `src/components/codex/CodexConfigConsistencyDialog.tsx`
- Modify: `src/lib/api/index.ts`
- Modify: `src/App.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`
- Test: `src/hooks/useCodexConfigConsistency.test.tsx`
- Test: `src/components/codex/CodexConfigConsistencyDialog.test.tsx`

**Interfaces:**
- `codexConfigConsistencyApi.inspect(): Promise<CodexConfigConsistencyReport>`.
- `codexConfigConsistencyApi.resolve(expectedFingerprint: string, action: CodexConfigConsistencyAction): Promise<CodexConfigConsistencyReport>`.
- `useCodexConfigConsistency(): { report: CodexConfigConsistencyReport | null; close: () => void }` listens to the event, performs one query fallback, and deduplicates by `actualFingerprint`.

- [ ] **Step 1: Write failing hook/dialog tests**

Test event plus fallback query produces one dialog; test each button sends `apply_ccsm`, `keep_codex`, or `later`; test a failed apply keeps the dialog open and renders a retryable error; test changed key paths render without values.

- [ ] **Step 2: Run Vitest and verify RED**

Run `npm test -- --run src/hooks/useCodexConfigConsistency.test.tsx src/components/codex/CodexConfigConsistencyDialog.test.tsx` from `cc-switch`.
Expected: FAIL because the API, hook and component do not exist.

- [ ] **Step 3: Implement API and hook**

Use `invoke` for the two commands and `useTauriEvent` for `codex-config-consistency`. Query once on mount after listener registration. Ignore non-drift states and maintain a `Set` of fingerprints so duplicate event/query payloads cannot stack dialogs.

- [ ] **Step 4: Implement the dialog and mount it in `App.tsx`**

Reuse existing Dialog/Button components. Make “应用 CCSM 配置” the primary action, “保留 Codex 修改” the secondary action, and close as “稍后处理”. Show provider/path/key paths and a no-secrets notice. Keep the dialog open on backend errors and expose a retry button.

- [ ] **Step 5: Add translations and run frontend tests**

Add complete keys for all four locales, then rerun the focused Vitest command. Expected: PASS with no missing translation warnings.

- [ ] **Step 6: Commit**

```bash
git add src/lib/api/codexConfigConsistency.ts src/hooks/useCodexConfigConsistency.ts src/components/codex/CodexConfigConsistencyDialog.tsx src/App.tsx src/lib/api/index.ts src/i18n/locales
git commit -m "feat: prompt before reconciling external Codex changes" -m "Add startup drift notification, explicit apply/keep/later actions, deduplication, and localized safe configuration details. 本次提交由BigStrongsSun完成"
```

### Task 5: Full verification and installation-boundary report

**Files:**
- Modify: `memory.md` (append verified implementation facts and any remaining bug notes)
- Test: repository test/build outputs only

- [ ] **Step 1: Run backend formatting and tests**

Run `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.

- [ ] **Step 2: Run frontend checks**

Run `npm test -- --run`, `npm run lint`, and `npm run build` from `cc-switch`.

- [ ] **Step 3: Verify encoding and inspect the diff**

Strictly decode every changed Markdown/JSON/TS/Rust file as UTF-8, assert no BOM/U+FFFD, run `git diff --check`, and inspect `git status --short` to ensure unrelated dirty files remain untouched.

- [ ] **Step 4: Perform bounded installed-state smoke test**

In the installed CCSM environment, change one non-sensitive Codex TOML value externally, restart CCSM, verify the prompt appears once, exercise all three actions, and confirm `config.toml` plus the settings fingerprint behavior. Record source-test results separately from this installed-state observation.

- [ ] **Step 5: Update memory and commit verification notes**

Append only verified facts, including command names, event name, backup behavior, and any acceptance limitation, then commit:

```bash
git add memory.md
git commit -m "docs: record Codex config reconciliation verification" -m "Record tested synchronization and startup reconciliation behavior while separating source and installed-state evidence. 本次提交由BigStrongsSun完成"
```

# Codex 启动配置保护与恢复实现计划（SSOT 配置保留扩展版）

> **For agentic workers:** REQUIRED SUB-SKILL: Use `sdd-implement` to implement this plan task-by-task after 世豪 approves the spec. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop Codex live recovery and provider writes from turning a missing config payload into an empty `config.toml`, preserve user-owned Codex tables from the current live file, make startup recovery safe, automatic, and visible so 世豪 never has to re-enable `max` reasoning or reinstall SDD after a restart, and stop every Codex MCP whole-table projection path from deleting user-level (live-only) MCP servers.

**Architecture:** Layer A (minimal deliverable) makes missing Codex config an explicit error at the provider live-write boundary, keeps `Some("")` as the only explicit empty-config payload, lets no-backup SSOT recovery fall through to placeholder cleanup, adds a normal-switch preflight, and guards every live write with optimistic concurrency (fingerprint recheck before atomic replace, bounded retry, `ConcurrentModificationDeferred` on exhaustion). Layer B classifies run-marker evidence before any destructive recovery, probes port 15721 ownership before takeover, persists and reports a named recovery outcome, and detects and repairs plugin registration inconsistency across cache, config, and marketplace. Layer C (formal cross-process locking, full user-config snapshots, long-term schema migration) is explicitly out of scope for this implementation.

**Tech Stack:** Rust/Tauri, rusqlite, serde_json, toml_edit, axum (proxy server), React/TypeScript frontend, Cargo tests, Vitest.

**Spec:** `docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`（第三版已获世豪批准，含第 15 节 MCP 所有权与对账补充；实现以兼容性设计为核心）

**Recovery ledger:** `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation-ledger.md`

## Global Constraints

- Use TDD for every behavior change and observe RED before production edits.
- Never read from or write to the real `~/.codex` or `~/.agents` during tests; use `TempHome`, the existing test support helpers, and `Database::memory()`.
- Do not restart Codex Desktop and do not kill any process during implementation or testing.
- Layer B makes `~/.agents/plugins/marketplace.json` an in-scope product artifact: product code may repair it through the approved merge-and-validate flow, but tests must exercise it only under `TempHome`; no one edits 世豪's real file by hand in this task.
- Preserve every pre-existing worktree change. Do not revert `PLAN-reasoning-narrow-wide.md`, `docs/diagnostics/`, or any other file not created for this task.
- Use `apply_patch` for source and document edits. Keep all text UTF-8 without BOM.
- Do not add an independent user-config snapshot database table in this fix.
- Do not create Git commits unless the main agent explicitly authorizes a commit boundary.
- Treat `BigStrongSun/ccswitchmulti` as the upstream repository and `zhushihao/ccswitchmulti` as the release fork.
- Never update historical upstream PR branches. Every upstream contribution for this work gets a fresh branch from the latest upstream `main`, a new Issue, and a new focused PR.
- Keep fork-only version bumps, signing configuration, release notes, updater endpoints, badges, and tags out of every upstream PR branch.
- Publish binaries only from a fresh fork integration/release branch through the existing `v*` tag-triggered GitHub Actions workflow. Never create an upstream release tag for this work.
- Fetch `origin` before implementation starts, again before constructing every upstream PR branch, and again immediately before the fork release tag. Rebuild and re-test on the new upstream base if it moved.
- Derive the fork version from the latest upstream stable Release tag: use `vX.1`, `vX.2`, and so on; reset the suffix to `.1` whenever upstream advances to a new `vY`.
- Phase A must land and pass its verification gate before Phase B tasks start; the two phases may share one worktree but not one commit boundary.

## Producer / Consumer / Conflict Scan

- `src-tauri/src/codex_config.rs` produces the Codex live write contract (`write_codex_live_atomic` line 247, `write_codex_live_config_atomic` line 514, `merge_codex_provider_config_texts` line 6817, provider write entries at lines 6901/6922/7531) and is consumed by provider switching, proxy restore, force repair, catalog projection, and integration tests.
- `src-tauri/src/services/provider/live.rs` produces effective provider settings with common config (`write_live_with_common_config` line 761, `write_codex_live_snapshot` line 1312) and is consumed by normal switching and SSOT restore.
- `src-tauri/src/services/provider/mod.rs` produces the normal switch transaction order and is consumed by Tauri commands.
- `src-tauri/src/services/proxy.rs` produces crash recovery and takeover cleanup behavior (`reconcile_codex_owned_projection_on_startup` line 1595, `takeover_live_config_best_effort` line 2017, `restore_live_configs` line 2159, `restore_live_config_for_app_with_fallback_inner` line 2192, `restore_live_from_ssot_for_app` line 2288, `recover_from_crash` line 2646) and is consumed by startup recovery and proxy disable flows.
- `src-tauri/src/app_exit_monitor.rs` produces run-marker and exit-event evidence (`record_startup`, `record_clean_exit`, `record_forced_exit`, `record_panic`) and is consumed by `src-tauri/src/lib.rs` startup and exit wiring.
- `src-tauri/src/lib.rs` wires startup recovery (`recover_from_crash` call at line 1243, `restore_proxy_state_on_startup` at line 2015) and clean-exit recording; it consumes `app_exit_monitor` and `ProxyService`.
- `src-tauri/src/proxy/server.rs` binds 15721 (line 142), formats the port-conflict message (line 541), and exposes `/health` (line 328) and `/status` (line 329); `src-tauri/src/proxy/handlers.rs` implements `health_check` (line 83) and `get_status` (line 94); `src-tauri/src/proxy/types.rs` defines `ProxyStatus` (line 60).
- `src-tauri/src/database/dao/proxy.rs` persists live backups (`save_live_backup` line 810, `get_live_backup` line 841, `delete_all_live_backups` line 878).
- `src/App.tsx` subscribes to backend events through `useTauriEvent` (for example `proxy-official-warning` at line 519) and is the consumer of the new recovery-outcome events; `src/hooks/useTauriEvent.ts` is the generic subscription hook; `src/hooks/useProxyStatus.ts` shows the existing toast patterns.
- New module `src-tauri/src/services/codex_plugin_registry.rs` (to be created in Phase B) produces plugin-registration detection and repair; it consumes `~/.codex/plugins/cache` manifests, Codex live config, and `~/.agents/plugins/marketplace.json`.
- Existing user-owned files are outside this task's ownership. This plan owns only the files named in each task.

Phase M（MCP 对账）补充扫描：

- `src-tauri/src/services/mcp.rs` 生产 `McpService::sync_all_enabled`（194 行）、`sync_enabled_for_app`（218 行）、`project_servers_to_app`（223-259 行）和单条操作（`upsert_server` 19 行、`delete_server` 57 行、`toggle_app` 72 行）；被供应商切换/保存、设置保存、SQL/云同步后置、会话开关、通用配置片段保存消费。
- `src-tauri/src/mcp/codex.rs` 生产 Codex live 的三个 MCP 写入口：`sync_enabled_to_codex`（286 行，当前整表替换）、`sync_single_server_to_codex`（419 行）、`remove_server_from_codex`（470 行）；被 `services/mcp.rs` 消费。
- `src-tauri/src/database/dao/mcp.rs` 生产数据库 MCP 行的读写；对账需要“全部 id 集合（含未启用）”，消费方是 `services/mcp.rs`。
- `src-tauri/src/app_config.rs` 的 `McpServer`（246-259 行）无所有权字段；方案甲不需要修改它。
- `src-tauri/src/codex_config.rs` 已生产 `CodexConfigFingerprint`（519 行）和 `write_codex_live_config_optimistic`（551 行，当前私有）；Phase M 消费它并可能新增一个闭包形态的 `pub(crate)` 文档更新 API。
- `src-tauri/tests/import_export_sync.rs`（434-457 行）和 `src-tauri/tests/provider_service.rs`（2495-2672 行）当前把删除用户级 MCP 固定为成功预期，是 Phase M 必须修正的消费方。
- 前端没有独立“同步 MCP”按钮；Phase M 不新增 commands/API/UI，除非世豪批准 15.8 的可见提示增强。

## Conflict Decisions

- 裁定：`None` 在 Codex 原子写入器中返回错误 — 原因是“缺少输入”不能和“显式空配置”混用 — 判断错误的代价是再次把用户配置写成 0 字节。
- 裁定：`Some("")` 保留现有“清供应商字段、留用户表”语义 — 原因是 official/config-only 清理路径已有该契约 — 判断错误的代价是破坏官方内置供应商回退。
- 裁定：schema-v2 MultiRouter 的 SSOT 缺配置时落入占位符清理 — 原因是数据库没有完整 live 配置时不能自称 SSOT 写回成功 — 判断错误的代价是留下本地代理占位符，但用户会看到明确错误而不是配置丢失。
- 裁定：普通切换在更新当前供应商前预检 — 原因是当前顺序会让数据库先指向不可写供应商 — 判断错误的代价是多读一次 common config，换来失败时状态一致。
- 裁定：乐观并发用“指纹复检加有限重试”，不引入文件锁 — 原因是 CCSM 和 Codex 都写同一文件且没有共享锁协议 — 判断错误的代价是极端冲突下放弃写入并报告，而不是静默覆盖。
- 裁定：marker 只作为证据输入，恢复动作由分类结果驱动 — 原因是 marker 残留可以由托盘常驻、关机、强制结束造成 — 判断错误的代价是正常双开也会触发接管关闭和 SSOT 恢复。
- 裁定：端口 15721 被占用时先探测 `/health` 与 `/status`，任何路径不杀进程 — 原因是占用者极可能是正在服务 Codex 的旧 CCSM 实例 — 判断错误的代价是要给 `/status` 响应补充实例身份字段（新增字段，向后兼容）。
- 裁定：恢复结果持久化为应用配置目录下的 JSON 文件（与 `app_exit_monitor` 同一目录策略），再发事件 — 原因是数据库初始化失败时恢复证据也不能丢 — 判断错误的代价是多维护一个小文件格式，换来 UI 可补拉最近一次结果。
- 裁定：插件登记修复默认提供修复按钮，自动修复留待世豪裁定 — 原因是 marketplace 文件归 Codex 安装流程所有 — 判断错误的代价是世豪多点一次，但不会误写登记文件。
- 裁定：MCP 所有权采用方案甲（数据库 id 集合即所有权，live-as-base 对账）— 原因是无需 schema 迁移即可兼容全部已有数据库行，并让整表投影与单条路径语义合一 — 判断错误的代价是 managed id 的外部手工改动会在下次对账被数据库版本还原，该取舍已写入规格裁定 14。
- 裁定：空数据库不得清空 live `[mcp_servers]` 表 — 原因是空库只表示“CCSwitchMulti 没有管理任何 MCP” — 判断错误的代价是全新安装后第一次供应商切换删除用户全部手工 MCP。
- 裁定：旧格式 `[mcp.servers]` 先迁移后清理，live-only 条目迁入 `[mcp_servers]` — 原因是旧格式内容同样是用户配置 — 判断错误的代价是存量旧格式用户仍丢失全部 MCP。
- 裁定：MCP 三个写入口统一走 `codex_config.rs` 的乐观并发写入器，必要时新增闭包形态 `pub(crate)` 更新 API — 原因是对账依赖 live 内容，只有写入点复检才能在并发下兑现“保留 live-only” — 判断错误的代价是极端冲突下保持原文件并报 `ConcurrentModificationDeferred`。
- 裁定：MCP 修复是独立的第五组上游 Issue/PR 与独立提交边界 — 原因是缺陷在 `origin/main` 已存在且责任边界清晰 — 判断错误的代价是多一个小 PR，换来可独立审查和回滚。
- 裁定：Phase M 准入前先修 `codex_plugin_registry.rs` 编译错误并完成修改前快照（HEAD `6923a996`，25 个未提交条目，快照覆盖 tracked diff 与 3 个未跟踪文件，禁止 reset/clean）— 原因是测试门当前退出码 101，任何新 RED 都无法观察 — 判断错误的代价是先做一步与 MCP 无关的编译修复，换来所有后续 RED/GREEN 证据可信。

## Baseline

The following baseline commands were run in `D:\CCSwitchMulti\ccswitchmulti\src-tauri` before implementation planning:

- `cargo test --lib merge_provider_config_preserves_live_user_sections -- --nocapture` — PASS, 1 passed, 3288 filtered out.
- `cargo test --lib codex_restore_from_backup_preserves_live_desktop_settings -- --nocapture` — PASS, 1 passed, 3298 filtered out.
- `cargo test --lib restore_falls_through_to_ssot_when_backup_is_proxy_placeholder -- --nocapture` — PASS, 1 passed, 3298 filtered out.
- `cargo test --lib merge_empty_official_config_clears_provider_fields_but_keeps_user_sections -- --nocapture` — PASS, 1 passed, 3298 filtered out.

Known RED evidence from the approved exploration report:

- Temporary repro test `repro_sdd_startup_config_none_ssot_drops_live_user_tables` failed twice deterministically.
- Restored `config.toml` length was 0.
- `has_desktop=false`, `has_max=false`, and `has_sdd=false`.

Known baseline failure (Phase M admission gate), recorded by `docs/diagnostics/sdd-codex-user-mcp-overwrite-explore-2026-08-23.md`:

- `cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync sync_enabled_to_codex -- --nocapture` — exit code 101.
- Compiler errors in the untracked work-in-progress file `src-tauri/src/services/codex_plugin_registry.rs`: `enabled_codex_plugins` not found (lines 386, 494) and missing field `repair_action` in `RepairableCodexPlugin` initializers (lines 415, 514), plus one unused-import warning at line 17.
- Consequence: no MCP test can run until this compile error is fixed; Phase M Task 21 fixes it before any RED work.

---

## Phase A：数据防丢（最小可交付）

### Task 1: Add RED tests for fail-closed Codex writer semantics

**Files:**
- Modify tests only in `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consume `write_codex_live_atomic`, `write_codex_live_config_atomic`, and `write_codex_live_for_provider`.
- Produce assertions that `None` returns an error and leaves both live files byte-for-byte unchanged.

**Steps:**
- [ ] Add a test named `write_codex_live_atomic_rejects_missing_config_without_touching_files`; seed temporary `auth.json` and `config.toml`, call the writer with `config_text_opt=None`, and assert both files remain unchanged.
- [ ] Add a test named `write_codex_live_config_atomic_rejects_missing_config_without_touching_file`; seed temporary `config.toml`, call the config-only writer with `None`, and assert the file remains unchanged.
- [ ] Add a test named `write_codex_live_for_provider_rejects_missing_config_without_touching_live`; seed a non-empty live config with `[desktop]`, `[plugins."sdd@personal"]`, `[projects]`, `[marketplaces]`, and `[custom_user_table]`, call the provider writer with `config_text=None`, and assert the config remains unchanged.
- [ ] Run `cargo test --lib rejects_missing_config -- --nocapture`.
- [ ] Record the RED result in the ledger. The expected current failure is that the live config becomes empty or the call unexpectedly succeeds.

### Task 2: Add RED recovery tests for the SSOT missing-config path

**Files:**
- Modify tests only in `src-tauri/src/services/proxy.rs`

**Interfaces:**
- Consume `ProxyService::restore_live_config_for_app_with_fallback` with `TempHome` and `Database::memory()`.
- Produce a restored Codex config that preserves user-owned tables and removes takeover-owned fields.

**Steps:**
- [ ] Add a test named `codex_ssot_restore_missing_config_preserves_live_user_owned_tables`; use a schema-v2 MultiRouter current provider with no `settings_config.config`, no live backup, and a taken-over live config containing `[desktop].enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "ultra", "max"]`, `[plugins."sdd@personal"]`, `[projects]`, `[marketplaces]`, and `[custom_user_table]`.
- [ ] Assert the restore call succeeds, the restored config is not empty, every user-owned table remains present, `max` remains present, `sdd@personal` remains present, `PROXY_MANAGED` is absent, and `http://127.0.0.1:` is absent.
- [ ] Add a test named `codex_ssot_restore_missing_config_without_takeover_leaves_empty_live_untouched`; seed an empty or absent live config, no backup, and a config-less current provider; assert the recovery does not create a new empty config as a side effect.
- [ ] Add a test named `codex_proxy_placeholder_backup_with_configless_ssot_cleans_takeover_and_preserves_user_tables`; seed a placeholder backup, a taken-over live config with user-owned tables, and a config-less current provider; assert the placeholder backup is not written back and the cleanup result preserves user-owned tables.
- [ ] Run `cargo test --lib codex_ssot_restore_missing_config -- --nocapture`.
- [ ] Run `cargo test --lib codex_proxy_placeholder_backup_with_configless_ssot -- --nocapture`.
- [ ] Record the RED result in the ledger. The expected current failure is an empty restored config.

### Task 3: Implement the fail-closed Codex write boundary

**Files:**
- Modify `src-tauri/src/codex_config.rs`

**Interfaces:**
- Add one shared localized error helper for missing Codex live config input.
- Keep `Some(&str)` as the only valid config payload for both atomic writers.
- Keep `Some("")` available for explicit empty-provider semantics.

**Steps:**
- [ ] Change `write_codex_live_atomic` so `config_text_opt=None` returns the shared missing-config error before reading old bytes or writing either file.
- [ ] Change `write_codex_live_config_atomic` so `config_text_opt=None` returns the shared missing-config error before writing `config.toml`.
- [ ] Add the same missing-config guard at the start of `write_codex_provider_live_with_catalog_and_provider_context` and `write_codex_provider_live_with_catalog_without_provider_context`, before either function prepares catalog files, managed agent files, or cache projections.
- [ ] Change `write_codex_live_for_provider` so `config_text=None` returns the shared missing-config error before unified-session injection, auth decisions, catalog work, or file writes.
- [ ] Change `write_codex_provider_config_only_with_catalog_and_provider_context` so a missing provider config uses an explicit live-as-base cleanup helper; do not use `unwrap_or("")` to hide the missing input.
- [ ] Keep `merge_codex_provider_config_texts` as the live-as-base merge implementation and do not add a database snapshot.
- [ ] Run `cargo test --lib rejects_missing_config -- --nocapture` and verify Task 1 is GREEN.
- [ ] Run `cargo test --lib merge_provider_config_preserves_live_user_sections -- --nocapture` and `cargo test --lib merge_empty_official_config_clears_provider_fields_but_keeps_user_sections -- --nocapture`.

### Task 4: Make SSOT recovery use the safe fallback without empty writes

**Files:**
- Modify `src-tauri/src/services/proxy.rs`
- If a small shared read helper is needed, modify `src-tauri/src/services/provider/live.rs`

**Interfaces:**
- `restore_live_from_ssot_for_app` must return `Ok(false)` or an error before a config-less Codex provider can reach the live writer.
- `restore_live_config_for_app_with_fallback_inner` must then use the existing placeholder cleanup path.
- Placeholder cleanup must preserve user-owned tables and remove only takeover-owned fields.

**Steps:**
- [ ] Add a narrow Codex preflight in `restore_live_from_ssot_for_app` or in the effective-settings construction used by it; the preflight must inspect the provider after common config application and reject a missing `config` string before catalog side effects.
- [ ] Log that SSOT recovery is unavailable because the provider config is missing; do not log that SSOT restore succeeded in this case.
- [ ] Let the existing placeholder cleanup path handle the taken-over live config.
- [ ] Run `cargo test --lib codex_ssot_restore_missing_config -- --nocapture` and verify Task 2 is GREEN.
- [ ] Run `cargo test --lib codex_proxy_placeholder_backup_with_configless_ssot -- --nocapture`.
- [ ] Run `cargo test --lib codex_restore_from_backup_preserves_live_desktop_settings -- --nocapture` and `cargo test --lib restore_falls_through_to_ssot_when_backup_is_proxy_placeholder -- --nocapture`.

### Task 5: Add the normal Provider switch preflight

**Files:**
- Modify `src-tauri/src/services/provider/mod.rs`
- Modify `src-tauri/src/services/provider/live.rs` if the preflight helper belongs with effective-settings construction
- Modify `src-tauri/tests/provider_service.rs`

**Interfaces:**
- Add a Codex normal-switch preflight that consumes `AppState`, the target `Provider`, and the current common config.
- Produce either `Ok(())` before any switch mutation or a localized error that identifies the missing `config` field.

**Steps:**
- [ ] Add an integration test named `provider_service_switch_codex_missing_config_returns_error_and_preserves_state`; seed an old current provider, a writable live config, and a target Codex provider with object `auth` but no `config`.
- [ ] Call `ProviderService::switch` for the target and assert it returns an error.
- [ ] Assert the settings current provider, database current provider, live `auth.json`, and live `config.toml` all remain on the old state.
- [ ] Run the new integration test and record RED if the current implementation switches state or clears live config.
- [ ] Implement the preflight before the backfill and before both current-provider writes in `ProviderService::switch_normal`.
- [ ] Ensure common config is applied before deciding that `config` is missing, so a shared snippet that produces a config remains valid.
- [ ] Run `cargo test --test provider_service provider_service_switch_codex_missing_config_returns_error_and_preserves_state -- --nocapture`.
- [ ] Run `cargo test --test provider_service provider_service_switch_codex_updates_live_and_config -- --nocapture`.

### Task 6: Add optimistic concurrency to Codex live atomic writes

**Files:**
- Modify `src-tauri/src/codex_config.rs`

**Interfaces:**
- Add a fingerprint helper (length plus content hash) for the live `config.toml`.
- Produce a `ConcurrentModificationDeferred` error variant (or localized error string) that callers can distinguish from validation errors.

**Steps:**
- [ ] Add a RED test named `write_codex_live_config_atomic_rechecks_fingerprint_before_replace`; use a test hook or a slow-merge seam so the live file is externally modified between merge and replace; assert the writer re-reads and merges against the new content, and the externally written user table survives.
- [ ] Add a RED test named `write_codex_live_config_atomic_defers_after_bounded_retries`; arrange for the fingerprint to change on every attempt; assert the writer stops after the bounded retry count, leaves the file with the last external content, and returns the deferred error.
- [ ] Implement fingerprint capture at read time, recheck immediately before the atomic replace, re-read plus re-merge on mismatch, and a bounded retry limit of 2.
- [ ] Apply the same guard to `write_codex_live_atomic` for the config payload; `auth.json` keeps its existing atomic replace without the merge loop.
- [ ] Run `cargo test --lib fingerprint -- --nocapture` and verify both new tests are GREEN.
- [ ] Run `cargo test --lib rejects_missing_config -- --nocapture` and `cargo test --lib codex_restore_from_backup_preserves -- --nocapture`.

### Task 7: Expand user-table proof and complete the Phase A verification gate

**Files:**
- Modify tests in `src-tauri/src/codex_config.rs`
- Modify tests in `src-tauri/src/services/proxy.rs`

**Interfaces:**
- Existing merge tests must prove that current live values win for user-owned tables.
- New recovery tests must prove unknown user tables survive.

**Steps:**
- [ ] Extend `merge_provider_config_preserves_live_user_sections` or add a neighboring test so `[plugins."sdd@personal"]`, `[marketplaces]`, and `[custom_user_table]` are explicitly asserted.
- [ ] Extend the backup restore coverage so a healthy backup cannot roll back newer live values under `[desktop]`, `[plugins]`, `[projects]`, `[marketplaces]`, and `[custom_user_table]`.
- [ ] Add a test for a syntactically corrupt live config asserting the recovery reports an error and does not overwrite the file with a new empty config.
- [ ] Run `cargo test --lib codex_ssot_restore_missing_config -- --nocapture`.
- [ ] Run `cargo test --lib codex_proxy_placeholder_backup_with_configless_ssot -- --nocapture`.
- [ ] Run `cargo test --lib rejects_missing_config -- --nocapture`.
- [ ] Run `cargo test --lib fingerprint -- --nocapture`.
- [ ] Run `cargo test --lib codex_restore_from_backup_preserves -- --nocapture`.
- [ ] Run `cargo test --lib merge_provider_config -- --nocapture`.
- [ ] Run `cargo test --test provider_service provider_service_switch_codex -- --nocapture`.
- [ ] Run `cargo fmt --check`.
- [ ] Run `git diff --check`.
- [ ] Record every command, exit status, and relevant output in the recovery ledger; Phase A must be fully GREEN before Phase B starts.

---

## Phase B：启动自动恢复与可见反馈

### Task 8: Classify run-marker evidence before destructive recovery

**Files:**
- Modify `src-tauri/src/app_exit_monitor.rs`
- Modify `src-tauri/src/lib.rs`

**Interfaces:**
- Add a classification function consuming the previous `RunMarker`, the panic and crash-log evidence, and a liveness check for the recorded PID; produce one of `NoPreviousRun`, `UncleanExit`, `ConfirmedCrash`, `PlannedRestartOrUpdate`, `ActivePreviousInstance`.
- `recover_from_crash` in `src-tauri/src/services/proxy.rs` consumes the classification; only `UncleanExit` and `ConfirmedCrash` may enter takeover recovery.

**Steps:**
- [ ] Add RED tests in `app_exit_monitor.rs` for each classification branch, using temporary config dirs and a fake PID liveness probe injected as a function parameter.
- [ ] Implement the classification in `app_exit_monitor.rs`; keep the marker lifecycle (`record_startup`, `record_clean_exit`, `record_forced_exit`) unchanged.
- [ ] Wire the classification into the `recover_from_crash` call site in `src-tauri/src/lib.rs` (line 1243) so `ActivePreviousInstance` and `PlannedRestartOrUpdate` skip destructive recovery and only log.
- [ ] Run `cargo test --lib app_exit_monitor -- --nocapture` and verify GREEN.
- [ ] Run `cargo test --lib codex_ssot_restore_missing_config -- --nocapture` to confirm Phase A tests still pass.

### Task 9: Probe port 15721 ownership before takeover decisions

**Files:**
- Modify `src-tauri/src/proxy/types.rs`
- Modify `src-tauri/src/proxy/handlers.rs`
- Modify `src-tauri/src/services/proxy.rs`

**Interfaces:**
- Extend the `/status` payload with instance identity fields (`app`, `version`, `pid`, optional instance id) using `#[serde(default)]` for backward compatibility.
- Add an async probe helper consuming a port and producing `CompatibleInstance`, `UnknownOwner`, or `Unreachable`.
- `set_takeover_for_app` and startup takeover restore consume the probe result when the initial bind fails.

**Steps:**
- [ ] Add a RED test that starts a minimal axum listener on an ephemeral port serving the extended `/health` and `/status`, then asserts the probe returns `CompatibleInstance`.
- [ ] Add a RED test where the listener serves a non-CCSM response and assert the probe returns `UnknownOwner`; add a RED test with no listener and assert `Unreachable`.
- [ ] Extend `ProxyStatus` (or the `/status` handler payload) with the identity fields and populate them at server startup.
- [ ] Implement the probe with a short timeout; require both `/health` success and identity match for `CompatibleInstance`.
- [ ] Change takeover restore so `CompatibleInstance` records outcome `PortOwnedByCompatibleInstance` and skips takeover, while `UnknownOwner` or `Unreachable` records `PortOwnedByUnknownOwner`, exits the takeover flow, and surfaces a clear message; never terminate the owning process.
- [ ] Run `cargo test --lib port_probe -- --nocapture` and the takeover-related test filter, and verify GREEN.

### Task 10: Persist and expose the named recovery outcome

**Files:**
- Create `src-tauri/src/services/recovery_outcome.rs`
- Modify `src-tauri/src/services/proxy.rs`
- Modify `src-tauri/src/commands/mod.rs` (or the commands module that registers Tauri commands)

**Interfaces:**
- Define the outcome enum from the design spec (`HealthyBackupRestored`, `LivePreservedProviderRepaired`, `ProviderOnlyRestored`, `UserBackupCandidateFound`, `UnrecoverableUserTables`, `ConcurrentModificationDeferred`, `PluginRegistrationRepairAvailable/Completed/Failed`, `PortOwnedByCompatibleInstance/UnknownOwner`) plus kept-fields summary, lost-fields summary, suggested next step, and timestamp.
- Produce `record_recovery_outcome(outcome)` (persist first, then emit `codex-config-recovery-outcome`) and a Tauri command `get_last_recovery_outcome`.

**Steps:**
- [ ] Add RED tests for serialization, persistence to a temporary config dir, and read-back through the query path.
- [ ] Implement the outcome module; persist as JSON next to the app config logs directory using the same directory strategy as `app_exit_monitor`, with atomic write.
- [ ] Wire outcome recording into `restore_live_config_for_app_with_fallback_inner`, placeholder cleanup, the port-occupied branch from Task 9, and the concurrency-deferred branch from Task 6.
- [ ] Register the query command.
- [ ] Run `cargo test --lib recovery_outcome -- --nocapture` and verify GREEN.

### Task 11: Surface recovery outcomes in the UI

**Files:**
- Modify `src/App.tsx`
- Modify `src/hooks/useTauriEvent.ts` only if the existing hook cannot carry the payload type
- Modify `src/lib/api/` command bindings for `get_last_recovery_outcome`

**Interfaces:**
- Consume the `codex-config-recovery-outcome` event and the `get_last_recovery_outcome` command.
- Produce toast behavior: silent on full success, warning toast with kept/lost field summary on partial restore, error toast with next-step actions (open log directory, restore `.bak` candidate, repair plugin registration) on unrecoverable outcomes.

**Steps:**
- [ ] Add a Vitest component test asserting a warning toast renders for `ProviderOnlyRestored` and an error toast with actions renders for `UnrecoverableUserTables`; observe RED.
- [ ] Subscribe to the event in `App.tsx` next to the existing `proxy-official-warning` subscription; on mount, call `get_last_recovery_outcome` once to catch outcomes emitted before the frontend subscribed.
- [ ] Map each outcome kind to the toast severity and message from spec section 8.10; reuse the existing `openLogDir` API for the log-directory action.
- [ ] Run `pnpm test:unit` (or the focused Vitest file) and verify GREEN.

### Task 12: Detect and repair plugin registration inconsistency

**Files:**
- Create `src-tauri/src/services/codex_plugin_registry.rs`
- Modify `src-tauri/src/commands/mod.rs` (or the commands module that registers Tauri commands)
- Modify `src/App.tsx` for the repair prompt and button

**Interfaces:**
- Add a detection function consuming the Codex plugin cache dir, Codex live config, and `~/.agents/plugins/marketplace.json`; produce a list of repairable plugins (cache manifest valid, config enabled, marketplace entry missing).
- Add a repair command that validates manifest `name`, version, and marketplace source, requires the canonical path to stay under the plugins cache root, structurally merges into `marketplace.json` with dedupe by `name` and `source.path`, preserves existing entries, and replaces the file atomically.
- Emit `PluginRegistrationRepairAvailable/Completed/Failed` through the Task 10 outcome channel.

**Steps:**
- [ ] Add RED tests under `TempHome`: cache plus config enabled plus missing marketplace entry yields `PluginRegistrationRepairAvailable`; a manifest with a path outside the cache root is rejected; repair preserves a pre-existing entry such as `investment-signal-monitor`; repairing twice produces no duplicates.
- [ ] Implement detection and repair in the new module; never modify the config `[plugins]` enable state from this flow.
- [ ] Register the detect and repair Tauri commands.
- [ ] Wire the UI: when detection reports a repairable plugin, show a prompt with a repair button; on success show a completion toast; on failure show the reason.
- [ ] Run `cargo test --lib codex_plugin_registry -- --nocapture` and the relevant Vitest files; verify GREEN.

### Task 13: Phase B verification gate

**Files:**
- No new source files; verification only

**Steps:**
- [ ] Run `cargo test --lib app_exit_monitor -- --nocapture`.
- [ ] Run `cargo test --lib port_probe -- --nocapture`.
- [ ] Run `cargo test --lib recovery_outcome -- --nocapture`.
- [ ] Run `cargo test --lib codex_plugin_registry -- --nocapture`.
- [ ] Re-run every Phase A command from Task 7.
- [ ] Run `pnpm test:unit`.
- [ ] Run `cargo fmt --check` and `git diff --check`.
- [ ] Record every command, exit status, and relevant output in the recovery ledger.

---

## Approved adjacent bug addendum

世豪在 2026-08-23 明确要求 Luna 在本轮顺手修复 `docs/diagnostics/2026-08-23-fork-follow-up-bug-audit.md` 中的六项问题。下列任务必须保持独立测试和独立提交，不能混入前述四组上游 PR。

### Task 14: Protect the provider-delete projection path

- [ ] Add the RED test `deleting_shared_target_publishes_only_the_active_router` using two routers sharing one target and a profile-bound active router.
- [ ] Reuse the active-router ownership rule in the delete publish loop; inactive routers remain updated in the database but cannot publish the shared live catalog.
- [ ] Run focused mutation tests and record RED/GREEN/REFACTOR.

### Task 15: Tolerate missing schema-v2 routes

- [ ] Add a RED frontend test with `codexRouting.schemaVersion=2` and no `routes`; assert `readCodexRouting` returns an empty route list without throwing.
- [ ] Normalize missing routes with an empty array without weakening validation of present malformed route values.
- [ ] Run the focused workspace test and the existing missing-`modelSelection` regression.

### Task 16: Unify include-mode gates in secondary UI paths

- [ ] Add separate RED tests for routed catalog cards, route preview, and request-log route attribution.
- [ ] Reuse one pure include-aware match helper across `collectRoutedCatalogModels`, `handlePreviewRoute`, and `routeMatchesModel`.
- [ ] Confirm deselected prefix matches stay absent while exact included models and `mode=all` remain valid.

### Task 17: Repair an invalid persisted reasoning default

- [ ] Add the RED test `reasoning_declaration_recovers_when_default_points_to_removed_effort`.
- [ ] After persisted-effort cleanup, replace an invalid `default_effort` with an explicit remaining provider default or deterministic first legal effort; use `None` only when no legal effort remains.
- [ ] Revalidate the complete declaration and confirm it is not discarded.

### Task 18: Use the stable current provider in plan selection

- [ ] Extract a pure selection function or add a correct component seam and observe RED when proxy `activeProviderId` is empty but database `currentProviderId` identifies a routing plan.
- [ ] Use this order: explicit user selection/navigation target, proxy active provider, database current provider, deterministic sorted fallback.
- [ ] Pass `currentProviderId` from `App.tsx`; add component and pure-function regression coverage.

### Task 19: Remove fork-only model-order debug errors

- [ ] Remove or downgrade `[model-order]` diagnostic `console.error` calls while keeping real user error handling.
- [ ] Run focused model-order tests and `rg -n '\[model-order\]' src` to prove no accidental error-level debug logs remain.
- [ ] Do not create an upstream Issue/PR for this cleanup unless the same logs exist on the then-current upstream base.

### Task 20: Adjacent bug verification gate

- [ ] Run every focused test from Tasks 14 through 19.
- [ ] Run the full affected Rust library and workspace frontend suites in proportion to runtime.
- [ ] Run `cargo fmt --check`, frontend format/type checks, and `git diff --check`.
- [ ] Record all RED/GREEN/REFACTOR evidence and identify which changes are fork-only versus independently applicable upstream.

---

## Phase M：用户级 MCP 所有权对账（第三版已批准）

Phase M 依赖规格第 15 节。它不改动 Phase A/B 的契约，可以与 Phase A/B 共用 worktree，但提交边界必须独立。Phase M 的前置条件是世豪批准第三版规格。

### Task 21: 准入门——修改前快照与编译修复

**Files:**
- Modify `src-tauri/src/services/codex_plugin_registry.rs`（未跟踪的 Phase B 半成品，仅修编译错误，不改行为契约）

**Steps:**
- [ ] 创建修改前快照：在 worktree 之外（`D:\CCSwitchMulti\snapshots\2026-08-23-codex-startup-config-preservation\`）保存 `git diff` 全量输出（含 `git diff --stat`）和 3 个未跟踪文件（`src-tauri/src/commands/recovery.rs`、`src-tauri/src/services/codex_plugin_registry.rs`、`src-tauri/src/services/recovery_outcome.rs`）的完整副本；记录 HEAD `6923a996` 与 25 个未提交条目清单。禁止 `git reset`、`git clean`。
- [ ] 修复 `codex_plugin_registry.rs` 的编译错误：补上 `enabled_codex_plugins`（386、494 行）与 `RepairableCodexPlugin.repair_action` 字段（415、514 行），移除 17 行未使用的 `value as toml_value` 导入。只做到编译通过，不改变 Phase B 的检测/修复语义。
- [ ] 运行 `cargo check --manifest-path src-tauri/Cargo.toml --tests`，预期退出码 0；记录到账本，替换基线失败记录。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync sync_enabled_to_codex -- --nocapture`，确认现有（错误预期的）测试恢复可运行并记录结果。

### Task 22: 修正错误预期测试并新增 RED 对账测试

**Files:**
- Modify `src-tauri/tests/import_export_sync.rs`
- Modify `src-tauri/tests/provider_service.rs`
- Create `src-tauri/tests/mcp_codex_reconcile.rs`

**Interfaces:**
- Consume `cc_switch_lib::sync_enabled_to_codex`（或 Task 23 引入的对账入口）with `TempHome` and `Database::memory()`.
- Produce assertions that live-only entries survive every projection shape.

**Steps:**
- [ ] 把 `import_export_sync.rs:434-457` 的预期改为“空投影时 live-only 条目 `disabled` 保留、`[mcp_servers]` 表保留”；观察 RED。
- [ ] 把 `provider_service.rs:2495-2672`（`switch_codex_syncs_shared_keys_from_live_into_common_config`）的预期改为“切换后 live-only `echo` 保留；旧格式 `ghost-legacy` 迁移到 `[mcp_servers]` 后保留”；观察 RED。
- [ ] 在 `mcp_codex_reconcile.rs` 新增规格 15.6 测试矩阵的全部用例：空 DB+live-only、DB 启用项+live-only、同名同内容、同名不同内容、外部手改 managed id、显式禁用、显式删除、导入后所有权、SQL 导入移除 DB 行、旧格式迁移、无效 TOML 保持原字节、敏感字段只记 id。全部使用 `TempHome` + `Database::memory()`。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_codex_reconcile -- --nocapture`，记录 RED。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync sync_enabled_to_codex -- --nocapture` 和 `cargo test --manifest-path src-tauri/Cargo.toml --test provider_service switch_codex_syncs_shared_keys -- --nocapture`，记录 RED。

### Task 23: 实现 live-as-base 对账

**Files:**
- Modify `src-tauri/src/mcp/codex.rs`
- Modify `src-tauri/src/services/mcp.rs`
- Modify `src-tauri/src/database/dao/mcp.rs`（仅当现有 DAO 没有“返回全部 id（含未启用）”的读取方法时新增一个只读方法；不新增列、不做 schema 迁移）

**Interfaces:**
- `sync_enabled_to_codex` 改为（或新增）对账入口，输入为“数据库全部 id 集合 + 数据库启用 Codex 条目内容”，不再接受伪造的 `MultiAppConfig` 整表投影语义。
- `McpService::project_servers_to_app`（`services/mcp.rs:223-259`）的 Codex 分支改为传递全量 id 集合与启用条目。

**Steps:**
- [ ] 对账以当前 live 文档为底：upsert 数据库启用项（同名同内容跳过）；仅删除“id 在数据库且 Codex 未启用”的 live 条目；live-only 条目原样保留；空数据库不删除任何条目也不删除 `[mcp_servers]` 表。
- [ ] `sync_single_server_to_codex` 与 `remove_server_from_codex` 维持单条语义，与对账共用所有权判定。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_codex_reconcile -- --nocapture`，除旧格式迁移、并发修改外预期 GREEN。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_commands -- --nocapture`，确认 Claude 等非 Codex 路径回归通过。

### Task 24: 旧格式迁移保留与乐观并发接线

**Files:**
- Modify `src-tauri/src/mcp/codex.rs`
- Modify `src-tauri/src/codex_config.rs`（仅提升写入器可见性或新增闭包形态 `pub(crate)` 文档更新 API，不改 Phase A 已实现的契约）

**Steps:**
- [ ] 按规格裁定 17 实现 `[mcp.servers]` 先迁移后清理：live-only 条目迁入 `[mcp_servers]`（live 同 id 已存在时保留 live 版本），DB 拥有条目按 DB 版本写入，然后移除 `[mcp.servers]`。
- [ ] 把 `mcp/codex.rs` 的三个写入口改经 `write_codex_live_config_optimistic` 或其闭包形态 API：读指纹、应用变更、替换前复检、冲突重读重做、最多重试 2 次、超限保持原文件并返回 `ConcurrentModificationDeferred`；解析失败保持原文件字节不变。
- [ ] 新增并发修改 RED->GREEN 用例（模拟替换前外部写入），放在 `mcp_codex_reconcile.rs` 或 `codex_config.rs` 测试模块。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_codex_reconcile -- --nocapture`，预期全部 GREEN。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib fingerprint -- --nocapture`，确认 Phase A 并发测试不回退。

### Task 25: 触发路径一致性与敏感字段审计

**Files:**
- Modify `src-tauri/src/services/provider/mod.rs`、`src-tauri/src/services/provider/live.rs`（仅当发现绕过对账的 Codex MCP 写入时）

**Steps:**
- [ ] 用 `rg -n "sync_enabled_for_app|sync_all_enabled|sync_enabled_to_codex" src-tauri/src` 枚举全部触发点，逐一确认 Codex MCP 影响都汇聚到 Task 23 的对账函数；发现绕过点就收口。
- [ ] 用 `rg` 审计 MCP 日志与错误文案，确认只出现 `id` 和字段名，不出现 `env`、`headers`、`token` 值；必要时补一条含敏感字段的对账测试断言日志不含值。
- [ ] 运行供应商切换、设置保存、SQL 导入后置三类触发路径的集成测试（复用 `provider_service.rs`、`import_export_sync.rs` 已有套件），确认 live-only 保留。

### Task 26: Phase M 验证门

**Files:**
- No new source files; verification only

**Steps:**
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_codex_reconcile -- --nocapture`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync -- --nocapture`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test provider_service -- --nocapture`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_commands -- --nocapture`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp -- --nocapture`
- [ ] `cargo fmt --check` 与 `git diff --check`
- [ ] 把每条命令、退出码、关键输出记入恢复账本；Phase M 全绿后才允许进入第五组上游 PR 重构。

---

## Upstream PR and fork release decomposition

The implementation may be developed and verified together in an isolated worktree, but upstream delivery must be reconstructed as clean commits on fresh branches from the latest `origin/main`. Do not use the current `fix/issues-28-30-v3.19.2-15` branch or any historical PR head as a base.

1. **Upstream Issue/PR A — Codex config data preservation.** Include only Tasks 1 through 7: missing-config fail-closed semantics, live-as-base preservation, safe SSOT fallback, normal-switch preflight, optimistic concurrency, and their tests.
2. **Upstream Issue/PR B — Startup evidence and port ownership.** Include only Tasks 8 and 9: marker classification, compatible-instance identity, port probing, and their tests. Do not mix UI or plugin repair.
3. **Upstream Issue/PR C — Recovery outcome UX.** Include only Tasks 10 and 11: persisted named outcomes, query/event wiring, localized UI feedback, and tests/i18n.
4. **Upstream Issue/PR D — Codex plugin registration consistency.** Include only Task 12: three-source detection, validated repair command, repair UI, and tests. This PR must not change provider switching or crash recovery.
5. **Upstream Issue/PR E — Codex MCP ownership reconciliation.** Include only Tasks 21 through 26: the live-as-base reconcile, legacy-format migrate-then-clean, optimistic-concurrency wiring for MCP writes, the corrected test expectations in `import_export_sync.rs` and `provider_service.rs`, and the new `mcp_codex_reconcile.rs` suite. The compile-error fix in `codex_plugin_registry.rs` (Task 21) belongs to the Phase B code line, not to PR E. This PR must not change startup recovery, marker classification, port probing, or recovery-outcome UX.

For each group:

- [ ] Create a new upstream Issue with symptom, minimal reproduction, confirmed root cause, compatibility scope, and expected behavior.
- [ ] Create a unique branch from the then-current upstream `main`; never push to PR #35/#37/#39/#41/#43/#45/#47/#49 heads.
- [ ] Apply only the group's verified commits and confirm the diff contains no fork release metadata or unrelated user files.
- [ ] Run the group's focused tests plus the relevant shared regression gate.
- [ ] Open a new focused PR using the repository template and link only its matching Issue.

After all fork-required groups are verified, create a separate fresh fork integration/release branch, cherry-pick the exact verified commits, add fork-only version/release-note changes in a separate commit, push the branch to `zhushihao/ccswitchmulti`, and push a new `v*` tag to that fork. The tag triggers `.github/workflows/release.yml`. Do not tag or publish from `BigStrongSun/ccswitchmulti`.

Immediately before that integration branch and again before tagging:

- [ ] Run `git fetch origin --prune` and query the latest stable upstream Release.
- [ ] If upstream moved, rebuild the integration branch from the new `origin/main`, reapply only verified commits, and rerun all affected gates.
- [ ] Query existing fork releases and choose the next suffix for the current upstream version. With upstream `v3.19.2-16` and fork already at `v3.19.2-16.2`, the next tag is `v3.19.2-16.3`; if upstream advances to `v3.19.2-17`, the next fork tag becomes `v3.19.2-17.1`.
- [ ] Confirm the tag and release target are `zhushihao/ccswitchmulti`, never `BigStrongSun/ccswitchmulti`.

---

## Phase C：深层兼容（不在本次实现范围）

- 正式跨进程文件锁或与 Codex `expectedVersion` 的协同协议。
- 完整用户配置快照与历史恢复来源。
- 旧 schema、schema-v2 MultiRouter 完整非接管配置投影、插件安装协议的长期迁移。

Phase C 只保留契约占位，不写实现任务；对应需求进入后续 SDD 周期。

## Handoff Brief For Luna

- Approved spec path: `docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`.
- Implement Phase A Tasks 1 through 7 in order, then Phase B Tasks 8 through 13 in order. Phase A must pass its Task 7 gate before Phase B starts.
- The first acceptable evidence is a RED test proving the current empty-write failure; do not skip RED because the exploration report already contains a repro.
- The implementation must not create a user-config snapshot table.
- The implementation must preserve unknown top-level TOML tables, not only the four named tables.
- If `write_codex_provider_config_only_with_catalog_and_provider_context` needs empty-provider cleanup, make that choice explicit at the call site or helper name.
- If any caller intentionally needs auth-only writing, add a separately named auth writer; do not reuse `None` config to mean auth-only.
- Port probing must never kill a process; `CompatibleInstance` requires both `/health` success and identity match.
- Plugin repair defaults to a repair button; do not implement silent auto-repair unless 世豪 explicitly approves that upgrade.
- Keep implementation commits separable into the four upstream PR groups above. Do not include fork-only release changes in product-code commits.
- Report status must be one of `DONE`, `DONE_WITH_CONCERNS`, `NEEDS_CONTEXT`, or `BLOCKED`, with commands and outputs for each test gate.

## Handoff Brief For Luna（Phase M，待世豪批准第三版规格后生效）

- 先执行 Task 21 准入门：worktree 外快照（tracked diff + 3 个未跟踪文件，禁止 reset/clean），再修 `codex_plugin_registry.rs` 编译错误；测试门不恢复，不允许写任何 RED。
- Task 22 必须先观察 RED：两个既有错误预期测试改成保留语义后必须失败，新 `mcp_codex_reconcile.rs` 套件必须失败。
- 对账语义一句话：数据库 id 集合即所有权；live-only 默认保留；只删“在库但未启用 Codex”的 id；空库不清表。
- 旧格式 `[mcp.servers]` 先迁移后清理；任何路径不得直接清掉旧格式内容。
- 三个写入口（对账、单条 upsert、单条删除）都走乐观并发写入器；解析失败或并发超限保持原文件字节不变。
- 不新增数据库列、不做 schema 迁移；DAO 只允许新增只读方法。
- 不新增 commands/API/UI；同名冲突只写文档和日志，且日志只含 id。
- Phase M 的提交边界独立于 Phase A/B 和相邻 bug 附录；对应上游第五组 Issue/PR E。

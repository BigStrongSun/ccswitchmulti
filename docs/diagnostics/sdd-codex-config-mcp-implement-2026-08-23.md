# SDD Codex 配置防丢与用户级 MCP 兼容性实现报告

日期：2026-08-23  
状态：`DONE_WITH_CONCERNS`  
实现 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`  
基线 HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`

## 结论

第三版已按“兼容性设计”实现。Codex live 配置现在以当前文件为基底进行受限对账，CCSwitchMulti 只修改数据库能够证明拥有的 MCP id；用户级 live-only MCP、旧格式 MCP、已有用户表和并发写入现场不会被整表投影静默清空。  

本轮没有新增“同步 MCP”按钮、Tauri command、前端 API 或 UI。用户看到的同步仍然是供应商切换、供应商保存、设置保存、SQL/云同步后置、会话开关和通用配置片段保存等后端流程自动触发的对账。

第 7 项 prefix-only legacy migration 和第 8 项 fail-closed/defaultRouteId 一致化已经在本 worktree 完成。两项都按“保留旧数据、收窄运行时边界、明确未来行为”的兼容性原则实现。仓库既有格式基线已单独记录，不影响本轮功能交付。

## 已实现的兼容性契约

### MCP 所有权与对账

- 数据库 `mcp_servers` 表的全部 id 集合代表 CCSwitchMulti 管理权，包含当前未启用 Codex 的行。
- 对账从当前 Codex `config.toml` 出发；live-only id 原样保留，空数据库不删除 `[mcp_servers]`。
- 只删除“数据库存在但 Codex 未启用”的 id；显式禁用/删除的 managed id 可以被移除。
- managed 同名且内容不同由数据库版本覆盖；managed 同名且 TOML 语义相同不重写，保留用户注释、空白和键顺序。
- 旧 `[mcp.servers]` 在删除旧容器前先迁移到 `[mcp_servers]`，其中数据库不认识的条目保留。
- `sync_enabled_to_codex`、单条 upsert、单条 delete 和 `McpService` 全量/定向投影使用同一 live-as-base 语义。

### 并发与失败安全

- MCP 三个写入口使用 `write_codex_live_config_reconcile`：读取指纹、构造候选、替换前复检，冲突最多重试两次。
- 解析失败、写入失败或超过重试次数时保留原 live 文件字节，并返回 `ConcurrentModificationDeferred` 或对应解析错误。
- 代理/恢复的 `write_codex_live_snapshot` 原先有三处 raw `write_text_file`，会在回滚或 OAuth/空 auth 分支覆盖当前 live；现在统一走乐观写入器。
- 代理快照正常合并路径以最新 live 为基底；精确回滚路径只补入当前 live 的 MCP 缺失条目，从而同时恢复 provider 字段并保留并发新增的 live-only MCP。
- 日志和错误只出现 MCP id、字段名或通用错误，不写入 `env`、`headers`、`token` 等敏感值。

### Codex MultiRouter 第 7/8 项兼容性增补

- prefix-only 旧路由在迁移预览时按目标 Provider 当前可见 `modelCatalog.models` 展开为精确 `include`；不会扩大成 `mode=all`，也不会把未来新增模型自动加入既有路由。
- prefix 比较会 trim、去空、去重并忽略大小写；旧 `modelMap` alias key 可以命中当前 catalog 条目，但 include 始终写入可见 `model`，alias target 仍必须通过编译校验。
- prefix-only 目标目录为空或没有任何可见模型/有效 alias 命中时分别返回稳定错误码 `prefix_selection_catalog_empty` 和 `prefix_selection_no_matches`，不再暴露 `include_models_empty`。
- 迁移预览使用 `prefix_selection_frozen` 和 `prefix_selection_no_current_matches` 警告码；正文已改为中文，明确“未来新增模型不会自动加入”，并指导刷新目录后重新勾选、保存。
- 当前 V2 未命中请求继续 fail-closed；设置页删除可编辑的“默认路由”下拉框。旧 `defaultRouteId` 只读展示并在普通保存时保留有效值，但不参与当前运行时兜底；新建向导计划不再主动写入该字段，更新旧 V2 计划仍保留它。
- 诊断增加 `default_route_legacy_ignored` 警告，发布检查和预览均明确“未命中请求将被拒绝”。

## RED/GREEN 证据

- 新增 `semantically_same_managed_entry_preserves_comments_and_formatting`。临时恢复旧的字符串比较后，测试失败并显示生成表删除了顶层、条目和行内注释；接入 TOML 语义比较后测试通过。
- 新增 `codex_snapshot_write_rebases_on_latest_live_mcp`。该测试验证代理快照应用 provider 字段时保留当前 live-only MCP，并保留快照 MCP。
- 既有空数据库、供应商切换、旧格式迁移、非法 TOML、指纹冲突和敏感字段测试均已改为兼容性预期。
- 第 7 项 RED：旧 prefix-only 迁移回归先观察到 6 个用例因 `include_models_empty` 失败；GREEN 后 prefix-only 专项 6/6、prefix 展开 1/1、混合 models+prefix 1/1 通过，并覆盖空目录、无匹配、alias、未来目录刷新、幂等。
- 第 8 项 RED：页面旧测试仍寻找可编辑“默认路由”下拉框；按新契约改为只读兼容字段并验证保存保留旧 id 后，页面文件 73/73 通过。wizard 新增“新计划不写字段”和“旧 V2 保留字段”两项，4/4 通过。
- REFACTOR：只格式化本轮修改的 Rust/前端文件；全量格式检查仍只剩既有 `CodexFormFields.tsx` 和 4 个 Rust 基线文件，未顺手改动它们。

### Provider 写入阶段 A 复核：merge 入口竞态

替代验证 Sol 在 `sdd-verify` 阶段 A 发现了一个重要的 TOCTOU 竞态，原先的绿灯不能覆盖该时序：provider 路径先读取 live、完成 `merge_codex_provider_config_texts`，再进入首次指纹检查。若 Codex Desktop 在“merge 完成后、首次 fingerprint 前”写入，首次 fingerprint 会把新文件当作基底确认，随后仍用旧候选替换它，用户表和用户级 MCP 会被旧快照覆盖。

- RED：
  `cargo test --lib write_codex_live_for_provider_preserves_change_after_provider_merge_before_writer_entry -- --nocapture`
  退出码 1，`0 passed / 1 failed`，失败断言为 `restored.contains("external = true")`。
- 根因修复：provider merge 不再发生在 writer 入口前；`write_codex_live_for_provider` 改为把 merge 放进 `write_codex_live_config_reconcile` 的每轮 live 快照闭包，连续冲突时重新基于最新字节构造候选。官方 auth 路径通过 `write_codex_auth_with_reconciled_config` 在 config reconcile 失败或超限时回滚旧 `auth.json`；config-only、代理快照和接管恢复路径统一复用该复检 writer。无用的入口前 merge helper 已移除。
- GREEN：上述聚焦测试 `1 passed / 0 failed`。新增两项补强回归均通过：
  `provider_write_defers_after_bounded_merge_conflicts_and_rolls_back_auth`（连续三次冲突保留第三版外部 config、返回 `ConcurrentModificationDeferred`、auth 回滚）和
  `provider_write_rejects_invalid_live_toml_without_touching_config_or_auth`（非法 live TOML 保持原 config 字节、官方 auth 回滚）。
- REFACTOR：仅在 `src-tauri/src/codex_config.rs`、`src-tauri/src/services/proxy.rs` 收口 provider 入口与写入器；测试注入队列只在 `#[cfg(test)]` 编译，不访问真实 `~/.codex`/`~/.agents`。

### MultiRouter 投影阶段 A-2 复核：变换入口竞态与副作用回滚

替代验证 Sol 又定位到两条同类 TOCTOU 入口：`force_codex_builtin_openai_live_provider` 和
`publish_codex_multirouter_projection` 原先都在 writer 外完成整份候选配置，再进入首次指纹检查。
若 Codex Desktop 在变换完成后、writer 入口前写入，首次 fingerprint 会确认新文件而旧候选仍会覆盖用户版本。

- RED：`force_builtin_openai_preserves_change_after_transform_before_writer_entry` 与
  `multirouter_projection_preserves_change_after_transform_before_writer_entry` 均真实捕获到
  `external = true` 被旧候选覆盖，分别为 `0 passed / 1 failed`。
- 根因修复：force-transform 和 projection prepare 均移入
  `write_codex_live_config_reconcile` 的每轮 live 快照闭包；每次冲突都从最新字节重新生成候选，
  不再把变换结果跨越首次 fingerprint 带入 writer。新增
  `CodexProjectionSideEffectsSnapshot`，在 config reconcile 失败、非法 TOML 或并发超限时恢复
  catalog、models cache、cache backup 与本轮新增的 CCSM managed agent 文件；成功时保留投影副作用。
- GREEN：两项变换入口竞态均 `1/1`；`multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects`
  与 `multirouter_projection_invalid_live_toml_keeps_catalog_and_cache_bytes` 均 `1/1`。MultiRouter
  projection `10/10`、mutation `7/7`、provider `1353/1353`、proxy `90/90`、MCP `68/68` 和
  MCP commands `23/23` 全部通过。
- REFACTOR：本轮只收口 `src-tauri/src/codex_config.rs`；副作用快照只在投影写入路径创建，
  不改变前端 API，也不新增“同步 MCP”按钮或 Tauri command。

### MultiRouter 投影阶段 A-3 复核：agent 文件所有权回滚

替代验证 Sol 在 A-2 复核中发现，`CodexProjectionSideEffectsSnapshot` 虽然删除新文件时检查了
managed marker，却曾经把 agents 目录中的所有普通文件读入快照并在失败时无条件回写。用户若在
投影变换与第三次冲突之间编辑同名自定义 agent，失败回滚会把用户新内容覆盖。

- RED：临时恢复旧的“快照所有普通 agent 文件”行为后，
  `multirouter_projection_conflict_preserves_user_agent_edit_and_owned_agent_rollback` 退出码 101，
  `0 passed / 1 failed`；断言显示实际内容为 `description = "user original"`，而并发版本为
  `description = "user concurrent edit"`。
- 根因修复：snapshot capture 现在只读取 `codex_agent_file_is_cc_switch_managed` 判定为 CCSwitchMulti
  所有的 agent；用户文件仍可参与同名占用判断，但不会被捕获、回写或删除。restore 只恢复原 managed
  文件；本轮新建文件仍只有在 marker 判定为 managed 时才删除。
- GREEN：A-3 专项 `1/1`；验证同时覆盖同名用户文件、原 managed 文件、projection 新建 managed 文件、
  连续三次 config 冲突、catalog/cache 回滚；A-2 projection 专项升为 `5/5`，managed-agent 专项 `4/4`。
- REFACTOR：新增测试注入仅在 `#[cfg(test)]` 下写入临时 agents 目录，不访问真实 `~/.agents`；生产代码只
  收窄快照所有权过滤，没有新增 UI/API。

## 验证结果

在隔离临时 HOME 与内存数据库下运行：

| 验证门 | 结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| 全量 Rust library tests | 3335 passed / 0 failed / 5 ignored |
| `codex_multirouter::migration::tests` | 13 passed / 0 failed |
| `codex_multirouter::compiler` | 15 passed / 0 failed |
| `codex_multirouter::schema` | 6 passed / 0 failed |
| fail-closed runtime named tests | 1 + 1 passed / 0 failed |
| `cargo test --lib mcp`（实现收尾后） | 68 passed / 0 failed |
| `mcp_codex_reconcile` | 11 passed / 0 failed |
| `services::proxy::tests` | 90 passed / 0 failed |
| `provider_service` | 37 passed / 0 failed |
| `import_export_sync` | 26 passed / 0 failed |
| `mcp_commands` | 23 passed / 0 failed |
| `--lib fingerprint` | 13 passed / 0 failed |
| 前端 `pnpm typecheck` | PASS |
| `CodexRouterWorkspacePage.test.ts` | 73 passed / 0 failed |
| `codexMultiRouterWizard.test.ts` | 4 passed / 0 failed |
| 前端全量 Vitest | 144 files / 1177 passed / 0 failed |
| `git diff --check` | PASS |

TOCTOU 修复后的补充门：A-3 user-agent ownership `1/1`、A-2 projection `5/5`、managed-agent `4/4`、`--lib fingerprint` 13/13、provider library filter 1353/1353、provider_service 37/37、services::proxy 90/90、`--lib mcp` 68/68、`mcp_codex_reconcile` 11/11、mcp_commands 23/23、import/export 26/26、并发与 provider 回归 7/7、`cargo check --lib` PASS、`pnpm typecheck` PASS、全量 Rust library 3335/3335（5 ignored）。`import_export_sync` 期间出现一次既有临时测试目录清理的 Windows `os error 32` 日志，但 26/26 仍通过；前端全量测试仍有既有 React `act(...)`、MSW 未匹配 Tauri command、jsdom Tauri window 警告，无失败。

前端依赖原本不存在。为执行验证，使用了 `pnpm install --offline --frozen-lockfile --ignore-scripts`，只复用本机 pnpm 缓存并生成被 `.gitignore` 忽略的 `node_modules`；未联网、未修改 lockfile、未改版本号、未写真实 `~/.codex` 或 `~/.agents`。

## 格式基线

`cargo fmt --check` 仍会报告 HEAD 既有、不是本轮新增的：

- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/proxy/providers/codex.rs`
- `src-tauri/src/proxy/providers/openai_compat.rs`
- `src-tauri/src/services/provider/live.rs`

前端 `pnpm format:check` 只剩未改动的 `src/components/providers/forms/CodexFormFields.tsx`。本轮改动文件已单独运行 Prettier，`git diff --check` 通过。

## 当前工作树清单

当前 worktree 有 30 个已修改文件（包含本轮之前的 Phase A/B/Phase M 改动）；本轮 TOCTOU 收口实际触及 `src-tauri/src/codex_config.rs` 与 `src-tauri/src/services/proxy.rs`，两项补强回归也位于 `src-tauri/src/codex_config.rs`：

```text
src-tauri/src/codex_config.rs
src-tauri/src/services/proxy.rs
```

完整工作树清单如下：

```text
src-tauri/src/app_exit_monitor.rs
src-tauri/src/codex_config.rs
src-tauri/src/codex_multirouter/migration.rs
src-tauri/src/codex_multirouter/mutation.rs
src-tauri/src/codex_multirouter/projection.rs
src-tauri/src/commands/mod.rs
src-tauri/src/commands/proxy.rs
src-tauri/src/lib.rs
src-tauri/src/mcp/codex.rs
src-tauri/src/mcp/mod.rs
src-tauri/src/proxy/providers/codex_reasoning.rs
src-tauri/src/proxy/server.rs
src-tauri/src/proxy/types.rs
src-tauri/src/services/mcp.rs
src-tauri/src/services/mod.rs
src-tauri/src/services/provider/mod.rs
src-tauri/src/services/proxy.rs
src-tauri/tests/import_export_sync.rs
src-tauri/tests/provider_service.rs
src/App.tsx
src/components/codex/CodexRouterWorkspacePage.test.ts
src/components/codex/CodexRouterWorkspacePage.tsx
src/i18n/locales/en.json
src/i18n/locales/ja.json
src/i18n/locales/zh-TW.json
src/i18n/locales/zh.json
src/lib/api/index.ts
src/lib/api/settings.ts
src/lib/codexMultiRouterWizard.test.ts
src/lib/codexMultiRouterWizard.ts
```

当前有 4 个未跟踪 Rust 文件：

```text
src-tauri/src/commands/recovery.rs
src-tauri/src/services/codex_plugin_registry.rs
src-tauri/src/services/recovery_outcome.rs
src-tauri/tests/mcp_codex_reconcile.rs
```

修改前快照位于 `D:\CCSwitchMulti\snapshots\2026-08-23-codex-startup-config-preservation\`，包含 HEAD、状态、tracked diff 和三个当时未跟踪 Rust 文件的完整副本。实现期间没有使用 `reset`、`clean`、`checkout --`，没有重启或结束 Codex，也没有 push、发布或创建 PR。

## 后续边界

- 同名 MCP 冲突的界面可见提示、provenance 列和正式跨进程锁仍是后续规格项。

## Luna 第三版兼容性收口证据（待 Sol 独立复核）

本节记录 Luna 对 provider 快照 MCP 来源边界的最终收口和定向验证，属于实现证据，尚未由 Sol 独立复核，不能单独宣称验证阶段通过。

- 通用 `merge_codex_provider_config_texts()` 不再无条件剥离 MCP，避免误删公共配置片段新增的 `[mcp_servers.*]`。
- 普通有效配置构建先在 provider 快照克隆上剥离旧 MCP，再应用公共配置片段；provider 数据库原始快照不变，live-only MCP 仍以 current live 和数据库所有权对账。
- 代理备份刷新使用保留 provider MCP 的专用构建路径，因为备份是显式恢复快照；正常 live 写入仍走默认剥离边界。
- provider projection writer 先与最新 live 合并，再应用 bearer token，兼容剥离后只剩 MCP 的旧 provider 配置。
- 定向结果：两个公共配置 MCP 回归各 `1 passed / 0 failed`；`provider_commands` 为 `10 passed / 0 failed`。
- 最终 Rust 门（当前工作树）：`cargo check --manifest-path src-tauri/Cargo.toml --tests` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib --quiet` 为 `3342 passed / 0 failed / 5 ignored`；`git diff --check` 通过。

## Luna 第四轮修复证据（待 Sol 独立复核）

本节是第四轮兼容性收口的实现证据。Luna 已完成修复和本地回归，但 Sol 的独立复核仍保持 `BLOCKED`，因此本节不把实现证据改写成最终审核通过。第四轮继续遵守“当前 live 是事实来源、每个写入尝试只认领自己确切写出的字节、外部版本优先保留”的兼容性原则。

### 四项 RED/GREEN

| 兼容性问题 | RED 证据 | GREEN 证据 |
|---|---|---|
| attempt receipt 会把写入后出现的第三方版本误认成本次输出 | `committed_attempt_does_not_claim_external_write_after_replace_before_receipt` 在旧实现中可把写后外部版本纳入 `after_fingerprint`，回滚会覆盖第三方字节 | 同名回归 `1 passed / 0 failed`；receipt 现在直接使用实际 candidate bytes 计算 fingerprint，不通过写后重读认领外部版本 |
| catalog/cache/backup/managed agent 的 companion 缺少 capture 指纹条件提交 | `provider_projection_cache_does_not_overwrite_external_update_before_companion_write`、`multirouter_projection_rejects_companion_change_after_config_commit` 和连续冲突场景可以复现 companion 与 config 的竞态 | `provider_projection_cache...` `1/1`；`multirouter_projection_` 过滤器 `6/6`；`multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects` `1/1`。提交和补偿均按 capture fingerprint 条件执行，无法安全提交时返回 deferred，并保留外部第三版 |
| raw Codex 全文 writer 仍可被应用层使用 | Sol 的 A-6 静态 RED 指出两个 raw writer 是公共 API | `raw_codex_fulltext_writer_is_not_reexported_to_application_callers` 与 `raw_codex_fulltext_writers_are_not_public_module_api` 共 `2 passed / 0 failed`；生产 writer 已收窄为 `#[cfg(test)] pub(crate)`，集成测试使用隔离 HOME seed，不再调用公共 raw API |
| takeover backup 会把 provider snapshot-only MCP 当成 live-only MCP 复活 | Sol 的 A-7 RED 指出 takeover backup 绕过普通 provider MCP 来源边界 | `update_live_backup_drops_stale_provider_mcp_and_keeps_live_and_common_entries` `1 passed / 0 failed`；backup 现在保留当前 live MCP 和 common-config MCP，丢弃 provider snapshot-only MCP |

### Companion deferred 裁定

第四轮明确采用 deferred，而不是静默合并并宣称成功。配置 commit 成功后，如果 catalog、models cache、cache backup 或 managed agent 在 capture 后出现外部变化，提交器会停止本次投影，记录 `codex.live.concurrent_modification_deferred`，并只在仍匹配本次 after fingerprint 时补偿。补偿遇到第三方版本时跳过恢复，保留第三方字节；配置仍由本次 attempt 所有时才恢复原始配置。这样可以让用户稍后重试，而不会把外部写入伪装成 CCSwitchMulti 的成功结果。

### 既有测试 fixture 与测试布局修正

- `profile_roundtrip` 的 `codex_profile_reapplies_same_multirouter_after_takeover_cleanup` 夹具已经补齐 `schemaVersion: 2`、`targetProviderId`、`modelSelection`、`authPolicy` 和独立 upstream model catalog。生产 migration guard 没有放宽；这是既有 fixture 修复，不是生产行为放宽。最终 `profile_roundtrip` 为 `8 passed / 0 failed`。
- `import_export_sync` 从旧记录的 26 个变为当前 24 个不是测试遗漏。两个公共 raw-writer persistence/rollback 集成测试已移入 `codex_config.rs` crate-private 单元测试；`removes_servers_when_none_enabled` 改名为 `preserves_live_only_servers_when_none_enabled`，仍然只占一个测试。当前 `import_export_sync` `24 passed / 0 failed`。

### 第四轮最终验证门

| 命令或测试组 | 实际结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | `3348 passed / 0 failed / 5 ignored` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| `provider_commands` | `10 passed / 0 failed` |
| `provider_service` | `37 passed / 0 failed` |
| `import_export_sync` | `24 passed / 0 failed` |
| `profile_roundtrip` | `8 passed / 0 failed` |
| `pnpm test:unit` | `144 files / 1177 tests passed / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，Vite 3342 modules，11.54s |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS（本轮已执行 `cargo fmt`） |
| `git diff --check` | PASS |

前端单元测试没有失败；输出包含 5 类既有提示：baseline-browser-mapping 过期、React `act(...)`/DOM 属性提示、MSW 未匹配 `tauri.local` 请求、jsdom 下 Tauri window API 错误日志，以及测试故意触发的错误日志。Renderer build 没有失败；输出包含 4 类既有提示：baseline-browser-mapping 过期、Browserslist 数据过期、subscription 动静态导入提示和 chunk 大于 500KB 提示。`pnpm typecheck` 没有 warning 或 error。

第四轮没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex，没有提交、推送或发布。工作树累计为 38 个已修改文件和 4 个未跟踪 Rust 文件；第四轮核心实现实际集中在 `src-tauri/src/codex_config.rs`、`src-tauri/src/services/provider/live.rs`、`src-tauri/src/services/provider/mod.rs`、`src-tauri/src/services/proxy.rs` 及对应 Codex/MCP 回归测试。Sol 仍需独立复核上述 receipt、companion deferred、raw API 和 takeover MCP 来源边界。

## Luna 第五轮兼容性收口证据（待 Sol 独立复核）

本轮针对第四轮复核留下的 A-4 auth attempt ownership 与 A-5 force-repair companion receipt 进行了实现收口。状态仍为 `DONE_WITH_CONCERNS`；本节只记录 Luna 的实现和本地验证，不改写 Sol 既有 `BLOCKED` 结论。

- auth 写入现在使用 typed `CodexAuthWriteAttempt`。同一读取快照同时保存原始字节和指纹；提交前复检 fingerprint，`written_fingerprint` 直接由真正序列化并写入的 candidate bytes 计算。回滚只在 current fingerprint 仍等于本次写入 fingerprint 时执行；发现外部更新则保留第三方 auth，并返回 deferred/恢复未完成信息。
- typed auth receipt 已贯穿普通 provider auth+config、provider catalog projection 和 snapshot projection 三条生产路径；proxy hot-switch 回滚显式处理 auth/companion 条件恢复失败。
- force-repair 不再通过写后重读推断 companion 所有权。真实 writer 透传 config/auth/companion receipts；`SwitchResult.codex_receipt` 仅供内部回滚使用（serde skip）。force-repair 聚合 config/auth/companion 的 conditional restore 结果，任一返回 `false` 时报告“自动回滚恢复未完成/并发修改”，不再声称已恢复完整原配置。
- 已删除生产 `CodexProjectionSideEffectsAttempt::finish()` 及无用 `agents_dir` 后门；companion restore 返回 `Result<bool, AppError>`，只恢复本 attempt receipt 仍拥有的版本。

### 第五轮 RED/GREEN

| 兼容性问题 | RED 证据 | GREEN 证据 |
|---|---|---|
| auth capture 后、commit 前的外部更新会被覆盖 | `provider_auth_commit_defers_when_external_update_occurs_after_capture` 旧路径为 `Ok(())` 并覆盖外部 auth | 同名回归 `1 passed / 0 failed`；外部 auth 与旧 config 保留 |
| auth commit 后、config 失败前的外部更新会被无条件回滚 | `provider_auth_rollback_preserves_external_update_after_commit` 旧路径只报告 TOML 错误并覆盖外部 auth | 同名回归 `1 passed / 0 failed`；外部 auth 保留，错误明确报告 deferred/恢复未完成 |
| force-repair 外层 finish 会错误认领第三方 companion | `force_repair_does_not_claim_companion_update_before_finish` 旧路径会删除外部 catalog | 同名回归 `1 passed / 0 failed`；第三方 catalog 保留 |
| companion 条件恢复 false 被忽略 | `force_repair_reports_deferred_when_companion_restore_is_skipped` 旧路径仍声称已恢复原配置 | 同名回归 `1 passed / 0 failed`；外部 catalog 保留，错误包含并发修改/恢复未完成 |

### 第五轮验证门

| 验证门 | 实际结果 |
|---|---:|
| 串行 Rust 全量 `cargo test -- --test-threads=1` | PASS，0 failed |
| Rust library 全量 | `3352 passed / 0 failed / 5 ignored` |
| `cargo check --tests` | PASS |
| 前端 Vitest `pnpm test:unit` | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |

前端和 renderer 仅出现既有 baseline-browser-mapping、Browserslist、React `act(...)`/DOM 属性、MSW 未匹配 Tauri command、jsdom Tauri window、动态导入和大 chunk 提示，没有测试失败。没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex，没有提交、推送或发布。Sol 仍需独立复核本轮 auth receipt、force-repair receipt 与 deferred 传播。

## Luna 第六轮最终串行验证（2026-08-24，待 Sol 独立复核）

本轮只收口逐项 runtime 恢复条件并完成格式化后的串行复核，不改变既有兼容性裁定，也不把 Sol 的 `BLOCKED` 改写为通过。

- `CodexSwitchStateSnapshot::restore_files_if_unchanged` 现在按 proxy config、backup、派生 takeover、proxy running、DB/local current 分别判断所有权；current 被外部第三方改动时保留第三方 current，同时仍恢复本次 attempt 独占的代理与备份副作用。
- 配置、认证、projection companions、proxy/backup/current 的 receipt 继续使用精确 before/after 快照；managed agent 删除使用 tombstone，认证删除使用 missing-file ownership proof，外部版本优先保留并报告 deferred。
- `cargo fmt --manifest-path src-tauri/Cargo.toml` 已完成；`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 与 `git diff --check` 均通过。前端本轮修改文件单独 Prettier 检查通过；全量 `pnpm format:check` 仍只报告未修改的既有 `src/components/providers/forms/CodexFormFields.tsx`。

### 第六轮验证门

| 验证门 | 实际结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib force_repair_ -- --nocapture --test-threads=1` | `15 passed / 0 failed` |
| 串行 Rust 全量 `cargo test --manifest-path src-tauri/Cargo.toml --tests -- --test-threads=1` | `3361 passed / 0 failed / 5 ignored`，集成测试全部通过 |
| `pnpm test:unit` | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| 本轮修改前端文件 `pnpm exec prettier --check ...` | PASS |
| 全量 `pnpm format:check` | 仅既有未修改 `CodexFormFields.tsx` 格式基线失败 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS |
| `git diff --check` | PASS |

前端测试保留既有 React `act(...)`、MSW 未匹配 Tauri command、jsdom Tauri window 和测试故意触发错误日志；这些没有造成失败。所有测试继续使用隔离临时 HOME/数据库，没有访问真实 `~/.codex` 或 `~/.agents`，没有重启/结束 Codex，未提交、推送或发布。状态仍为 `DONE_WITH_CONCERNS`，等待 Sol 独立复核。

## Luna 第七轮最终验证与交接（2026-08-24，待 Sol 独立复核）

本轮继续按世豪批准的第三版兼容性原则收口：当前 live 是事实来源，CCSwitchMulti 只认领自己确切写出的 candidate/receipt，外部新版本优先保留。特殊 switch 的 finish 只组装 typed receipts，不再通过写后读取文件认领 after-state。

- 普通 provider switch 的 MCP reconcile receipt 已接入最后 writer 链；MCP 写入不再让旧 provider receipt 过期。数据库中出现过的 MCP id 才代表 CCSwitchMulti 所有权，live-only 用户 MCP 永远保留；disabled managed MCP 才可删除，provider 快照中的陈旧 MCP 不复活，旧 `[mcp.servers]` 先迁移后清理，同语义内容不重写以保留注释和格式。
- 受控旧实现 mutation 已得到真实 RED：`force_repair_hot_switch_restores_backup_when_finalize_fails` 会把外部第三版本错误认领并覆盖；普通 switch 的 finish-boundary 与 MCP 最后 writer 两条回归同样能复现旧 receipt 过期。当前实现对应回归全部 GREEN。
- 静态审计：`finish_codex_switch_result` 和 `finish_codex_switch_mutation_result` 均没有 `capture(state)`、`capture_after`、`read_codex_config_text` 或 `ExactCodexSnapshot::read`；after-state 仅由 provider/MCP/auth receipts、已知 target current 和未修改 runtime-before 组装。

### 本轮验证门

| 验证门 | 实际结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| 串行 Rust 全量 `cargo test -p cc-switch -- --test-threads=1` | library `3363 passed / 0 failed / 5 ignored`；所有 integration binaries PASS |
| `force_repair_` 聚焦回归 | `16 passed / 0 failed` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| `pnpm test:unit -- --run` | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules，11.59s |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS |
| `git diff --check` | PASS |

全量前端格式检查仍只报告未修改的既有 `src/components/providers/forms/CodexFormFields.tsx`；本轮没有改动该文件。测试与构建中的 baseline-browser-mapping、Browserslist、React `act(...)`、MSW/Tauri mock、动态导入和大 chunk 提示均为既有 warning，不影响退出码。

当前实现 worktree `D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation` 为 `39 M + 4 ??`，未提交、未推送；没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex。Sol 仍需独立复核上述 receipt、MCP ownership 与静态 after-read 结论。

## Luna Task 8/9 启动恢复与端口所有权兼容性修复（待 Sol 独立复核）

本节记录第三版在启动恢复和端口探测边界上的追加修复。修复仍遵守同一条兼容性原则：先把当前现场当作事实来源，再判断 CCSwitchMulti 是否有足够证据认领；无法确认所有权时保留现场、停止接管，并把结果交给后续复核。以下是 Luna 的实现与自测证据，不能替代 Sol 对新增范围的独立复核，也不改写文档前面已有的 Sol 阶段裁定。

### Task 8：启动退出证据按时序分类

旧实现曾在生产入口固定传入 `planned=false`，因此启动入口无法区分活跃旧实例、计划重启和真实崩溃。Luna 先读取旧 marker、`clean_exit`、`panic` 和 `crash.log` 的修改时间，再完成分类，最后才写入本次新 marker。分类优先级与证据如下：

- 旧 marker 对应的 PID 仍活跃时，返回 `ActivePreviousInstance`，优先级高于其他时间证据。
- `panic` 或 `crash.log` 晚于旧 marker，且晚于最新 clean exit 时，返回 `ConfirmedCrash`。
- clean exit 晚于旧 marker，且其后没有更新的 panic/crash 证据时，返回 `PlannedRestartOrUpdate`。
- marker 已过期且没有更新证据时，返回 `UncleanExit`；没有旧 marker 时返回 `NoPreviousRun`。

`ActivePreviousInstance` 和 `PlannedRestartOrUpdate` 通过现有 `record_recovery_outcome` 原子持久化并发出事件。Rust outcome、TypeScript 联合类型、英文/中文/繁中/日文文案和 `App.tsx` 分支已同步：Active 显示 warning，Planned 静默处理；其他分类保持原有 UI 行为。

### Task 8 受控 RED/GREEN

| 证据 | 旧实现的真实失败 | 当前结果 |
|---|---|---|
| `cargo test --lib app_exit_monitor --no-default-features -- --nocapture` | 临时把生产分类固定为 Planned 后，3 项回归失败：Active 和 Confirmed 实际被错误归类为 Planned；退出码 101 | 恢复按 PID、marker、clean-exit、panic/crash 时序分类后，app-exit-monitor 组 `9 passed / 0 failed` |

这组 RED 证明分类不能由固定布尔值代替现场时序；GREEN 证明生产入口现在先分类、后写 marker，并能为 Active/Planned 持久化明确 outcome。

### Task 9：端口探测只兼容可证明的同主版本实例

`/health` 与 `/status` 现在使用真实 HTTP 探测。远端实例只有同时满足下列条件才会被判定为可兼容实例：应用名属于允许的 CCSwitchMulti 别名；远端和本地版本都是合法 SemVer；两者 major 相同；PID 大于 0。`instance_id` 缺失仍保持向后兼容，空字符串或纯空白值则拒绝。预发布标识和 build metadata 不影响同主版本兼容性。

跨 major、非法/未知版本、错误应用名、无效 PID、空白 `instance_id` 或未知所有者都返回带 `PORT_OWNERSHIP_GUARD` 前缀的错误。启动恢复识别该错误后停止 Codex takeover，保留原 listener 和现有状态，不调用破坏性关闭或清理。实现新增直接 `semver = "1"` 依赖，lockfile 只增加现有 semver 包的直接引用。

### Task 9 受控 RED/GREEN 与 E2E

| 证据 | 旧实现的真实失败 | 当前结果 |
|---|---|---|
| `cargo test --lib port_probe --no-default-features -- --nocapture` | 临时恢复“版本非空即兼容”后，跨 major 实例被判为 `CompatibleInstance`，而回归期望 `UnknownOwner`；退出码 101 | same-major SemVer 规则恢复后，port-probe 组 `4 passed / 0 failed` |
| 不兼容 listener E2E | 旧接管逻辑会把跨 major listener 当作可接管候选 | `1 passed / 0 failed`：记录 `PortOwnedByUnknownOwner`，不启用 Codex takeover，本实例不绑定该端口，原 listener 仍能响应，证明没有杀掉原进程 |

这组 RED/GREEN 把“能连上端口”与“有权接管端口”分开：跨 major 只能作为未知所有者处理，不能因为 HTTP 可达就覆盖用户或其他版本的实例。

### Task 8/9 最终实现方验证门

| 验证门 | 实际结果 |
|---|---:|
| `app_exit_monitor` | `9 passed / 0 failed` |
| `recovery_outcome` | `2 passed / 0 failed` |
| `port_probe` | `4 passed / 0 failed` |
| 不兼容 listener E2E | `1 passed / 0 failed` |
| takeover 聚焦回归 | `1 passed / 0 failed` |
| `cargo check --tests` | PASS |
| 串行 Rust `cargo test --tests -- --test-threads=1` | library `3371 passed / 0 failed / 5 ignored`；所有 integration binaries PASS；`mcp_codex_reconcile` `11/11`、MCP commands `23/23`、`import_export_sync` `24/24`、`provider_commands` `10/10`、`provider_service` `37/37`、`profile_roundtrip` `8/8` |
| 前端 Vitest | `144 files / 1177 tests / 0 failed` |
| TypeScript typecheck | PASS |
| renderer build | PASS，3342 modules |
| Rust fmt | PASS |
| `git diff --check` | PASS |

上述数字只代表 Luna 已完成的实现方验证。Task 8/9 的当前状态是“Luna 已修复，待 Sol 独立复核”；不能把新增范围提前写成 Sol 已通过，也不能据此改写 Sol 既有的 `BLOCKED` 结论。实现未提交、未推送，未访问真实 `~/.codex` 或 `~/.agents`，未重启或结束 Codex。

## Luna 启动恢复 outcome 生命周期与四语言兼容性收口（2026-08-24，待 Sol 复核）

本节只记录 Luna 的修复证据，状态仍为 `DONE_WITH_CONCERNS`，不得改写 Sol 已有的 `BLOCKED` 结论。

### 真实 RED/GREEN

- RED：在职责收敛前，正常启动后的 `normal_startup_` 回归为 `1 passed / 1 failed`；失败断言证明旧 `ActivePreviousInstance` 仍留在 outcome 文件，下一次启动会重复 warning。
- GREEN：`normal_startup_` `2/2`；`app_exit_monitor` `11/11`；`recovery_outcome` `6/6`；`port_probe` `4/4`；不兼容 listener E2E `1/1`；四语言 `recoveryOutcome.test.ts` `4/4`。
- 最终串行 Rust：`cargo test --manifest-path src-tauri/Cargo.toml --tests --quiet -- --test-threads=1` 的 library 为 `3375 passed / 0 failed / 5 ignored`，所有 integration binaries 通过。`cargo check --tests`、Rust fmt、`git diff --check` 均 PASS。

### 单一职责清理契约

`record_startup_report()` 只读取上次 marker/退出证据、完成分类并写入本次 marker；它不清理恢复 outcome。`persist_startup_recovery_outcome()` 是唯一的启动 outcome 入口：Active/Planned 写入新瞬态结果，其他分类只按 kind 条件删除旧的 `ActivePreviousInstance` 或 `PlannedRestartOrUpdate`；`ProviderOnlyRestored` 等非瞬态恢复历史保持不变。outcome 的读、写和条件清理共用进程内 `Mutex`，清理在删除前复读并比较原始字节，发现内容变化即放弃删除，让新写入优先。

### 翻译与边界

四种语言都补齐 `closeOtherInstanceOrInspectProcess`，测试确保文案非空且不会显示内部驼峰 key。本轮没有新增“同步 MCP”按钮、API 或 Tauri command。进程内锁不能覆盖多个进程之间的第二次读取到删除窗口；正式跨进程文件锁仍是明确的后续规格边界，不能把该残余竞态描述成已解决。

结论：Luna 已修复，待 Sol 独立复核；未提交、未推送，也未访问真实 `~/.codex` 或 `~/.agents`。

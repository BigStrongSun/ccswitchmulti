# 2026-08-23-codex-route-compatibility-audit-delta

计划名称：Codex 路由兼容性审计增补（第 7、8 项）

规格：`docs/superpowers/specs/2026-08-23-codex-route-compatibility-audit-delta-design.md`

计划：`docs/superpowers/plans/2026-08-23-codex-route-compatibility-audit-delta.md`

当前阶段：256 已完成独立增补规格、计划和恢复账本；规格已获世豪批准（2026-08-23）；Luna 可以启动产品代码和测试实现

## 1. 用户确认记录

- 2026-08-23：世豪批准第三版主体规格，并要求实现从本质上按兼容性设计处理。
- 2026-08-23：世豪要求把 `THREAD-AUDIT-01a02dd1-2026-08-23.md` 中仍存在的问题加入检查，存在的修掉。
- 2026-08-23：世豪确认加入审计第 7、8 项。
- 2026-08-23：世豪说明赶时间，允许“边界独立的话，这次并发”。
- 2026-08-23：世豪明确批准第 7、8 项增补规格及边界独立时并发。并发裁定同时明确：Luna 统一负责所有产品代码和测试；256 当前只修改本规格、本计划和本账本。

## 2. 输入证据

- Flash 只读探索：`docs/diagnostics/sdd-prefix-migration-preview-failclosed-explore-2026-08-23.md`，状态 `DONE_WITH_CONCERNS`。
- 审计输入：`THREAD-AUDIT-01a02dd1-2026-08-23.md`。
- 已批准总规格：`docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`。
- 已批准总计划：`docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`。
- 当前 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`。

## 3. 当前基线

| 项目 | 当前值 |
|---|---|
| 当前 HEAD | `6923a99693ef38f8fbc25ff5042b58c0679eaa73` |
| 当前 `origin/main` | `9b0fd548301b2734772c155d8475deb285352bba` |
| 当前 worktree 状态 | 30 个未提交状态条目 |
| 当前分支 | `fix/codex-startup-config-preservation` |

只读核对结果：

- `migration.rs`、`schema.rs` 和 `compiler.rs` 当前仍未实现 prefix-only 展开；`legacy_route_canonical_models()` 只读取 `match.models`。
- 当前前端仍有 `会走默认路由`、`没有精确命中 model 时才会使用默认路由`、`已设置默认路由` 三处旧语义。
- 当前 fork 后端已有 fail-closed 回归测试；`origin/main` 后端仍有 `defaultRouteId` 兜底。
- Task 16 已在当前工作树中引入共享的 `routeCanMatchVisibleCatalogModel()` include-gate 助手；本增补前端任务必须复用它，且需要避开同一文件的并发覆盖。

## 4. 本次创建文件

- `docs/superpowers/specs/2026-08-23-codex-route-compatibility-audit-delta-design.md`
- `docs/superpowers/plans/2026-08-23-codex-route-compatibility-audit-delta.md`
- `docs/superpowers/plans/2026-08-23-codex-route-compatibility-audit-delta-ledger.md`

文件归属裁定：这三份文件归 256 规格阶段所有。Luna 正在修改产品 worktree 和原恢复账本，因此本阶段没有修改产品代码、原规格、原计划或原恢复账本。

## 5. 推荐契约摘要

第 7 项推荐“按当前 catalog + prefix 展开为 include”：

- 不把 prefix-only 路由改成 `mode=all`。
- 预览时只把当前目标 catalog 中命中 prefix 的可见模型加入 include。
- alias key 可以参与 prefix 匹配，但 alias target 必须继续通过 catalog/include 校验。
- 目标 catalog 为空时报 `prefix_selection_catalog_empty`；有 catalog 但无匹配时报 `prefix_selection_no_matches`。
- prefix 展开后必须给 `prefix_selection_frozen` 警告；未来新增模型不自动加入，刷新 catalog 后需要重新勾选并保存。
- apply 使用预览快照，重复 apply 保持幂等。

第 8 项裁定“字段保留、运行时兜底语义移除”：

- 当前 fork 未命中请求必须表达为拒绝，不承诺默认路由。
- `defaultRouteId` 保留为可选数据和诊断字段；新建计划不再主动写入；设置面板不再提供默认路由兜底编辑入口。
- 发布检查不再要求已设置默认路由，改为明确 fail-closed。
- 诊断中旧字段显示为 `旧版默认路由字段`，并给 `default_route_legacy_ignored` 警告。
- 上游 PR 必须和后端 fail-closed 同组，或等待后端 fail-closed 先合入；禁止在 `origin/main` 上孤立提交误导性文案。

## 6. 测试与执行记录

本阶段是规格和计划阶段，只执行只读核对，没有运行产品测试，也没有修改产品代码。

| 时间 | 动作 | 结果 |
|---|---|---|
| 2026-08-23 | 重新读取 SDD 总入口 | 完成 |
| 2026-08-23 | 读取 flash 报告、审计输入、总规格、总计划、原恢复账本 | 完成 |
| 2026-08-23 | 扫描 `migration.rs`、`schema.rs`、`compiler.rs`、V2 runtime、前端预览/设置/发布检查、诊断接口和相关测试 | 完成 |
| 2026-08-23 | 对比 `origin/main` 后端和前端默认路由语义 | 完成 |
| 2026-08-23 | 创建三份独立增补文档 | 完成 |

Flash 报告记录的验证限制继续保留：一次 Rust 聚焦测试曾在编译阶段遇到 Windows 页面文件错误 `os error 1455`，该结果既不是代码断言通过，也不是代码断言失败。后续实现阶段必须重新运行计划中的 RED/GREEN 命令。

## 7. 阶段状态

| 阶段 | 状态 | 证据 |
|---|---|---|
| Flash 第 7、8 项复核 | DONE_WITH_CONCERNS | 两项均存在；测试缺口和 Windows 工具链限制已记录 |
| 256 增补规格 | DONE | 规格文件已创建并更新为“已批准（2026-08-23）” |
| 256 增补计划 | DONE | 计划文件已创建，包含准确文件、符号、测试路径、命令和 RED/GREEN 预期 |
| 256 增补账本 | DONE | 本文件已创建 |
| 世豪批准增补技术方案 | DONE | 世豪明确批准第 7、8 项及边界独立时并发 |
| Luna 实现 | NOT_STARTED | 规格已批准；Luna 统一负责所有产品代码和测试 |
| 256 交叉验证 | NOT_STARTED | 等待 Luna 自测完成 |
| Sol 最终审核 | NOT_STARTED | 等待全部验证完成 |

## 8. 并发与边界裁定

- 本次 256 规格任务只修改三份增补文档：`docs/superpowers/specs/2026-08-23-codex-route-compatibility-audit-delta-design.md`、`docs/superpowers/plans/2026-08-23-codex-route-compatibility-audit-delta.md` 和本账本。
- Luna 统一负责所有产品代码和测试；后端迁移边界（Task R1-R2）与前端诊断边界（Task R3-R5）用于控制 diff、提交和上游 PR 范围，不表示不同实现者同时写入。
- 后端迁移边界只拥有 `src-tauri/src/codex_multirouter/migration.rs`。
- Task R3-R5 与 Task 16 都触碰 `CodexRouterWorkspacePage.tsx` 和对应测试；必须排在 Task 16 当前修改之后，或由同一位 Luna 实现者顺序完成。
- `src-tauri/src/commands/proxy.rs` 只属于 fail-closed 诊断边界。
- `src-tauri/src/proxy/providers/codex.rs` 的 fail-closed 行为已有后端测试覆盖，本增补不改该文件。
- 上游交付至少拆成两个新 Issue/PR：prefix-only 迁移兼容；fail-closed 后端与 UX 一致化。第二个上游 PR 必须包含后端 fail-closed 或等待其先合入。

## 9. 给 Luna 的准确任务简报

增补规格已获世豪批准（2026-08-23）。Luna 现在可以按以下顺序执行，并统一负责所有产品代码和测试：

1. 从 Task R1 开始，在 `src-tauri/src/codex_multirouter/migration.rs` 测试模块新增全部 prefix-only RED 测试；先运行计划命令并记录 `include_models_empty` 失败。
2. 在 `migration.rs` 内实现当前 catalog + prefix 展开 include；不改 `schema.rs`、`compiler.rs` 或 V2 runtime；不把路由改成 `mode=all`。
3. 增加 `prefix_selection_catalog_empty`、`prefix_selection_no_matches`、`prefix_selection_frozen` 和 `prefix_selection_no_current_matches` 的稳定行为。
4. 在前端路由编辑器为 include + matchPrefixes 的迁移结果显示“未来模型不自动加入，刷新后重新保存”的提示；该任务避开 Task 16 并发。
5. 为未命中预览、设置说明、发布检查和诊断 warning 写 RED 测试，再实现当前 fork 的 fail-closed 文案。
6. 保留 `defaultRouteId` 数据字段，停止新计划主动写入，设置面板只读展示旧字段，发布检查不再要求默认路由。
7. 运行 Task R6 的全部验证命令，把命令、退出码、关键输出、RED/GREEN 转变写回本账本。
8. 不创建提交，除非主智能体明确授权提交边界。

## 10. 批准与下一阶段

世豪已批准本增补技术方案（2026-08-23），下一阶段不再是批准门。主智能体可以把本文件和配套计划交给 Luna；Luna 统一负责所有产品代码和测试。256 当前阶段只维护三份增补文档，不修改主体实现报告或产品代码。

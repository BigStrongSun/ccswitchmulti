# Fork 后续 bug 核验报告

日期：2026-08-23

状态：DONE_WITH_CONCERNS

## 基线

- 开工前已执行 `git fetch origin --prune`，退出码 0。
- 母仓库：`BigStrongSun/ccswitchmulti`，`origin/main=9b0fd548`，代码版本 `v3.19.2-16`。
- fork：`zhushihao/ccswitchmulti`，核验提交 `6923a996`，发布版本 `v3.19.2-16.2`。
- 红灯测试全部在隔离 worktree 和既有构建缓存中运行，没有修改真实用户配置、当前工作树或远程仓库。

## 结论

### 1. 删除共享 target 时非激活 router 仍会发布：已确认

fork 的 provider 更新路径通过 `active_codex_router_id` 只发布激活 router，但删除路径在 `apply_codex_provider_delete_with_hooks` 中仍遍历并发布全部受影响 router。两个 router 共用同一 target 时，临时测试实际得到 `router-company`、`router-personal` 两次发布，而预期只有激活的 `router-personal`。

这会短暂或持续覆盖共享的落盘模型目录，但不会删除数据库中的 router 声明。重新发布激活 router 可以恢复，因此风险是“投影覆盖”，不是不可恢复的数据库数据丢失。

该代码依赖上游尚未合并的 PR #47 所引入的激活 router 规则。fork 可以立即修；母仓库 PR 应等待 #47 合并后从新的 `origin/main` 创建，或提交一份包含更新和删除两条路径的完整独立新 PR，不能继续更新 #47。

### 2. schema-v2 缺少 `routes` 会执行 `undefined.map`：已确认

母仓库和 fork 都直接执行 `v2.routes.map(...)`。临时 Vitest 使用缺少 `routes` 的同步/旧数据，稳定抛出：

```text
TypeError: Cannot read properties of undefined (reading 'map')
```

这与之前 `modelSelection.mode` 崩溃属于同类兼容缺口，应独立提交新 Issue 和新 PR，修复为 `(v2.routes ?? []).map(...)` 并保留回归测试。

### 3. include 门控在三个次要界面路径不一致：已确认

fork 的 `collectRoutedCatalogModels`、`handlePreviewRoute`、`routeMatchesModel` 都仍使用“精确命中或前缀命中”，没有应用 include 模式的反选门控。临时测试反选 `gpt-5.6-sol` 后，子 Agent 路由卡片仍返回 `gpt-5.6-luna` 和 `gpt-5.6-sol`，稳定变红。另两处使用同一裸谓词，静态证据一致。

真实运行时已 fail-closed，因此这是界面显示和日志归属错误，不会让被反选模型真正发往上游。母仓库尚未包含 fork 的主 include 门控修复；上游贡献应是一份覆盖主路径和三个次要路径的完整、独立新 PR，不能作为历史 PR #35 的追加提交。

### 4. 推理档位清洗后默认档位可能失效：已确认

fork 的 `sanitize_persisted_efforts` 会删除非法 `supportedEfforts` 和 `effortMap`，但不修正 `defaultEffort`。临时 Rust 测试使用 `supportedEfforts=[low, ultra]`、`defaultEffort=ultra`，清洗后整份能力声明返回 `None`，而不是保留 `low` 并回落默认值。

该清洗器尚未进入母仓库。上游新 PR 必须包含完整的历史脏档位自愈契约及默认档位回落测试，不能只提交一个依赖 fork 私有前置提交的小补丁。

### 5. `selectedPlan` 最终兜底仍依赖不稳定对象顺序：已确认结构缺口

`App.tsx` 已持有数据库返回的稳定 `currentProviderId`，但没有把它传给 `CodexRouterWorkspacePage`。工作台只接收代理请求产生的 `activeProviderId`；代理尚未处理请求时该值为空，最终退回 `routingPlans[0]`。因此报告中的场景成立。

当前组件接口没有表达稳定 current provider 的测试入口。正式修复应新增明确 prop 或提取纯选择函数，再先补红灯测试，优先级顺序为：用户显式选择、导航目标、代理激活 provider、数据库 current provider、稳定排序后的首项。

### 6. `[model-order]` 错误级调试日志残留：已确认

fork 中存在多处 `console.error("[model-order] ...")`，覆盖保存开始、模型列表、命中、更新数量和保存完成。母仓库 `v3.19.2-16` 没有这些 fork 后续日志。该项不影响功能，fork 修复时删除；不为母仓库创建没有对应代码的 Issue 或 PR。

## 红灯反馈环

以下命令连续运行两次，结果一致：

```text
cargo test --lib deleting_shared_target_publishes_only_the_active_router -- --nocapture
```

结果：FAIL；实际发布 `["router-company", "router-personal"]`，预期 `["router-personal"]`。

```text
cargo test --lib reasoning_declaration_recovers_when_default_points_to_removed_effort -- --nocapture
```

结果：FAIL；清洗后的能力声明为 `None`。

```text
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "does not crash when schema v2 routing is missing routes|keeps include-deselected prefix matches out of routed catalog cards"
```

结果：2 FAIL；分别为 `undefined.map` 和反选模型仍出现在 routed catalog。

## 交付建议

- fork 紧急修复可以覆盖六项，但产品提交必须按责任边界保持可独立挑选。
- 母仓库至少拆成新的 routes 缺省兼容 PR、include 门控一致性 PR、推理档位自愈 PR、稳定 selected plan PR。
- 删除路径修复依赖激活 router 规则，先观察历史 PR #47 是否合并；无论结果如何都只创建新 PR，不更新 #47。
- 调试日志只在 fork 删除，不向缺少这些日志的母仓库制造无效 PR。

## 仍需验证

- 第3条的路由预览和日志归属需要在正式实现时各补一个正确调用层测试，当前红灯只直接覆盖 routed catalog 卡片。
- 第5条需要先提取纯选择函数或扩展组件接口，才能形成准确的红灯测试。
- 发布前需要重新同步母仓库；如果母仓库版本或相关代码发生变化，本报告的分支和版本建议必须重算。

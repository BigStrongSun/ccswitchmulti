# Task 7：MultiRouter 四步向导移植报告

## 中断恢复

- 恢复起点：`68012439f127cf2b5945eca470f866bfdc2015ac`；RED `21a06a9f` 已在该起点中，报告文件尚未创建。
- 工作树原有 `docs/audits/` 未跟踪内容始终未修改、未暂存。
- 使用一次性空 `core.hooksPath` 完成 GREEN `737dd735` 的 cherry-pick。冲突只在 `CodexMultiRouterWizard.tsx`：当前分支的 V1/V2 向导步骤与 GREEN 四步导航重叠。
- 解决方式：采用 GREEN 的 `sources / prepare / review / activate` 向导层；没有恢复、隐藏或折叠旧 V1/V2 步骤。现有 plan 的 `subagentVersion` 仍由 builder 从既有 routing 保留，`spawnAgentModels` 仍随最终可路由目录保存；未修改后端 routing schema、V1/V2 Sub-Agent 数据或独立入口。

## RED

命令：

```powershell
pnpm exec vitest run src/components/codex/CodexMultiRouterWizard.test.tsx tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts --no-file-parallelism
```

结果：`63 passed / 1 failed`（64 项）。失败断言找不到“选择模型源”；真实可访问导航仍为 12 步，包含“理解 MultiRouter”“Sub-Agent V1 兼容”“Sub-Agent V2 当前使用”等旧项。这是预期的 RED 证据。

## GREEN

- GREEN cherry-pick：`b495befd feat(codex): 收敛 MultiRouter 为四步向导`。
- 初次 GREEN 聚焦测试显示四步 UI 已生效，但两个后续加入的测试仍断言 V1/V2 导航，Qwen-only 用例也未走到第四阶段保存。根因是测试契约仍属于旧向导，不是 UI 或 routing 退回的理由。
- 测试对齐提交：`d64088b9 test(codex): 对齐四步 MultiRouter 向导测试`。更新为验证 V1/V2 设置不再嵌入主向导；Qwen-only 用例完整经过 `sources → prepare → review → activate`，并验证保存的 routing `subagentVersion` 为 `v2`。

## 验证

- 聚焦 Vitest：3 个文件、64/64 通过。
- `pnpm typecheck`：通过。
- Prettier：`CodexMultiRouterWizard.tsx` 及两个关联测试文件通过。
- `git diff --check`：通过。

已知非阻塞提示：Vitest 输出 `baseline-browser-mapping` 数据超过两个月的既有依赖提示；本任务未变更该依赖。

## 编码验证

对以下中文文件按原始字节检查：`CodexMultiRouterWizard.tsx`、两个关联测试、本文档。

- 均能严格按 UTF-8 解码。
- 均无 UTF-8 BOM。
- 均未发现 U+FFFD 替换字符。

## Self-review

- 四阶段顺序、可访问按钮、保存/启用路径均由聚焦测试覆盖。
- V1/V2 不再被藏进四步导航；独立设置边界未被折叠回向导。
- 未修改 routing schema、后端、Sub-Agent 数据结构或独立入口。
- 未触碰 `docs/audits/`。

## 联网检索

- Codex 内置搜索命中 Git 官方 `git-config` / `githooks` 文档，确认 `core.hooksPath` 可用于单次命令的 hooks 覆盖。
- Matrix WebSearch 独立检索可用，但精确官方查询未命中，放宽查询只返回 Git 安装/源码等非直接证据；因此不将 Matrix 结果作为该 Git 机制的正证据。

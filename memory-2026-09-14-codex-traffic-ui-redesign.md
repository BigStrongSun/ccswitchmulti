# Codex 流量页 UI 重构（2026-09-14，开发分支）

- `CodexSessionTrafficPanel` 仅消费既有 `CodexSubagentUsageStats` 与 `SessionCollectionStatus`；未新增 IPC、网络请求、计时器或后端字段，手动同步仍使用原 `onSync`。
- 顶端只汇总 `modelStats` 中已观测子任务的 Tokens、请求和本地价表可计算部分；`totalCost <= 0` 只能表达未定价/待核验，故存在这类已观测行时必须标为“部分可计价”，不得称完整总成本。
- 模型页使用现有 Tabs/Select，按 Tokens 或模型名排序，且把未采集用量显示为未知而非零；轻量条形仅表达当前可见行的 token 相对构成。
- 任务页只使用 `parentGroups` 与 `agent.parentThreadId` 的显式关联；未关联会话单列，绝不推测父模型。`unknown_may_overlap` 的父直接账仅提示可能重叠，绝不与子账相加。
- 详情使用既有 `DialogContent` 以右侧抽屉布局实现，从而保留项目的 layer context；采集诊断折叠在紧凑状态行内。
- TDD：新增交互回归覆盖概览的部分可计价语义、模型排序/筛选、显式父子/未关联会话和详情抽屉来源/未知归因。focused 8/8、`pnpm typecheck`、Prettier、diff 检查及 UTF-8 无 BOM/U+FFFD 通过；未启动服务、构建、安装或推送。
- 后续审查补强：加载、无快照和仅未采集分别显示“加载中”或“—”，不制造零流量；任务页在前两种状态不假称无关系。任务展开显示逐子会话并可分别查看；cache write 计入详情 token 构成。模型聚合不伪造单一来源，任务来源区分同步、rollout、混合、含未采集和因 display limit 无逐条会话的不可确认状态。右侧抽屉显式 `left-auto`、`h-dvh`，模型工具栏可换行。

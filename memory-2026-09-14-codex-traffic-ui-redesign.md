# Codex 流量页 UI 重构（2026-09-14，开发分支）

- `CodexSessionTrafficPanel` 仅消费既有 `CodexSubagentUsageStats` 与 `SessionCollectionStatus`；未新增 IPC、网络请求、计时器或后端字段，手动同步仍使用原 `onSync`。
- 顶端只汇总 `modelStats` 中已观测子任务的 Tokens、请求和本地价表可计算部分；`totalCost <= 0` 只能表达未定价/待核验，故存在这类已观测行时必须标为“部分可计价”，不得称完整总成本。
- 模型页使用现有 Tabs/Select，按 Tokens 或模型名排序，且把未采集用量显示为未知而非零；轻量条形仅表达当前可见行的 token 相对构成。
- 任务页只使用 `parentGroups` 与 `agent.parentThreadId` 的显式关联；未关联会话单列，绝不推测父模型。`unknown_may_overlap` 的父直接账仅提示可能重叠，绝不与子账相加。
- 详情使用既有 `DialogContent` 以右侧抽屉布局实现，从而保留项目的 layer context；采集诊断折叠在紧凑状态行内。
- TDD：新增交互回归覆盖概览的部分可计价语义、模型排序/筛选、显式父子/未关联会话和详情抽屉来源/未知归因。focused 8/8、`pnpm typecheck`、Prettier、diff 检查及 UTF-8 无 BOM/U+FFFD 通过；未启动服务、构建、安装或推送。
- 后续审查补强：加载、无快照和仅未采集分别显示“加载中”或“—”，不制造零流量；任务页在前两种状态不假称无关系。任务展开显示逐子会话并可分别查看；cache write 计入详情 token 构成。模型聚合不伪造单一来源，任务来源区分同步、rollout、混合、含未采集和因 display limit 无逐条会话的不可确认状态。右侧抽屉显式 `left-auto`、`h-dvh`，模型工具栏可换行。

## 主代理集成与最终验收

- 主体 UI 已由 Terra 提交 `a5037995`；主代理把会话消费面板移动到流量页首屏、代理最近请求样本之前，并以真实渲染及切换流量页测试验证顺序（RED → GREEN）。
- 审查发现详情抽屉仍无条件渲染零值 Breakdown：根因是明细没有沿用列表的用量证据门槛。现按模型 observed、子任务 usageStatus、任务组 observedUsageChildren 分别控制明细；未采集组不显示零请求/零 Tokens。新增回归覆盖三类抽屉以及 nonnull parentDirectUsage 与 unknown_may_overlap 并存时后者优先。
- 最终独立验收：面板 10、事件桥 4、工作台 85，合计 99/99；tsc --noEmit、Vite production build、五个改动文件 Prettier 检查通过。构建仍提示浏览器数据过期、既有混合导入与大 chunk 警告；未修改依赖。本轮未重跑全前端或 Rust，不能称全仓测试全绿。
- 交互预览入口 codex-session-collection-fixture.html，fixture 支持完整、缺失、错误、首次使用及明暗主题；数据全部合成，非实际账单。主题切换应用于 documentElement，确保 portal 抽屉跟随。HTTP HTML/module 200 已验证。
- 视觉验收未完成：Computer Use 因无法可靠识别当前浏览器 URL 触发安全停止；没有绕过或抓取新版截图。组件交互测试与构建不能代替真实视觉、桌面安装态验收。
- 本轮已使用内置搜索与 Matrix 独立检索 W3C Tabs/Dialog 规范；前者无可读结果、后者检索无关且官方页面访问受阻，双链交叉验证证据不足。实现采用本地既有 Radix Dialog/Tabs 及测试，不据此宣称外部规范符合性。
- 保持代理路由、15721 监听、安装程序与运行配置不变；只本地提交，不安装、不重启、不推送。效率/任务墙钟数据仍缺证据，不生成模型性能排名。

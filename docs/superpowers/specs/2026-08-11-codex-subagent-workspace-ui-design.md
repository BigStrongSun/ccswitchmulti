# Codex MultiRouter 子 Agent 独立工作区 UI 设计

## 背景

CCSwitchMulti `3.19.1-19` 已实现 Sub-Agent V1/V2 双协议和 V2 能力问卷，但 MultiRouter 工作台把完整 Sub-Agent 设置放在“路由规则”页底部。真实安装界面暴露出四个问题：入口难发现、当前协议按钮仍显示为可执行的“启用”、所有模型卡片全部展开导致页面过长，以及“同步模型目录能力配置”等术语无法让普通用户预判操作结果。

本设计只重构 MultiRouter 工作台的信息架构和交互表达，不改变 V1/V2 持久化结构、后端能力编译器、路由规则、Codex custom role 生成或代理转发行为。

## 目标

- 顶层导航增加独立“子 Agent”入口，把协议选择和能力配置从路由规则中移出。
- 用不可歧义的按钮状态表达当前协议与可切换协议。
- 让用户在大量模型中快速找到、判断和配置目标模型。
- 默认只显示完成当前任务所需的信息，把手工字段和 TOML 降为按需展开的高级内容。
- 保留 V1 direct model override 与 V2 questionnaire/profile 的全部兼容语义。

## 非目标

- 不修改 `settingsConfig.codexRouting.subagentVersion` 或 `subagentV2` schema。
- 不修改 Rust compiler、managed role 文件格式、Provider 分类或模型目录同步契约。
- 不更改 Qwen 行为、reserved `spawn_agent` schema、父模型选择或代理路由。
- 不把语义选型改造成确定性规则引擎。
- 不在本轮公开发布 GitHub Release。

## 信息架构

### 顶层导航

MultiRouter 工作台导航调整为：

1. 总览
2. 模型源
3. 路由规则
4. 子 Agent
5. 状态
6. 测试发布

新增 `WorkspaceTab = "subagents"`。桌面宽度使用单行六列；窄宽度退化为两行三列，避免文字被压缩。现有 Radix Tabs 继续负责 `tablist`、`tab`、`tabpanel`、键盘方向键和 `aria-selected` 语义。

`RoutesTab` 只保留路由运行摘要、规则选择、候选 Router、模型源刷新和路由保存。原 `SpawnAgentCandidatesPanel` 从页尾移出，由新的 `SubagentsTab` 独立承载。

### 子 Agent 页结构

页面从上到下分为：

1. 当前 MultiRouter 摘要，明确当前编辑的方案。
2. 协议选择卡，展示 V1/V2 的用途和当前状态。
3. 当前协议配置区：V1 显示 direct override 排序；V2 显示选择策略和能力画像列表。
4. 全局保存状态条。

切换协议只更新 `subagentVersion`，V1 `spawnAgentModels` 和 V2 `subagentV2` 始终保留。切换成功后提示“重启 Codex/app-server 并新建会话后生效”。

## V1/V2 协议状态

协议卡继续同时展示，按钮遵循以下状态机：

| 协议状态 | 按钮文字 | 外观 | 可操作性 |
| --- | --- | --- | --- |
| 当前协议 | `已启用 V1` / `已启用 V2` | 中性灰色 | disabled |
| 非当前协议 | `启用 V1` / `启用 V2` | 蓝色主按钮 | enabled |
| 切换中 | `切换中…` | 中性灰色 | disabled |

当前卡片仍用边框和背景强调“正在使用”，但不再用蓝色主按钮制造可重复执行的错觉。按钮的 disabled 状态来自 `version === activeSubagentVersion || isSavingSubagentVersion`，而不是只依赖保存状态。

切换成功和失败都通过 `aria-live` 或 `role="alert"` 向辅助技术传达；失败时保持原协议，不做乐观伪切换。

## V2 模型能力列表

### 工具栏

选择策略下方提供：

- 搜索框：按模型名、请求角色名、实际角色名和 Provider 类型匹配。
- 状态筛选：`已启用`、`待配置`、`不可路由`、`全部`。
- 目录操作按钮：`从模型目录添加可配置模型`。
- 解释文案：`发现当前可路由模型并加入列表；新模型默认关闭，已有问卷和手工设置不会被覆盖。`

`sync_catalog` 后端 action 保持不变，只修改用户可见文案。成功提示改为“已从模型目录添加可配置模型；已有设置保持不变”。

默认排序为：已启用优先、可路由优先、模型名稳定排序。DeepSeek Flash/Pro 因默认启用而自然位于前部，不额外硬编码只针对模型名称的 UI 排序规则。

### 折叠摘要

模型列表使用项目现有 Radix Accordion，`type="single"` 且 `collapsible`，同一时间最多展开一个模型。第一次进入时默认展开第一个已启用模型；用户可以全部收起。

每个折叠摘要始终显示：

- 模型名称和角色名称。
- 官方/第三方 Provider 类型。
- 可路由、目录中缺失或配置无效状态。
- 已启用/未启用状态。
- 模型偏好和最终 reasoning。
- 1 至 3 个任务强项摘要。
- 自动生成或含手工覆盖标记。

启用开关作为摘要行中独立的相邻操作，不嵌套在 Accordion Trigger 内；这样避免按钮中再嵌套 checkbox/switch，并保持 WAI-ARIA Accordion 标题结构有效。

搜索或筛选没有结果时显示当前查询对应的空状态和“清除筛选”动作，不误报为配置丢失。

### 展开内容

展开模型后按渐进披露组织：

1. `能力问卷` 默认可见，包含任务优势、优化目标、写入范围、模型偏好和推理强度。
2. `高级字段` 默认折叠，包含角色名称、角色描述、开发者指令、昵称和模型推理强度的字段级覆盖与恢复自动值。
3. `生成结果与 TOML` 默认折叠，包含实际角色名、模型、Provider、reasoning、生成路径、warnings、未生成原因和 TOML 预览。

后端仍是所有自动字段和 TOML 的唯一编译器。折叠仅改变展示，不复制生成逻辑到前端，也不停止现有 preview/status 请求。

## 保存与反馈

长列表底部增加粘性保存条，始终显示：

- 是否存在未保存变更。
- `保存 V2 子 Agent 配置` 主按钮。
- 保存中、保存成功、保存失败和重启提示。

现有 focused `update_codex_subagent_v2` 持久化命令保持不变，不退回整份 Provider 的 stale merge。问卷修改、启用切换、字段覆盖和恢复都只更新前端 draft，点击保存后一次提交。

目录同步仍采用现有受控 backend reconcile，并继续携带完整未保存 draft，防止同步动作覆盖用户尚未保存的问卷修改。

## 组件边界

- `CodexRouterWorkspacePage.tsx`
  - 扩展 `WorkspaceTab` 和六项导航。
  - 新增 `SubagentsTab`，负责方案上下文、协议状态与 V1/V2 内容切换。
  - `RoutesTab` 不再渲染 Sub-Agent 设置。
  - 保留 V1 direct override 的现有保存与 live 校验逻辑。
- `CodexSubagentProfileEditor.tsx`
  - 负责搜索、筛选、排序、Accordion、渐进披露和粘性保存条。
  - 不实现问卷编译或 Provider 判定。
- `src/components/ui/accordion.tsx` 与 `collapsible.tsx`
  - 复用现有封装，不新增 UI 依赖。

如果现有两个生产文件因职责增加而难以维护，允许把纯展示单元拆为同目录私有组件；不得借机重构无关路由、状态或测试发布页面。

## 错误与边缘状态

- 未选择 MultiRouter：子 Agent 页显示明确空状态并引导选择/创建方案，不渲染无归属编辑器。
- V2 尚未初始化：保留“一键初始化能力配置”，解释不会修改 V1 配置。
- 模型不可路由：profile 仍可查看和编辑，但摘要标明不会生成 role。
- 无效或冲突 profile：保留现有修复/删除入口，默认排在正常模型之后。
- preview/status 失败：模型编辑仍可继续，生成结果区显示局部错误，不使整个列表不可用。
- 同步失败或保存失败：保留当前 draft，显示可恢复错误，不清空筛选和展开状态。

## 可访问性

- 顶层继续使用 Radix Tabs，并保证新增 tab 与 panel 一一关联。
- 模型列表使用 Radix Accordion；trigger 保持单一按钮语义，暴露 `aria-expanded` 和 `aria-controls`。
- 搜索框有可见 label；筛选按钮暴露选中状态。
- 当前协议按钮既有文字“已启用”又为 disabled，不仅依赖颜色。
- 状态徽标不能只靠颜色，必须包含“可路由”“不可路由”“已启用”等文字。
- 保存、切换和目录同步的异步结果进入 live region；错误使用 alert。
- 折叠状态、搜索、筛选、启用和保存均需覆盖键盘测试。

## 测试设计

### 工作台测试

- 顶部存在六个 tab，“子 Agent”位于“路由规则”和“状态”之间。
- 路由规则页不再出现 Sub-Agent 设置；子 Agent 页出现协议卡和对应配置。
- 当前 V2 时按钮为灰色 disabled `已启用 V2`，V1 为蓝色可点击 `启用 V1`；V1 状态反向对称。
- 切换期间两按钮不可重复提交；成功后状态和提示更新，失败保持旧状态。
- 切换版本不改变 routes、V1 direct overrides 或 V2 profiles。

### V2 编辑器测试

- 多 profile 默认折叠且最多展开一个，第一个已启用模型位于列表首位。
- 搜索覆盖模型名、角色名和 Provider；筛选覆盖已启用、待配置、不可路由和全部。
- 折叠摘要显示路由、启用、偏好、reasoning、强项和 override 状态。
- 展开后问卷可见，高级字段与 TOML 默认折叠并可独立展开。
- 目录按钮的新文案和说明可见，仍调用 `sync_catalog` 且携带完整 draft。
- 同步新增 profile 不覆盖已有问卷/override；新 profile 默认关闭。
- 粘性保存条显示 dirty/saving/success/error，并继续调用 focused update command。
- 无效 profile、不可路由 profile、无搜索结果和 preview/status 错误都有稳定可访问输出。

### 回归门禁

- 现有 V1 direct override、V2 持久化、catalog reconcile、wizard/shared-source 与 authoritative status 测试继续通过。
- `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts src/components/codex/CodexSubagentV2ProfileEditor.test.tsx --no-file-parallelism`
- `pnpm typecheck`
- `pnpm format:check`
- `git diff --check`

## 构建与安装验收

源码和安装态 UI 验收通过后，将四个版本源统一升级到 `3.19.1-20`，运行一次本地 release 流水线，并使用同一个独立事务进程完成 `kill -> 等待退出/端口释放 -> 卸载 -> 安装 -> 隐藏启动 -> health/version/hash 校验 -> 失败回滚`。不得在普通交互 shell 中单独停止 CCSM。

安装后在实际 MultiRouter 工作台检查六项导航、协议按钮、搜索筛选、单模型折叠、高级字段/TOML 渐进披露和保存反馈。除非另行授权，不 push、不创建 PR、不创建 GitHub Release。

## 研究依据

- W3C WAI-ARIA APG Tabs：一个 tab 对应一个 tabpanel，当前 tab 通过 `aria-selected=true` 表达，并定义方向键与 Enter/Space 键盘行为。
- W3C WAI-ARIA APG Accordion：用于减少多段内容造成的页面滚动；标题按钮通过 `aria-expanded` 和 `aria-controls` 表达展开关系。
- Radix Accordion：项目现有依赖支持 single/collapsible、完整键盘导航和 WAI-ARIA Accordion pattern，可直接复用。

Codex 内置 Web Search 与 Matrix WebSearch 两条独立链都定位到 Radix 官方 Accordion 文档并得到一致结论；Matrix 对 W3C 页面受 403 安全验证阻挡，W3C 细节以 Codex 内置搜索读取的官方页面为准，没有使用二手页面替代。

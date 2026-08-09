# Codex Sub-Agent V1/V2 设置设计

日期：2026-08-09

## 目标

CCSwitchMulti 在每个 Codex MultiRouter 方案中同时保留 Sub-Agent V1 与 V2 的配置，由用户选择当前生效版本。新方案与没有显式版本字段的旧方案默认使用 V2；切换版本不删除另一套配置。

本次交付必须覆盖配置模型、向导左侧导航、MultiRouter 工作台、Codex 配置投影、兼容迁移、自动化测试、Windows 安装包和真实运行态验收。

## 官方实现边界

OpenAI Codex 当前源码把 V1 与 V2 注册为两套不同运行时：

- V1 使用 `multi_agent_v1` namespace，主要工具为 `spawn_agent`、`send_input`、`resume_agent`、`wait_agent` 和 `close_agent`，以 agent id 和深度限制组织子线程。
- V2 使用可配置 namespace 下的任务路径工具，主要为 `spawn_agent`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent` 和 `list_agents`，支持 task path、mailbox、follow-up 和更清晰的并发协作语义。
- Codex 先读取 feature override，再读取模型目录的 `multi_agent_version`。启用 `features.multi_agent_v2` 会覆盖模型目录，因此 V1 不能只改 models.json。
- V1 与 V2 的 `spawn_agent` schema 和 handler 不同。同一 MultiRouter、同一会话只能选择一套协议，不能把“同时保留”实现成同一会话混用。
- 两个版本都能暴露 `agent_type`；本产品将 V2 作为 managed roles 自动选型的默认体验，将 V1 保留为显式模型覆盖和旧工作流兼容入口。

参考源码固定在调研时的 OpenAI Codex commit `646f7c0a91b8e327d263335da68ae8ef212895ce`。

## 产品方案

### 生效模型

每个 MultiRouter 保存一个当前生效值：

```ts
type CodexSubagentVersion = "v1" | "v2";
```

两套配置都可查看和编辑，但运行时只投影当前版本。切换版本属于显式操作，UI 必须说明需要重新启动 Codex 新会话才能完整生效。

### V1 定位

显示名为 `Sub-Agent V1（兼容）`。

适用场景：

- 需要兼容旧版 multi-agent 工具工作流。
- 需要显式控制 direct model override 的候选顺序。
- 依赖 agent id、`send_input`、`resume_agent` 或 `close_agent` 的既有操作方式。

V1 页面继续使用现有 `spawnAgentModels` 编辑能力，清楚标注最多前 5 个候选会进入 Codex 工具描述。它不是 V2 自动角色目录，也不与 managed role 数量绑定。

### V2 定位

显示名为 `Sub-Agent V2（推荐）`。

适用场景：

- 新版 Codex 的 task path、follow-up、mailbox 和并发协作。
- 用户不选择子模型，由父 Codex 按任务选择 managed custom role。
- DeepSeek Flash 用于长上下文阅读、代码扫描、架构追踪和轻量验证；DeepSeek Pro 用于复杂调试、跨模块推理、架构决策、高风险审查和复杂实现。

V2 页面显示运行时角色预览，包括 role 名称、模型、provider、reasoning 和用途说明。角色由完整可路由模型目录派生，不提供必填模型选择器。

## 持久化与迁移

### 单一数据源

新增方案级字段：

```json
{
  "settingsConfig": {
    "codexRouting": {
      "subagentVersion": "v2"
    },
    "modelCatalog": {
      "spawnAgentModels": ["deepseek-v4-pro", "deepseek-v4-flash"]
    }
  }
}
```

- `codexRouting.subagentVersion` 是当前生效协议的唯一数据源。
- `modelCatalog.spawnAgentModels` 保持原字段和语义，作为 V1 direct model override 顺序；不重命名、不复制到新结构。
- V2 roles 由完整可路由 catalog 派生，不额外保存一份容易漂移的角色列表。

### 默认与迁移

- 新建 MultiRouter 默认 `v2`。
- 旧方案缺少 `subagentVersion` 时读取为 `v2`，保存时补写 `v2`。这是对当前 CCSM 已强制 V2 运行行为的无损迁移。
- 旧 `spawnAgentModels` 原样保留，即使当前选择 V2；切回 V1 后继续使用。
- 模型目录刷新仅移除已不存在的 V1 模型引用，不改变 `subagentVersion`。
- 未识别的版本值安全回退为 V2，并在 UI/诊断中显示可修复警告。

## UI 设计

### MultiRouter 设置向导

左侧导航在“生成路由规则”之后、“保存并发布”之前增加两个独立步骤：

1. `Sub-Agent V1`
2. `Sub-Agent V2`

两个步骤始终可见，便于用户比较。每页包括：

- 一句话定位与适用场景。
- 与另一版本的关键区别。
- 当前是否生效。
- `启用 V1` 或 `启用 V2` 操作。
- 对应配置或运行时预览。

导航项显示状态标签：`当前使用`、`已配置` 或 `推荐`。用户走完整向导时可以查看两页；只有选择版本是必需决策，V2 不要求选择模型。

### MultiRouter 工作台

把当前折叠的“高级：子 Agent 模型覆盖”替换为完整 `Sub-Agent 设置` 区域：

- 顶部显示 V1/V2 分段切换和当前生效版本。
- V1 面板复用现有候选排序编辑器，标题明确为 `V1 direct model override`。
- V2 面板显示 managed roles、路由可用状态和角色缺失诊断。
- 保存、刷新、模型同步和重启后保持版本与 V1 候选顺序。
- 不在应用全局 Settings tabs 新增重复入口；协议选择属于具体 MultiRouter 方案。

### 文案原则

V1 不描述为“旧且不可用”，而是“兼容与显式控制”；V2 不描述为“更多模型”，而是“新任务路径协议与自动角色选型”。页面明确提示：修改后应重启 Codex app-server 并开启新会话，已有会话可能已经锁定协议版本。

## 后端投影

### 版本解析

新增集中式版本解析函数，所有模型目录、TOML feature、managed role 和诊断逻辑都调用它，避免 UI 字段与运行时分叉。

### V1 投影

- 对当前 MultiRouter catalog 的所有模型同时写入 `multi_agent_version="v1"` 与兼容 camelCase 字段。
- 将 `[features.multi_agent_v2].enabled` 显式写为 `false`，防止 feature override 把模型元数据重新覆盖成 V2。
- 保持 `[agents].enabled` 可用以及现有并发默认值。
- 不强制 V2 的 `tool_namespace="agents"`、`hide_spawn_agent_metadata` 等策略。
- 不生成 CCSM V2 DeepSeek managed roles；清理时只删除 CCSM 自己有 marker 的托管文件，用户 role 不动。
- `spawnAgentModels` 继续决定 direct override 前 5 个展示顺序。

### V2 投影

- 对所有当前路由模型统一写入 V2 metadata。
- 显式启用 `features.multi_agent_v2`，保持 `hide_spawn_agent_metadata=true`。
- 混合 provider 路由继续使用非保留 `tool_namespace="agents"`；用户已有其它非保留 namespace 时保留。
- 从完整当前可路由 catalog 生成 `deepseek-flash` 与 `deepseek-pro` managed roles，不受前 5 个 direct overrides 限制。
- 用户同名 role 保留，CCSM 使用 `ccswitch-<role>` 回避覆盖。

### 切换与清理

切换版本只改变下次配置投影，不改历史 rollout。保存当前 provider 后同步 models.json、config.toml 和 CCSM managed role 文件；任何一步失败都返回错误，不在 UI 假报已切换。

## 错误处理与诊断

- 诊断接口返回配置版本、有效运行版本、feature override 状态、模型 metadata 一致性、V1 候选可见数量和 V2 managed role 状态。
- UI 区分“配置已保存”和“新会话已验证”。
- 若 catalog 内出现 V1/V2 混杂，诊断为阻塞错误并提供重新投影操作。
- 若 V2 所需 Pro 或 Flash 模型当前不可路由，显示降级警告；不伪造 role 文件。
- 若 V1 direct override 模型已移除，沿用现有 prune 行为并列出被移除项。

## TDD 与提交边界

按仓库规则拆分本地提交，不推送、不发布：

1. 设计规格与 memory。
2. RED：版本解析、迁移和后端投影测试。
3. GREEN：后端实现。
4. RED：向导与工作台 V1/V2 UI 测试。
5. GREEN：前端实现。
6. 版本号与本地构建交付。
7. 真实安装和运行验收记录。

每次提交信息必须细致说明改动与证据，并以 `本次提交由BigStrongsSun完成` 结尾。

## 自动化验收

### 后端

- 缺少版本字段默认 V2，非法值回退 V2并产生诊断。
- V1 同时写入 catalog V1 和 `multi_agent_v2.enabled=false`。
- V2 同时写入 catalog V2、启用 feature 并保持混合路由 namespace 规则。
- V1/V2 切换不会丢失 `spawnAgentModels`。
- V1 不生成 CCSM managed roles，且不删除用户 role。
- V2 在 Pro 位于第 6 位或更后时仍生成 Pro/Flash 两个 role。
- direct override 排序不增删 V2 roles。
- stale prune、reserved schema 和官方认证来源回归继续通过。

### 前端

- 向导左侧同时显示 V1/V2 两项及区别文案。
- 新方案默认 V2；旧方案迁移 V2并保留 V1 候选。
- 两页均可切换生效版本，另一套配置不丢失。
- 工作台显示当前版本并分别提供 V1 编辑器和 V2 role 预览。
- 保存、刷新、模型同步和重新挂载保持状态。
- 非 Sub-Agent 路由、认证、模型碰撞和发布流程无回归。

## 构建、安装与真实验收

实现完成后提升版本到 `3.19.1-15`，同步 package、Cargo、lockfile 和 Tauri 配置，执行完整 Windows Tauri 构建并导出安装器、portable 包、签名和 SHA-256。

安装前记录当前 CCSM/Codex 版本、进程和配置；安装新包后完全退出并重启 CCSM 与 Codex app-server，使用新会话验收：

- UI：向导左栏与工作台均显示 V1/V2；切换、保存、刷新和重启后持久化。
- V1：选择 V1 后确认 session/turn/child rollout 记录 V1，direct override 子模型真实执行至少一次只读工具并返回结果。
- V2 Flash：提示只描述长上下文扫描，不出现模型名；父 Codex 自动选择 `deepseek-flash`，child 完成只读工具、final 和 follow-up。
- V2 Pro：提示只描述复杂深度排查，不出现模型名；父 Codex 自动选择 `deepseek-pro`，child 完成只读工具、final 和 follow-up。
- Rollout：核对真实 `agent_role`、model、provider、协议版本和任务正文。
- Router 日志：Flash Responses 与 Pro Chat bridge 分别命中正确上游并返回 HTTP 200。

手工 `model=` canary 只能补充验证，不能代替 V2 无模型名自动选型。若当前 Codex 版本对 V1 或 V2 存在运行限制，必须记录精确版本、错误、rollout 和日志，不用配置存在替代真实验收。

## 非目标

- 本轮不改变 Qwen 行为，也不把 Qwen 纳入运行验收。
- 不在代理层改写 `spawn_agent` 参数。
- 不修改 Codex reserved tool schema。
- 不自动推送分支、不创建 PR、不发布 GitHub Release。
- 不为每个模型提供独立 V1/V2 混用开关。

## 搜索证据与不确定性

- Codex 内置 Web 搜索未返回能直接说明内部 `multi_agent_version` 的官方文档页面。
- OpenAI 官方 Codex 仓库当前源码确认 V1/V2 handler、工具 schema、feature 优先级和模型 metadata 选择边界。
- Matrix WebSearch 独立链路返回 HTTP 521，未提供正证据。
- 第三方 parent/child 在本机 V1 下的最终兼容性必须由安装后的真实 canary 决定，设计阶段不把源码可配置性等同于运行成功。

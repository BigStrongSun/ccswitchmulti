# CCSM Agent Mesh 后端开发路线

- 状态：规划文档，未开始实现
- 日期：2026-08-01
- 分支：`bigstrongsun/ccsm-agent-mesh`
- 范围：统一模型聚合网关、共享路由、Agent 适配层、凭据、部署与验证
- 不包含：ACPs Agent Adapter、Token Exchange、智能体互联 UI

## 1. 目标

把 CCSM 从“Codex MultiRouter”演进为“统一本地模型聚合网关 + Agent 适配层”：

- 一个网关对外提供 `/v1/models`、`/v1/responses`、`/v1/chat/completions`
- 所有 Agent 都指向同一个网关
- 网关按请求中的 `model` 路由到不同上游
- 每个 Agent 只做薄适配：配置、认证、模型投影、重启、验证、恢复
- 不重写现有协议转换器，而是封装成可复用协议适配层

## 2. 目标架构

```text
统一网关层          /v1/models /v1/responses /v1/chat/completions
共享模型服务层      ModelService / ProtocolEndpoint / CapabilityEvidence
共享路由层          RoutePolicyTemplate / AgentBinding / Overlay / Instance
协议适配层          Responses / Chat / Anthropic / 媒体 / cache / compact / SSE
Agent 适配层        Codex / Claude Code / OpenClaw / Hermes / Gemini CLI
部署与验证层        Snapshot / Canary / Rollback / Drift
控制面/数据面       Control backend / routerd
```

## 3. 当前 CCSM 与目标差距

| 后端大功能 | 当前 CCSM | 目标 | 优先级 |
|---|---|---|---|
| 模型服务模型 | Provider 按 App 分多套，Codex 有 MultiRouter | 一个模型服务可挂多个协议 endpoint | 先做 |
| 统一模型目录 | Codex 有 catalog，其它 App 各自维护 | 一份目录快照 + 能力信息 + 多 Agent 投影 | 先做 |
| 统一 LLM 网关 | Codex 代理与 External API 分离 | 统一 `/v1/models`、`/v1/responses`、`/v1/chat/completions` | 先做 |
| 能力证据 | 有探测和日志，无持久化能力模型 | 每个协议/端点有可过期能力证据 | 先做 |
| 凭据管理 | Key/OAuth 存在配置和 DB | CredentialRef + Secret Broker + 短期 lease | 先做 |
| 认证来源 | native/managed/账号池 | caller/live/managed/provider/none 显式建模 | 先做 |
| 共享路由 | codexRouting 只服务 Codex | 共享模板 + 每个 Agent 独立绑定/投影 | 先做 |
| Agent 模型投影 | 只有 Codex picker/spawnAgent | 每个 Agent 都有 return/picker/subagent 三套投影 | 先做 |
| 协议适配层 | 转换已实现，但与 Codex 深度耦合 | 按方向和能力声明的协议适配器 | 先做 |
| Agent 配置适配器 | 只有 Codex takeover | 每个 Agent 一个薄适配器 | 网关稳定后 |
| 部署生命周期 | 添加/切换直接写 live config | draft → validate → publish → active + rollback | 后做 |
| 控制面/数据面分离 | Tauri 后端持有代理 | Control backend + 独立 routerd | 后做 |
| 端到端验证 | 有状态/日志/stream check | 发布 canary + 证据过期 | 后做 |
| compact 能力 | 按 route 转换 compact | 每目标显式声明 compact 兼容策略 | 后做 |
| 观测与用量 | 已有日志/usage，按 App 分散 | 统一 route evidence、request、usage、quota | 后做 |
| operation/revision | 无版本化 mutation | revision + operation id + 可重连 | 后做 |

## 4. 开发阶段

### 第一阶段：统一网关基础

- 建立 ModelService / ProtocolEndpoint / CapabilityEvidence 领域模型
- 建立 CredentialRef 与 Secret Broker 边界
- 建立统一认证来源模型和 caller bearer 丢弃规则
- 提供统一 `/v1/models`、`/v1/responses`、`/v1/chat/completions`
- 把现有协议转换器封装成协议适配器

### 第二阶段：共享路由与 Agent 投影

- 建立共享路由模板和 Agent 独立绑定
- 建立模型目录三套投影：return / picker / subagent
- 保留 Codex 适配器，并接入第二个真实 Agent 验证链路

### 第三阶段：部署与验证

- 增加不可变部署快照、stage/activate/rollback
- 控制面与数据面拆分
- 增加端到端 canary 和证据过期
- 统一观测、用量、compact 能力约束

### 后续

- AgentMesh 原型定稿后再设计 UI
- ACPs Adapter / Token Exchange 另行规划

## 5. 验收标准

- 一个非 Codex Agent 可以通过统一网关真实完成一次模型请求
- 模型列表、默认模型、子 Agent 候选按 Agent 正确投影
- 第三方目标不会收到 Codex caller bearer
- 发布失败可以回滚到上一个可用快照
- compact 请求与普通请求使用同一路由和认证语义

## 6. 明确不做

- 不立即重构现有 UI
- 不实现 ACPs Adapter / Token Exchange
- 不按粗糙原型提前创建复杂领域对象
- 不重写已经可用的协议转换实现

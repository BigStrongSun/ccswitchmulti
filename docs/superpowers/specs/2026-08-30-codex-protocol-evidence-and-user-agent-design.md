# Codex 分协议探测证据、用户覆盖与 User-Agent 同源设计

## 背景与根因

当前深探测会对一个模型依次运行 Responses 与 Chat Completions，但最终只生成一条以 selector 推荐协议为 target 的 `ProtocolCompatibilityRecord`。另一协议的 branch 虽嵌在同一 JSON 中，却没有自己的持久化 target，无法独立查询、保留失败原因或在高级覆盖后继续解释结果。

前端又把 `apiFormat` 纳入探测结果的 UI identity，并仅在自动模式提交 probe receipt。探测推荐 Responses 后，用户把最终协议改为 Chat 会立即丢失当前 receipt；保存阶段只能二次探测或把协议重新改回推荐值。这把“探测事实”“自动推荐”“用户最终选择”错误地合并成了一个状态。

User-Agent 还有一处请求同源缺口：探测请求只在 Provider 配置了 `customUserAgent` 时发送 UA；生产转发在未配置时却可能透传 Codex 客户端 UA。于是同一个 Provider/model 的探测和真实请求可能经过不同的网关策略，探测得到的 403/5xx 不能证明生产协议不兼容。

## 不变量

- 每个启用模型仍只执行一次双协议深探测事务，不重复收费或重复发起两轮探测。
- Responses 与 Chat 的阶段结果、reasoning 形态、工具 Schema、续轮策略和失败原因必须分别持久化。
- selector 的结果是 `automatic recommendation`，不得被用户覆盖写回或删除。
- 高级模式的协议选择是 `final user choice`，只改变最终 Provider/Provider Set 形态，不篡改探测证据。
- 普通模式仍只允许 selector 选择的 Verified 协议自动归组；Partial/Unverified 分支永远不能自动归组。
- 可执行 compatibility profile 与诊断 observation 分开：运行时只消费已选、Verified、target 精确匹配的 profile。
- 推理强度、raw reasoning 与 native summary 的语义不在本设计中改变。

## 数据模型

新增 `protocol_probe_observations` 表。每次模型双协议探测生成两条 observation：

- target 为逻辑 source Provider、public/upstream model、具体 transport、endpoint/auth/credential/request-policy fingerprint；
- result 只保留该 transport 的 branch；
- `result.readiness` 为该 branch 自己的 readiness；
- `result.selected_transport` 保留同一次 selector 的自动推荐，哪怕推荐的是另一协议；
- Verified observation 保存 30 天，Partial/Unverified 保存 7 天；同一精确 target 的新结果覆盖旧结果。

现有 `protocol_compatibility_profiles` 继续只保存 Provider Set commit 后可执行的已选 profile。Provider Set 的自动归组、leaf rebind 和运行时解析不从 observation 表读取。

observation 以稳定的逻辑 source Provider ID 保存，不随自动叶子拆分迁移。删除逻辑 source 时删除其 observations；删除或折叠内部叶子不得删除 source observation。

## 探测、保存与覆盖数据流

1. production-equivalent preparer 为一个模型准备 Responses/Chat 请求并执行双分支探测。
2. runner 返回完整 result，selector 产生 automatic recommendation。
3. command 从完整 result 构建两条 transport-specific observations，并在返回 preflight 结果前原子保存。
4. preflight 仍生成一条 selection receipt 供 Provider Set planner 使用；receipt 不等于 observation。
5. 普通自动保存消费 receipt，只把推荐且 Verified 的协议归组为 Single/Split，并生成可执行 profile。
6. 高级覆盖允许用户把最终协议改为 Responses 或 Chat。已有 receipt 可随表单提交并在成功后清理，但 planner 不使用它改变用户选择；两条 observations 保持不变。
7. UI identity 只包含真正改变双协议请求的 endpoint、auth、UA/header/body override、模型映射、reasoning/tool/history 请求策略等字段。单纯切换最终 `apiFormat` 或自动/手动选择源不使探测证据失效。
8. endpoint、Key、UA、header/body override、模型或兼容参数变化仍立即使 receipt 失效，并由 request-policy fingerprint 阻止旧 observation/profile 被当作当前结果。

## User-Agent 请求策略

第三方 Codex 请求使用确定性 UA 优先级：

1. `meta.customUserAgent`；
2. CCSwitchMulti 默认产品 UA `CCSwitchMulti/<version>`。

Copilot、Codex OAuth、xAI OAuth、Claude Code impersonation 等已有专用身份路径不经过此第三方默认策略，保持原优先级。

探测和生产都调用 `CodexThirdPartyRequestPolicy` 的同一 header policy。生产第三方请求不再在无自定义 UA 时偶然透传某个 Codex 客户端版本。UA 的实际 header value 已进入 prepared-request fingerprint，因此自定义 UA 或应用版本变化会使旧 receipt/profile 失效。

默认 UA 只标识 CCSwitchMulti 本身，不伪装浏览器或其他客户端。若网关明确要求浏览器/Codex 等特定 UA，用户在 Provider 高级设置填写自定义值，探测和生产同时采用该值。

## 前端交互

- 普通模式显示两条协议的独立探测过程和 automatic recommendation，不提供逐模型协议编辑。
- 高级模式明确显示“最终使用协议”选择；用户可在探测推荐 Responses 后选 Chat，反之亦然。
- 切换最终协议不会清空刚完成的双协议结果或 receipt。
- 修改 UA、headers/body override、URL、Key、模型或请求兼容设置仍清空结果并要求重测。
- 保存提示同时区分“自动推荐”和“最终用户覆盖”，避免把覆盖显示成探测结论。

## 失败与迁移

- 401/403/429、网络、5xx 继续按 availability/auth 分类保存为 observation，不据此推断协议不支持。
- observation 保存失败会使 preflight 返回持久化错误；不能向用户声称探测结果已保存。
- schema v18 升级到 v19 只新增表，不改写现有 executable profiles。
- 旧版只有一条选中 profile 的数据继续可运行；只有重新深探测后才产生完整双协议 observations。

## 验收

- 一个 Responses 推荐、Chat 也 Verified 的模型在 observation 表有两个不同 target_key，且 recommendation 相同、branch/readiness 各自独立。
- 一个 Responses Verified、Chat Partial 的模型仍保存两条 observations，但 Provider Set 只能自动选择 Responses。
- 探测推荐 Responses 后切手动 Chat，提交 payload 仍携带当前 receipt，最终 Provider 为 Chat，observations 不变。
- 无自定义 UA 时探测与生产都发送 `CCSwitchMulti/<version>`；有自定义 UA 时两者都发送自定义值。
- UA 变化导致 request-policy fingerprint 改变，旧 observation/profile 不命中。

# 2026-08-25 Qwen MultiRouter 推理档案同步修复

## 现象与证据

- 安装态 `3.19.2-17` 的 Qwen 任务已实际使用 `qwen3.8 / ultra`，MultiRouter 转发到 vLLM Chat Completions 且请求成功。
- 上游协议探测确认 Qwen 在非流与流式响应中分别返回 `choices[].message.reasoning` 和 `choices[].delta.reasoning`。
- 独立 Qwen Provider 的协议档案后来已是 `verified`，Chat 的 baseline、streaming、forced tool、continuation 四阶段全部通过。
- 同一 endpoint、模型和凭据指纹对应的 MultiRouter route 专属档案仍是更早的 `partial`，forced tool 为 `unsupported`。运行时严格按 parent router + route id 查询档案，因此自动推理投影返回 `None`。

## 根因

- Provider 保存会重建 MultiRouter 配置投影并同步 Subagent 目录，但此前没有同步第三类派生状态 `protocol_compatibility_profiles`。
- 自动 preflight 把新档案保存为独立 Provider 身份；已有 route 身份档案不会随 Provider 更新，导致路由配置已更新而协议能力证据仍陈旧。

## 修复

- Provider mutation 在保存前重新编译所有受影响且启用的 V2 Router 实际候选。
- 仅当公开模型、上游模型、transport、endpoint 指纹、认证类型和凭据指纹全部一致，且 Provider 档案为当前版本 `verified` 时，才把结果物化为 route 专属档案。
- Provider 自身档案和派生 route 档案在同一个 SQLite 事务中提交。
- endpoint 或凭据不一致时绝不物化 route 档案，避免跨路由借用证据。
- 后续工具循环审计又发现升级兼容缺口：已有安装升级后若用户不重新保存 Provider，旧 route `partial` 档案仍会残留。运行时现先查 route 精确档案；档案不可用时，仅通过物化 Provider 上的 `codexResolvedTargetProviderId` 查询目标 Provider 档案。由于目标键继续绑定公开/上游模型、transport、endpoint、认证类型和凭据指纹，这不是按名称或模型做宽松 fallback。

## 验证

- 新增 RED/GREEN 回归：Provider verified 档案必须生成等价 route 档案并得到 `RawReasoningText`。
- 新增反向回归：凭据指纹不一致时 route 档案必须不存在。
- `protocol_compatibility` 65/65、`codex_multirouter` 84/84、`commands::provider` 18/18、协议档案事务 5/5 通过。
- `cargo check --tests --no-default-features` 通过，保留 5 条既有 dead-code warning。
- 尚未构建、安装或替换当前运行中的 `3.19.2-17`。

## Qwen 工具循环补充审计

- 任务 `01a032c7-1005-7242-9f9b-88c56e16d13d` 在 2026-08-25 10:44-10:49 期间把两条完全相同的 `exec_command` 各执行 13 次。每轮 call ID 不同、两条结果都成功写回、CCSM 也收到新的 `/responses` 并生成新的 `/chat/completions` 请求；不是 UI 重复、代理重放或失败重试。
- 该时段 live DB 中独立 Qwen Provider 是 `verified/open_ai_chat/readable`，对应 MultiRouter route 仍是 `partial`。rollout 的 26 个循环工具调用之间没有 reasoning/commentary item，符合 route 投影为 `None`、真实 reasoning 未进入 Codex 历史的运行路径。
- Responses→Chat 转换已有并行工具回归，能生成一条 `assistant.tool_calls` 和两条 ID 匹配的 `tool` 消息；连续请求体每轮增长约 14.5 KiB，也证明工具结果没有整体丢失。
- 新 RED/GREEN 回归覆盖已有旧 route `partial` 且目标 Provider `verified` 时，无需重新保存 Provider即可继承 reasoning projection；endpoint 改变时继续返回 `None`。第二条回归覆盖相同条件下继承已检测 transport。
- 这修复的是 CCSM 确认存在的历史降级条件。Qwen/vLLM 本身已有收到工具结果后仍重复调用的公开同类问题，因此不能仅凭源码修复宣称模型永不重复；安装新构建后仍需用真实 Qwen 任务验证 reasoning item 恢复和重复行为。

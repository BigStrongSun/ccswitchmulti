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
- Provider 自身档案和派生 route 档案在同一个 SQLite 事务中提交；运行时仍严格按 route identity 查询，不增加宽松 fallback。
- endpoint 或凭据不一致时绝不物化 route 档案，避免跨路由借用证据。

## 验证

- 新增 RED/GREEN 回归：Provider verified 档案必须生成等价 route 档案并得到 `RawReasoningText`。
- 新增反向回归：凭据指纹不一致时 route 档案必须不存在。
- `protocol_compatibility` 65/65、`codex_multirouter` 84/84、`commands::provider` 18/18、协议档案事务 5/5 通过。
- `cargo check --tests --no-default-features` 通过，保留 5 条既有 dead-code warning。
- 尚未构建、安装或替换当前运行中的 `3.19.2-17`。

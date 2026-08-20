# 2026-08-21 Codex policy rejection SSE 可观测性

## 现象与根因边界

- `Invalid prompt: your prompt was flagged as potentially violating our usage policy` 不在现有 CCSM 日志正文中出现；问题 session 的上游请求大多是 HTTP 200，说明不能只按 HTTP 状态码判断成功。
- 现有 `forwarder.rs` 只在首个 SSE chunk 阶段识别 `event: error` / `response.failed`，并在响应头/首包通过后记录 `response_ready`。原生 Codex Responses 透传的中途 SSE 失败事件随后直接交给客户端，CCSM router log 没有独立事件，因此无法从旧日志还原具体策略命中内容。
- 官方 GPT-5.6 文档说明生成过程中存在实时安全分类器，且长 session 会放大重复的 prompt/tool 上下文；这支持“上游安全拦截 + 本地流式错误不可观测”的判断，但不能证明具体命中了哪段 prompt。

## 本次源码改动

- `src-tauri/src/proxy/providers/streaming_retry.rs` 新增原生 Responses SSE 错误观察器：识别中途 `response.failed`、`error`、`response.error`，记录 `upstream_sse_error`。
- 日志只保留 `session`、`model`、`provider`、事件名、受限 `error_type`、固定 `message_class`（如 `policy_or_safety`）和 SHA-256 短 hash，不写 prompt、headers 或完整错误正文。
- 观察器在有无重连工厂的原生流都生效；原有字节透传和重连行为不改变。
- `handlers.rs` 为原生 Codex Responses 流传入 session/model/provider 关联信息，并对所有成功非 JSON 流启用观察包装。

## 验证

- 先加入脱敏分类回归测试，未实现时确认编译失败；实现后 `native_responses_sse_error_diagnostic_is_redacted_and_classified` 通过。
- `native_codex_stream_recovery_only_wraps_successful_non_json_streams` 通过。
- `cargo fmt` 通过；尚未做安装版构建/替换，源码测试通过不等于当前运行中的 CCSM 可观测性已经更新。要让后续问题写入新事件，必须构建并安装包含本提交的 CCSM。

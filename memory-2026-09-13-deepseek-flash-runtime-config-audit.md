# 2026-09-13 DeepSeek Flash 运行态配置复核

## 结论

- 当前运行实例不是旧版未安装：`127.0.0.1:15721` 由 `3.20.2-5` 的 CCSwitchMulti 进程监听，运行文件与安装目录中的同版本二进制一致。
- 活动 `config.toml` 已启用 `show_raw_agent_reasoning = true`、`hide_agent_reasoning = false`，并指向 `C:\Users\sunda\.codex\cc-switch-model-catalog.json`。
- 活动 catalog 的 `deepseek-flash` 已包含 `reasoning_summary_format`/`reasoningSummaryFormat = experimental`、`default_reasoning_summary`/`defaultReasoningSummary = none`；该配置符合 DeepSeek 原生 Responses 的语义。
- 当前 route 命中 `DeepSeek-responses` 的 `openai_responses` 原生路径，日志确认 `responses_to_chat = false`。CCSM 没有吞掉 raw reasoning。

## 运行证据

- 最近请求连续返回 HTTP 200，目标为 `https://api.deepseek.com/v1/responses`，模型为 `deepseek-flash`。
- 对应 Codex rollout 中的 reasoning item 具有非空 `raw_content`，而 `summary_text` 为空；说明 raw reasoning 已进入 Codex 会话存储，但没有上游摘要可供 Desktop 主会话摘要投影使用。

## 边界与后续

- 该问题不是缺少 CCSM provider 开关，也不是需要把 `default_reasoning_summary` 改成 `detailed`。DeepSeek 文档说明其 Responses API 接受 summary 参数但不生成摘要。
- CCSM 不应把完整 raw reasoning 伪装成 `summary_text`，否则会改变协议语义并污染后续历史/续轮。
- 要让 raw-only reasoning 在 Desktop 主会话中可见，需要 Codex Desktop renderer 读取 `reasoning.content`/`item/reasoning/textDelta`；仅重启 CCSM 或重写 catalog 不能修复该渲染缺口。重启 Codex Desktop 只能确保它重新读取最新配置，不能替代 renderer 修复。

# 2026-09-11 DeepSeek Flash 通过 CCSM 无可见推理的根因

## 结论

- 这是“同一可见症状、不同 CCSM 根因”。GLM 旧 Chat 路由曾在 CCSM 的
  `reasoning_content -> Responses` 投影策略处丢失推理；DeepSeek Flash 当前走原生
  Responses，CCSM 没有把推理丢掉。
- DeepSeek Flash 的当前请求命中 `router-98e3bdc6-710d-4236-b47b-7ce7e4884365`，目标
  Provider 为 `DeepSeek-responses`，`apiFormat=openai_responses`，上游为
  `https://api.deepseek.com`，模型为 `deepseek-flash`。
- 数据库中的协议兼容档案为 `open_ai_responses/verified`，推理形状为
  `semantic=readable/source=native_responses`，探测事件包含大量
  `response.reasoning_text.delta` 与 `response.reasoning_text.done`。
- 当前 Codex 配置仍是 `show_raw_agent_reasoning=true`、`hide_agent_reasoning=false`。
  对应的 2026-09-11 DeepSeek Flash rollout 中，`response_item.type=reasoning` 的
  `content[].type=reasoning_text` 持续有非空内容，而 `summary=[]`；说明原始推理已被
  Codex 接收并持久化，缺口在主会话可见投影/桌面渲染，不在上游或 CCSM 传输。

## 源码边界

- 原生 Responses 成功流在 `src-tauri/src/proxy/handlers.rs` 的
  `handle_responses_for_app` 路径只包裹可重连 SSE 流并透传字节；当前 DeepSeek 请求不
  经过 Chat -> Responses 转换。
- 同文件约 4517 行的 `responses_response_to_full_sse` 只用于缓冲的非原生/compaction
  fallback，并且只读取 `/summary/0/text`。不能据此推断当前 DeepSeek 原生流会丢掉
  `content[].reasoning_text`。

## 证据与修复边界

- DeepSeek 官方 Responses 文档明确：`reasoning` 的 `summary` 参数“接受但不生成摘要”，
  推理正文在 reasoning item 的 `content` 中，并通过 `response.reasoning_text.delta/done`
  发送。因此不能在 CCSM 中把 raw content 伪装成 `summary_text`。
- 正确修复属于 Codex Desktop 的 raw-reasoning renderer：在允许显示 raw reasoning 时，
  主会话投影必须读取 `reasoning.content`，并保留 summary 与 raw content 的不同语义。
  仅修改 DeepSeek 的 CCSM projection、切换到 Chat bridge 或伪造 summary 都不能形成
  正确修复。

## 复核边界

- 运行中的 CCSM 进程为本地安装的 3.20.2-3；源码 `main` 当前已包含 GLM 投影修复及后续
  3.20.2-4 release-prep 提交。源码提交、安装二进制和 Desktop renderer 仍需分别验收，
  不能用源码已合并替代安装运行时已修复。

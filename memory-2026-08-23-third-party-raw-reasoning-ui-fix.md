# 2026-08-23 第三方模型在 Codex 中只显示单行闪烁推理的根因与修复

## 现场证据

- 截图对应任务 `01a02d95-045f-72b1-a7e2-8bdb888c8abc` 实际走
  `qwen3.8 -> /v1/chat/completions`；`codex-router.log` 证明其请求和上游均为
  HTTP 200 的 SSE 流，而非 UI 本身卡死。
- 会话文件中每轮均保存了一整段第三方 reasoning summary，但每条 `Reasoning`
  的 `raw_content` 均为空；即使 live `config.toml` 已有
  `show_raw_agent_reasoning = true`，Codex 也只能渲染灰色摘要，而无法显示可展开的
  原始推理。
- 同一任务后来反复产生完全相同的两条 `exec_command` 调用；每轮 `call_id` 与工具
  结果都一一对应，且命令结果真实存在。这是 Qwen/vLLM 在收到正确工具结果后仍重复采样的
  工具循环，不能误判为命令未运行或 CCSM 静默吞掉结果。

## 根因

- `streaming_codex_chat.rs` 自 2026-06 的基础实现起，把所有第三方
  `reasoning_content` 统一写为 Responses `summary_text`，并只发送
  `response.reasoning_summary_*` 事件；完成 item 也只有 `summary`。
- 第三方已经明确返回的是可显示的 raw reasoning，而不是 OpenAI 生成的摘要。Codex 只有在
  收到 `reasoning_text` content 与 `response.reasoning_text.*` 时才会填充 raw reasoning
  视图。因此设置已启用仍表现为截图中的单行/闪烁状态。
- 这不是 2026-08-21 的 mixed hosted/function stream 或 commentary 合并修改造成：相关
  转换回归确认 assistant tool_calls、tool_call_id、role=tool 的顺序正确，符合 Qwen/vLLM
  官方 function-calling 形状。重复工具调用仍需作为上游模型/模板行为单独观测，不能用代理层
  任意篡改模型策略掩盖。

## 修复

- 新增 raw reasoning SSE 生命周期：in-progress `content: []`、
  `response.reasoning_text.delta`、`response.reasoning_text.done` 与完成 item 的
  `content: [{ type: reasoning_text, text }]`。
- Chat 流式转换仅将第三方 `reasoning_content` 走上述 raw 生命周期；原有 summary
  生命周期保留给真正只提供摘要或加密 reasoning 的路径。
- Responses -> Chat 回放识别 raw `reasoning.content[]`，将其重新附着在对应的 assistant
  tool-call message 上，使 Qwen/vLLM 在紧接的 `role=tool` 结果前保有连续推理上下文。

## 验证

- RED：原实现不含 `response.reasoning_text.delta`，并把 raw content 回放降级为
  `reasoning_content = "tool call"`；现已分别加入流式和回放回归断言。
- 当前工作树的 Rust runner 被另一项未提交的 `preset_registry.rs` 改动阻断（签名 payload
  格式占位符数不匹配，且测试模块缺少 `ed25519_dalek::Signer` import）；干净临时工作树再跑时
  被 Windows 磁盘空间不足（错误 112）阻断。因此本轮没有把测试标为已通过。先修复/隔离该无关
  改动并释放足够构建空间，再运行流式转换、raw-replay、`streaming_codex_chat` 与
  `transform_codex_chat` 定向测试。
- 仍需构建并安装新版本后，以新建的第三方 Codex 任务进行桌面端 UI 验收。

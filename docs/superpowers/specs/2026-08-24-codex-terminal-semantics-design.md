# Codex 代理终止语义设计

## 背景与根因

CCSwitchMulti 同时承接 OpenAI Responses、OpenAI Chat Completions 以及若干兼容实现。Codex 客户端只会在收到合法工具 item 时进入下一轮；如果代理发送 `response.completed`，但输出中没有可执行工具 item，当前 turn 会直接结束。

现有 Chat 适配在 2026-06-02 初次引入时把 SSE 的 `[DONE]` 直接交给 `ChatToResponsesState::finalize()`，并把除 `finish_reason=length` 外的所有情况映射成 `completed`。2026-06-11 为兼容“有 `[DONE]`、无 `finish_reason`”的非规范网关，假流式聚合又明确接受这一形状。该兼容意图可以理解，但协议边界错误：`[DONE]` 只能证明 SSE 传输关闭，不能证明模型以何种语义结束。

原生 Responses 包装器还有另一条缺口：已经向 Codex 转发正文后，如果上游干净 EOF 且没有任何终止事件，代理只关闭字节流；终态集合还遗漏 `response.incomplete`，也没有核对事件名、`response.status` 与最终输出是否一致。

因此故障不专属于 DeepSeek、Qwen 或 vLLM。第三方上游只要漏发、错发或误标终态，都可能被 CCSwitchMulti 合成为成功，从而让 Codex 静默结束。

## 目标

- 只有协议上完整的最终回答或完整工具调用可以成为 `response.completed`。
- token 上限和内容过滤截断成为 `response.incomplete`。
- 缺失、未知、互相矛盾或没有最终产出的终态明确失败。
- 尚未向下游发送实质输出的临时流故障可以自动重连。
- 已向下游发送实质输出后绝不重放请求，避免重复执行工具；改为发送明确流错误。
- Chat 非流、Chat 流和 Chat 假流式聚合共享同一套终止判据。

## 非目标

- 不根据“我继续处理”“正在检查”等自然语言猜测模型是否还想继续。
- 不在代理层自动补一条用户消息或伪造工具调用。
- 不把推理强度配置、Sub-Agent V1/V2 或模型能力探测并入本修复。
- 不安装、重启、替换当前运行中的 CCSwitchMulti，也不改 live Provider 配置。

## 终止语义分类器

新增一个纯函数模块，接收上游终止信号和已经解析出的输出证据，返回以下三类结果：

- `Completed`：可以生成或保留 `response.completed`。
- `Incomplete(reason)`：生成或保留 `response.incomplete`。
- `Failed(code, message)`：非流返回转换错误；流式生成明确失败/错误事件。

分类器只使用结构化协议字段，不读取输出文本的语义。

### Chat Completions

| 上游终止 | 输出证据 | 下游结果 |
| --- | --- | --- |
| `length` | 任意 | `response.incomplete`, `max_output_tokens` |
| `content_filter` | 任意 | `response.incomplete`, `content_filter` |
| `tool_calls` / `function_call` | 至少一个完整工具调用 | `response.completed`，由 Codex 工具循环继续 |
| `tool_calls` / `function_call` | 没有完整工具调用 | `response.failed` / 转换错误 |
| `stop` | 非空最终消息、refusal 或完整工具调用 | `response.completed` |
| `stop` | 只有 reasoning 或完全空输出 | `response.failed` / 转换错误 |
| 缺失 | 任意 | `response.failed` / 转换错误 |
| 未知值 | 任意 | `response.failed` / 转换错误 |

完整工具调用至少需要非空名称、call id 和参数字符串。已有的兼容性合成 call id 可以保留，但 finish reason 声称有工具、最终却没有任何可用工具时必须失败。

`[DONE]` 只结束 SSE 传输。看到它时不调用 `finalize()`，也不填充 `finish_reason`。流自然结束后仍按上表分类。

### 原生 Responses

- `response.completed` 只有在 `response.status=completed`，且已经出现非空最终 message/refusal 或完整客户端工具调用时才能原样转发。
- `response.incomplete` 是合法终态，原样转发并停止读取。
- `response.failed` 和 `error` 是合法失败终态，原样转发并停止读取。
- 取消类终态作为失败终态处理，不得继续等待或重试。
- `response.completed` 携带 `failed`/`incomplete` 状态，或只有 reasoning、没有最终消息/工具调用时，不转发该伪成功事件，改发协议错误。
- 第一个终态后立即停止读取，后续冲突事件不再泄漏给 Codex。
- 已转发 semantic output 后 EOF 但无终态时发送明确 `error` 事件；未转发 semantic output 时沿用有界重连。

## 数据流

1. Chat 非流转换器解析 message、reasoning 和工具调用。
2. Chat 流状态机解析增量并在真正结束时关闭 output item。
3. 两者把相同的 `finish_reason + output evidence` 交给共享分类器。
4. 假流式聚合只负责还原 Chat 响应；缺失 `finish_reason` 直接返回转换错误，`[DONE]` 不再作为替代证据。
5. 原生 Responses 包装器在完整 SSE block 层观察输出证据和终态，决定透传、重连或发出错误。
6. Codex 收到合法 tool item 时自然进入下一轮；收到 incomplete/failed/error 时显示可诊断失败；只有完整最终回答才正常结束。

## 重试边界

- `response.created`、注释 keepalive 和空脚手架不算 semantic output，可安全重连。
- reasoning/text/tool/output item 等任何实质事件一旦发给客户端，不再重连。
- 协议结构错误不是瞬态网络错误，不自动重试；立即报告。
- 非流请求在下游尚未收到响应体，转换错误可以交给现有请求级故障转移策略处理。

## 与协议选择及 Sub-Agent V2 的关系

Provider 使用 Responses 还是 Chat 决定进入哪一个适配器，但不改变“完成必须有完整终态和最终产出”的原则。DeepSeek/GLM/Qwen/vLLM 的差异应由协议探测选择正确 dialect，再由对应适配器执行同一终止契约。

Sub-Agent V2 不是根因。它同样消费 Codex Responses 事件，所以错误的 `response.completed` 会影响主任务和子任务；修复应留在代理协议边界，而不是在 V2 调度层补“继续”。

## 测试与验收

回归至少覆盖：

- Chat SSE `[DONE]` 无 `finish_reason`。
- Chat 非流缺失、未知、`content_filter`、空 `tool_calls`、reasoning-only `stop`、空 message `stop`。
- 合法 final text、refusal、工具调用、`length` 不回归。
- 假流式 `[DONE]` 无 `finish_reason` 被拒绝。
- 原生 Responses semantic output 后无终态 EOF。
- `response.completed` 与 `status=failed/incomplete` 冲突。
- `response.completed` 只有 reasoning。
- `response.incomplete` 被识别为终态且不重连。

每个生产改动必须先有能在旧实现上按预期失败的回归。最终运行定向 Rust 测试、相关模块测试、格式、diff 和 UTF-8 无 BOM 检查。

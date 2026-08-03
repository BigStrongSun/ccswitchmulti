# Codex 跨 Provider Responses 兼容性审计（2026-08-03）

## 结论

任务 `019fc68a-e5fa-7961-a8d4-d2ffaf0ff8bb` 不是 DeepSeek 专属故障。真正边界是：
任意 Chat Completions、Anthropic Messages 或第三方原生 Responses 上游生成的历史，
后来被回放给另一个更严格的 Responses 上游。DeepSeek 在现场同时触发了 Chat 转换和
data-only SSE，但换成其他具有相同行为的 provider 仍会复现。

现场两个错误是独立根因：

1. HTTP 400 来自跨 provider 历史 item ID 命名空间不兼容。
2. HTTP 422 来自 CCSM 把上游成功返回的压缩 SSE 聚合成了空摘要。

## 证据链

- 本机 `~/.cc-switch/logs/codex-router.log` 记录了 official route 对
  `resp_chatcmpl-..._msg` 的 `msg` 前缀拒绝，以及对 `call_...` web search item 的
  `ws` 前缀拒绝。
- OpenAI Responses 参考资料要求手动管理上下文时回放 response output items；示例中
  message、reasoning、function/custom call、web/file search、image generation 分别使用
  独立 item identity，而 tool output 通过 `call_id` 配对。
- OpenAI Codex 当前源码的 `ResponseItem` 和 `attach_item_ids` 证明 Codex 会把 message、
  reasoning、function call、custom tool call、web search、tool search、local shell 的 ID
  放回后续 Responses input；output 类型则以 `call_id` 连接对应调用。
- 独立 Matrix WebSearch 链已执行，但其索引对 OpenAI API/Codex 精确源码查询没有返回
  可用结果；因此没有把该链的低信号结果当成协议事实。关键结论由官方文档、官方源码
  和本机实际 400 日志交叉确认。

## Item ID 审计矩阵

| Responses item | 官方/当前惯例 | CCSM 合成 | official-boundary 处理 | 判断 |
| --- | --- | --- | --- | --- |
| `message` | `msg_` | 已统一生成 `msg_` | 非规范 ID 确定性映射为 `msg_ccswitch_<sha>` | 已覆盖，现场验证 |
| `reasoning` | `rs_` | Chat/Anthropic 流已生成 `rs_` | 仅无非空 `encrypted_content` 时映射；加密项原样保留 | 已覆盖安全子集 |
| `function_call` | `fc_`，另有 `call_id` | 已生成 `fc_` | 非规范 item ID 映射为 `fc_ccswitch_<sha>`，不改 `call_id` | 已覆盖 |
| `function_call_output` | 无需调用 item ID；用 `call_id` | 保持 `call_id` | 不新增/改写 `id` | 已覆盖结构边界 |
| `custom_tool_call` | `ctc_`，另有 `call_id` | 已生成 `ctc_` | 非规范 item ID 映射为 `ctc_ccswitch_<sha>` | 已覆盖 |
| `custom_tool_call_output` | 用 `call_id` | 保持 `call_id` | 不新增/改写 `id` | 已覆盖结构边界 |
| `web_search_call` | `ws_` | 原生 item 可来自任意上游 | 非规范 ID 映射为 `ws_ccswitch_<sha>` | 已覆盖，现场验证 |
| `tool_search_call` | Codex 可回放 ID，但公开前缀证据不足 | CCSM 转换项当前只生成 `call_id` | 不猜测前缀，不改 | 有意保守 |
| `local_shell_call` | Codex 可回放 ID，但公开前缀证据不足 | CCSM 不合成此类原生 item | 不猜测前缀，不改 | 有意保守 |
| `file_search_call` / `image_generation_call` | API 示例分别常见 `fs_` / `ig_` | 当前 Codex replay 枚举不按普通 input item 挂回这两类 ID | 不扩大当前边界 | 无现场缺口 |
| `compaction` / `context_compaction` | 使用 `encrypted_content`，不是普通 ID 命名空间问题 | CCSM 使用 `ocx1:` envelope | 不走 ID 规范化 | 已分离处理 |

“确定性映射”只发生在目标是内建 OpenAI official route 时，不写回会话存储，也不改变
发往第三方 provider 的请求。这样重试得到相同 ID，缓存前缀稳定，同时不会把 OpenAI
特有约束错误施加给兼容实现。

## 加密 Reasoning 边界

不能把所有 `reasoning.id` 一律改成 `rs_`。带非空 `encrypted_content` 的 reasoning
可能由原 provider 加密并绑定其 item identity；只改 ID 可能从“前缀错误”变成内容验证
或配对错误。当前实现只重写没有非空加密内容的普通 reasoning。跨 provider 携带加密
reasoning 仍属于受限场景，应由来源 provider 继续处理、使用可移植摘要，或由未来明确
的解密/重封装协议解决，不能静默伪造。

## 压缩链审计

| 路径 | 请求 | 返回 | CCSM 处理 |
| --- | --- | --- | --- |
| OpenAI official remote compact | 原生 compact/Responses v2 | 原生唯一 compaction item | 透传官方语义 |
| 显式支持 remote compact 的第三方 Responses | 原生 Responses | 普通 message/reasoning + 文本 | 聚合后包装唯一 `ocx1:` compaction item |
| Chat Completions provider | compact context 转 unary Chat | 非流或 Chat SSE 文本 | 转 Responses message，再包装 compaction |
| Anthropic Messages provider | compact context 转 Messages | 非流或 Messages SSE 文本 | 转 Responses message，再包装 compaction |
| 默认第三方 provider | Codex 本地压缩 | 不调用私有 remote compact | 保持默认策略 |

本次 422 的直接缺口已覆盖两种并不限于 DeepSeek 的 SSE 变体：

- 没有 `event:` 行、只在 JSON `data.type` 标注事件类型；
- reasoning item 已完成，但摘要文本只通过 `response.output_text.delta` 返回。

聚合器现在在缺 `event:` 时读取 `data.type`，且只要尚无 message item，就把已收集的文本
delta 合成 message，即使 output 中已有 reasoning。随后压缩转换器从 message 文本生成
唯一 compaction item。空文本仍明确报错，避免用空摘要污染历史。

## 不做的事情

- 不要求 MultiRouter 的 `name` 必须永远等于 `OpenAI`。该字段决定 Codex 的官方能力
  分支，不是路由正确性的证明；mixed/第三方默认应继续本地压缩，remote compact 显式
  opt-in。
- 不把 `compaction_trigger` 粗暴改成普通“请总结”提示词。那会改变压缩策略和历史语义。
- 不批量重写持久化会话；兼容修复位于目标 provider 的发送边界。
- 不猜测 `tool_search_call`、`local_shell_call` 等尚无充分前缀证据的命名空间。
- 不改写带加密内容的 reasoning ID。

## 验证范围

- 回归覆盖 `msg_`、`rs_`（无加密内容）、`fc_`、`ctc_`、`ws_`，以及加密 reasoning
  保持不变。
- 已有压缩回归覆盖 data-only SSE、reasoning + text delta、Chat/Anthropic 转换、唯一
  compaction output 和后续 summary 恢复。
- 本次只修改源码、测试和审计文档；没有 build、安装或替换本机正在运行的 CCSM。

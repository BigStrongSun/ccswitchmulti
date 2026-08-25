# 2026-08-25 Codex Desktop raw reasoning 渲染缺口

## 现场结论

- 当前安装的 Codex Desktop 为 `26.818.5345.0`。配置已启用
  `show_raw_agent_reasoning = true` 且未隐藏 reasoning。
- Qwen MultiRouter 运行态档案恢复后，CCSwitchMulti 已把 Chat Completions 的
  `reasoning` 正确投影成 Responses `reasoning_text`。任务 rollout 中 reasoning item
  具有非空 `content`，但 `summary` 为空；本轮共记录 10 个 raw reasoning item、约
  322,964 个字符。
- 安装包前端会接收 `item/reasoning/textDelta`，store 也把 delta 追加到
  `reasoning.content`。但是主会话投影和可见性过滤只读取 `reasoning.summary`，不读取
  `reasoning.content`。因此 raw-only reasoning 在生成中最多显示“正在思考”占位，完成后
  可能消失。
- 这推翻了 `memory-2026-08-23-third-party-raw-reasoning-ui-fix.md` 中“只要 CCSM 发出
  `reasoning_text`，Codex Desktop 就会显示原始推理”的验收假设。该次 CCSM 修复仍然保证
  transport 与历史回放语义正确，但不能单独修复 Desktop 的投影缺口。

## 正确修复边界

- 正确修复属于 Codex Desktop：当 raw reasoning 配置允许显示时，会话投影和可见性判断
  应读取 `reasoning.content`，并保持 summary 与 raw content 的不同 UI 语义。
- CCSM 不应把 raw reasoning 伪装成 `summary_text`，也不应复制到最终回答。这会篡改协议
  语义，并可能把本应独立保存的模型推理当作摘要或用户可见答案。
- CCSM 现有 reasoning-only 终态校验必须保留：`finish_reason=stop` 但没有最终 content 或
  工具调用时，应返回 `upstream_final_output_missing`，不能把 reasoning 当成成功答案。

## Qwen 长思考的独立问题

- 12:31 左右的异常请求使用 `qwen3.8 / xhigh`，输入 130,407 tokens，未显式设置
  `max_tokens`、`max_completion_tokens` 或 `max_output_tokens`。
- vLLM 生成 69,341 completion tokens、263,767 reasoning 字符，最终
  `finish_reason=stop`，但 content 与工具调用均为空；请求没有自定义 stop，客户端也没有
  断开。模型在仍处于 reasoning 状态时生成 EOS，随后 CCSM 正确发出 failed 终态。
- 服务器的 Qwen3.8 chat template 与 vLLM `qwen3` parser 都使用 `<think>` / `</think>`，
  因此没有证据支持“Qwen3.8 标记与 parser 不兼容”。
- vLLM 官方文档提供独立的 `thinking_token_budget`，达到预算后强制产生 reasoning end；当前
  安装的 vLLM 代码也包含该能力。是否给 provider 增加可配置 reasoning budget，应作为单独
  产品设计处理，不能在 CCSM 中按 Qwen 模型名写死或静默限制所有用户。

## 搜索与证据

- OpenAI 官方配置参考确认 `show_raw_agent_reasoning` 的语义是当模型发出 raw reasoning 时
  显示它；当前 Desktop 行为与该配置语义不一致。
- Codex 公开 issues 中最接近的是 `#27197`、`#38120`、`#38160`，但没有检索到准确描述
  “store 保存 `reasoning.content`，主会话只投影 `summary`”的报告。
- vLLM 官方 reasoning outputs 文档确认 Qwen3 使用 `qwen3` parser，并说明未配置
  `thinking_token_budget` 时不会对 reasoning 施加独立上限。内置 Web Search 与 Matrix
  WebSearch 两条独立链结论一致。

## 操作边界

- 本次调查未重启 CCSwitchMulti、Codex Desktop 或 vLLM，也没有向活动 Qwen 任务追加测试
  请求。
- 证据只记录长度、状态、token 数与字段形状，不记录或公开模型的具体推理文本。

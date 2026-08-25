# 2026-08-25 Qwen3.8 reasoning-only 与 Codex raw reasoning 投影

## 结论更正

- 先前把用户看到的现象主要归因于 Codex Desktop 不渲染 raw reasoning，结论不完整。
  DeepSeek V4 在同一 Desktop 中可正常显示，证明必须同时比较 Provider 请求参数和 CCSM
  的 reasoning 投影类型。
- 本次现场实际包含两个独立问题：Qwen Provider 的过期采样覆盖会提高 reasoning-only 长生成
  的概率；CCSM 自动探测又把 Qwen 的可读 `reasoning` 投影为 raw `reasoning_text`，而当前
  Desktop 主会话只投影 `summary`。前者影响模型是否正常结束，后者影响已经收到的推理是否
  可见。任何单一归因都不能解释完整现象。

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

## Qwen Provider 采样根因与现场修正

- 现场 `Qwen` Provider 的 `meta.localProxyRequestOverrides.body` 从至少 2026-08-16 的备份起
  一直保存 `temperature=0.1`。MultiRouter 按设计继承 Provider 最终请求参数，因此 CCSM
  发往 Qwen Chat Completions 的异常请求原始值就是 `temperature=0.1`；roglinux 透明代理
  只为缺失字段补了 `top_p=1.0`，不能把 `0.1` 归因于代理。
- Qwen3.8 官方 thinking 推荐是 `temperature=1.0`、`top_p=0.95`、`top_k=20`、
  `min_p=0`、`presence_penalty=0`、`repetition_penalty=1`。现场 2026-08-25 已通过 CCSM
  Provider 编辑/保存通道把 Qwen 的本地代理 Body 覆盖改为这组参数；SQLite 回读一致，
  CCSM、Codex 和 vLLM 均未重启，也没有额外发送模型测试请求。
- 这暴露的是模型切换后的配置生命周期缺口：模型目录、能力和推理档位已经从 Qwen3.6
  更新为 Qwen3.8，但用户维护的请求覆盖仍保留旧采样值。不能静默覆盖用户显式配置；后续
  产品应在模型标识或预设发生变化时，把与新模型预设冲突的 Body 覆盖作为可见 drift，要求
  用户确认保留、应用新预设或清除覆盖。

## vLLM 边界确认

- 运行中的 vLLM 为 `0.27.2rc1.dev91+g1f7427bc0`，启动参数使用官方 Qwen3.8 推荐的
  `--reasoning-parser qwen3` 与 `--tool-call-parser qwen3_coder`。
- 模型 chat template 在 generation prompt 中写入 `<think>`；vLLM parser 初始状态为
  `REASONING`，遇到 `</think>` 才进入 `CONTENT`，遇到 `<tool_call>` 可隐式结束 reasoning。
  异常轮没有生成这两个终止标记而直接 EOS，所以 vLLM 输出 reasoning-only 是对实际 token
  流的忠实分类，不是 parser 名称或模板标记配置错误。
- 代理日志显示这种长 reasoning-only 不是单次偶发：2026-08-15 至 2026-08-25 已出现多次
  `content_chars=0`，终态既有 `stop` 也有 `length`。这支持“旧采样覆盖加长上下文触发模型
  终止不稳定”，但在新采样参数下尚未做独立负载复现，因此不能宣称概率问题已经验收消失。

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

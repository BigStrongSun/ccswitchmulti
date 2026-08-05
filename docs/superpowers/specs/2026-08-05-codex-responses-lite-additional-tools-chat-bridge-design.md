# Codex Responses Lite `additional_tools` 转 Chat 根修设计

## 背景与现场证据

session `019fbd59-0c1a-7592-87d3-1e2ad654fd0d` 在 2026-08-05 10:40:18 切到 Qwen `qwen3.6` 后触发 `pre_turn compaction`。CCSwitchMulti 把 Codex `/responses` 请求转换到 Qwen `/chat/completions`，366,973 字节请求连续六次收到 HTTP 400；上游指出 `messages[1]` 为 `role=system, content=null`。

Codex `0.146.0-alpha.3.1` 的 Responses Lite 请求会在 `input` 前缀插入结构项：

```json
{
  "type": "additional_tools",
  "role": "developer",
  "tools": []
}
```

该项不是消息，协议上没有 `content`。Codex 随后另行插入含文本的 developer 基础指令。`additional_tools` 是请求时动态项，按 Codex rollout policy 不持久化，因此不能从会话 JSONL 中直接检索到。

当前 `transform_codex_chat.rs` 不识别 `additional_tools`。未知项兜底分支只要看到 `role` 就按消息转换；developer 又映射为 Chat system，缺失的 `content` 被 `unwrap_or(Value::Null)` 补成 null。system 合并器会把后续正常 developer 指令提到首位，却把 null system 留在 `rest`，最终构造出上游拒绝的 `messages[1]`。

## 目标

- 把 Responses Lite `additional_tools.tools` 完整投影到 Chat Completions 顶层 `tools`。
- `additional_tools` 只作为结构元数据消费，不生成 Chat `messages` 项。
- top-level `tools`、`additional_tools` 和 `tool_search_output.tools` 共用既有 `CodexToolContext` 去重、命名空间和 custom tool 转换规则。
- 阻止 system/developer/user 等非 assistant Chat 消息携带 `content:null` 离开转换边界。
- 保留 assistant tool-call 消息允许 `content:null` 的既有语义。
- 不修改会话 JSONL、SQLite、Qwen 配置或 Codex 客户端历史。

## 方案比较

### 方案 A：识别结构项并增加角色感知防线（采用）

在工具上下文构造阶段递归收集 `input` 内的 `additional_tools.tools`，复用 `add_response_tool`。在 input item 分派阶段显式消费 `additional_tools`，不进入消息兜底。最后对真实 message 的 missing/null content 按角色处理：assistant 保持 null；非 assistant 转成合法空字符串，空 system 由现有合并逻辑丢弃。

该方案既保留工具，又修复非法 Chat schema，是最小的根因修复和必要的边界防御。

### 方案 B：只把 null 改为空字符串（不采用）

可以消除本次 400，但会静默丢失 `additional_tools.tools`，导致工具不可用或模型错误声称无法调用工具。这只是隐藏症状。

### 方案 C：最终序列化前全局删除 null 消息（不采用）

会误伤合法的 assistant tool-call `content:null`，也无法恢复被遗漏的工具定义。

## 转换契约

### 工具收集

`build_codex_tool_context_from_request` 按请求顺序处理：

1. 顶层 `body.tools`；
2. `body.input` 中所有 `type=additional_tools` 的 `tools`；
3. 现有 `tool_search_output.tools`。

每个工具继续走 `CodexToolContext::add_response_tool`，继承已有函数、自定义工具、namespace、hosted web search、image generation、名称展平和去重行为。重复定义不能在 Chat 顶层产生重复工具。

### 消息投影

- `additional_tools`：不生成消息，不改变 pending reasoning、pending tool call 或 media 顺序。
- system/developer message 的 missing/null content：转换成空字符串，随后由 `collapse_system_messages_to_head` 作为空 system 消费掉。
- user/tool message 的 missing/null content：输出合法字符串；tool output 已有独立转换路径。
- assistant message：继续允许 null；synthetic assistant tool-call 行为不变。

### 错误与诊断

本次不增加 Provider 特判或重试。转换器输出必须在首次网络发送前满足 Chat 消息 schema；如果未来出现无法表达的结构项，应在转换层返回明确错误，而不是把非法 JSON 交给上游。

## 测试设计

TDD RED 必须先证明当前实现会：

1. 把 Responses Lite `additional_tools` 错生成为 `system/content:null`；
2. 丢失仅存在于 `additional_tools.tools` 的函数工具。

GREEN 后覆盖：

- Responses Lite 请求只生成一个正常 system 指令消息，且 Chat 顶层保留函数工具；
- additional tools 中的 custom 和 namespace 工具继续走已有转换规则；
- top-level 与 additional tools 重复定义不重复输出；
- 显式 null 和缺失 content 的 developer/system 不产生 null Chat 消息；
- user missing/null content 为合法字符串；
- assistant tool-call `content:null` 保持；
- 现有 system 合并、tool search、hosted tool 和普通 Responses→Chat 测试继续通过。

## 验收与发布

- 定向回归测试完成明确 RED→GREEN。
- `transform_codex_chat` 模块测试、`cargo check --lib`、`cargo fmt --check` 和 `git diff --check` 通过。
- 在资源允许时运行 Rust 全量库测试；若存在既有失败，必须逐项给出与本改动无关的证据，不能概括为通过。
- 项目 `memory.md` 记录根因、协议契约、测试证据和提交。
- 本地提交信息说明现场、根因、实现和验证，结尾包含 `本次提交由BigStrongsSun完成`。
- fork 分支推送后，向 `farion1231/cc-switch` 创建一个去敏 Issue，附最小 Responses Lite 输入、错误 Chat 输出、影响版本和根因。
- 向上游创建独立修复分支和 ready-for-review PR；PR 只包含本根修需要的源码、测试和必要说明，不夹带 BigStrongSun fork 的其它功能提交。PR 关联 Issue，并列出 RED→GREEN 和完整验证结果。

## 非目标

- 不修改 Codex Responses Lite 协议或关闭 `use_responses_lite`。
- 不针对 Qwen/vLLM 放宽服务端校验。
- 不迁移或重写现有 session 历史。
- 不借本次修复重构整个 Responses→Chat 转换文件。

# Codex Hosted Tool Bridge Design

## 背景

Codex 官方 provider 在调用 OpenAI Responses API 时，可以把 OpenAI 托管工具放进 `tools` 数组，由 OpenAI 服务端执行。抓包确认，`web_search = "live"` 场景下，Codex 发往 `/v1/responses` 的请求会包含：

```json
{
  "type": "web_search",
  "external_web_access": true,
  "search_content_types": ["text", "image"]
}
```

同一请求的关键形态：

```json
{
  "model": "gpt-5.5",
  "tool_choice": "auto",
  "stream": true,
  "store": false,
  "parallel_tool_calls": true,
  "tools": [
    { "type": "tool_search", "execution": "client" },
    {
      "type": "web_search",
      "external_web_access": true,
      "search_content_types": ["text", "image"]
    }
  ]
}
```

请求体默认可能带 `content-encoding: zstd`，并且 Codex 可能先尝试 WebSocket `/v1/responses`，失败后回退到 HTTP SSE。CCSwitchMulti 如果要在本地代理层 inspect 或改写请求，需要先处理这两个传输细节。

第三方 OpenAI-compatible provider 通常不能执行 OpenAI hosted tool。也就是说，不能把 `{"type":"web_search"}` 原样发给 DeepSeek、Qwen、GLM 等上游。正确思路是让 CCSwitchMulti 充当工具执行器：

1. Codex 请求进入 CCSwitchMulti。
2. CCSwitchMulti 把 hosted tool 替换成第三方模型能理解的普通 function tool。
3. 第三方模型发 function call。
4. CCSwitchMulti 调 OpenAI Responses API 执行真正 hosted tool。
5. CCSwitchMulti 把结果规整成 tool output 回填给第三方模型。
6. 第三方模型生成最终回答。
7. CCSwitchMulti 把最终回答转换成 Codex 期望的 Responses SSE/JSON 返回。

## Web Search 桥接方案

### 入站识别

在 Codex `/v1/responses` 入站请求中识别：

```json
{ "type": "web_search" }
```

保存原始配置，例如：

```json
{
  "external_web_access": true,
  "search_content_types": ["text", "image"]
}
```

然后从发给第三方模型的 `tools` 中移除 hosted tool，并注入普通 function：

```json
{
  "type": "function",
  "name": "web_search",
  "description": "Search the web and return concise source-backed results.",
  "parameters": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "The web search query."
      },
      "count": {
        "type": "integer",
        "description": "Maximum number of search results to use."
      }
    },
    "required": ["query"],
    "additionalProperties": false
  }
}
```

### 工具执行

当第三方模型返回 `function_call` / `tool_call`：

```json
{
  "name": "web_search",
  "arguments": "{\"query\":\"OpenAI Codex web_search config values\",\"count\":5}"
}
```

CCSwitchMulti 内部调用 OpenAI Responses：

```json
{
  "model": "<configured-openai-hosted-tool-model>",
  "input": "Search the web for: OpenAI Codex web_search config values. Return concise source-backed results.",
  "tools": [
    {
      "type": "web_search",
      "search_content_types": ["text"]
    }
  ],
  "tool_choice": "auto"
}
```

如果原始 Codex hosted tool 允许 image，则可传：

```json
{
  "type": "web_search",
  "search_content_types": ["text", "image"]
}
```

### 工具输出

不要把 OpenAI 原始 response 全量塞给第三方模型。建议规整为稳定 JSON：

```json
{
  "query": "OpenAI Codex web_search config values",
  "summary": "Codex web_search supports cached, live, and disabled modes.",
  "sources": [
    {
      "title": "OpenAI Codex Config Basics",
      "url": "https://developers.openai.com/codex/config-basic",
      "snippet": "web_search can be cached, live, or disabled."
    }
  ],
  "raw_text": "Short synthesized result from OpenAI Responses web_search."
}
```

对 Chat Completions 上游，回填为：

```json
{
  "role": "tool",
  "tool_call_id": "call_xxx",
  "content": "{\"query\":\"...\",\"summary\":\"...\",\"sources\":[...]}"
}
```

对 Responses-shaped 上游，回填为：

```json
{
  "type": "function_call_output",
  "call_id": "call_xxx",
  "output": "{\"query\":\"...\",\"summary\":\"...\",\"sources\":[...]}"
}
```

### 执行循环

MVP 可以实现非流式内部循环：

```text
request_from_codex
  -> normalize hosted tools to local function tools
  -> call third_party_model
  -> if function_call(web_search):
       call_openai_responses_web_search
       append tool output
       call third_party_model again
  -> return final assistant message as Responses-compatible SSE
```

建议设置：

- `max_tool_iterations = 3`
- `web_search_timeout_ms = 30000`
- `max_results = 5`
- 每轮只允许执行白名单工具名：`web_search`
- search 结果进入日志时必须脱敏或摘要化，避免把完整网页内容写入普通日志

## 扩展到其他 OpenAI Hosted Tools

这套模式可以发散，但每类 hosted tool 的落地成本不同。判断标准是：能否把 hosted tool 包装成第三方模型可理解的 function，并把 OpenAI Responses 返回规整成普通 tool output。

### Image Generation

OpenAI Responses 支持 `tools: [{ "type": "image_generation" }]`。官方示例中，结果会出现在 `output` 里的 `image_generation_call`，图片数据在 `result` 字段，通常是 base64。也支持 `partial_images` 流式返回局部图像。

可以桥接。

第三方模型暴露普通 function：

```json
{
  "type": "function",
  "name": "generate_image",
  "description": "Generate an image using OpenAI hosted image generation.",
  "parameters": {
    "type": "object",
    "properties": {
      "prompt": { "type": "string" },
      "size": { "type": "string" },
      "quality": { "type": "string" },
      "format": { "type": "string", "enum": ["png", "jpeg", "webp"] }
    },
    "required": ["prompt"],
    "additionalProperties": false
  }
}
```

CCSwitchMulti 内部调用：

```json
{
  "model": "<configured-openai-hosted-tool-model>",
  "input": "Generate an image: <prompt>",
  "tools": [
    { "type": "image_generation" }
  ]
}
```

返回给第三方模型时，不建议直接塞超大 base64。推荐：

```json
{
  "prompt": "...",
  "mime_type": "image/png",
  "artifact_path": "C:/Users/.../generated/image.png",
  "preview_base64": "<small thumbnail only>",
  "openai_output_id": "ig_..."
}
```

如果最终要返回给 Codex UI，可再把 artifact 路径或 markdown 图片链接放进 assistant final。MVP 不做 partial image streaming，先完整生成后返回。

### File Search

OpenAI Responses 支持：

```json
{
  "type": "file_search",
  "vector_store_ids": ["<vector_store_id>"]
}
```

可以桥接，但前置条件更重：必须有 OpenAI vector store、文件上传、权限和生命周期管理。适合做成“OpenAI knowledge search”工具，不适合默认开启。

第三方 function 可以是：

```json
{
  "type": "function",
  "name": "openai_file_search",
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string" },
      "vector_store_id": { "type": "string" },
      "max_num_results": { "type": "integer" }
    },
    "required": ["query", "vector_store_id"],
    "additionalProperties": false
  }
}
```

风险点：

- vector store ID 不能让模型随便猜，应由用户配置或 provider preset 注入。
- OpenAI file search 默认不返回完整 search results；如果需要结果详情，要加 `include: ["file_search_call.results"]`。
- 文件内容可能包含隐私数据，日志必须只记录文件 ID 和摘要。

### Code Interpreter

理论上可桥接为 `run_python_analysis` / `openai_code_interpreter` 之类工具，但安全和成本边界更高。它可能产生文件、图表和中间状态，CCSwitchMulti 需要 artifact 管理、执行超时、文件下载、结果摘要和沙箱策略。建议晚于 web_search/image_generation。

### Computer Use

不建议按这个模式优先桥接。Computer Use 是交互式环境控制工具，涉及屏幕状态、动作确认、安全权限和长会话状态。它不像 web_search/image_generation 那样是一次函数调用即可规整。CCSwitchMulti 如果要做，应单独设计“远程浏览器/桌面控制会话”，而不是塞进普通 provider proxy 的 tool loop。

## 实现落点建议

### 后端模块

建议新增或拆分：

```text
src-tauri/src/proxy/providers/hosted_tools/
  mod.rs
  bridge.rs
  openai_client.rs
  web_search.rs
  image_generation.rs
  file_search.rs
```

职责：

- `bridge.rs`: 识别 hosted tools、替换为 function tools、驱动 tool loop。
- `openai_client.rs`: 管理 OpenAI API key、Responses 调用、zstd/SSE/JSON 解析。
- `web_search.rs`: OpenAI web_search 请求构造和结果规整。
- `image_generation.rs`: image_generation 请求构造、base64 解码、artifact 写入。
- `file_search.rs`: file_search 请求构造和结果规整。

### 配置建议

新增 provider 或全局配置：

```toml
[hosted_tools.openai]
enabled = true
env_key = "OPENAI_API_KEY"
model = "<openai-hosted-tool-model>"
allow = ["web_search", "image_generation"]
web_search_max_results = 5
timeout_ms = 30000
```

不要复用第三方 provider 的 API key。OpenAI hosted tool bridge 应该有独立 OpenAI credential。

### 安全边界

- 默认只允许 `web_search`，`image_generation` 需要显式开启。
- `file_search` 必须绑定允许的 `vector_store_ids`。
- `computer_use` 默认不支持。
- 每次 hosted tool 调用都要带 trace id。
- 日志记录 tool name、query hash、耗时、状态码，不记录完整网页正文、完整 prompt、base64 图片或 API key。
- 对 OpenAI 返回内容做长度裁剪后再回填第三方模型。

## 推荐分期

### Phase 1: Web Search MVP

- 识别 `type=web_search`
- 替换成 function `web_search`
- 内部调用 OpenAI Responses hosted `web_search`
- 非流式 tool loop
- 返回 Responses-compatible final SSE
- 覆盖 Chat 上游和 native Responses 上游各一条测试

### Phase 2: Streaming 和可观测性

- 支持中间状态事件
- 记录 request_shape，不记录敏感内容
- UI 展示 hosted tool bridge 是否启用、调用次数、失败原因

### Phase 3: Image Generation

- 暴露 function `generate_image`
- 调 OpenAI `image_generation`
- base64 解码为 artifact 文件
- tool output 返回 artifact path / MIME / 尺寸 / 缩略图

### Phase 4: File Search

- 支持配置 vector store
- 支持 include `file_search_call.results`
- 做权限和日志保护

## 结论

这套逻辑可以发散到官方生图等 hosted tools，但要区分两类：

- 适合桥接：`web_search`、`image_generation`、部分 `file_search`
- 不建议直接桥接：`computer_use`、复杂长会话工具、强交互工具

核心原则保持不变：第三方模型永远只看普通 function tool；CCSwitchMulti 负责把 function call 转成 OpenAI Responses hosted tool 调用，再把结果规整成第三方模型能继续推理的 tool output。

## 参考资料

- OpenAI Web Search tool: https://developers.openai.com/api/docs/guides/tools-web-search
- OpenAI Image Generation tool: https://developers.openai.com/api/docs/guides/tools-image-generation
- OpenAI File Search tool: https://developers.openai.com/api/docs/guides/tools-file-search
- OpenAI Computer Use tool: https://developers.openai.com/api/docs/guides/tools-computer-use

# 第三方模型 hosted web_search 真实链路审计（2026-08-21）

## 结论

当前 CCSwitchMulti 的第三方 hosted `web_search` 桥接不是全局失效：

- `deepseek-v4-pro` 通过 `router-98e3...` 进入第三方 Responses 路径，真实触发 `web_search`，收到 `response.web_search_call.in_progress/searching/completed`，随后返回最终文本 marker。
- `qwen3.8` 通过 `router-274c...` 正常进入第三方 Chat Completions 路径，HTTP 200，但本次真实请求没有产生任何工具调用事件，只有 reasoning 相关事件和 `response.completed`。

因此，当前证据支持的判断是：

1. 路由、鉴权、Responses-to-Chat 转换和 hosted loop 至少在 DeepSeek 路径已经闭环。
2. Qwen 本次失败发生在“上游模型是否实际发起 function call”这一层，不能据此判定 CCSM 没有把官方搜索能力桥接给第三方。
3. 在没有看到 Qwen 上游实际请求体、工具声明回显或工具调用 delta 之前，不应继续修改 hosted loop；否则可能破坏已经工作的 DeepSeek 路径。

## 复现方式

运行中的实例保持不变，地址为 `http://127.0.0.1:15721`。脚本只使用 `Bearer PROXY_MANAGED`，不读取或输出 provider 密钥：

```powershell
python -X utf8 scripts/verify_third_party_hosted_web_search.py
$env:CCSM_CANARY_MODEL = "deepseek-v4-pro"
python -X utf8 scripts/verify_third_party_hosted_web_search.py
```

请求使用：

- `stream=true`
- `tools=[{"type":"web_search"}]`
- `tool_choice={"type":"web_search"}`
- 指令要求模型只调用一次搜索后返回固定 marker

## 运行证据

### DeepSeek

- HTTP `200`
- `responses_to_chat=false`
- route：`router-98e3bdc6-710d-4236-b47b-7ce7e4884365`
- 上游：`https://api.deepseek.com/v1/responses`
- 事件包含：
  - `response.web_search_call.in_progress`
  - `response.web_search_call.searching`
  - `response.web_search_call.completed`
  - `response.output_text.delta`
  - `response.completed`
- 最终 marker：`CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`

### Qwen3.8

- HTTP `200`
- `responses_to_chat=true`
- route：`router-274cfc2c-e4eb-4572-ba6f-7fdcc0b6008c`
- 上游：`https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions`
- 事件只有 `response.created`、reasoning summary 和 `response.completed`
- 没有 `response.web_search_call.*`
- 没有最终 marker

## 尚未完成的验证

1. 增加不泄露正文的请求投影诊断，确认 Qwen 发出的 Chat 请求中确实包含 `web_search` function 定义及强制选择映射。
2. 使用同一 Qwen 上游做非流式和普通 function-call 对照，区分“模型不触发工具”与“流式转换丢失工具调用”。
3. 若 Qwen 仍不产生工具调用，再检查 Qwen/vLLM 对 `tool_choice` 的支持边界；这属于模型服务能力或上游兼容性问题，不能用 CCSM 的 hosted loop 修复替代。
4. 新构建安装后重新执行 DeepSeek/Qwen canary，确认源码修复与已安装运行态一致。

## 当前不确定性

当前日志对请求体采用脱敏摘要，不能仅凭日志证明 Qwen 上游收到的完整 `tools` 和 `tool_choice` 字段。需要新增字段级、无敏感内容的诊断或使用受控 mock 上游补齐这一证据。

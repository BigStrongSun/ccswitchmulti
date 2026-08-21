# 第三方模型 hosted web_search 真实链路审计（2026-08-21）

## 诊断能力补齐（当前工作树）

- 当请求显式 `tool_choice` 指向 CCSM 自有 hosted tool，且上游以成功状态返回但没有任何 hosted function call 时，router 现在追加脱敏事件 `hosted_tool_not_called`。
- 事件只记录 trace/session/model/provider/tool/status/reason/streaming，不记录 prompt、工具 schema、响应正文或凭据；普通 `tool_choice=auto` 不产生该告警。
- Codex MultiRouter Debug 检查新增“Hosted tool 调用”告警、最近未调用摘要和事件表字段，用户可直接区分“已投影但模型未调用”与“路由/认证/上游 HTTP 失败”。
- 该实现只负责边界诊断，不会根据用户 prompt 猜搜索词，也不会在代理层替上游模型伪造 function call。
- 定向验证：Rust hosted streaming 30/30、router diagnostics 13/13、explicit choice 1/1；全量 Rust `3251 passed / 0 failed / 5 ignored`，TypeScript、fmt 和 diff check 通过。

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

## 新构建安装态复核（2026-08-21）

- 提交 `789b91e0d1793fc1716b22e3b62c7035cc587fcd` 已完成新构建；产物元数据明确绑定该提交。
- 事务安装 `ccsm-20260821-130436-f717d7ce615445bb8dabbb79cc91bafe` 成功，旧 PID `24240` 替换为新 PID `12568`，错误与回滚错误均为空。
- 安装二进制哈希为 `EBAEC57F5A45C72BF550DF24049F91BABB1C650E48AEAD323BA334DB22266C13`，`/health` 返回 `200`。
- Qwen3.8 复测仍返回 HTTP `200` 且没有工具调用，但新日志明确记录：
  `responses_to_chat=true`、`hosted_tools=[web_search]`、`hosted_tool_choice=object(keys=[function,type])`。
  这证明 CCSM 已把 hosted `web_search` 投影为 Chat function tool 并发送给 Qwen；缺失发生在 Qwen/vLLM 没有产生 function call 的上游能力边界。
- DeepSeek V4 Pro 复测通过，事件包含 `response.web_search_call.in_progress/searching/completed`，最终 marker 为 `CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`。

因此，CCSwitchMulti 侧的 hosted 搜索桥接可以视为已完成运行态验收；Qwen 仍是上游工具调用兼容性问题，不能继续通过修改 CCSM loop 来“强行修复”。

## 当前不确定性

当前日志只保留字段级脱敏摘要，不记录工具描述、参数、prompt 或密钥；这足以证明 CCSM 的工具投影边界，但不能证明 Qwen 服务端内部是否启用了 tool calling。后者需要 Qwen/vLLM 侧的服务日志或独立上游 function-call 对照。

## 认证根因修复后的安装态复核（2026-08-21）

- 提交 `2c41f638` 修复 hosted tool 凭据来源：只有 native Codex 官方路由允许复用入站真实 Bearer；第三方路由拒绝 `PROXY_MANAGED`/第三方 API key 作为 ChatGPT OAuth，并回退 CCSM 托管 OAuth。
- 新构建 `CCSwitchMulti_3.19.2-9_x64-setup.exe` 的 SHA-256 为 `EF80037B1E5662C7DE9051F8067F588E59ADCF3ADD9F66202D2E7DD95B23DB33`；事务 `ccsm-20260821-135500-70ff5151640c4a70bbb62be77f60f5e9` 成功，新 PID `5952`，无回滚错误，`127.0.0.1:15721/health` 返回 `200`。
- Qwen canary 仍为 HTTP `200`、无 `response.web_search_call.*`、无最终 marker；其 CCSM 日志只显示第三方上游请求成功（`upstream_status=200`），未再出现 hosted tool 对 OpenAI 的 `401`。
- DeepSeek V4 Pro canary 通过，仍包含 `response.web_search_call.in_progress/searching/completed` 和最终 marker `CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`。

因此，认证修复已在安装态生效；当前剩余的 Qwen 失败不能再归因于 CCSM 误用 `PROXY_MANAGED`，而是 Qwen/vLLM 没有实际发起 function call。正式 release 仍需新版本号和 macOS/Linux 产物验收。

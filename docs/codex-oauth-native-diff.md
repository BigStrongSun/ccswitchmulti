# Codex OAuth 接管与原生请求差异调研

本文档记录 `Selected model is at capacity` 排查中的可复用取证办法。目标是先比较原生 Codex 和 CCSwitchMulti OAuth 接管路径的请求形态，再决定是否需要 TLS 抓包。

## 路径边界

- 纯原生：Codex 直接调用官方 ChatGPT Codex OAuth 后端。
- 当前接管：Codex 调用本地 `127.0.0.1:<port>/v1`，再由 CCSwitchMulti route 转到官方 `https://chatgpt.com/backend-api/codex/responses`。
- 第三方中转：Codex/MultiRouter route 命中非官方 OpenAI-compatible 上游。

## 已知源码差异点

- 本次对照官方 Codex `refs/remotes/live/main` 提交 `6219b7c40fc9c702c0aef9964e72b492558f60e4`。`ModelProviderInfo::is_openai()` 只按 `name == "OpenAI"` 判断；`model_provider` 的配置键不进入 Responses 请求身份字段。
- 自定义 `name = "OpenAI"` 能进入 OpenAI 专属请求构造分支，但不会继承内建 `openai` 的字段默认值。内建 provider 还显式提供当前 Codex `version`、`requires_openai_auth = true`、`supports_websockets = true` 和 `supports_standalone_web_search = true`；其中 Web Search 当前按 `is_openai() || uses_openai_actor_authorization() || supports_standalone_web_search` 判断，所以 `name = "OpenAI"` 时最后一个字段属于与内建配置对齐的冗余声明，不是单独的解锁开关。
- 原生 Codex `ResponsesApiRequest` 会发送 `store`、`stream`、`include`、`prompt_cache_key`、可选 `service_tier` 和 `client_metadata`。
- 原生 Codex 会过滤 `service_tier = "default"`，只有模型 catalog 支持且配置为非默认值时才发送。
- 原生 Codex 会把 `prompt_cache_key` 默认设为 thread id，并在 `client_metadata` 中写入 `x-codex-installation-id`。
- CCSwitchMulti Codex OAuth adapter 会把本地 `/v1/responses` 归一化到官方 `/backend-api/codex/responses`；forwarder 对可信本地 Codex 请求保留唯一且位于官方 first-party 白名单内的 `originator`，缺失、重复、未知值及 External API/协议转换请求回退为 `codex_cli_rs`。
- 旧实现提交 `af58740b` 曾成对硬编码 `originator = codex_cli_rs` 与 `version = 0.144.1`；提交 `cd8d6bc6` 把 originator 统一移到 forwarder 时漏迁 version，造成自定义 OpenAI 门面只有来源、没有版本。当前实现从可信 first-party Codex User-Agent 动态恢复真实构建版本，线程 originator 与进程 User-Agent 不同时也能恢复；UA 中的可信版本覆盖独立 `version` 头里的旧值，第三方 User-Agent 不伪造版本。
- `User-Agent`、host integration 生成的 `x-oai-attestation` 和 OpenAI 私有协商头沿本机 Codex 请求链透传；External API 的 attestation 与任意 version 会剥离。`x-cc-switch-*` 本地控制头在普通和 raw passthrough 出站前统一删除，CCSM 自己不生成 attestation。
- 当前官方 Codex `ContentItem` 原生覆盖 `input_text`、`input_image`、`input_audio`，`encrypted_function_args` 的类型是字符串数组；这些字段以及 `client_metadata`、`internal_chat_message_metadata_passthrough` 在官方 route 保持结构。当前 Codex 普通消息类型没有 `input_file`，不能把兼容 API 的文件项说成 Desktop 已实际发送；文件上传等未知 `/v1/*` endpoint 由 raw passthrough 保留原始字节、Content-Type 和官方认证。
- CCSwitchMulti 对 Responses-Lite 采用负缓存 fallback：只有上游明确返回不支持 Lite header 的错误时，才去头重试。

## 能做到与不能宣称的对齐

- 可以对齐：OpenAI 能力分支、Responses 语义、模型和工具字段、官方 Codex 当前发送的图片/音频内容、raw 文件 endpoint、User-Agent、first-party originator、真实 Codex version，以及本机 host 提供时的 attestation。
- 不能宣称完全不可区分：经过本地代理后，上游 TCP/TLS 连接、HTTP 客户端实现、header 序列化细节和源网络属于 CCSM；某个 Codex host 是否提供 attestation 也不是 CCSM 能保证的。
- 不应伪造：CCSM 版本不能冒充 Codex 版本，第三方客户端不能凭任意 User-Agent 或自报头获得 first-party version/attestation。

## 工具

### 采集当前机器状态

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/codex-oauth-diagnostics.ps1 -LogTailLines 1200 -SinceMinutes 60
```

输出位于 `scripts/logs/codex-oauth-diagnostics/<timestamp>`，默认包含：

- `config.sanitized.toml`：脱敏后的 Codex live config。
- `auth.metadata.json`：只含 token 长度/短哈希的 auth 元数据。
- `router-events.json`：解析后的 `codex-router.log` 事件。
- `router-events.capacity-or-error.json`：非 2xx 或疑似 capacity 事件。

### 比对请求形态

内置自检：

```powershell
node scripts/codex-request-shape-compare.mjs --self-test --out scripts/logs/codex-request-shape-compare/self-test
node scripts/codex-request-shape-compare.mjs --serve-self-test --out scripts/logs/codex-request-shape-compare/serve-self-test
```

使用已有捕获 JSON：

```powershell
node scripts/codex-request-shape-compare.mjs --native native-request.json --proxy proxy-request.json --out scripts/logs/codex-request-shape-compare/manual
```

启动 mock 并运行外部 harness：

```powershell
node scripts/codex-request-shape-compare.mjs --serve --native-command "<run native harness>" --proxy-command "<run proxy harness>"
```

mock 模式会提供这些环境变量给外部命令：

- `CODEX_COMPARE_BASE_URL`
- `CODEX_COMPARE_NATIVE_BASE_URL`
- `CODEX_COMPARE_PROXY_BASE_URL`

外部请求可用 header `x-codex-compare-side: native|proxy` 或 query `?side=native|proxy` 标记归属。未标记时，第一条请求按 native，第二条请求按 proxy。

## 判断顺序

1. 先用 `codex-oauth-diagnostics.ps1` 确认报错时 route 是否命中官方 OAuth、第三方 route 或本地模型。
2. 若命中官方 OAuth，检查同 trace 的 `service_tier`、request shape、Responses-Lite fallback 和上游 status。
3. 用 `codex-request-shape-compare.mjs` 比对原生与接管路径的 `service_tier`、`prompt_cache_key`、`client_metadata`、`originator`、account id 和 session/window id。
4. 只有字段级 diff 无法解释问题，或线上原生与接管行为仍稳定分叉时，再进入 Fiddler/mitmproxy 抓包。

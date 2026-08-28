# Codex 深度探测与生产请求等价性设计

## 目标

深度协议探测必须使用与真实 Codex 第三方请求相同的 Provider 请求策略。探测可以使用固定、无敏感信息的 prompt 和工具，但不得自行拼接 URL、认证、Provider header/body override、模型映射、推理参数或 Chat/Responses wire body。

本设计修复的根因不是某个模型特判缺失，而是生产 forwarder 与 `protocol_compatibility::runner` 各自拥有一套请求准备逻辑。两套逻辑会随 Provider 配置和生产转换器演进而漂移，现有 71 项测试只证明 probe runner 自洽，不能证明它与生产出站请求等价。

## 范围

本轮覆盖普通第三方 Codex Provider 的两种 OpenAI 兼容协议：

- 原生 Responses；
- Responses 转 Chat Completions。

以下内容不纳入主动探测：

- Codex OAuth、GitHub Copilot、xAI OAuth 等动态托管认证；
- OpenAI 官方 Provider；
- Responses 转 Anthropic/Messages；
- 用户真实会话 prompt、历史缓存和 hosted-tool 本地执行循环。

推理强度设置 UI 仍是独立逻辑。本轮只确保已解析的 Provider/model reasoning policy 在 probe 和 production 中经过同一个请求准备器。

## 根因

当前 `ProbeCandidate` 把 effective Provider 压缩成 base URL、Bearer token、model 和 transport。runner 随后通过 `build_probe_url` 与独立 `reqwest` 请求自行构造 wire request。这样会丢失或绕过：

- `CodexAdapter::extract_base_url`、`extract_auth`、`build_url`、`get_auth_headers`；
- `customUserAgent`；
- `localProxyRequestOverrides.headers/body`；
- model catalog/route 的 upstream model 映射；
- Chat reasoning resolver、thinking 参数与最小输出预算；
- native Responses effort 映射；
- canonical/private-field filtering；
- 生产 `codex_terminal` 对 completed/incomplete/failed/missing-output/tool-call 完整性的解释。

因此 probe 得到的成功或失败可能不代表真实 Codex 请求的行为，旧 profile 也可能在上述策略变化后继续命中。

## 架构

### 1. 共享 Provider 请求策略

新增 `proxy/providers/codex_request.rs`，定义：

```rust
pub(crate) const CODEX_REQUEST_PREPARER_VERSION: u32 = 2;

pub(crate) enum CodexRequestTransport {
    Responses,
    ChatCompletions,
}

pub(crate) struct CodexThirdPartyRequestPolicy { /* secret-bearing, redacted Debug */ }

pub(crate) struct PreparedCodexRequest {
    pub url: String,
    pub headers: http::HeaderMap,
    pub body: serde_json::Value,
}
```

`CodexThirdPartyRequestPolicy::compile(&Provider)` 是唯一的 Provider 请求策略编译入口。它保存完整 effective Provider 的必要语义和认证材料，不暴露或序列化 secrets，并提供稳定的 secret-safe policy fingerprint。

`prepare(...)` 接收逻辑 Responses body、目标 transport 和必要的无状态选项，输出最终 URL、Provider-controlled headers 与最终 JSON body。

### 2. 职责边界

共享 preparer 必须负责：

- 使用 `CodexAdapter` 解析 base URL、full URL 与认证策略；
- 生成协议 endpoint；
- 应用可见模型到 upstream model 的映射；
- 应用 Chat reasoning resolver 或 native Responses effort mapping；
- 使用生产 Chat 转换器；
- canonical/private-field filtering；
- Provider body override；
- 认证 header、custom User-Agent、Provider header override 与 protected-header 规则；
- 生成 policy fingerprint。

forwarder 继续负责：

- route/effective Provider 选择；
- 会话历史恢复和 prompt-cache session key；
- hosted-tool 本地执行策略；
- 入站 header 清理、动态托管 OAuth、传输、重试和日志；
- media prevention、Responses-Lite 与其他非普通第三方路径。

生产路径在完成会话级预处理后调用共享 preparer，并将返回的 Provider-controlled URL/headers/body 合并到最终请求。probe 使用固定逻辑请求直接调用同一 preparer。不得再保留第二套 URL、认证和 wire-body 构造。

### 3. Candidate 与指纹

`ProbeCandidate` 持有已编译的 `CodexThirdPartyRequestPolicy`，而不是裸 base URL/Bearer token。其 `Debug` 只显示：

- provider/route/model/transport；
- 脱敏后的 endpoint origin/path；
- authentication kind；
- 是否存在认证材料；
- policy fingerprint。

`PartialEq/Eq` 比较公开身份字段与 fingerprint，不比较或打印 secret-bearing Provider。

`ProbeTargetKey` 新增 `request_policy_fingerprint`。fingerprint 至少覆盖：

- request preparer version；
- endpoint/full-URL 语义；
- auth strategy 与凭据哈希；
- custom UA；
-有效的 header/body overrides；
- model/reasoning/cache 配置；
- probe 输出预算与固定语料版本。

任何一项变化都必须让旧 profile 失效。

### 4. 终态复用

runner 不再用 `[DONE]` 或事件名存在作为成功条件：

- Chat SSE 收集 finish reason、可见正文/refusal、compaction 和完整工具调用证据，调用 `classify_chat_terminal`；
- Responses SSE 对终态事件 payload 调用 `classify_native_responses_terminal`；
- `incomplete`、`failed`、缺失 status、缺失最终输出、半截工具调用、未知/缺失 finish reason 均不能标记阶段通过；
- 失败仍只输出结构化、脱敏的 `RedactedProbeFailure`。

### 5. 请求等价 contract

测试使用同一个完整 Provider fixture 和同一个逻辑 probe request，分别从 production-facing 入口与 probe-facing 入口准备请求，并逐项比较：

- URL；
- 非顺序敏感的 header 集合；
- JSON body；
- policy fingerprint。

fixture 必须同时包含 custom UA、合法和受保护 header override、嵌套 body override、model catalog 映射、Chat reasoning mapping 与 full/base URL 变体。预期值由手写 literal 断言，不能由被测 helper 自己生成。

## 错误与安全

- Provider 配置不完整或认证 header 无效时，candidate 编译失败，探测不发请求；
- managed/official Provider 明确拒绝进入本 preparer；
- `Debug`、序列化结果、日志、profile 和 progress event 不得出现 API key、Authorization 值、prompt、工具参数或响应正文；
- 请求准备失败映射为 `invalid_request`，网络和 HTTP 失败沿用现有脱敏分类；
- 任何 terminal protocol ambiguity 都 fail closed，不能把连接结束当成完成。

## 验收

1. 原有 71 项 protocol compatibility 测试保持通过；
2. 新增 contract 测试证明 production/probe 对同一输入产生相同 Provider-controlled request；
3. 新增 RED/GREEN 终态测试覆盖 Chat `[DONE]` 无 finish reason、Responses `response.completed` 缺 status/最终输出、failed/incomplete 和半截工具调用；
4. `protocol_compatibility`、`codex_terminal`、`forwarder`、provider 相关测试通过；
5. 完整 Rust library、`cargo check --tests --no-default-features`、rustfmt、UTF-8 strict decode 与 `git diff --check` 通过；
6. 项目 `memory.md` 记录新边界、测试证据和未覆盖范围。

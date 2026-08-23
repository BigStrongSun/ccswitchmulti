# Codex 推理兼容性主动探测：后端设计

- 状态：待实现评审稿；本文件只定义后端，不实现前端。
- 日期：2026-08-23。
- 范围：第三方 Chat Completions 上游经 CCSM 转成 Codex Responses 时，自动判定并安全投影 reasoning；同时为将来的高级模式提供手工覆盖接口。

## 1. 目标

当用户显式测试一个 `provider + route + effective upstream model + transport` 组合时，CCSM 必须以真实、无敏感数据的请求验证：端点、流式帧、可读推理字段、工具调用顺序，以及工具结果后的历史续接。测试成功后，CCSM 自动生成该组合的推理投影档案；运行时只读取该档案，不再根据字段名把所有文本硬编码为 raw 或 summary。

普通模式没有 raw/summary/字段名选择器。高级模式只调用同一套后端的手工覆盖接口，覆盖经过校验后优先于自动档案。

## 2. 非目标

- 不在本阶段新增设置页、向导、文案、Codex Desktop 配置写入或视觉验收。
- 不在每个用户请求中主动探测，也不因探测失败阻断基础模型连通性。
- 不解密、不展示、不跨供应商转发 `encrypted_content` 或 provider 签名块。
- 不把 prompt、推理文本、API key、Authorization、Cookie、工具参数或工具结果写入 SQLite、日志、审计记录或探测结果。
- 不重新设计 reasoning effort、Sub-Agent V2 加密消息清洗或官方 OAuth 续接；它们只作为回归边界。

## 3. 两条能力链必须分离

现有 `reasoning_capabilities` 是被动元数据发现：其结果可短期缓存，并用于 effort/thinking 参数。现有 `model_fetch` probe 只验证 `/responses` 或 `/chat/completions` 的 HTTP 成功。

新 `reasoning_probe` 是用户显式发起的主动兼容性测试。它消费真实响应形态，产出“响应投影”而不是 effort 能力；两者不可互相覆盖或复用同一 `outputFormat` 字段。

```text
被动 metadata discovery  -> effort / enable-thinking 请求参数
主动 compatibility probe  -> reasoning 输出语义、SSE 事件、历史回放方式
```

## 4. 领域模型与优先级

### 4.1 稳定目标键

`ProbeTargetKey` 包含：`provider_id`、可选 `route_id`、请求模型、有效上游模型、`transport`（`openai_chat` / `openai_responses`）、规范化 endpoint 指纹和认证绑定类别。endpoint 指纹只能是去掉 query、credentials 后的规范化 URL 的 SHA-256；不得存储 API key。

配置、路由映射、模型名、endpoint 或认证类别变化时，现有结果不再匹配，自动失效。

### 4.2 自动档案

```rust
enum ReasoningSemantic { Readable, Summary, Opaque, None }
enum ReasoningSource { ReasoningContent, Reasoning, ReasoningDetails, ThinkTags, NativeResponses, None }
enum HistoryReplay { ChatReasoningContent, Omit, NativeOnly }

struct VerifiedReasoningProfile {
    target: ProbeTargetKey,
    source: ReasoningSource,
    semantic: ReasoningSemantic,
    stream_verified: bool,
    non_stream_verified: bool,
    tool_turn_verified: bool,
    history_replay: HistoryReplay,
    probe_version: u16,
    evidence_fingerprint: String,
    tested_at: i64,
    expires_at: i64,
}
```

`semantic=Readable` 才允许 Responses `reasoning_text`；`Summary` 只能输出 summary SSE；`Opaque` 永远不产生可读文本；`None` 不创建 reasoning item。

### 4.3 手工覆盖

高级模式使用 `ManualReasoningOverride`，字段与 `VerifiedReasoningProfile` 的可配置子集相同，但必须携带目标键、预期 revision 和原因。校验规则：

1. `Opaque` 不能配置为 `Readable`；
2. `Summary` 不能要求 raw SSE；
3. `Readable` 必须指定非 `None` 的来源；
4. `NativeResponses` 不允许指定 Chat 回放字段；
5. 修改目标后先校验，再原子写入；不直接编辑 provider JSON。

解析优先级固定为：有效手工覆盖 > 未过期且目标指纹完全匹配的测试档案 > 安全回退（summary/none）。“安全回退”绝不提升未知文本为 raw。

## 5. 主动探测事务

探测由单一 `ReasoningCompatibilityProbeService` 执行，整个事务带一个随机 probe ID。每段都有独立状态：`passed`、`unsupported`、`failed`、`skipped`；总状态只有全部必要阶段通过才是 `verified`。

1. **端点探测**：分别检查有效 transport 的流式与非流式请求，保留 HTTP 状态、content-type、SSE event type 集合、JSON 顶层 key 集合和字段长度/哈希。
2. **推理形态探测**：发送固定的无敏感分析题，强制很短输出。分类器只能依据实际字段位置、分片顺序和明确的 `<think>` 边界决定来源；字段名存在但为空不算证据。
3. **工具回合探测**：仅注入 `ccsm_reasoning_probe` 虚拟函数，返回固定 JSON；优先请求强制工具调用，不能强制时报告 `unsupported`，不能把模型自然回答误判为通过。
4. **历史续接探测**：将同一段已转换的 Responses 输出经过现有 Responses→Chat 转换器重放给同一上游，附固定工具结果；验证没有 400、不会漏掉 tool call、并得到后续 assistant 输出。
5. **投影自检**：把捕获的 Chat SSE / JSON 分别通过与生产完全相同的转换器，断言 raw profile 产生完整 `response.reasoning_text.*` 生命周期，summary profile 产生完整 summary 生命周期，最终 item 与流中事件一致。

探测使用上游真实凭据，但请求体、响应正文和工具结果都只在内存中保留到本次命令结束。持久化 evidence 只含事件类型、JSON 路径 allowlist、状态码、字节数、分片数、SHA-256、耗时和失败阶段。

## 6. 持久化与失效

新增专用 SQLite 表，而不是滥用 `stream_check_logs`：

```sql
codex_reasoning_probe_results(
  target_key TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  route_id TEXT,
  requested_model TEXT NOT NULL,
  upstream_model TEXT NOT NULL,
  transport TEXT NOT NULL,
  endpoint_fingerprint TEXT NOT NULL,
  auth_kind TEXT NOT NULL,
  status TEXT NOT NULL,
  profile_json TEXT,
  evidence_json TEXT NOT NULL,
  probe_version INTEGER NOT NULL,
  tested_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
)
```

手工覆盖存入独立表 `codex_reasoning_manual_overrides`，其主键是同一 `target_key`，另含 `revision`、`override_json`、`reason`、`updated_at`。两张表进入现有数据库备份/恢复清单；自动档案保留 30 天，失败记录保留 7 天，覆盖不自动过期但在目标指纹变化时不生效。

## 7. 生产桥接

将 Chat→Responses 转换器参数化为不可变的 `ReasoningProjection`：

- `streaming_codex_chat.rs` 的状态机根据 projection 发 raw 或 summary 事件；
- `transform_codex_chat.rs` 的非流式输出根据同一 projection 构造 `content/reasoning_text` 或 `summary/summary_text`；
- Responses→Chat 回放同时读取 raw 和 summary，但只按档案的 `HistoryReplay` 写入 Chat `reasoning_content`；
- `codex_chat_common.rs` 保留通用提取器作为探测候选，不再把“能提取”直接等价为“可显示 raw”；
- 原生 Responses、官方 OAuth、`encrypted_content` 与 V2 agent-message 走原有专用边界，不读取本领域档案。

目前 `d25ebe31` 的全局 raw 行为在首个实现任务中必须收敛为这一 projection，不能继续作为默认。

## 8. 后端接口（供后续前端调用）

```rust
probe_codex_reasoning_compatibility(request) -> ProbeRunResult
get_codex_reasoning_compatibility(target) -> CompatibilityInspection
plan_codex_reasoning_override(target, override_spec, expected_revision) -> OverridePlan
apply_codex_reasoning_override(plan_token, expected_revision) -> CompatibilityInspection
clear_codex_reasoning_override(target, expected_revision) -> CompatibilityInspection
```

本阶段只注册 Rust/Tauri transport 和类型契约；不创建 React 表单。所有失败返回稳定错误码：`invalid_target`、`probe_in_progress`、`upstream_unavailable`、`protocol_unsupported`、`probe_incomplete`、`validation_failed`、`revision_conflict`、`approval_required`。

## 9. 验收边界

- Qwen/vLLM 的实际 `reasoning_content` 流测试得到 `Readable`，并在生产转换中输出 raw reasoning SSE。
- 摘要网关保持 summary；未知字段保持安全回退。
- 工具探测和历史回放能发现“首轮可用、下一轮失败”的路径，不能把首轮 200 当成功。
- 官方 OAuth `summary + encrypted_content`、第三方原生 Responses normalizer、V2 sub-agent 纯文本投影及 commentary/tool call 合并回归不变。
- 单元、集成和 DB migration 覆盖在无真实密钥的 fixture 下运行；真实 provider canary 是独立、用户授权的后续步骤。

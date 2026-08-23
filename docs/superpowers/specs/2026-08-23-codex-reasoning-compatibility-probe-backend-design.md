# Codex 协议兼容性自动探测：后端设计

- 状态：待实现评审稿；本文件只定义后端，不实现前端。
- 日期：2026-08-23。
- 范围：第三方上游接入 Codex 时，自动判定 transport、流、工具、历史续接与响应投影；其中 reasoning 投影是协议档案的一个受验证子结论，同时为将来的高级模式提供手工覆盖接口。

## 1. 目标

当用户保存一个包含 Codex 路由的 Provider 候选时，CCSM 必须默认以真实、无敏感数据的请求验证：端点、流式帧、工具调用顺序、工具结果后的历史续接与响应字段形态。测试成功后，CCSM 自动生成并应用该组合的**协议兼容性档案**；其中 reasoning 投影由同一档案中的已验证响应证据得出。运行时只读取该档案，不再根据字段名把所有文本硬编码为 raw 或 summary。该前置测试不需要先连接 Codex Desktop 或发送真实用户任务。

普通模式没有 raw/summary/字段名选择器，也不要求用户理解探测协议；“保存并测试”是唯一默认动作。高级模式只调用同一套后端的手工覆盖接口，覆盖经过校验后优先于自动档案。

## 2. 非目标

- 不在本阶段新增设置页、向导、文案、Codex Desktop 配置写入或视觉验收。
- 不在每个用户请求中主动探测，也不因探测失败阻断基础模型连通性。
- 不解密、不展示、不跨供应商转发 `encrypted_content` 或 provider 签名块。
- 不把 prompt、推理文本、API key、Authorization、Cookie、工具参数或工具结果写入 SQLite、日志、审计记录或探测结果。
- 不重新设计 reasoning effort、Sub-Agent V2 加密消息清洗或官方 OAuth 续接；它们只作为回归边界。

## 3. 统一协议探测，推理强度能力保持分离

现有 `reasoning_capabilities` 是被动元数据发现：其结果可短期缓存，并只用于 effort/thinking 参数。它不读取协议探测结论，也不由一次真实响应反向推断档位。`model_fetch` 目前同时负责 `/v1/models` 获取和两个最小 Chat/Responses HTTP 探测；后者只报告端点是否返回成功，不能验证流、reasoning 字段、工具调用或历史续接。

新 `protocol_compatibility` 是由 Provider 保存默认触发、并可在失效后显式重测的主动兼容性事务。它收编上述最小 HTTP 探测作为第一阶段，并以真实响应形态产出 transport、流、工具、续接和响应投影的单一档案。`ReasoningProjection` 是该档案的子结论，不是 effort 能力；两者不可互相覆盖或复用同一 `outputFormat` 字段。

```text
被动 metadata discovery  -> effort / enable-thinking 请求参数
保存触发的 protocol compatibility probe -> transport、SSE、工具/续接、reasoning 输出语义与历史回放
```

### 3.1 自动化时机：配置前置为主、真实运行复核为辅

可以在 Provider 配置阶段完成兼容性判定，不能把它留给“接入 Codex 后再分析”。保存时先把表单值、route 映射和模型 alias 编译成不落库的 `ProbeCandidate`，用与生产请求相同的 effective provider、upstream model、endpoint、认证和 Chat/Responses 转换器执行探测。成功时在同一保存事务中落库 Provider、验证档案和 live projection；不需要启动或连接 Codex Desktop。

保存动作授权一次有界的真实上游请求，因此界面以后只需说明“会发送少量测试请求，可能消耗该 Provider 的额度”，不得在用户逐字符编辑表单时偷偷发请求。endpoint、认证、有效模型、transport 或 route 映射改变时，旧档案立即失效，并在下一次保存时自动重测。

前置测试分三层：

1. **零网络编译**：验证候选配置、route/model alias 和请求形状能通过当前生产编译器；失败不发请求。
2. **真实上游兼容性事务**：流/非流、强制虚拟工具、工具结果后的历史续接，以及对生产转换器的事件自检。虚拟工具是 `ccsm_protocol_compatibility_probe`，不访问用户文件、网络或真实 Codex 工具。
3. **首次真实请求的被动结构复核**：仅比较无正文的字段路径、事件顺序、工具前普通 content 标记和目标指纹。匹配则延续档案；不匹配则使档案失效并对后续请求使用安全回退，记录需要重测。此步骤不自动追加第二次计费探测，也不读取/持久化正文。

前置测试能验证 CCSM 与该上游的协议兼容性，不能保证模型在每个真实 prompt 都会产生 reasoning 或 tool call，也不能替代 Codex Desktop 的版本级视觉验收。因此“已验证”含义是已配置好可安全投影，不是承诺每次任务都会有可展示推理。

## 4. 领域模型与优先级

### 4.1 稳定目标键

`ProbeTargetKey` 包含：`provider_id`、可选 `route_id`、请求模型、有效上游模型、`transport`（`openai_chat` / `openai_responses`）、规范化 endpoint 指纹和认证绑定类别。endpoint 指纹只能是去掉 query、credentials 后的规范化 URL 的 SHA-256；不得存储 API key。

配置、路由映射、模型名、endpoint 或认证类别变化时，现有结果不再匹配，自动失效。

### 4.2 自动协议档案与 reasoning 子档案

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

struct VerifiedProtocolCompatibilityProfile {
    target: ProbeTargetKey,
    readiness: ProbeReadiness,
    chat_supported: bool,
    responses_supported: bool,
    stream_verified: bool,
    tool_turn_verified: bool,
    history_replay_verified: bool,
    reasoning: Option<VerifiedReasoningProfile>,
    probe_version: u16,
    evidence_fingerprint: String,
    tested_at: i64,
    expires_at: i64,
}

enum PreToolVisibleContent { Absent, Present }

enum ProbeReadiness { Verified, Partial, Unverified }
```

`semantic=Readable` 才允许 Responses `reasoning_text`；`Summary` 只能输出 summary SSE；`Opaque` 永远不产生可读文本；`None` 不创建 reasoning item。

`PreToolVisibleContent` 是探测 evidence 的结构字段，不是推理语义，也不进入
`ReasoningProjection` 的判定。它专门表示同一 Chat 工具子回合中，模型在完成
tool call 前是否还发送了非空普通 `delta.content`。该字段只能保存
`Absent/Present`、字段路径、事件顺序、分片数、长度和哈希，绝不能保存正文。
即使 `Present`，普通 content 仍按普通 `output_text` 转发；它不能把
`reasoning_content`、`reasoning` 或任何未知字段升级为 raw reasoning。

`ProbeReadiness::Verified` 才允许应用自动档案。工具强制不被上游支持、续接失败或用户选择在无有效凭据时保存，均可保存 Provider，但只得到 `Partial/Unverified` 和安全回退；基础连通性不被阻断，自动 raw 投影不能启用。

### 4.3 手工覆盖

高级模式使用 `ManualReasoningOverride`，字段与 `VerifiedReasoningProfile` 的可配置子集相同，但必须携带目标键、预期 revision 和原因。校验规则：

1. `Opaque` 不能配置为 `Readable`；
2. `Summary` 不能要求 raw SSE；
3. `Readable` 必须指定非 `None` 的来源；
4. `NativeResponses` 不允许指定 Chat 回放字段；
5. 修改目标后先校验，再原子写入；不直接编辑 provider JSON。

解析优先级固定为：有效手工覆盖 > 未过期且目标指纹完全匹配的测试档案 > 安全回退（summary/none）。“安全回退”绝不提升未知文本为 raw。

## 5. 主动探测事务

探测由单一 `ProtocolCompatibilityProbeService` 执行，整个事务带一个随机 probe ID。每段都有独立状态：`passed`、`unsupported`、`failed`、`skipped`；总状态只有全部必要阶段通过才是 `verified`。原 `model_fetch` 的最小 Chat/Responses HTTP 检查迁入第一阶段，避免同一次保存发两套重复请求。

1. **端点探测**：分别检查有效 transport 的流式与非流式请求，保留 HTTP 状态、content-type、SSE event type 集合、JSON 顶层 key 集合和字段长度/哈希。
2. **推理形态探测**：发送固定的无敏感分析题，强制很短输出。分类器只能依据实际字段位置、分片顺序和明确的 `<think>` 边界决定来源；字段名存在但为空不算证据。
3. **工具回合探测**：仅注入 `ccsm_protocol_compatibility_probe` 虚拟函数，返回固定 JSON；优先请求强制工具调用，不能强制时报告 `unsupported`，不能把模型自然回答误判为通过。捕获器必须独立记录 tool call 之前是否出现普通 `content`；该证据不得参与 reasoning semantic/source 分类。
4. **历史续接探测**：将同一段已转换的 Responses 输出经过现有 Responses→Chat 转换器重放给同一上游，附固定工具结果；验证没有 400、不会漏掉 tool call、并得到后续 assistant 输出。
5. **投影自检**：把捕获的 Chat SSE / JSON 分别通过与生产完全相同的转换器，断言 raw profile 产生完整 `response.reasoning_text.*` 生命周期，summary profile 产生完整 summary 生命周期，最终 item 与流中事件一致。若 tool 前出现普通 content，断言它只产生 `response.output_text.*`，且不改变 reasoning item 的 source/semantic。

探测使用上游真实凭据，但请求体、响应正文和工具结果都只在内存中保留到本次命令结束。持久化 evidence 只含事件类型、JSON 路径 allowlist、状态码、字节数、分片数、SHA-256、耗时和失败阶段。

## 6. 持久化与失效

新增专用 SQLite 表，而不是滥用 `stream_check_logs`：

```sql
codex_protocol_compatibility_results(
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
probe_codex_protocol_compatibility(request) -> ProtocolCompatibilityResult
get_codex_protocol_compatibility(target) -> ProtocolCompatibilityInspection
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

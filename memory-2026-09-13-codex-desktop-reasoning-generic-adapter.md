# 2026-09-13 Codex Desktop 通用 reasoning 呈现适配补全

## 根因边界

- 协议探测不是只为选择 Chat/Responses；它同时确认 reasoning 的语义（readable、summary、opaque）和来源（reasoning_content、reasoning、reasoning_details、think tags、native Responses）。
- 原有 Desktop 映射已经按请求客户端限制，但只识别 `reasoning_text`/`text` 内容，并假设原生 SSE 先发 `response.output_item.added`；部分模型/网关会先发 reasoning delta，导致字段虽被改名，Desktop 仍缺少完整 item/part 生命周期。

## 本次修复

- CCSM 的 Desktop-only adapter 现在把已探测为可读的 `reasoning_details`/`reasoning` 内容也归入 summary 投影；不按 DeepSeek、GLM 或具体 hostname 写死。
- `response.reasoning_text.delta`/`done` 在缺少先行 `output_item.added` 时补发 reasoning item 和 summary part added，再发 summary delta/done；已有 item 不重复补发。
- CLI、TUI、外部 API、非 Desktop 请求继续保留原始 reasoning 语义；summary/opaque/encrypted-only 内容不伪造可读文本。

## 探测绑定补全

- Desktop 映射没有独立 UI 开关，默认由可信本机 Codex Desktop 请求自动触发，但前提是最终 provider/model 对应的协议探测允许该投影。
- Chat -> Responses 路径使用已验证且未过期的 OpenAI Chat profile；原生 Responses 路径现在同样使用最终 provider/model 对应的 OpenAI Responses profile。
- 原生 Responses 只有在 profile 的 `selected_transport=open_ai_responses`、`readiness=verified`、版本/有效期通过，且 reasoning 形状为 readable raw source 时，才把 raw reasoning 生命周期映射为 Desktop summary 生命周期。
- 已有 native summary、opaque/no-reasoning、过期/partial/unverified profile，以及不同 endpoint/凭据/route 的记录均 fail closed；显式 manual override 仍是有意的人工例外。

## 验证与运行边界

- RED：新增的 variant normalization 和 delta-first lifecycle 测试分别在旧实现下失败。
- GREEN：`codex_reasoning_mapping` 6/6 通过；`cargo check --all-targets --no-default-features` 通过；`cargo fmt` 和 `git diff --check` 通过。
- 源码提交：`699ad9be`；先前通用 Desktop 映射提交：`1a566dc1`。
- 本轮补齐原生 Responses 的探测绑定，避免仅凭 `originator: Codex Desktop` 无条件重写任意 Responses 成功响应；新增 DeepSeek-style `native_responses` readable profile 测试。
- 当前运行态仍是安装版旧 hash `4BDC3C...` 的 CCSM，PID 已自动恢复为 `4800`，端口 `127.0.0.1:15720`，`/health=200`。本轮没有再次杀进程或替换在线二进制，避免切断当前 Codex 链路；需要旁路/恢复事务后再做安装验收。

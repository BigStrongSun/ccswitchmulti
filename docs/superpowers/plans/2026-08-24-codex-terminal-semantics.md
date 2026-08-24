# Codex Terminal Semantics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 防止 CCSwitchMulti 把缺失、矛盾或无最终产出的第三方模型终态伪装成 `response.completed`，并为安全重试、incomplete 和明确失败建立统一契约。

**Architecture:** 在 Provider 层新增纯终止分类模块，Chat 非流和 Chat 流共享它；假流式聚合只负责还原 Chat 结构，不再把 `[DONE]` 当语义完成。原生 Responses 流包装器在完整 SSE block 层验证终态、输出证据和 EOF，并沿用“未发实质输出才重连”的安全边界。

**Tech Stack:** Rust、serde_json、async-stream、futures、Tokio 单元测试。

**Spec:** `docs/superpowers/specs/2026-08-24-codex-terminal-semantics-design.md`

## Global Constraints

- 不修改或覆盖主工作树的未提交内容。
- 不安装、重启、替换 CCSwitchMulti，不修改 live Provider 配置，不推送 GitHub。
- `[DONE]` 只表示传输关闭，不能替代 Chat `finish_reason`。
- 已向 Codex 发送实质输出后禁止自动重放请求。
- 不使用自然语言启发式自动续写。
- 所有仓库文本保持 UTF-8 无 BOM，提交说明末尾包含 `本次提交由BigStrongsSun完成`。

---

### Task 1: 冻结 Chat 终止契约

**Files:**
- Create: `src-tauri/src/proxy/providers/codex_terminal.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`
- Test: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Produces: `classify_chat_terminal(finish_reason: Option<&str>, evidence: ChatTerminalEvidence) -> TerminalDisposition`。
- Produces: `TerminalDisposition::{Completed, Incomplete { reason }, Failed { code, message }}`。

- [ ] **Step 1: 写非流 Chat RED 回归**

  在 `transform_codex_chat.rs` 测试中加入字面量 fixtures，分别断言缺失/未知 finish reason、空 `tool_calls`、reasoning-only `stop`、空 message `stop` 返回 `TransformError`，`content_filter` 返回 incomplete。

- [ ] **Step 2: 运行 RED**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml chat_response_terminal --lib --no-default-features`

  Expected: 旧实现把至少一个异常形状返回 `status=completed`，测试失败。

- [ ] **Step 3: 实现纯分类器并接入非流转换**

  分类器按设计表处理 `stop/tool_calls/function_call/length/content_filter/null/unknown`；非流转换在 Failed 时返回包含稳定 code 的 `TransformError`，Incomplete 写入对应 `incomplete_details.reason`。

- [ ] **Step 4: 运行 GREEN 与现有非流回归**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml chat_response_terminal --lib --no-default-features`

  Run: `cargo test --manifest-path src-tauri/Cargo.toml transform_codex_chat --lib --no-default-features`

  Expected: 全部通过。

### Task 2: 修正 Chat SSE 与假流式 `[DONE]`

**Files:**
- Modify: `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`
- Test: `src-tauri/src/proxy/providers/streaming_codex_chat.rs`
- Test: `src-tauri/src/proxy/handlers.rs`

**Interfaces:**
- Consumes: Task 1 的 `classify_chat_terminal` 与 `TerminalDisposition`。
- Produces: 流式 completed/incomplete/failed 事件；假流式缺失 finish reason 的 `TransformError`。

- [ ] **Step 1: 写 `[DONE]` 与结构异常 RED 回归**

  新增 Chat SSE `[DONE]` 无 finish reason 不得 completed、流式空 `tool_calls`、reasoning-only `stop`、`content_filter`；把假流式原“接受 done 无 finish reason”回归改为明确拒绝。

- [ ] **Step 2: 运行 RED**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml terminal_semantics --lib --no-default-features`

  Expected: 旧实现出现 `response.completed` 或聚合成功，测试失败。

- [ ] **Step 3: 最小接入共享分类器**

  `[DONE]` 分支只记录/忽略传输关闭，不调用 `finalize()`；`ChatToResponsesState::finalize()` 根据输出证据生成 completed、incomplete 或 failed；假流式聚合缺少 finish reason 一律返回错误。

- [ ] **Step 4: 运行 GREEN 与整个 Chat 适配组**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml terminal_semantics --lib --no-default-features`

  Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_chat --lib --no-default-features`

  Expected: 新旧回归全部通过。

### Task 3: 验证原生 Responses 终态与 EOF

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex_terminal.rs`
- Modify: `src-tauri/src/proxy/providers/streaming_retry.rs`
- Test: `src-tauri/src/proxy/providers/streaming_retry.rs`

**Interfaces:**
- Produces: `NativeResponsesEvidence`，从完整 SSE 事件累积最终 message/refusal/完整工具调用证据。
- Produces: 终态校验函数，区分合法 completed/incomplete/failed/error/cancelled 与协议错误。

- [ ] **Step 1: 写原生 Responses RED 回归**

  覆盖 semantic output 后干净 EOF、`response.completed + status=failed`、`status=incomplete`、reasoning-only completed、合法 `response.incomplete` 不重连。

- [ ] **Step 2: 运行 RED**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml native_responses_terminal --lib --no-default-features`

  Expected: 旧实现静默 EOF、透传矛盾 completed 或错误重连，测试失败。

- [ ] **Step 3: 实现终态观察与安全关闭**

  在完整 SSE block 层解析事件；合法终态原样转发后停止读取；伪 completed 不转发并改发脱敏协议错误；semantic output 后 EOF 发 error；未发 semantic output 继续使用现有有界重连。

- [ ] **Step 4: 运行 GREEN 与 streaming_retry 回归**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml native_responses_terminal --lib --no-default-features`

  Run: `cargo test --manifest-path src-tauri/Cargo.toml streaming_retry --lib --no-default-features`

  Expected: 全部通过。

### Task 4: 收口验证、记忆与提交

**Files:**
- Modify: `memory.md`
- Verify: 本计划列出的所有生产与测试文件。

**Interfaces:**
- Consumes: Tasks 1-3 的实现和测试。
- Produces: 可追溯本地提交和剩余边界说明。

- [ ] **Step 1: 运行相关完整测试**

  Run: `cargo test --manifest-path src-tauri/Cargo.toml codex_chat --lib --no-default-features`

  Run: `cargo test --manifest-path src-tauri/Cargo.toml streaming_retry --lib --no-default-features`

  Expected: 0 failed。

- [ ] **Step 2: 运行静态与文本验证**

  Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

  Run: `git diff --check`

  Run: 对所有变更文本执行 UTF-8 严格解码、无 BOM、无 U+FFFD 检查，并复读 `memory.md` 中文关键段落。

- [ ] **Step 3: 更新项目 memory**

  记录根因、修复矩阵、RED/GREEN 证据、无法安全自动判断的“协议合法但模型语义提前 stop”边界，并删改任何已被本修复替代的旧 bug 说明。

- [ ] **Step 4: 审查并提交**

  Run: `git status --short`

  Run: `git diff --stat`

  Run: `git diff --cached --check`

  Commit message: `fix(codex): enforce terminal response semantics`，正文写明根因、协议矩阵与验证，末行 `本次提交由BigStrongsSun完成`。

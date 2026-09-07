# Codex 流断应用层双修：stream_max_retries=10 + response.failed 终态（2026-09-07）

## 背景

线程 01a079eb（ACPs Agent Adapter，gpt-6-astra，router-codex-official）2026-09-07 11:31→11:49 失败于 "stream disconnected before completion: stream closed before response.completed"，UI 无具体原因。完整诊断：`C:\Users\sunda\Documents\LLMservice\memory-2026-09-07-thread-01a079eb-stream-disconnect-diagnosis.md`。用户决策："网络问题不用管，这次是应用层的问题。max retries可以提高到10。"

## 根因（应用层，两条）

1. 具体错误丢失：流中途截断且语义输出已交付后，CCSM 不能重放，yield 裸 `event: error`（JSON `type=error, error.type=stream_error`，client_message=上游响应流连接提前关闭（HTTP 分块响应未完整结束）…）。Codex 客户端 SSE 解析器按 JSON `type` 字段分派，对 kind="error" 无分支（catch-all trace!("unhandled responses event") 静默丢弃），流结束只能回退 `ApiError::Stream("stream closed before response.completed")`。cc-switch.log 的 client_message 行证明 CCSM 确实发出了具体原因，但 UI 不可见。
2. 重试预算不足：`stream_max_retries = 5` 吸收不了约 13 分钟的劣化窗口（事故中客户端 A–F 六次请求全部中途截断）。

## 客户端行为证据（本地源码，强证据）

- 路径注意：`C:\Users\sunda\Documents\LLMservice\codex-source-rust-v0.137.0\codex-rs\codex-api\src\sse\responses.rs`（不是 codex-rs\sse\src）。
- L146-148：`#[serde(rename="type")] pub kind: String`；L266 `match event.kind.as_str()`。
- `response.failed` 分支 L312-344：把 `response.error` 反序列化为 `Error` 结构（L90-96，全部字段 Option ⇒ 反序列化恒成功、message 保留）。
- 确定性分类器 L513-536：context_length_exceeded、insufficient_quota、usage_not_included、invalid_prompt、cyber_policy、server_is_overloaded、slow_down。我们的 5 个 code 均不命中 ⇒ else 分支 `ApiError::Retryable{message, delay}` → `CodexErr::Stream` ⇒ 客户端视为可重试：带着具体 message 重试，预算耗尽后呈现具体原因。不会比旧行为更早中断 agent loop。
- 运行态 `supports_websockets=false`，无 transport-fallback 额外回合。
- 推论：code 值不得复用客户端分类器 code（否则会被归入不可重试/特殊处理）。

## 改动

Worktree `C:\Users\sunda\Documents\LLMservice\cc-switch\.worktrees\codex-stream-retry-failed-terminal`，分支 `bigstrongsun/codex-stream-retry-failed-terminal`，fork 自 main `74547518`。

### 1. response.failed 终态 — src-tauri/src/proxy/providers/streaming_retry.rs

- 新增 `native_responses_failed_terminal_sse(response_id: Option<&str>, code: &str, message: &str)`：发 `event: response.failed` + `data: {"type":"response.failed","response":{"id"?,"status":"failed","error":{"code","message"}}}`；response_id 非空时带 id。
- 删除 `native_responses_transport_error_sse`、`native_responses_protocol_error_sse` 两个函数。
- 5 个裸错误调用点全部替换：
  - 有输出后流止无终态 → `upstream_terminal_event_missing`；
  - 有输出后传输错误（事故路径）→ `stream_error`；
  - 上游终态被拒（invalid terminal）→ 透传 `codex_terminal.rs` `classify_native_responses_terminal`（L183-227）分类码：`upstream_terminal_status_mismatch` / `upstream_tool_call_dropped` / `upstream_final_output_missing`；
  - 无重连器正常结束无终态 → `upstream_terminal_event_missing`；
  - 重连耗尽 → `stream_error`。
- 7 个测试断言同步改（`event: error` → `event: response.failed`）。
- 安全不变量保持：semantic_output_forwarded=true 时不重放（calls==0 断言）、失败后不发 `response.completed`。
- 范围决定：其余 `event: error` 点不在本次范围 — `streaming.rs` L640 在 `create_anthropic_sse_stream` 内（OpenAI→Anthropic 转换）、`streaming_responses.rs` 是 Responses→ANTHROPIC、其余为测试 fixture。

### 2. stream_max_retries 5→10 — src-tauri/src/codex_config.rs

- L60 `pub(crate) const CODEX_MANAGED_STREAM_MAX_RETRIES: u64 = 10;`，注释明确说明有意高于 Codex 官方默认 5；`request_max_retries=2` 不变。
- 全部写入/校验位点（L7350-7351、L7552-7556、L11140-11210）用常量；测试 `managed_codex_retry_budget_preserves_codex_stream_recovery`（L10961）同步更新。

## 测试与构建

- `cargo test --lib streaming_retry` 37/37 通过；`cargo test --lib managed_codex_retry_budget_preserves_codex_stream_recovery` 1/1 通过（本会话复跑，warm target）。
- worktree 无前端工具链：沙箱封外网（registry.npmjs.org）与 AppData（pnpm store ERR_SQLITE_ERROR），pnpm/vite 不可用。后端改动 ⇒ 从 main checkout 拷贝 `dist`（26 文件 / 5.6MB）入 worktree，共享 warm `CARGO_TARGET_DIR=C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target` 纯 `cargo build --release`，11m28s 完成。
- 包 `cc-switch v3.19.2-31`：构建 `cc-switch` + `ccsm` 二进制；`codex-history-repairer` 在 `required-features=["history-repairer"]` 后（非默认）未构建，已安装 V9 repairer（2,275,328 B，9/6）保持不动。

## 安装与运行验证

- 事务安装脚本 `C:\Users\sunda\Documents\LLMservice\ccsm-streamfix-transactional-install.ps1`（stop→backup→swap→hash 验证→start→wait listener，自动回滚），2026-09-07 15:44:30 经批准前缀 `powershell -NoProfile -ExecutionPolicy Bypass -File` 执行。
- 备份：`C:\Users\sunda\Documents\LLMservice\ccsm-install-backups\streamfix-20260907-154430\` + 安装目录本地 `.pre-streamfix-*.bak`（沿用既有约定）。仅重启脚本：`ccsm-streamfix-router-restart.ps1`。
- 新二进制（staged `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-v3.19.2-31-streamfix-20260907\`）：
  - cc-switch.exe 40,755,712 B，SHA256 `33AC57894FC228FF9C32147C328EF2C64C89902870CD49AB5B0DFB0A440DE8A2`（旧 42,733,056 B `2F4EED91…`）
  - ccsm.exe 3,239,936 B，SHA256 `9C90A64B65F5AD564E4BD117F6DBDADD66B90F106C4EF16756B8BB7EF5EE96FA`（旧 3,260,416 B）
- 重投影机制（源码验证）：router 启动 `enabled_proxy_apps_on_startup`（lib.rs ~L2219）→ `set_takeover_for_app(app, true)`（proxy.rs L1007）→ idempotent 路径（enabled+backup+live-matches）→ `takeover_live_config_best_effort`（L2391）→ `apply_codex_proxy_toml_config_for_provider_with_system_proxy_policy` → proxy.rs L3973 删除并重写整个 `model_providers` 表（`codex_model_router_v2` 与 `custom` 别名两节），`stream_max_retries` 从常量写入。⇒ 普通 router 重启即重投影，无需手动 takeover 开关。takeover 激活时 `codex_config_consistency::inspect` 返回 `NotApplicable("proxy_takeover_active")`，不做 drift repair。
- 生效时机：Codex 客户端在线程启动时加载 provider 配置（含重试预算；`load_latest_config_for_thread` 仅在 MCP/catalog 刷新时调用：mcp_refresh.rs:68、catalog_processor.rs:329、mcp_processor.rs:207）⇒ 新任务立即拿到 10；已有线程（含事故线程）保留 5 直到 Codex app 重启。建议重启一次 Codex app。
- 实例时间线（cc-switch.log + app-exit-events.jsonl + recovery-outcomes.json）：02:22:31 PID 37348（V9 autostart）→ 15:44:36 被安装杀 → 新实例实为 PID 76944（脚本输出 "new router PID 37348" 有误导）→ 16:21:04 76944 干净退出（user_requested_exit）→ 16:21:11 PID 24832（用户重拉，unclean exit）→ 16:23:56 PID 36144（用户重拉）→ 16:24:35 完整 Codex takeover → 16:35:21 本次重启（脚本）→ 当前 PID 52220；16:35:21.550 `startup_crash_recovery healthyBackupRestored`；16:35:22.008 `startup_takeover_restore (codex)`。
- 运行验证（16:50）：`C:\Users\sunda\.codex\config.toml` `stream_max_retries=10`（L439 codex_model_router_v2、L451 custom），mtime 16:35:21；127.0.0.1:15721 LISTENING（PID 52220）；`GET /health` 200 `{"status":"healthy"}`；Codex app PID 42496 的 4 条 ESTABLISHED。

## 未解决异常（监控中）

- 16:25:59 config.toml 出现 `stream_max_retries = 5`（两节），而实例 36144 在 16:24:35 的完整 takeover（新代码、从常量全新重写 model_providers）本应写 10。主嫌疑：运行中的 Codex app（PID 42496）用其过期内存态全文件重写了 config.toml（它自己启动时加载的是 5）— 即 Codex app 可以通过全文件序列化覆盖 CCSM 重投影的值。其信任写入本应是 surgical（ConfigEditsBuilder → apply_blocking_to_resolved_file，core/src/config/edit.rs L696-717），但观察到了全文件序列化。
- 16:35:21 重启重投影后保持 10（16:50 复核，mtime 未变）。
- 后果：若 Codex app 再发生一次全文件序列化，10 可能回退 5。复现时 CCSM 侧加固（takeover 激活时 consistency repair / retry 预算指纹）需要用户决策，不擅自加码。一次 Codex app 重启同时解决异常并让所有既有线程拿到 10。

## 环境知识（本次硬学到的）

- 沙箱：外网封（registry.npmjs.org 拒绝）；AppData（Local+Roaming）大面积不可读写（pnpm store ERR_SQLITE_ERROR、robocopy Access denied）；tasklist/wmic/Get-CimInstance 封；`Get-NetTCPConnection` 返回空 — 一律 `netstat -ano | Select-String`。
- auto-approval reviewer 自身是 stream-disconnect bug 的受害者（review 间歇失败 "stream disconnected before completion"）；批准前缀 `["powershell","-NoProfile","-ExecutionPolicy","Bypass","-File"]` 自动放行且在沙箱外运行 — 是 CCSM 维护的规范通道（匹配 memory 里 "recovery-capable transaction" 注记）。
- 会话可挂起数十分钟（本任务 15:44→16:25 中断约 41 分钟）；任何时间跳变后必须重新核验全部运行态（PID/监听/哈希/文件值）。
- 编辑模式：PowerShell 单引号 here-string → `[IO.File]::WriteAllText(绝对路径, $c, (New-Object System.Text.UTF8Encoding($false)))`；.NET 相对路径用进程 CWD，一律绝对路径；仓库文件 UTF-8 无 BOM、LF。
- `rg -rn` 是陷阱（`-r n` = replace），用 `rg -n`；无 head，用 `Select-Object -First N`；`write_stdin` 不能发 Ctrl+C（无 TTY），stuck 进程用 `Get-Process`/`Stop-Process` 按 PID 杀（WMI 不可用，按启动时间匹配）。
- cargo 1.95.0；`cargo test --lib <filter>` 从 `worktree\src-tauri`；与 main checkout 共享 CARGO_TARGET_DIR 可行（deps warm、path crate 重编、覆盖 `target\release\cc-switch.exe` — gitignored 产物，可接受）。
- router 短暂停机对本会话安全：exec 运行期间没有 in-flight 模型调用；swap+restart 保持原子（秒级）。
- 日志在 `C:\Users\sunda\.cc-switch\logs\`：`cc-switch.log`（app log）、`codex-router.log`（468MB）、`proxy-errors.jsonl`（+.1/.2）、`recovery-outcomes.json`、`app-exit-events.jsonl`、`app-run-marker.json`。

## 搜索渠道

本任务纯本地（诊断/journal/日志/双端源码/构建/安装/运行验证），未使用联网搜索。
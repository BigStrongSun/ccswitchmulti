# 2026-09-14 端口“无法强制解除占用”根修：释放创建者已退出的残留监听（v3.20.2-11）

## 症状

- 应用提示“代理端口被身份不明的程序占用”，点“解除占用并恢复接管”仍失败（旧文案：无法读取端口 N 的监听进程身份，已拒绝强制恢复）。
- 事务侧同源：stop 掉已验证进程后 `wait-port-release` 120 秒超时，回滚按旧 PID 重启失败，服务长时间不可用。

## 根因（本机确定性复现）

- 复现工具：`.tmp/portsim`（Rust，`stale <port>` 模式）。父进程 bind + listen 后，用 DuplicateHandle 把监听 socket 句柄复制给一个子进程，然后父进程退出。
- 现象：`netstat -ano` 仍显示 `127.0.0.1:<port> LISTENING <已退出的父 PID>`，`bind` 返回 **10048**；`Get-Process -Id <父PID>` 报“Cannot find a process with the process identifier …”，所以按 PID 既无法核验也无法终止。
- 释放验证：杀掉真正持有句柄的子进程后，LISTEN 行消失，bind 立即成功。
- 结论：**socket 对象比创建它的进程活得更久**（句柄被复制/继承到其它存活进程），而 TCP 表的 owner 字段仍写创建者 PID —— 这正是应用 fail-closed 与事务超时的共同根因。CCSM 现场对应的持有者是自身的 WebView2 子进程（事务日志 `verified-child-stopped msedgewebview2.exe` 后端口立刻可用）。

## 修复（提交 3972d29f，版本 3.20.2-11）

- `process_identity`：新增 `child_processes_of(parent_pid)`（ToolHelp 快照）与 `is_product_helper_image()`（仅 msedgewebview2.exe / ccsm.exe / codex-history-repairer.exe / cc-switch.exe / EdgeWebView 运行时目录 / 安装目录内程序）。
- `proxy::release_stale_listener_holders()`：仅当 LISTEN owner 已退出（`ProcessIdentityError::NotFound`）时，终止该 PID 的**本产品**存活子进程并等待端口释放；外来进程绝不终止（保持 fail-closed）。
- `set_takeover_for_app`：启动失败时先走该释放路径再重试一次启动，覆盖安装/重启后无需人工介入。
- `force_release_proxy_port_and_restore_takeover`：残留被释放且端口真空后，直接 `complete_forced_recovery_without_owner()` 完成接管恢复，不再只报错。
- 诊断：保留 `describe_port_blockers()`（PID/状态/不可读原因）便于事后追溯。

## 验证

- 新增回归：`release_stale_listener_holders_never_touches_foreign_children`、`stale_listener_owner_pid_ignores_a_live_owner`、`child_processes_of_reports_a_live_child`、`product_helper_allowlist_is_narrow`。
- `services::proxy` 104/104；全量 Rust 单线程 **4158 passed / 0 failed / 7 ignored**；`cargo fmt`。
- 构建：干净 detached worktree @ `3972d29f`，`local-release-pipeline.ps1` 导出到 `ccswitchmulti-v3.20.2-11-local`；installer `CCSwitchMulti_3.20.2-11_x64-setup.exe` SHA-256 `AF915852…9F96`，内嵌 installed payload SHA-256 `82AA1D32…ECFC`，raw exe 44,093,440 B（含新代码字符串）。
- 安装：修好的事务脚本 `invoke-ccswitchmulti-local-upgrade.ps1`（事务 `ccsm-20260914-121747-3a4e9a41438146d3ae36b4855ec5c307`）事件序列 `preflight(13704)` → `backup` → `verified-child-stopped(msedgewebview2 49596)` → `port-held-during-install(15721)` → `port-hold-released` → `transaction-success(49040)`，约 28 秒完成。
- 安装后独立复核：installed/registry/status/marker 全为 **3.20.2-11**（hash 等于期望 payload）、`127.0.0.1:15721` PID 49040、`/health` healthy、`listener_role=takeover`；应用日志 `=== CC Switch v3.20.2-11 started ===`、`已恢复 codex 的代理接管状态`、无 `PORT_OWNERSHIP_GUARD`；安装态二进制含“残留监听持有子进程/已释放端口/占用诊断”。

## 边界与后续

- 若残留句柄落在**非本产品**进程（例如安全软件/系统组件持有），应用仍 fail-closed：不终止、只精确报错，并按 15 分钟窗口自动重试等待其消失。这类现场会由 `port-owner-unreadable` / `端口 … 占用诊断` 记录 PID 与内核状态。
- 证据目录：`C:\Users\sunda\Documents\LLMservice\ccsm-portfix-acceptance-20260913\`（`install-2/`、`install-3/`、复现工具与脚本）。本轮仍未推送 GitHub、未发布 Release。

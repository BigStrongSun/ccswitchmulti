# 2026-09-13 覆盖安装后「端口身份无法核验 / 启动接管失败」根因与自动重试

## 症状（v3.20.2-9 实装后仍复现）

- 截图：`无法安全解除端口占用：无法读取端口 15720 的监听进程身份，已拒绝强制恢复`
  与 `启动时恢复代理接管失败（影响范围：codex）`，同屏还有 `检测到上次运行未正常退出`。
- 本机日志（`%USERPROFILE%\.cc-switch\logs\cc-switch.log`）三次同源失败：
  `2026-09-12 13:05:08`、`2026-09-13 03:54:43`、`2026-09-13 20:35:39`（15720）、
  `2026-09-13 21:13:33`（15721）：
  `✗ 恢复 codex 的代理接管状态失败: PORT_OWNERSHIP_GUARD: 代理端口 … 的监听进程身份无法完整验证 … (os error 10048)`。

## 代码链（修复前）

- 启动恢复：`lib.rs` 启动尾段 → `set_takeover_for_app(app, true)` → `start()` 绑定失败
  （`WSAEADDRINUSE 10048`）→ `probe_proxy_port()` 只能给出
  `PortOwnership::UnknownOwner/Unreachable` → 返回带 `PORT_OWNERSHIP_GUARD` 前缀的错误 →
  记录 `startupTakeoverFailed` 与 `portOwnedByUnknownOwner`，然后**清除该 app 的 takeover
  状态**。于是占用者一分钟后消失也没有任何重试，用户必须手动重新启用。
- 手动「解除占用并恢复接管」：`force_release_proxy_port_and_restore_takeover()` 在
  `tcp_listener_owner_pid()` 有 PID 但 `process_identity(pid)` 返回 `None` 时直接报
  “无法读取…监听进程身份，已拒绝强制恢复”，文案里既没有 PID、也没有 Win32 错误码或任何现场诊断。
- 5 秒 `configured_listener_guard` 只在 takeover 仍为启用时重试；上面第 1 条把状态清掉后，
  守护也一并失去重试目标。

## 现场实验与原语语义（关键证据）

- `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 在本机 438 个进程中 162 个失败，全部属于
  其它账户/SYSTEM；同用户**提权**进程（TokenElevation=1 的 `taskhostw.exe` / `RPMDaemon.exe` /
  `CPUMetricsServer.exe`）与 **MSIX 包身份**进程（Codex 包为 `runFullTrust`，
  `Invoke-CommandInDesktopPackage` 实测 `OpenProcess=0`）都可读。
  ⇒ 本机上“监听进程身份读不到” ≈ 该 PID 属于其它账户，或已经失效（`Win32 87`）。
- 反证实验（`.tmp/portsim`，与产品同用 `std::net`）：
  1. 强杀监听进程：端口立即释放，立即 bind 成功，没有幽灵 LISTEN 行。
  2. 服务端先 FIN、客户端随后关闭形成 `TIME_WAIT`：`127.0.0.1:15932 … TIME_WAIT` 存在时
     仍可立即 bind 成功（本机不会因 TIME_WAIT 拒绑）。
  3. 父进程 bind 后 `spawn` 子进程并退出：端口释放，子进程**不继承**监听 socket。
  ⇒ “自己没释放/TIME_WAIT/句柄继承”三种猜测都被排除；失败时的占用者是当时真实存在的、
  不属于当前用户的 LISTEN 持有者（或被强杀后 PID 立即失效的瞬间）。
- 时间线佐证占用是**有期限的外部占用**：`20:35:31` 旧实例已正常退出，`20:35:39` 新实例仍被占；
  `21:13:33` 失败、`21:14:01/21:14:23` 仍占用（≥50 秒），`21:16:43` 自行恢复可绑。
- 旧日志缺失 PID/错误码/状态，无法回溯具体进程；这正是本次修复首先补齐的诊断能力。

## 修复

1. **诊断**：`process_identity_result()` 返回
   `ProcessIdentityError{NotFound | AccessDenied | Unavailable(code)}`；
   新增 `tcp_port_rows()` / `describe_port_blockers()`，用 `TCP_TABLE_OWNER_PID_ALL` 同时读取
   IPv4/IPv6 的**所有状态**（LISTEN、TIME_WAIT、ESTABLISHED…），把 PID、状态与
   “可读路径 / 不可读原因”写进日志和错误文案（`host:port` 形式，最多 6 条）。
2. **自动重试**：`ProxyService::schedule_pending_takeover_restore()` 记录有期限的接管意图
   （默认 15 分钟），5 秒监听守护新增 `retry_pending_takeover_restore()`；启动恢复遇到
   端口占用类失败时不再永久放弃，端口一释放就自动完成 `set_takeover_for_app(true)`。
   只有 `is_port_ownership_guard_error()` 认定的端口占用失败会进入重试，其它失败仍一次性报错。
3. **提示收敛**：新增 i18n `notifications.recovery.nextStep.retryingTakeoverRestore`
   （zh/zh-TW/ja/en）；恢复成功后后端发 `recovery-outcome-resolved` 事件，前端收起
   `startupTakeoverFailed` / `portOwnedByUnknownOwner` 提示。
4. **乱码修复**：`disable_takeover_for_app_after_switch_lock` 里 6 条 UTF-8→GBK 双重编码的
   错误文案（`鑾峰彇 … 澶辫触`）恢复为正常中文。
5. **边界不变**：仍然 fail-closed —— 不结束、不接管无法核验的进程；只是把原因讲清楚并自动重试。

## 验证

- 新增回归：`pending_takeover_restore_recovers_once_the_blocking_port_frees_up`
  （真实占住配置端口 → 启动失败且错误带 `PORT_OWNERSHIP_GUARD` → 占用释放后自动恢复
  `is_running()==true` 且 `takeover.codex==true`）、
  `pending_takeover_restore_keeps_one_bounded_intent`、
  `port_ownership_guard_errors_are_distinguishable_from_other_failures`、
  `port_blocker_summary_*`、`live_port_rows_describe_a_listening_socket`、
  `resolving_port_busy_outcomes_only_acknowledges_that_apps_port_failures`。
- `cargo test --lib services::proxy::` 102 passed；`services::recovery_outcome::` 7 passed；
  `process_identity::` 11 passed；`cargo fmt --check`、`pnpm typecheck`、Prettier、
  `pnpm run test:unit`（194 files / 1583 tests，仅既有失败
  `tests/components/AddProviderDialog.test.tsx::普通 Codex 新增没有 receipt 时…`，
  已在 detached HEAD 干净 worktree 复现为既有失败）。
- 本轮没有安装、停止、重启或替换本机 CCSM（当前 Codex 会话依赖 `127.0.0.1:15721`）；
  安装态是否真的自动恢复仍需新版本安装后由真实覆盖安装/重启触发验收。

## 后续待办（运行态）

- 安装带本修复的版本后，复现覆盖安装并观察日志中新的
  `端口 <port> 占用诊断：LISTEN ipv4 127.0.0.1:<port> pid=<pid> …`：
  若占用者仍是别的账户进程，应据 PID 定位该外部程序；若诊断为“TCP 表里没有该端口的任何条目”，
  则要转向 Windows 端口保留（`netsh int ipv4 show excludedportrange`）与 portproxy 方向继续排查。

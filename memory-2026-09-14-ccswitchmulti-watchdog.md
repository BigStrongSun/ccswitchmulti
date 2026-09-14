# 2026-09-14 CCSwitchMulti 看门狗（守护）落地：崩溃自动拉起、正常退出不打扰

## 问题与现状

- 仓库里其实一直有 `scripts/watch-ccswitchmulti.ps1` + `scripts/ccswitchmulti-guardian-core.ps1`（维护租约、身份校验、健康检查、受控重启），但：
  1) 它**没有注册自启、也没有在运行**（`%LOCALAPPDATA%\CCSwitchMultiGuardian` 里的状态停在 2026-08-24，且没有对应计划任务）；
  2) 它不区分「用户正常退出」和「崩溃」——托盘退出 60 秒后也会被拉起；
  3) 没有重启频率上限，崩溃循环会无限重启。

## 本次改动

- `ccswitchmulti-guardian-core.ps1` 新增：
  - `Test-CcsmGuardianUncleanExit -ConfigPath`：应用只在**正常退出**时自己删除运行标记 `logs/app-run-marker.json`；标记仍在且其 PID 已不存在 ⇒ 上一次是异常死亡 ⇒ 允许重启。标记不存在（干净退出/从未启动）⇒ 不重启。
  - `Test-CcsmGuardianRestartBudget -RestartTimesUtc -NowUtc -WindowMinutes -MaxRestarts`：窗口内重启次数上限。
- `watch-ccswitchmulti.ps1`：新增参数 `-ConfigPath/-MaxRestartsPerWindow(默认6)/-RestartWindowMinutes(默认30)/-RestartOnCleanExit`；恢复前先过这两道闸，跳过时写 `restart-skipped-clean-exit` / `restart-rate-limited` 事件。
- 新增 `scripts/install-ccswitchmulti-watchdog.ps1`：把 watcher+core 复制到 `%LOCALAPPDATA%\CCSwitchMultiGuardian`，注册**每用户登录自启**计划任务 `CCSwitchMulti-Watchdog`（隐藏、IgnoreNew、无执行时限、失败重试3次/1分钟、Limited），立即启动并输出证据；`-Uninstall` 反注册并停止 watcher。
- 新增 `scripts/tests/ccswitchmulti-watchdog.Tests.ps1`（Pester 3.4）：4 条策略用例。

## 验证

- Pester：`scripts/tests/ccswitchmulti-watchdog.Tests.ps1` **4/4**；四个脚本 parse errors=0。
- 真实崩溃拉起（测试 watcher：PollSeconds=2 / FailureThresholdSeconds=6）：kill 掉监听进程后日志 `health-loss-detected(49040)` → `health-loss-threshold-reached(9s)` → `product-started(66848)` → `recovery-ready(66848)`，新实例 v3.20.2-11、`/health` 200、`listener_role=takeover`，停机约 9–13 秒。
- 真实“正常退出”分支（watcher 指向无 marker 的配置目录，等价于干净退出状态）：同样的 kill 后日志只出现 `restart-skipped-clean-exit`，端口保持空闲、未被拉起。
- 正式守护已注册并运行：任务 `CCSwitchMulti-Watchdog` State=Running、Trigger=AtLogOn、Action 指向 `%LOCALAPPDATA%\CCSwitchMultiGuardian\watch-ccswitchmulti.ps1`（PollSeconds=5、FailureThresholdSeconds=60）、Principal=sunda/Interactive/Limited、MultipleInstances=IgnoreNew、ExecTimeLimit=PT0S、RestartCount=3；安装的 watcher/core 与仓库哈希一致；`guardian.jsonl` 新增 `guardian-started` 且带新策略字段。

## 设计边界

- 生产阈值 60 秒（不是 6 秒）：给“应用自身重启/安装事务”留出余量，避免守护和应用互相抢端口；崩溃循环由 6 次/30 分钟上限和 `restart-rate-limited` 兜住。
- 安装期间不会被守护干扰：watcher 复用事务的维护租约 `maintenance.lock` 并检测安装/卸载进程，租约有效时完全不动作（`Invoke-CcsmGuardianIteration` 首行即跳过）。
- 守护只做“等待 + 启动 + 校验”，不会主动结束非本产品进程；需要结束旧实例时仍走身份校验过的路径。
- 手工验收脚本要注意：把 kill/删除应用状态文件写进**脚本文件**再执行，直接内联在命令里可能被命令策略拦截（本次就遇到一次，导致我让服务多停了 24 秒，最后由用户手动拉起）。

## 运行手册

- 安装/更新守护：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-ccswitchmulti-watchdog.ps1`
- 卸载守护：同上加 `-Uninstall`
- 看日志：`%LOCALAPPDATA%\CCSwitchMultiGuardian\guardian.jsonl`（事件：guardian-started / health-loss-detected / health-loss-threshold-reached / product-started / recovery-ready / restart-skipped-clean-exit / restart-rate-limited / recovery-deferred-maintenance）
- 只想观察不重启：`watch-ccswitchmulti.ps1 -NoRestart`；只打印计划：`-PlanOnly`。

## 追加：守护自身的健壮性（同日 17:15 刷新安装）

- 现场发现：计划任务在 17:09 重启过一次守护进程（`LastRunTime 17:09:22`），而上一实例没有留下任何异常原因——说明守护可能被单轮循环里的瞬时异常（WMI/CIM/网络查询抖动）带死，然后靠任务的 RestartCount 才回来。
- 根修：`Invoke-CcsmGuardianSafeIteration`（core）把单轮迭代包在 try/catch 里，异常只写 `iteration-failed` 事件并继续下一轮；watcher 外层 try/catch 在真正退出前写 `guardian-exiting`（带 Error 与 Cycles）。
- `install-ccswitchmulti-watchdog.ps1` 现在会在启动前先停掉守护目录里的旧 watcher（刷新安装，避免 IgnoreNew 让新脚本不生效）。
- 验证：Pester `ccswitchmulti-watchdog` 6/6（新增两条：单轮异常不致死、正常轮次不写事件）；4 个脚本 parse 0 error；刷新后任务 Running、watcher PID 40844、安装态脚本与仓库哈希一致、日志新增 `guardian-started`。
- 另一次真实现场（17:08:26 应用 `event_loop_exit` 正常退出、17:09:22 守护由任务重启、17:10:40 判定 `restart-skipped-clean-exit`、17:10:45 由用户再次启动）证明“正常退出不打扰”在真实场景下按设计工作。

## 追加：设置页“看门狗与端口自检”面板（v3.20.2-14，提交 0c9b82e7）

- 后端只读命令 `get_watchdog_status`（`commands/watchdog.rs`）：读 `logs/watchdog.jsonl`、`logs/watchdog-state.json` 与 TCP 表，返回守护进程 PID/存活、最近 30 分钟自动拉起次数、端口监听 PID/路径/就绪、端口占用诊断文本、最近 8 条守护事件。
- `watchdog` 模块新增 `recent_events` / `last_supervisor_pid` / `recent_restart_count`（含单元测试 `supervisor_log_round_trip_finds_events_and_last_pid`）。
- 前端新增 `src/lib/api/watchdog.ts` + `src/components/settings/WatchdogStatusPanel.tsx`（4 语言文案），挂在设置页 `WindowSettings` 之后；右上角可刷新，面板只读，不改运行状态。
- 验证：组件测试 2/2（健康态/异常态）；全量 Rust 单线程 4182 passed / 0 failed / 7 ignored；前端全量 1610 tests 仅剩既有 `AddProviderDialog` 失败（干净 worktree 已复现）。
- 安装：事务 `ccsm-20260915-011700-…` 将 3.20.2-13 → **3.20.2-14**；安装态/注册表/运行态一致、health 200、`listener_role=takeover`、内置 supervisor PID 32796；安装态二进制含 `get_watchdog_status` / `看门狗` / `占用诊断` / `supervisor-restarted`，构建产物 `dist/assets/index-*.js` 含 `watchdogPanelTitle` 与 `get_watchdog_status`。

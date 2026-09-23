# 2026-09-23 Codex app-server 非预期退出：证据与边界

## 只读现场

- `C:\Users\sunda\AppData\Local\Codex\Logs\2026\09\23\codex-desktop-757852ac-8fe9-4c5b-bd9f-c765e29c2c06-32636-t0-i1-083909-0.log` 使用 UTC。其第 1452 行记录上海时间 16:40:57 旧 app-server stdio 连接关闭，`code=3221225786`，最近 stderr 错误为 `codex_core::util: Custom tool call output is missing for call id`（完整 call ID 不写入项目记忆）。第 1457–1458 行明确标记 `classifiedAsExpected=false` 与 `fatal_error_broadcasted`；第 1461、1468–1471 行记录 Desktop 随即启动 PID 44652、初始化 app-server `0.155.0-alpha.16` 成功。旧任务 renderer 继续向新进程 steer 时的 `thread not found` / `unknown conversation` 是进程更替后的表现。
- 退出码 `3221225786 = 0xC000013A` 对应 Windows `STATUS_CONTROL_C_EXIT`。这是进程退出方式的重要线索，不能仅凭它判定哪个进程发送了控制事件；Desktop 的 “Most recent error” 是退出前最近的 stderr 错误，不能自动推为触发退出的因果原因。类似签名见 OpenAI Codex 官方仓库的 Windows issue #36778、#41988、#40400；它们是相似案例而非本机根因证明。
- `~/.codex/logs_2.sqlite`（只读 SQLite 查询）中，PID 42152 的记录覆盖 16:39:45–16:40:52；新 PID 44652 的该数据库记录从 16:43:36 开始。16:40:25 远程控制 WebSocket 报 `pong timeout`，紧接着有重连周期；16:40:52 有缺失工具输出错误。数据库本身没有退出码；退出码来自上述 Desktop 原始日志。WebSocket 超时与缺失工具输出可能相关，但现有时间顺序不足以判定因果。
- Windows Application 近 10 天没有 `codex.exe`/`ChatGPT.exe` 的 1000/1001/1026/1002 崩溃事件；`%LOCALAPPDATA%/CrashDumps` 和 WER archive 没有对应转储/报告。16:52 的 SCM 7034 实际指向 `codex-windows-sandbox-service.exe`（Sandbox Service），不是 app-server；17:17 的 Codex 包更新也晚于 16:40 事件，不能倒推为其原因。
- `~/.cc-switch/logs/cc-switch.log` 在 16:40 附近有 Codex 代理转发和被动会话用量同步；未见历史修复、关闭 Codex 或终止 app-server 的动作。因此当前证据不支持把 16:40 更替归因于 CCSM 历史修复或守护进程。
- 17:19:13 启动的当前 `codex.exe app-server` PID 9292 由 Codex Desktop `ChatGPT.exe` PID 25112 拉起，核查时仍存活。其启动在 17:17 包更新后，证明当前运行态不是 16:40 的旧进程。

## 尚缺的决定性证据

现有证据已确定 **旧 app-server 非预期退出、Windows 控制事件式退出码、最近内部错误以及 Desktop 自动重启成功**；但仍缺控制事件发送者、旧 PID 当时的父进程生命周期、进程级堆栈/转储或可重现触发序列。不能区分“缺工具输出引发退出”和“进程先被中断导致工具输出缺失”等路径，也不能把 WebSocket 超时、Sandbox Service 异常或 CCSM 代理动作直接归因于本次退出。下一次复现需只读、限定范围地捕获进程生命周期与控制事件来源；不要为制造证据而重启现有进程。

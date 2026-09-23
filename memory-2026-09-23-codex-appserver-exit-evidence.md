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

## 17:17 Codex 更新后的只读复核

- Windows AppModel Runtime/Admin 在本机时间 17:17:02 记录 `OpenAI.Codex` MSIX 从 `26.917.6896.0` 切换到 `26.917.8451.0`，17:19:08 新包启动 Desktop PID 25112。`check_app_update` 返回内部应用版本 `26.917.62051`、build `10789`、prod 渠道 `up_to_date`；内部应用版本与 MSIX 包号不可混为一谈。
- 新 Desktop 的 app-server PID 9292 于 17:19:13 启动（PPID 25112），二进制为 `%LOCALAPPDATA%\OpenAI\Codex\bin\80f78947ad880e6e\codex.exe`，`--version` 为 `codex-cli 0.155.0-alpha.16.3`，SHA256 为 `A19F8F6C3C9DD5B71B6B1E3EB1EC55D75AAFB2FDFB686D9E1F7A5F47DB07D0D2`。16:40:57 旧 Desktop 日志所拉起的后端报告 `0.155.0-alpha.16`；旧二进制已不在原路径，不能再对它做哈希核对。
- 扫描 2026-09-23 的 Desktop 日志（`%LOCALAPPDATA%\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\Codex\Logs\2026\09\23`）共发现四次 `Codex CLI process exited classifiedAsExpected=false code=3221225786`：本机时间 16:17:34、16:33:37、16:40:57、16:57:10，**全部早于 17:17 更新**。其中 16:40 与 16:57 两次的最近 stderr 是缺失 custom-tool 输出；另两次最近 stderr 不是此错误，反证不能把该错误当成所有退出的必要条件。
- 截至本机约 18:00，更新后约 43 分钟的日志/进程观测没有同退出码或缺输出错误，17:19 启动的 PID 9292 仍存活。这支持“新版本在当前短窗口内尚未复现”，**不支持“官方已确认修复”或“长期稳定”**。日志保留范围及工作负载未作等量控制，不能由零事件计算修复率。
- 2026-09-23 查阅 OpenAI 官方 GitHub issue [#36778](https://github.com/openai/codex/issues/36778)、[#40231](https://github.com/openai/codex/issues/40231)、[#41988](https://github.com/openai/codex/issues/41988)，均仍显示 Open，未找到与本机新包/后端修订明确对应的修复声明；官方 Codex changelog 也未找到该退出码的修复条目。Codex 内置 Web 搜索发现相似案例；独立 Matrix 搜索没有找到相关权威修复结果。以上是当前检索结果，不等于证明官方没有私下修复。

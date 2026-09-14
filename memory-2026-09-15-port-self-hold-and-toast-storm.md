# 2026-09-15 “端口一直被自己占着、提示连环弹”根修（v3.20.2-13）

## 症状

- 界面反复弹“无法安全解除端口占用 / PORT_OWNERSHIP_GUARD”与“代理端口被身份不明的程序占用”。
- 诊断里占用者其实是**本进程自己**：`pid=<自身>`、路径可读，而且只有 `ESTABLISHED` 没有 `LISTEN`。

## 根因（实测）

- `ProxyServer::stop()` 只关闭监听 socket，**已接受的连接任务从未被中止**（keep-alive/流式连接继续持有本地端口）。Windows 下紧接着的重绑定直接 `10048`。
- 复位/重试循环每 5 秒调一次 `set_takeover_for_app`，每次失败都**新写一条未确认的恢复结果** → 前端为每条结果弹一个提示，于是“一直弹”。
- 另一处判定缺陷：残留监听的 owner PID 已消失时，`OpenProcess` 可能返回 `Win32 31 (GEN_FAILURE)` 而不是 `87`，导致按错误码判定的“释放残留监听”分支不触发（00:01 那次就是这样漏掉的）。

## 修复（提交 45fc0ab4，版本 3.20.2-13）

1. `stop()` 中止所有在途连接任务（记录数量），端口立刻真正释放，可立即重绑；新增回归 `restart_rebinds_while_previous_connections_still_open`（保持客户端连接不关，stop 后同端口 start 必须成功）。
2. 端口只被本进程残留连接占用时，按“不是外部程序”处理：不报错误提示、不写恢复结果，只等待并重试。
3. 相同（operation + kind + appType）的未确认恢复结果在**同一代际**内去重，杜绝提示堆叠；新增测试 `repeated_identical_failures_do_not_stack_duplicate_outcomes`。
4. 残留监听改按“PID 是否还存在”判定（`process_exists`，ToolHelp 快照），不再依赖 Win32 错误码；新增 `process_exists_matches_reality`、`port_rows_are_attributed_to_the_listening_process`。
5. 监听 socket 句柄显式清掉继承位（`harden_socket_handle_not_inheritable`），从源头减少“已死 PID 的 LISTEN 行”。

## 验证

- 全量 Rust 单线程 4180 passed / 0 failed / 7 ignored。
- 安装 v3.20.2-13（事务 `ccsm-20260915-004027-…`：3.20.2-12 → 3.20.2-13）后实测：kill 主进程 → 内置 supervisor 1.1 秒内拉起（`supervisor-restart-ready`，新 PID 33932），应用日志只有 `SRV-001 启动` + `已恢复 codex 的代理接管状态`，**没有** 任何 `PORT_OWNERSHIP_GUARD`。
- 历史遗留：`recovery-outcomes.json` 里 v3.20.2-13 之前生成的 7 条重复 `portOwnedByUnknownOwner` + 1 条 `startupTakeoverFailed` + 2 条测试导致的 `uncleanExit` 已标记为已确认（备份 `recovery-outcomes.backup-before-stale-cleanup.json`），否则界面仍会把这些旧条目逐个弹出来。

## 运行手册

- 看端口真实占用：应用日志里的 `端口 15721 占用诊断`（PID/状态/路径或不可读原因）以及 `netstat -ano | findstr 15721`。
- 看到“只被本进程的残留连接占用”说明是应用自己的旧连接，修复后应在数秒内自动恢复；不需要手动杀进程。

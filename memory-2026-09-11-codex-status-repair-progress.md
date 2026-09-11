# 2026-09-11 Codex 状态与修复进度/卡死检测根修

## 症状

- Codex“状态与修复”面板进入刷新后只显示阶段列表，没有具体修复日志、文件进度、最后活动时间。
- 后端长时间无返回时，前端仍显示“正在刷新”；即使实际上任务/事件链已经卡死，也没有超时或失败反馈。

## 根因

1. `codex-runtime-refresh-progress` 只携带 `stage`。长操作（等待 Codex 退出、分页历史修复、新运行态验证）没有心跳，也没有步骤级日志。
2. `repair_paginated_history_after_codex_exit` 没有超时，也没有文件/迁移游标进度；底层文件或 SQLite 卡住时 `spawn_blocking` 可以让 Tauri 命令永久不返回。
3. 前端没有“事件静默”看门狗；`invoke` Promise 挂起时状态永远停在 `refreshing`。即使未来加上失败标记，迟到的后端结果也会覆盖失败状态。
4. 事件缺少序号和时间戳，迟到/乱序事件可能覆盖更新状态。

## 修复

- `CodexRuntimeRefreshProgress` 增加 `kind`（stage/log/heartbeat）、`sequence`、`emittedAtMs`、`code`、`message`；事件发送器统一分配序号和时间。
- 长操作通过 `await_with_heartbeat` 每 2 秒发送心跳；关闭、等待、强制结束、历史修复、配置应用、启动、验证都发送步骤级日志。
- 分页历史修复增加 `PlanScanStarted/PlanReady/ProviderMigration*/RepairFile*` 进度事件；修复本身增加 15 分钟硬超时，超时返回 `codex_paginated_history_repair_timed_out`。
- 前端新增 `logs/lastProgressAt/stageStartedAt`，在刷新、完成、失败界面展示日志和“最后活动/当前阶段已等待”。
- 前端增加 30 秒无事件看门狗，超时标记失败；每次刷新分配 run id，迟到结果不会覆盖失败状态。
- 增加中文/繁体/日文/英文进度日志文案。

## 验证

- TDD：后端 heartbeat 测试先 RED 后 GREEN；前端日志渲染、事件收集、静默看门狗和迟到结果忽略测试均通过。
- 前端全量：189 files / 1566 tests 通过；typecheck、Prettier 通过。
- Rust：聚焦 `codex_runtime_refresh` 36/36、`paginated_history` 23/23；全量串行 4113 passed / 0 failed / 7 ignored；`cargo check --all-targets`、rustfmt 通过。
- 已知 `codex_config_consistency::apply_ccsm_uses_compare_and_swap_and_creates_a_drift_backup` 在并行全量下受共享测试环境干扰，单独运行和串行全量均通过。

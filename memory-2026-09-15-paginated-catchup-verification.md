# 2026-09-15 「验证新运行态」死等 450 秒后失败：追平判据不可达

## 现象

用户点「备份、修复并重新打开」：关 Codex、修历史、重投影配置、重启 Codex 全部成功，
最后一步「验证新运行态」一直不通过，`当前阶段已等待 450 秒` 后弹
`Codex 状态刷新失败`，错误码 `codex_paginated_history_projection_not_caught_up`。

## 证据（真实数据，只读）

1. 恢复快照 `~/.cc-switch/backups/codex-paginated-history-migration-recovery-v1/20260915_105948_31320_463675500/cursor-repairs.json`
   记录 **1114** 条游标修复（oldOffset → newOffset，expectedOrdinal）。
2. 复核现网投影库：这 1114 条的游标 **1114/1114** 都 ≥ 各自的 newOffset（修复生效）。
3. 现网仍有 18 条“游标落在记录中间”的线程，但**都不在修复集合里**（属于之前被
   fail-closed 跳过的那批），说明失败不是修复没做，而是校验判据本身。
4. 日志 `11:00:01 Rewound 1 later Codex paginated-history projection cursor(s) during verification`
   —— 验证轮询里还在写 Codex 的投影库。

## 根因

`repaired_projections_caught_up` 要求每个被修复游标满足
`next_offset >= 修复当时的文件长度` **且** `next_ordinal >= 最后序号 + 1`，
即“Codex 必须把这些历史全部物化到文件末尾”。Codex Desktop 只在打开/继续任务时
物化历史，1114 条老线程绝大多数永远不会被触碰 —— 条件不可达，只能等到
`runtime_verification_timeout`（按最大字节数算，现场 450 秒）超时。
同时 `verify_fresh_runtime` 每轮都调用 `repair_newly_stalled_projection_cursors`，
在 Codex 运行时反复写它的投影库，和 Codex 自己的物化互相覆盖。

## 修复（v3.20.2-18，提交 8a74d747）

- `codex_paginated_history_repair.rs`：新增
  `RepairedProjectionStatus { damaged, pending }` 与可测的
  `repaired_projection_status_at(projection_db, outcome)`：
  - `damaged` = 游标仍落在记录内部（`offset-1` 不是换行，且不在 0/EOF）→ 仍然 fail-closed；
  - `pending` = 游标是合法记录边界、只是还没被 Codex 物化到 EOF → 正常，不阻塞；
  - `repaired_projections_caught_up` 现在只看 `damaged == 0`。
- `codex_runtime_refresh.rs`：pending 只记一条 info 日志；
  follow-up 补写每轮验证最多执行一次。
- 确认页（`CodexConfigConsistencyDialog`）：顶部先给现状结论
  （需要修复 N 个 / 没有需要修复、N 个保持原样 / 没有发现需要修复），
  按钮随现状改名（无可修复项 →「重新应用配置并打开」），原泛化段落改为
  「修复范围与边界」小节。i18n zh/zh-TW/ja/en 各新增 6 个 key。

## 验证

- 新增回归：`repaired_cursor_on_a_record_boundary_counts_as_caught_up_pending_lazy_materialization`
  （合法边界 → pending、`is_caught_up() == true`）与
  `repaired_cursor_inside_a_record_is_still_damaged`（记录内部 + 缺行 → damaged）。
- `cargo test --lib paginated_history` 29 passed / 1 ignored；`cargo fmt`；
  `vitest CodexConfigConsistencyDialog` 15 passed；`tsc --noEmit`、Prettier 通过。

## 教训

凡是“等外部系统异步完成某件事”的校验，判据必须是**可达成**的：
这里真正要保证的是「游标不再是损坏形态（不在记录中间）」，而“Codex 把每条历史都
物化到末尾”只是 Codex 的懒加载策略，不该当成硬门禁。否则会变成用户看得见的
“流程卡死 450 秒然后失败”，而待验证的修复其实早就生效了。

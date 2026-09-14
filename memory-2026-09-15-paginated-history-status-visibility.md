# 2026-09-15 分页历史状态面板：说清“需要处理什么”，并找回可见性修复备份世代

## 现象

用户截图：开机即弹出「Codex 状态与修复」，

- 配置与运行时 → 正常
- 分页历史 → **需要处理**，说明只有一行 `另有非单纯重复序号的历史文件将保持原样、不会自动修改：1152`
- 历史查询兼容层 → 重启后验证

问题：为什么每次开机都弹？分页历史的问题到底是什么？「需要处理」没写清要做什么，也看不到细节。

## 证据链（本机真实数据，只读）

1. `blocked` 的构成（`cargo test --lib real_history_blocked_reason_census -- --ignored --nocapture`，修复前）：
   `affected=0 blocked=1152`，其中 `provider_migration_cursor_mapping_missing=1114`，
   其余 `history_base_offset_not_record_boundary=11`、`provider_migration_history_base_mapping_missing=11`、
   `rollout_session_id_mismatch=10`、`unsafe_projection_duplicate_record=6`。
2. 逐会话对比 Codex `thread_history_1.sqlite` 的 `next_rollout_byte_offset`：1128 个会话的
   `offset-1` 不是换行（游标落在记录中间），`len(文件) - offset` 恒为 15 的倍数
   （607 个正好 15，220 个 30，……）。
3. 字节级 diff（`backups/codex-history-current-desktop-visibility-repair-v1/20260910_093845` 快照 vs 当前 rollout）：
   唯一差异是 `"model_provider":"openai"` → `"model_provider":"codex_model_router_v2"`，
   每条恰好 **+15 字节**；偏移倍数 15 由此得到解释。
4. 游标值精确等于该快照的文件长度，但 `codex_history_provider_migration_backup_parents()`
   的六个目录里**不含** `codex-history-current-desktop-visibility-repair-v1`。
   恢复流程 `mapped_offsets_for_cursor` 因此拿不到可核验映射，只能 fail-closed 成
   “保持原样”，且这个数量只增不减 → 每次开机重复弹窗。

结论：这不是 rollout 文件损坏，而是**旧 CCSM Provider 迁移改写了 rollout 但没有同步 Codex 投影游标的字节偏移**；
游标序号仍然正确，只是偏移短了 15×k 字节。Codex 之后从记录中间继续读，正是此前“历史缺失/投影停滞”的同一类根因。

## 根修

1. `codex_history_migration.rs`：`codex_history_provider_migration_backup_parents()` 增加
   `CURRENT_DESKTOP_HISTORY_REPAIR_NAME`。核验规则不变（逐记录只有 provider 字段差异、记录边界、
   `backup_end_ordinal + 1 == expected_ordinal`），证据不足仍然 fail-closed。真实数据只读预检：
   `affected 0 → 1114`，`blocked 1152 → 38`。修复动作仍然只改 `thread_history_1.sqlite` 的游标
   （事务 CAS + 边界后校验），不重写 rollout 字节。
2. `codex_paginated_history_repair.rs`：`PaginatedHistoryRepairPreflight` 新增
   `blocked_reason_groups`（code / detail / count / 最多 5 条示例）+ 分类器
   `classify_blocked_reason` + 分组 `group_blocked_reasons`，泄漏面从“一条原始报文”变成结构化原因。
   新增单测 3 个（分类、分组与示例上限、可见性世代回归）；`real_history_blocked_reason_census`
   作为 `#[ignore]` 本机诊断用例保留。
3. 前端 `CodexConfigConsistencyDialog`：分页历史一行拆成「需要修复 / 无需修复 / 正常」三态，
   blocked-only 用中性色并明说“不需要你处理”；新增「查看原因明细」（原因 / 数量 / 示例）与
   “修复会做什么”“什么情况才需要人工排查”的说明文案；`useCodexConfigConsistency` 的自动弹窗
   只由 `affectedRolloutCount > 0` 触发（blocked-only 不再每次开机弹），指纹去掉 blocked 字段。
4. i18n zh / zh-TW / ja / en 各新增 29 个 key（含 20+ 条原因码解释）。

## 验证

- Rust：`paginated_history` 27 passed / 1 ignored；`cargo test --lib codex -- --test-threads=1`
  1721 passed / 0 failed；`cargo fmt` 通过。
- 前端：`npx tsc --noEmit`、Prettier、`vitest CodexConfigConsistencyDialog + useCodexConfigConsistency`
  22 passed（含“blocked-only 不自动弹窗”的新回归）。
- 本机只读诊断（修复后）：`affected=1114 duplicate_ordinals=0 provider_cursors=1114 history_base=0 blocked=38`。
- 提交：`4c4c929d`（根修）、`1515573d`（v3.20.2-15 版本与发布说明）。

## 未做（需要用户自己点）

- 「备份、修复并重新打开」会关闭 Codex Desktop 与 app-server；本次工作就跑在 Codex 里，
  因此没有代用户执行这次写入式修复，也没有把“源码/单测通过”当成安装态修复证据。
- 剩余 38 个保持原样项：缺少可核验迁移前备份（父段引用 11+11）与真实身份异常
  （`rollout_session_id_mismatch` 10、`unsafe_projection_duplicate_record` 6），不猜着改写。

# 2026-09-12 Codex 分页历史 cursor 修复的全量 DB 备份根修

## 症状

- v3.20.2-5 的“Codex 状态与修复”在 11:37:01 完成历史扫描（5 个待修复文件、30 个迁移游标、6 个父段引用、1162 个 blocked），随后卡在 Provider 迁移恢复。
- 15 分钟后前端按新看门狗/超时逻辑报 `codex_paginated_history_repair_timed_out`，失败阶段为“重新打开 Codex”。
- 失败目录留下 0 字节 `projection.sqlite`，证明超时发生在 SQLite 全量备份阶段，而不是 30 条 cursor UPDATE。

## 根因

- `apply_plan` 在写任何修复前调用 `rusqlite::backup::Backup::run_to_completion(5, 25ms)`，把整个 `thread_history_1.sqlite` 复制到 recovery 目录。
- 当前投影库为 3.5 GB。该 API 的 5 页/25ms 节奏约 800 KB/s，3.5 GB 需要约 70 分钟，远超 15 分钟超时。
- 真正需要修改的只有 30 条 `thread_history_projection_state` 行。完整 DB 备份与修改规模完全不成比例，且 SQLite transaction + compare-and-set 本身已提供原子性。

## 修复

- 删除 `backup_projection_database` 和全量 `projection.sqlite` 复制。
- 改为在 recovery 目录写 `cursor-repairs.json`，只记录 `(sourceId, oldOffset, newOffset, expectedOrdinal)`，用于审计/人工恢复。
- 保留 JSONL history_base 文件备份和回滚；DB 更新继续使用单个 transaction、CAS 和 boundary postcheck。若 CAS 失败，仍恢复 JSONL 文件。
- 新增 TDD 回归 `apply_plan_records_cursor_snapshot_without_copying_projection_database`，先复现 `projection.sqlite` 被创建、snapshot 缺失，再转绿。

## 验证

- 聚焦：新增回归、cursor CAS 回滚、history_base 联合恢复、既有 backup 拒绝覆盖测试全部通过。
- `paginated_history` 24/24；`codex_runtime_refresh` 37/37。
- Rust 串行全量：4134 passed / 0 failed / 7 ignored；`cargo check --all-targets`、rustfmt 通过。
- C: 盘清理：移除 6 个 manifest 登记的 Cargo target 目录，约 130+ GiB，释放后 140 GB free；清理后空目录不影响后续构建。

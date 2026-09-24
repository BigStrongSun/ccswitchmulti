# 2026-09-23 分页历史可见性修复恢复可用

## 现场与根因

- Codex Desktop 在 16:40:57 的 app-server 退出后自动拉起；renderer 保留旧 resumed 状态并继续向新 app-server steer，产生 `thread not found` / `unknown conversation`。任务、rollout 和 worktree 均未丢失。
- `~/.cc-switch/logs/cc-switch.log` 在 16:27 后没有应用级动作，16:40 附近只有代理转发且请求返回 200；没有历史修复、关闭 Codex 或终止 app-server 的证据，因此不能把这次退出归因于 CCSM 历史修复流程。
- CCSM 的 `repair_codex_history_visibility_at` 在入口无条件执行 `ensure_legacy_history` / `ensure_legacy_db`，目录中只要存在任意分页历史，就连只更新 state DB、session index、global state 的安全可见性修复也整体失败。

## 根修边界

- 历史可见性入口改为逐 rollout 识别 envelope。legacy rollout 继续执行既有 Provider 状态改写；明确的 `history_mode=paginated` rollout 只读检查是否存在 Provider 差异，存在时计入 `paginated_rollout_provider_updates_skipped`，不备份、不重序列化、不改写任何 rollout 字节。
- 分页线程的 state DB Provider、索引、metadata、global state 和聚焦时间仍可按原事务/备份路径修复；恢复会话时由既有 Desktop `thread/resume` live Provider 兼容层覆盖旧 rollout Provider。
- 旧批量 Provider 迁移、直接 JSONL writer、压缩历史和未知/非法 envelope 的 fail-closed 保护保持不变；没有全局拆除 `codex_paginated_history_immutable`。
- 前端结果区新增“分页 Provider 原文保留”计数，并明确说明这不是整体失败。

## 验证

- 新增字节不变回归：分页 dry-run 成功，apply 更新 state DB Provider，rollout bytes 前后完全一致，legacy 路径仍由原测试覆盖。
- `cargo test --lib codex_history_migration -- --test-threads=1`：65 passed。
- `cargo test --lib paginated_history -- --test-threads=1`：29 passed / 1 ignored（真实目录只读诊断）。
- `cargo check --lib --no-default-features` 通过；前端组件测试 11 passed；`pnpm typecheck` 通过。
- 本轮未修改真实 `~/.codex` 历史、SQLite、配置和运行进程，未安装或重启应用。

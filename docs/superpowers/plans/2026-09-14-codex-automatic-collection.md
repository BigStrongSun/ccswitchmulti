# Codex Automatic Collection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Use red/green tests and explicit file ownership.

**Goal:** 在现有 Codex 多模型路由 → 状态 → 流量页面自动展示可追溯的主子会话用量及采集健康，不新增采集定时器或导航页。

**Architecture:** 复用现有 Rust 启动同步、60 秒循环和 session_sync_mutex。后台将 Codex 完整行增量解析、元数据、用量与版本化 checkpoint 原子落库；页面只查询 CCSM SQLite，由小型状态事件刷新缓存，不读取 Codex 历史文件。

**Tech Stack:** Rust / Tauri v2 / SQLite / React / TanStack Query / Vitest.

**Spec:** 根目录 memory-2026-09-14-codex-traffic-observability.md 的自动采集现状更正及本轮用户批准。

## Global Constraints
- 仅在 bigstrongsun/codex-traffic-observability 隔离 worktree 开发；保留主树所有无关修改。
- 不扫描或改写真实 Codex 历史来做测试；使用临时 fixture / 内存数据库。
- 不更改模型路由，不自动发起对照实验，不安装/重启当前 CCSM，不推送。
- 缺失用量是未知，不是零；proxy 请求账与 session 账不相加；普通 fork 不自动当 subagent。
- 原成熟 parser 的 last 优先、lane 签名去重、父继承剥离必须保留；费用传 inclusive input，DTO 展示 noncached input。
- UTF-8 无 BOM；每组关键修改单独本地提交，说明测试与边界，结尾为 本次提交由BigStrongsSun完成。

## Task 1 — Incremental ledger (Terra incremental_codex)
**Files:** src-tauri/src/services/session_usage_codex.rs (+ child modules), src-tauri/src/database/schema.rs, src-tauri/src/database/mod.rs.
**Interface:** schema v24, codex_usage_sessions(session_id, file_path, is_subagent, parent_thread_id, model, first_activity_at?, last_activity_at?, last_seen_at); session_id 对应 proxy_request_logs.session_id，时间为 Unix seconds。内部 checkpoint 存版本化状态和完整行 byte offset。
- [ ] Migration RED/GREEN：v23 升级及 fresh schema。
- [ ] full / append 共享逐行 parser，状态增长有界，继承关系不可猜测。
- [ ] usage / dedup / metadata / checkpoint 单事务；失败回滚且下次可重试。
- [ ] 追加、restart、空变化、半行/分裂 UTF8、截断/重写、归档、父晚到验证。非追加修改 fail-closed/deferred，不自动删除已验证历史账；需要既有受控 Codex 重建。
- [ ] 变化文件发现有界，保留周期补偿扫描；bytes-read 验证而非仅声称增量。
- [ ] focused Rust 测试与提交。

## Task 2 — Collector status and DB-only query (Terra collector_backend)
**Files:** services/session_collection.rs, services/session_usage.rs, services/usage_stats.rs, services/mod.rs, commands/usage.rs, lib.rs.
**Interface:** get_session_collection_status / session-collection-updated 共用 camelCase：revision, phase(not_started|idle|running|degraded), lastStartedAt?, lastCompletedAt?, lastSuccessAt?, imported, deferred, errorsCount, lastErrorSummary?, nextRunAt?, intervalSecs；所有时间 UTC Unix seconds。
- [ ] RED/GREEN：初始态、零新增成功、失败保留lastSuccess、并行手动同步串行化。
- [ ] 复用唯一后台 worker 及锁，手动/自动共享状态更新；事件失败不影响落库。
- [ ] nextRunAt 与实际调度一致；错误摘要不泄露路径或提示正文。
- [ ] 主子统计完全 DB-only，包括父关系；缺metadata时未知而非读侧扫描。
- [ ] coverage / parent overlap 边界测试及提交。

## Task 3 — Existing traffic UI (Terra collector_frontend)
**Files:** src/types/usage.ts, src/lib/api/usage.ts, src/lib/query/usage.ts, src/hooks/useUsageEventBridge.ts, src/components/codex/CodexSessionTrafficPanel.tsx, CodexRouterWorkspacePage.tsx and tests/fixture.
- [ ] RED/GREEN：异步订阅清理、revision乱序、零新增完成刷新、degraded、手动一次。
- [ ] 集中在已有 useUsageEventBridge，不新增sync timer；状态事件只刷新查询。
- [ ] 初次未运行不显示正常；展示最近成功、待处理、错误、立即同步。
- [ ] focused Vitest / tsc / formatting 及提交。

## Task 4 — Integration acceptance (main + independent Terra review)
- [ ] 独立审核计量/事务/父子归属/迁移/事件生命周期及待发现文件公平性。
- [ ] 独立运行 focused 前端、tsc、cargo tests(session_collection/session_usage_codex/usage_stats/schema)、cargo fmt、Vite build；逐命令核验exit code。
- [ ] 浏览器 fixture QA；临时文件和服务器只清理本任务资源。
- [ ] 更新项目 memory 与测试证据，记录已知全前端 baseline AddProviderDialog mock 失败及安装态未变。
- [ ] 本地提交文档并核对工作区状态；最终明确开发验证不等于已安装上线。

## Source validation
本轮已独立调用内置 web 与 Matrix；内置链无可读返回，不能声称双链确认。Matrix open 官方 Tauri calling-frontend 文档支持小型事件及异步unlisten清理设计。具体既有同步逻辑以本地源码为准，所有新计量语义以fixture验证。

## Review decisions during implementation
- last_seen_at 是采集时间，绝不作为使用发生时间；只有有效 token 事件提供 first/last_activity_at。区间两端跨越查询窗口也不能证明窗口内有事件。
- legacy 父会话可能包含旧错误子归属；没有完整归属版本证明时父直接用量返回 unknown_may_overlap，不把历史父行冒充纯主模型用量。
- 检测到文件截断/非追加重写只保留历史账并标待处理；清理日志不等于撤销已经消费的 Tokens，因此不自动删除proxy账或日汇总。
- 正常追加检测验证旧head窗口和cursor前tail；任意中段原地修改同时追加无法仅靠局部hash绝对识别，不宣称此能力。
- 原计划的有界发现补偿仅负责发现漏扫文件，不等于全文件hash审计。
- 首轮全量解析和checkpoint必须来自同一受限snapshot；usage/dedup/metadata/checkpoint同事务，任何插入错误不得吞掉后推进offset。

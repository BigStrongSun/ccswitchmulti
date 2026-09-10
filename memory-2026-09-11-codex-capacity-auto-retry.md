# Codex 模型容量错误自动续跑（2026-09-11）

## 用户目标

- CCSM 识别 Codex `model at capacity`、服务器过载和对应的 429/502/503/504，并在同一 Provider 上自动续跑。
- 该能力独立于普通自动故障转移，单 Provider 也生效；Codex 默认开启，并提供专用开关。
- 只有尚未向客户端交付正文、reasoning 增量或工具调用时才允许透明重放；额度、认证、策略和参数错误不得进入容量重试。
- 容量重试使用独立的 10 次预算，日志不得记录正文、密钥或完整错误体。

## 根因与协议边界

- OpenAI Codex 的 Responses SSE 会把 `server_is_overloaded` / `slow_down` 映射为 ServerOverloaded；社区也有 `model at capacity`、`high demand` 和重连耗尽报告。外部事实由 Codex Web Search 与 Matrix WebSearch 两条独立链核对，最终行为以官方源码、CCSM 本地源码和 RED→GREEN 测试为准。
- CCSM 原有 `prime_streaming_response()` 会把首个 chunk 中的 `event: error` / `response.failed` 提前转成 503，以便普通多 Provider failover。如果同一个首包先出现 `response.created` 再出现容量失败，这一步会发生在容量流 wrapper 之前；普通 failover 关闭或只有一个 Provider 时，专用容量开关因此失效。
- 根修让首包错误分类携带 `is_capacity`。容量开关开启时，明确容量终态保留为原始 SSE 并交给后续同 Provider 状态机；关闭时仍按旧契约转为 503。测试先稳定复现 `UpstreamError 503`，再修复为原 SSE byte-for-byte replay。
- 原生 Responses、Chat Completions→Responses、Anthropic Messages→Responses 三条流式链均在语义输出前识别容量终态。Chat 的 role/空 choices、Anthropic 的 `message_start`/`ping` 仅视为协议脚手架；一旦出现文本、reasoning、工具调用或终止语义，永久禁止重放。
- HTTP 429/502/503/504 在响应尚未提交前处理。502/503/504 即使 body 为空也进入容量预算；若结构化错误明确是 quota/billing/auth/permission/policy/invalid request/parameter，则拒绝重放。429 保留原有普通限流重试，并仅在明确容量语义时进入新的容量预算。

## 配置、迁移与 UI

- 数据库 schema v22→v23 新增 `proxy_config.capacity_retry_enabled INTEGER NOT NULL DEFAULT 0`；迁移只把 `app_type='codex'` 设为 1，fresh Codex seed 也是 1，其它 app 保持关闭。
- `AppProxyConfig`/TypeScript 类型、DAO、forwarder 和 handler 全链路传递该字段。
- Tauri 新增精确命令 `set_codex_capacity_retry_enabled`，只更新 Codex 的一个字段。前端 Codex-only 开关点击后立即持久化，不会把同一表单中尚未保存的 `maxRetries` 等值一起写回。
- Chat/Anthropic 转换 handler 使用 `CodexTransformUpstream` 把响应与对应重连器绑定，避免参数膨胀和错误配对；没有用 Clippy allow 掩盖接口问题。

## 验证证据

- TDD 红灯：首包 `response.created + response.failed(server_is_overloaded)` 原实现返回 503；bodyless 502 分类原实现为 false。
- Rust：`cargo test --lib capacity` 14/14；`cargo test --lib streaming_retry` 45/45；429 回归 3/3；首包旧行为、DAO 精确更新和 v22→v23 迁移均通过。
- 前端：`pnpm vitest run src/components/proxy/AutoFailoverConfigPanel.test.tsx` 2/2；`pnpm typecheck` 通过。
- 编译与静态检查：`cargo check --all-targets`、`cargo clippy --lib -- -D warnings`、rustfmt、Prettier、`git diff --check` 通过；18 个变更文件严格 UTF-8 解码、无 BOM、无 U+FFFD。
- `cargo clippy --all-targets -- -D warnings` 先发现本次新增 handler 参数过多并已根修；其后仍会被仓库既有 test-target Clippy 告警阻断（分布于 codex_config、timezone、provider_set、history、quota_collaboration 等未改文件），本任务不越界清理这些基线告警。

## 当前边界

- 隔离 worktree：`.worktrees/codex-capacity-auto-retry`；分支：`bigstrongsun/codex-capacity-auto-retry`；基线：`07417ec9`（v3.20.2-3）。
- 本轮只实现、验证和本地提交；未合入 main、未 push、未发布、未安装，也未停止或重启本机 CCSM/Codex。
- HTTP bodyless 502/503/504 被视为响应提交前的服务端/网关瞬态压力；若第三方网关用这些状态包装未结构化的永久业务错误，CCSM 无法可靠区分，只能在 10 次与总退避时间预算内有界重试。

## v22 真实升级启动失败与根修

- 首次从 `main@b648199e` 本地构建的 `3.20.2-3` 安装候选在真实 v22 数据库启动时弹出 `table proxy_config has no column named capacity_retry_enabled`。数据库保持 v22，用户随后恢复既有发布版；不能把 migration 单元测试通过等同于完整启动顺序通过。
- 根因是 `Database::init()` 先执行 `create_tables_on_conn()`，后执行 `apply_schema_migrations_on_conn()`。旧 v22 `proxy_config` 已有 `app_type`，所以当前 seed 分支会运行；但 Codex seed 直接引用尚未由 v22→v23 migration 添加的新列，启动在进入 migration 前失败。
- TDD 新回归按真实顺序执行 current table creation/seed 再执行 migration，并保留旧 Codex `max_retries=9`。旧实现精确 RED 为缺少 `capacity_retry_enabled`；最小修复让 Codex seed 按表的实际列能力选择 SQL，旧表不引用新列，正式 migration 随后添加列并设 Codex 为 1；fresh schema 仍直接 seed 为 1。
- 根修集中门禁：启动顺序回归 1/1、容量相关 15/15、数据库 schema 12/12、`cargo check --all-targets`、CI 同口径 `cargo clippy --lib -- -D warnings`、rustfmt、diff 与严格 UTF-8 校验通过。
- 首次安全替换事务在停止旧进程后的 run-marker 所有权检查失败，回滚验证又因 listener/health 超时报告 `RollbackFailed`；事务证据保存在 `%LOCALAPPDATA%\CCSwitchMultiTransactionBackups\ccsm-20260911-014024-24649599a9ac47cbb8f9d54e4fbb4c6a`。在迁移根修重新构建并完成真实安装验收前，不再使用该候选。

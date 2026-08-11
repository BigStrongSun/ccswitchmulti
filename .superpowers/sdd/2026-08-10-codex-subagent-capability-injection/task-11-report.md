# Task 11 整分支审查 GREEN 修复报告

## 结论

Task 10 的 10 条 Rust RED 契约与 4 条前端 RED 行为均已转为 GREEN。修复覆盖 canonical profile identity、early-return 前的字段校验、全 draft preview、事务内完整候选编译、强托管来源判定、backend-owned 初始化/目录同步/无效项批处理，以及 Wizard/MultiRouter 共用编辑器的后端结果采用。

数据库提交后的 live 投影失败现在返回显式 `projection.status = pending_retry`，不再把已成功的数据库写入伪装成整体失败。公开 warning 使用稳定、脱敏的 `code/message`；原始 `AppError` 不回显到 UI，本地日志也只记录错误类别，不记录 settings、任务文本、路径、HTTP body 或凭据值。

## 实现内容

### 1. Profile parser/compiler

- strict storage 强制 map key 等于 `normalize_profile_key(profile.model)`，错误码为 `profile_key_model_mismatch`。
- tolerant loader 保留 malformed raw entry，但 collision identity 优先使用 raw 中可提取的 canonical model；只有 model 不可提取时才回退到 raw key identity。
- description、developerInstructions 与 nickname candidates 的 trim/空值/字符集/去重校验移到 V1、disabled、unroutable 等 early return 之前。
- 生成的 description、developerInstructions 与 nickname 使用 trim 后值。
- Flash/Pro preset 均为 enabled + preferred；自动选择仍是 Codex semantic role best-effort，不构成硬路由保证。

### 2. Preview 与托管文件

- preview 将当前 profile 注入完整 `settingsConfig.codexRouting.subagentV2` draft，按 canonical model 替换旧 entry，复用共享 compiler、catalog、route classification、occupied role allocator，再按 canonical model 选回目标 role。
- preview 公共 payload shape 保持不变。
- marker ownership 改为 exact first-line marker + 合法 TOML + filename/name 一致 + 非空 model + CCSM provider。
- legacy ownership 需要完整旧生成签名：filename/name、model/provider、生成 description、developer instruction signature、nickname list 与 context window 全部匹配；provider/model 相同不再足以接管用户文件。

### 3. DAO、service 与 IPC

- DAO 使用 `TransactionBehavior::Immediate`，在同一事务内读取最新 Provider、合并 focused `subagentV2`、调用完整候选 validation closure，再提交。
- validation closure 不二次读取数据库，避免 latest-candidate 与验证对象分裂。
- 新增 backend-owned：
  - `initialize_codex_subagent_v2({ providerId })`
  - `reconcile_codex_subagent_v2_profiles({ providerId, action, subagentV2? })`
- catalog initialization/sync 为 Flash/Pro 建 enabled preferred preset，为其余可路由模型建 disabled generic draft；sync 使用前端当前未保存 draft。
- invalid batch remove/recover 由 backend 定位 raw entry；前端 request 不携带 raw invalid key。recover 仅按可提取且存在于可路由 catalog 的 canonical model 恢复，并保留 valid peers。
- mutation result 在原 Provider 顶层字段之外增加 `projection`。兼容测试证明既有 `Provider` consumer 可忽略新增字段直接反序列化。
- 投影 warning 为：
  - `codex_live_projection_pending_retry`
  - `codex_current_provider_lookup_pending_retry`
    UI 只展示固定 message；日志仅输出稳定 `error_kind`。

### 4. Shared editor

- 初始化、保存、目录同步、批量删除、批量恢复全部采用 backend 返回的 Provider，不再在前端构造 canonical initialization payload。
- questionnaire enum 做严格识别，malformed enum 进入可恢复 invalid 区域。
- 新增目录同步、删除全部无效配置、从模型目录恢复全部无效配置操作。
- 投影失败以非阻塞 warning 展示；数据库已保存状态仍被采用。
- Wizard 与 MultiRouter 继续复用同一个 `CodexSubagentProfileEditor` 与持久化源。

## RED → GREEN 证据

### Task 10 focused Rust

以下 10 个 exact test 已逐条复跑，全部 `1 passed; 0 failed`：

1. canonical key/model invariant
2. tolerant duplicate canonical model collision/redaction
3. whitespace-only nickname
4. description 四路 early-return 矩阵
5. developerInstructions 四路 early-return 矩阵
6. compiler-before-mutation
7. full-draft preview allocation
8. body-marker overwrite protection
9. body-marker prune protection
10. provider/model legacy lookalike protection

### 相关 Rust 回归

```powershell
cargo test --lib codex_subagent_profiles -- --nocapture
# 71 passed; 0 failed

cargo test --lib codex_config::tests::codex_subagent -- --nocapture
# 24 passed; 0 failed

cargo test --lib services::provider::tests::update_codex_subagent -- --nocapture
# 1 passed; 0 failed

cargo test --lib database::dao::providers -- --nocapture
# 11 passed; 0 failed

cargo test --lib codex_subagent_v2_mutation_result_keeps_provider_shape_and_redacts_projection_errors -- --nocapture
# 1 passed; 0 failed

cargo test --lib managed_agent_files_migrate_legacy_cc_switch_roles -- --nocapture
# 1 passed; 0 failed
```

`codex_config` 的 24 条中包含新增的 2 条真实 backend helper 测试，覆盖初始化/目录同步、批量删除/恢复、valid peer 保留、strict-storage 输出与 raw key redaction。

### 前端回归

```powershell
pnpm exec vitest run "src/components/codex/CodexSubagentV2ProfileEditor.test.tsx" --exclude ".worktrees/**" --reporter=basic
# 1 file passed; 93 passed; 0 failed
```

### 静态门禁

```powershell
pnpm typecheck
# exit 0

cargo check --lib
# exit 0；仅保留既有 openai_cache_read_tokens dead_code warning

rustfmt --edition 2021 --check src-tauri/src/codex_subagent_profiles.rs src-tauri/src/codex_config.rs src-tauri/src/database/dao/providers.rs src-tauri/src/services/provider/mod.rs src-tauri/src/commands/provider.rs src-tauri/src/lib.rs
# exit 0

pnpm exec prettier --check src/components/codex/CodexSubagentProfileEditor.tsx src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/lib/api/codexSubagentV2.ts
# All matched files use Prettier code style!

git diff --check
# exit 0
```

## 改动文件

- `src-tauri/src/codex_subagent_profiles.rs`
- `src-tauri/src/codex_config.rs`
- `src-tauri/src/database/dao/providers.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/commands/provider.rs`
- `src-tauri/src/lib.rs`
- `src/lib/api/codexSubagentV2.ts`
- `src/components/codex/CodexSubagentProfileEditor.tsx`
- `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`

`docs/superpowers/plans/2026-08-10-codex-subagent-capability-injection.md` 与 `memory.md` 的既有 dirty 修改未覆盖、未还原、未暂存。设计规范与本报告属于允许文档范围，随 Task 11 GREEN 提交。

## 自审

- 没有用前端第二套 compiler 或 normalization 替代 backend；初始化、sync、remove、recover 均以后端返回为准。
- preview 和真实文件物化共用 allocation；测试按 TOML model 定位实际文件，不依赖猜测文件名。
- 事务验证基于事务内 latest merged candidate；compiler rejection 的 before/after DB 内容一致。
- tolerant diagnostics 和 invalid action request 均不公开 raw key；恢复输出也验证不含 raw key sentinel。
- 托管接管只接受强 provenance；legacy migration fixture 已升级为真实旧生成签名，没有放宽生产判定。
- post-commit projection failure 为成功返回附带 retry 状态；warning payload 与日志均不包含原始错误正文。
- flattened Provider 为 additive shape；Rust 兼容测试验证现有 Provider consumer 忽略 `projection` 后仍能读取完整 Provider。
- 受保护的 implementation plan 与 `memory.md` 不在暂存范围；设计规范与 Task 11 报告按允许范围纳入。

## 联网检索与交叉验证

执行前已按项目要求完成两条独立搜索链：Codex 内置 Web 与 `matrix-websearch` 均直接核对官方 SQLite、rusqlite 与 Tauri 资料。

- SQLite 官方 transaction 文档确认 `BEGIN IMMEDIATE` 会立即启动 write transaction；本实现据此在读取 latest Provider 前取得写事务边界：<https://www.sqlite.org/lang_transaction.html>
- rusqlite 官方 `Transaction` 文档确认 transaction 未 commit 即 drop 时默认 rollback；与 DAO validation closure 返回错误时不提交的行为一致：<https://docs.rs/rusqlite/latest/rusqlite/struct.Transaction.html>
- Tauri 官方 command 文档确认 command 可使用 serde typed argument/result；新增 camelCase IPC 参数和 typed mutation result 与该边界一致：<https://v2.tauri.app/develop/calling-rust/>

两条搜索链结论一致，没有来源冲突。具体 API shape、SQLite 保存顺序、filesystem ownership 与 UI 行为最终均以当前源码及本报告列出的真实测试为准。

## 仍存在的关注点

- Codex semantic role 选择是运行时 best-effort，`preferred` 只能提高选择倾向，不能提供绝对硬路由保证。
- `cargo check/test` 仍报告仓库既有 `openai_cache_read_tokens` dead-code warning；与本任务无关，没有在本任务范围内顺手修改。
- Codex semantic role 选择仍由运行时基于 role description 做 best-effort 匹配；本轮没有、也不能把 `preferred` 解释成绝对硬路由保证。

## Task 11 round 1/5 整分支复审

### 本轮修复的根因

1. preview 以前把所选 profile 删除后追加到 ordered map 末尾，改变了 role allocator 的输入顺序。现在按 canonical identity 在原槽位替换，并把完整当前 draft 交给共享 compiler。
2. reconcile 的 remove/recover 分支曾从持久化 settings 重建，而不是使用调用方正在编辑的完整 draft；现在所有 action 都要求完整 draft，并原子持久化该候选。
3. 初始化曾把 catalog 中所有模型都建成 draft，且依赖 provider classification context 才判断可路由。现在只认显式启用的 exact/prefix/default route：Flash/Pro 为 enabled + preferred，其余可路由模型为 disabled。
4. strict/compiler 错误曾可能把原始 profile identity 或内部错误正文带到 IPC；现在只公开 allowlist code 和固定 message。
5. canonical alias recovery 曾按 catalog 新建 profile，丢失合法 raw profile 的字段；现在对结构完整且 model 可定位的 raw profile 直接 re-key，只有无可用结构时才回退到 catalog draft。
6. 前端恢复入口曾用本地估算 invalid/collision 数量，可能与 backend 诊断不一致；现在使用 backend authoritative count。
7. Wizard 在保存成功后刷新 Provider props 时会清掉 pending projection warning；现在用 persisted-key ref 保留同一保存结果的 warning。

### 首次 full-lib 的四个失败与 fixture 校正

首次默认线程 full-lib 为 `2938 passed / 4 failed / 2 ignored`。逐条读取失败断言、生产判定和旧 fixture 后确认这四项不是线程污染：

- `subagent_v1_prunes_only_ccswitch_managed_roles`
- `managed_agent_files_prune_stale_cc_switch_roles`
- `removing_model_catalog_prunes_managed_agents`

以上三个旧 fixture 只有首行 managed marker 与 `name/model`。真实 ownership 判定还要求 TOML 可解析、`name == filename stem`、model 非空，并且 `model_provider` 为 CCSM Router provider。缺 provider 的文件按安全契约必须被当作用户文件保留。本轮只给这三个 fixture 补 `model_provider = "codex_model_router_v2"`，没有放宽生产删除规则。

- `codex_takeover_catalog_projection_uses_db_provider_classification_context`

该 fixture 用 role 名 `reader` 作为 V2 profile map key；canonical schema 要求 map key 为 model identity `neutral-model`，展示 role 名属于 `overrides.roleName`。本轮在 `src-tauri/src/services/proxy.rs` 的唯一修改就是把测试 key 改为 `neutral-model` 并补 `overrides.roleName = "reader"`；它位于测试模块，没有修改 takeover、proxy forwarding、spawn schema 或生产 provider-classification 语义。

四个测试随后逐个以完整 module path 和 `--exact --nocapture` 复跑，全部 `1 passed; 0 failed`。

### 默认线程共享状态证据

fixture 校正后的 fresh 默认线程 full-lib 仍能复现一个独立失败：

```text
services::model_pricing::tests::creates_local_file_with_auto_sync_disabled_by_default
assertion failed: path.exists()
2941 passed / 1 failed / 2 ignored
```

根因是本轮新增的两个 provider service 测试调用 `with_test_home`，该 helper 修改进程级 `CC_SWITCH_TEST_HOME/HOME`，却遗漏了同模块其余同类测试使用的 `#[serial]`。定价测试先缓存 `model_pricing_file_path()`，之后 `get_models_dev_sync_state -> sync/load_or_create -> read/write` 会再次从全局环境解析路径；并行切换使断言路径与实际写入路径落到不同临时目录。只给这两个测试补 `#[serial]` 后，fresh 默认线程 full-lib 连续两次均为 `2942 passed / 0 failed / 2 ignored`。没有为环境性测试竞态修改生产逻辑。

### Round 1 新增回归证据

六个新 Rust regression 均以完整 module path、`--exact --nocapture` 独立通过：

1. `codex_subagent_v2_preview_preserves_selected_non_last_profile_allocation_order`
2. `codex_subagent_v2_initialization_only_includes_routable_catalog_models`
3. `codex_subagent_v2_strict_candidate_error_redacts_raw_profile_identity`
4. `codex_subagent_v2_backend_rekeys_a_structurally_valid_alias_without_losing_fields`
5. `reconcile_codex_subagent_v2_uses_the_complete_current_draft_for_batch_actions`
6. `update_codex_subagent_v2_service_error_redacts_raw_profile_identity`

前端新增四项行为覆盖 selected-slot ordering 对应操作、backend re-key recovery、backend invalid/collision count 与 persisted projection warning。完整 Vitest 为 `115 files / 920 tests / 0 failed`。

相关回归还包括：

- `codex_subagent_profiles`: `71/71`
- `codex_config::tests::codex_subagent`: `28/28`
- provider update: `2/2`
- provider reconcile: `1/1`
- provider DAO: `11/11`
- editor focused file: `95/95`
- `pnpm typecheck`: exit 0
- `cargo check --lib`: exit 0，仅既有 `openai_cache_read_tokens` warning
- 最终 fresh serial full-lib：`cargo test --lib -- --test-threads=1`，`2942 passed / 0 failed / 2 ignored`
- fixture 隔离修正后的默认线程 full-lib：连续两次 `2942 passed / 0 failed / 2 ignored`
- 完整前端 Vitest：`115 files / 920 tests / 0 failed`
- 最终 focused editor：`95/95`
- 最终 rustfmt、Prettier 与 `git diff --check`：exit 0

### Round 1 改动边界与自审

本轮 GREEN 源码范围只有：

- `src-tauri/src/codex_config.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/proxy.rs`（仅测试 fixture）
- `src/components/codex/CodexSubagentProfileEditor.tsx`
- `src/lib/api/codexSubagentV2.ts`
- `docs/superpowers/specs/2026-08-10-codex-subagent-capability-injection-design.md`

`docs/superpowers/plans/2026-08-10-codex-subagent-capability-injection.md` 与 `memory.md` 是进入本轮前已经存在的用户修改；未编辑、未还原、不会暂存。自审确认没有改变 proxy forwarding、spawn schema、Tauri command registration 或 runtime route matching；错误 payload 不回显 raw identity；reconcile 不再偷偷丢弃未保存 sibling；UI adoption 始终以后端 Provider 为准。

### 本轮联网交叉验证

Round 1 已分别使用 Codex 内置 Web 与 Matrix WebSearch 两条独立链，结论一致：

- serde_json `preserve_order` 下 map insertion order 可观察；`shift_remove` 保持其他 entry 相对顺序。<https://docs.rs/serde_json/latest/serde_json/map/struct.Map.html>
- React effect 会在 dependency 变化后重新执行，因此 Provider props refresh 必须显式保留同一持久化结果的 warning。<https://react.dev/reference/react/useEffect>
- Tauri command error 应使用可序列化、边界稳定且脱敏的公开错误。<https://v2.tauri.app/develop/calling-rust/>

关键 API 与实现行为最终以当前源码、RED/GREEN 回归和 full-suite 结果为准。两条搜索链没有关键事实冲突。

### 仍存在的不确定性

- semantic role 自动选择仍是 Codex runtime best-effort；本地 compiler 能保证 role 物化与偏好数据，不能保证每次任务都由指定 profile 命中。
- `openai_cache_read_tokens` dead-code warning 为仓库既有 warning，本轮未扩大范围处理。

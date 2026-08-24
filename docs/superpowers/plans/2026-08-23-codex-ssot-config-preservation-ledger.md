# 2026-08-23-codex-ssot-config-preservation

## 最新交接（2026-08-24）：Task 8/9 最终 Sol 审核收口

状态：`DONE`。替代验证 Sol 已按阶段 A、B、C 完成规格、代码质量、交叉测试与广泛回归，三阶段均为 `PASS`；独立最终 Sol 随后完成整体复审并允许进入 `sdd-finish`。最终审核记录为严重问题 0、重要问题 0、小问题 0、缺失材料 0。历史 `BLOCKED` 与 `DONE_WITH_CONCERNS` 记录继续保留在下文，作为问题发现、顺序回流和复审的审计历史。第三版的核心判断仍是兼容性设计：当前现场优先，只有能证明属于 CCSwitchMulti 的内容才认领；无法证明所有权时保留 listener/状态并停止接管。

### 最终 Sol 裁定

- Task 8 已裁定 `RESOLVED`：生产入口在覆盖旧 marker 前读取证据；Active、Planned、Confirmed、Unclean 分支均可从真实入口到达；outcome 先原子持久化再发事件，普通启动只清理旧的瞬态 outcome。
- Task 9 已裁定 `RESOLVED`：实例必须通过应用身份、合法 SemVer 同主版本和有效 PID 检查；无法证明兼容时进入 `UnknownOwner`，停止接管并保留真实 listener，不结束占用进程。
- typed receipt、force-repair deferred、MCP 数据库 ID 所有权、第 7 项 prefix-only 精确迁移、第 8 项 fail-closed 前后端一致性以及相邻 Tasks 14–19 均保持通过。
- 已批准的非目标继续是正式跨进程锁或 Codex `expectedVersion`、MCP provenance/冲突界面和动态未来 prefix；这些项目不阻塞本轮收尾。

### Task 8：启动恢复证据分类

- 生产入口先读取旧 marker、`clean_exit`、`panic` 和 `crash.log` mtime，完成分类后才写入当前 marker；不再固定传入 `planned=false`。
- 活跃旧 PID 优先返回 `ActivePreviousInstance`；panic/crash 晚于旧 marker 且晚于最新 clean exit 返回 `ConfirmedCrash`；clean exit 晚于旧 marker 且之后没有更新 panic/crash 返回 `PlannedRestartOrUpdate`；stale marker 无更新证据返回 `UncleanExit`；无旧 marker 返回 `NoPreviousRun`。
- Active/Planned 通过 `record_recovery_outcome` 原子持久化并发事件。Rust outcome、TypeScript 联合类型、四语言 i18n 和 `App.tsx` 已同步；Active 显示 warning，Planned 静默处理。

受控 RED：临时把生产分类固定为 Planned 后运行 `cargo test --lib app_exit_monitor --no-default-features -- --nocapture`，3 项失败（Active/Confirmed 被错误分类为 Planned），退出码 101。恢复真实启动时序后 GREEN 为 `app_exit_monitor 9 passed / 0 failed`。

### Task 9：端口所有权兼容性

- `/health` 与 `/status` 使用真实 HTTP 探测；兼容实例必须是允许的 CCSwitchMulti 别名、远端和本地版本均为合法 SemVer、major 相同且 PID 大于 0。
- `instance_id` 缺失仍兼容，空/纯空白值拒绝；预发布与 build metadata 后缀不影响 same-major 兼容。
- 跨 major、非法/未知版本、错误应用名、无效 PID、空白 `instance_id` 或未知所有者返回 `PORT_OWNERSHIP_GUARD` 前缀错误。启动恢复停止 Codex takeover，保留 listener 和状态，不做破坏性关闭清理。

受控 RED：临时恢复“版本非空即兼容”后运行 `cargo test --lib port_probe --no-default-features -- --nocapture`，跨 major 被错误判为 `CompatibleInstance`，退出码 101。恢复合法 SemVer 且 major 必须相同后 GREEN 为 `port_probe 4 passed / 0 failed`。

不兼容 listener E2E GREEN 为 `1 passed / 0 failed`：记录 `PortOwnedByUnknownOwner`，不启用 Codex takeover，本实例不绑定端口，原 listener 仍能响应，证明没有杀掉原进程。

### Task 8/9 最终验证数字

| 验证门 | Luna 实际结果 |
|---|---:|
| `app_exit_monitor` | 独立最终 Sol 复跑 `11 passed / 0 failed` |
| `recovery_outcome` | 独立最终 Sol 复跑 `4 passed / 0 failed` |
| `port_probe` | `4 passed / 0 failed` |
| 不兼容 listener E2E | `1 passed / 0 failed` |
| takeover 聚焦回归 | `1 passed / 0 failed` |
| 串行 Rust `cargo test --tests -- --test-threads=1` | 主智能体最新复跑：library `3375 passed / 0 failed / 5 ignored`；所有 integration binaries PASS；MCP reconcile `11/11`、MCP commands `23/23`、import/export `24/24`、provider commands `10/10`、provider service `37/37`、profile roundtrip `8/8` |
| `cargo check --tests` | PASS |
| 前端 Vitest | 主智能体最新复跑：`145 files / 1181 tests / 0 failed` |
| TypeScript typecheck | PASS |
| renderer build | PASS，3342 modules |
| Rust fmt | PASS |
| `git diff --check` | PASS |

### 实际产品与测试文件

产品实现文件：

```text
src-tauri/Cargo.toml
src-tauri/Cargo.lock
src-tauri/src/app_exit_monitor.rs
src-tauri/src/lib.rs
src-tauri/src/services/proxy.rs
src-tauri/src/services/recovery_outcome.rs
src/lib/api/settings.ts
src/App.tsx
src/i18n/locales/en.json
src/i18n/locales/ja.json
src/i18n/locales/zh.json
src/i18n/locales/zh-TW.json
```

Task 8/9 测试文件：

```text
src-tauri/src/app_exit_monitor.rs
src-tauri/src/services/recovery_outcome.rs
src-tauri/src/services/proxy.rs
```

其中端口不兼容 listener E2E 回归位于 `src-tauri/src/services/proxy.rs` 的隔离 listener 测试；启动恢复与 outcome 回归位于对应 Rust 模块测试，未访问真实用户目录。

### 未解决项与最终裁定边界

- 新增 Task 8/9 当前只能标记为 `DONE_WITH_CONCERNS`，等待最终 Sol 独立复核；上述 GREEN 数字不能替代最终审核，也不能把既有 Sol `BLOCKED` 结论改写为 PASS。
- 同名 MCP 冲突的 provenance/界面提示、正式跨进程锁或 Codex `expectedVersion`、动态未来 prefix/上游发布顺序仍是明确非目标，不是本轮隐藏完成项。
- 前端全量格式检查如仍提示未修改的既有 `src/components/providers/forms/CodexFormFields.tsx`，应按格式基线记录，不得误报为 Task 8/9 功能回归。
- 本轮没有新增“用户点击 MCP 同步”按钮、Tauri command 或前端 API；MCP 对账继续由既有切换、保存和同步后置路径自动触发。

## 最新交接（2026-08-24）：Luna 第三版 provider MCP 来源边界收口

状态：`DONE_WITH_CONCERNS`。以下是 Luna 的实现与本地验证证据，待 Sol 独立复核；在复核完成前不宣称验证阶段通过。

- 通用 `merge_codex_provider_config_texts()` 撤销无条件 MCP 剥离，避免公共配置片段新增的 `[mcp_servers.*]` 被误删。
- 普通 `build_effective_settings_with_common_config()` 只对 provider 快照克隆剥离旧 MCP，再应用公共配置片段；数据库原始快照不变，live-only MCP 继续由 current live 与数据库所有权对账。
- 代理备份刷新使用保留 provider MCP 的专用构建路径，因为备份是显式恢复快照；正常 live 写入仍走默认剥离边界。
- provider projection writer 先与最新 live 合并，再应用 bearer token，兼容剥离后只剩 MCP 的旧 provider 配置。
- 定向验证：两个公共配置 MCP 回归各 `1/1`；`provider_commands` `10/10`。
- 最终 Rust 门：`cargo check --manifest-path src-tauri/Cargo.toml --tests` 通过；`cargo test --manifest-path src-tauri/Cargo.toml --lib --quiet` 为 `3342 passed / 0 failed / 5 ignored`；`git diff --check` 通过。

## 最新交接（2026-08-24）：Provider merge 入口 TOCTOU 修复

状态：`DONE_WITH_CONCERNS`。同一位 Luna 已完成替代 Sol 在 `sdd-verify` 阶段 A 报告的重要竞态修复，未提交、未推送，待同一替代验证 Sol 复审。

- 根因：provider 写入先在 writer 外读取 live 并完成 merge，之后才首次记录指纹。外部写若落在“merge 完成后、首次 fingerprint 前”，新 live 字节会被当作指纹基底确认，但旧候选仍会替换它，导致用户级 MCP、`desktop`、插件和未知用户表被覆盖。
- RED：`cargo test --lib write_codex_live_for_provider_preserves_change_after_provider_merge_before_writer_entry -- --nocapture` 退出码 1，`0 passed / 1 failed`，`external = true` 断言失败，确认旧候选覆盖外部更新。
- 修复：`write_codex_live_for_provider` 及 config-only provider 路径把 merge 放入 `write_codex_live_config_reconcile` 每轮 live 快照闭包；连续冲突每轮重新 merge，最多两次重试后返回 `ConcurrentModificationDeferred` 并保留最新字节。官方 auth+config 通过 `write_codex_auth_with_reconciled_config` 在解析失败、写入失败或并发超限时回滚旧 `auth.json`。代理快照/接管恢复入口同步复用 reconciled writer；入口前 merge helper 已移除。
- GREEN：provider 入口竞态聚焦测试 `1/1`；连续三次 merge 冲突并 auth 回滚 `1/1`；非法 live TOML 原 config/auth 字节保护 `1/1`。此前 MCP、代理、provider、导入导出和 fingerprint 门保持全绿。
- REFACTOR：本轮实际触及 `src-tauri/src/codex_config.rs`、`src-tauri/src/services/proxy.rs`；测试注入队列只在 `#[cfg(test)]` 下启用。未访问真实 `~/.codex`/`~/.agents`，未重启或结束 Codex。
- 当前验证：`cargo test --lib` 为 `3335 passed / 0 failed / 5 ignored`；provider library filter `1353/1353`；`services::proxy::tests` `90/90`；`--lib mcp` `68/68`；`mcp_codex_reconcile` `11/11`；`provider_service` `37/37`；`import_export_sync` `26/26`；`--lib fingerprint` `13/13`；`cargo check --lib`、`pnpm typecheck`、`git diff --check` 通过；前端 Vitest `144 files / 1177 tests` 全通过。
- 既有警告：`import_export_sync` 有一次 Windows 临时目录清理 `os error 32` 日志但测试仍 `26/26`；前端有既有 React `act(...)`、MSW 未匹配 Tauri command、jsdom Tauri window 警告；Rust `cargo fmt --check` 和前端 `format:check` 仅剩已记录的 HEAD 基线文件。
- 最终状态：`DONE_WITH_CONCERNS`。关注项只剩同名 MCP 冲突 UI/provenance 与正式跨进程锁，以及第 7/8 项后续规格化；不影响本轮兼容性修复，但必须由替代 Sol 完成复审后才能宣告最终审核通过。

## 最新交接（2026-08-24）：MultiRouter 变换入口 TOCTOU 与副作用回滚

状态：`DONE_WITH_CONCERNS`。替代 Sol 阶段 A 继续发现的两条变换入口竞态已修复；未提交、未推送，待同一替代验证 Sol 复审。

- 根因：`force_codex_builtin_openai_live_provider` 和 `publish_codex_multirouter_projection` 原先在 writer 外完成 force-transform/projection prepare，再进入首次 fingerprint。外部写若落在变换完成后、writer 入口前，旧候选会覆盖最新 live config。
- RED：`force_builtin_openai_preserves_change_after_transform_before_writer_entry` 与
  `multirouter_projection_preserves_change_after_transform_before_writer_entry` 均真实失败，
  退出时分别为 `0 passed / 1 failed`，断言 `external = true` 消失。
- 修复：force-transform 和 projection prepare 移入 `write_codex_live_config_reconcile` 每轮 live 快照闭包，
  冲突后从最新字节重建候选。新增 `CodexProjectionSideEffectsSnapshot`，快照 catalog、models cache、cache backup
  和 agents 目录现有文件；config reconcile 失败、非法 TOML 或并发超限时恢复原字节并删除本轮新增的 CCSM managed agent，
  成功时保留投影副作用。
- GREEN：两项入口竞态 `1/1`；projection conflict/invalid-live 回滚 `2/2`；MultiRouter projection `10/10`、
  mutation `7/7`、provider filter `1353/1353`、proxy `90/90`、MCP `68/68`、mcp_commands `23/23` 全绿。
- 全量门：`cargo test --lib` 为 `3334 passed / 0 failed / 5 ignored`；`cargo check --lib`、`pnpm typecheck`、
  `git diff --check` 通过。`cargo fmt --check` 仅剩四个既有 Rust 基线文件；`import_export_sync` 仍有一次既有 Windows
  TempHome `os error 32` 清理日志，前端仍有既有 React/MSW/jsdom 警告但无失败。
- 当前状态：`DONE_WITH_CONCERNS`。本轮没有新增“同步 MCP”按钮、Tauri command、前端 API 或 UI；继续交由同一替代 Sol 复审。

## 最新交接（2026-08-24）：MultiRouter agent 所有权回滚 A-3

状态：`DONE`（实现与本地验证）。替代 Sol 发现的第三轮问题已按既有 managed marker 所有权边界修复；未提交、未推送，等待同一替代验证 Sol 完成最终复审。managed marker 是已批准的现有所有权信号，本轮没有新增未裁定契约。

- 根因：`CodexProjectionSideEffectsSnapshot::capture` 曾快照 agents 目录所有普通文件，`restore` 再无条件回写；用户在变换与连续冲突期间编辑无 marker 的同名 agent 会被失败回滚覆盖。
- RED：临时恢复旧行为后，`multirouter_projection_conflict_preserves_user_agent_edit_and_owned_agent_rollback` 为 `0 passed / 1 failed`；实际回滚回到 `user original`，并发内容为 `user concurrent edit`。
- 修复：capture 只读取 `codex_agent_file_is_cc_switch_managed` 返回 true 的文件；用户文件仅用于占名判断，不捕获、不回写、不删除。restore 只恢复原 managed 文件，新建文件仍须 managed marker 才可删除。
- GREEN：A-3 `1/1`；projection 专项 `5/5`；managed-agent 专项 `4/4`；catalog/cache 与连续冲突回归保持通过。测试覆盖同名用户文件、原 managed 文件、projection 新建 managed 文件和非法 live TOML。
- 全量门：`cargo test --lib` 为 `3335 passed / 0 failed / 5 ignored`；provider `1353/1353`、proxy `90/90`、MCP `68/68`、MCP 对账 `11/11`、mcp_commands `23/23`、provider_service `37/37`、import/export `26/26` 全绿；`cargo check --lib`、`git diff --check` 通过。
- 当前状态：`DONE_WITH_CONCERNS`。唯一保留疑虑是 managed marker 本身仍是既有所有权信号，正式跨进程锁和冲突 UI/provenance 仍属后续规格项；本轮未新增按钮/API/UI。

计划名称：Codex 启动配置保护与恢复（Codex SSOT Config Preservation 扩展版）

规格：`docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`

计划：`docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`

当前阶段：世豪已批准第三版 MCP 补充规格，Luna 进入兼容性实现与自测

## 1. 用户确认记录

- 2026-08-23：世豪明确授权“同意修复”。
- 2026-08-23：世豪补充证据“SDD 插件之前装着，后来也需要重新点安装”，并授权把规格从单纯防空写扩展为完整的启动体验修复（A 数据防丢、B 启动自动恢复与可见反馈、C 深层兼容后置）。
- 2026-08-23：本文件只记录规格和计划阶段；实现代码仍需等待世豪批准第二版书面规格。
- 2026-08-23：世豪要求产品只在 `zhushihao/ccswitchmulti` fork 通过 GitHub Actions 自动发布；母仓库只接收新的、独立的 Issue 与 PR，不更新任何历史 PR。
- 2026-08-23：世豪要求每次开工前和提交上游 PR 前同步母仓库；fork 发布版本必须从母仓库最新稳定版本追加 `.1`、`.2` 等递增后缀。
- 2026-08-23：世豪先确认“可以按照这个来”，在收到第二版文档后继续补充 PR 与版本规则，并明确询问“为什么停下来了”；主智能体据此确认第二版进入实现。插件登记沿用规格默认的“检测后给修复按钮”，不进行静默自动登记。
- 2026-08-23：世豪明确要求 Luna 在同一实现轮次顺手修复后续 bug 审计中的六项问题；这些修复必须保留独立红绿测试和独立提交，不得混入配置恢复的上游 PR。
- 2026-08-23：世豪确认继续本任务，并明确要求把“用户级 Codex MCP 会被 CCSwitchMulti 覆盖”纳入同一轮解决；世豪同时确认前端没有、也不需要虚构一个“同步 MCP”按钮。
- 2026-08-23：第三版规格与计划（MCP 所有权与对账补充）已写入文档，初始状态为“待世豪批准”；后续世豪已明确批准第三版，Phase M 按兼容性契约进入实现。
- 2026-08-23：世豪明确“批准第三版”，并要求实现从本质上按兼容性设计处理；Luna 必须兼容用户级与数据库管理 MCP、旧格式与新格式、已有数据库、显式操作、自动触发路径和 Codex 并发写入。

## 2. 输入证据

- 探索报告：`docs/diagnostics/sdd-codex-startup-preference-reset-explore-2026-08-23.md`。
- 探索报告状态：`DONE_WITH_CONCERNS`。根因与最小复现已确认；写入竞争、插件登记、恢复边界和用户反馈仍有遗留关注点，这些关注点已进入第二版规格。
- 最小复现：临时 HOME、内存数据库、schema-v2 MultiRouter、`config=None`、无备份、live 含 `[desktop]` max 和 `[plugins."sdd@personal"]`。
- 复现结果：连续两次失败；恢复后 `config.toml` 长度为 0；`has_desktop=false`；`has_max=false`；`has_sdd=false`。
- 扩展证据要点：
  - `app-run-marker.json` 只能证明“没有干净退出记录”，不能直接证明崩溃；托盘常驻、关机、强制结束都会留下同样现场。
  - 15721 当前由本机 `cc-switch.exe`（PID 3420）监听；18:39 的占用者极可能是旧或并发 CCSM 实例，但没有精确 PID 证据。
  - CCSM 与 Codex Desktop 都会写 `config.toml`；时序接近和 last-writer-wins 风险已证实存在，但尚未捕获到一次具体写竞争。
  - SDD 安装状态涉及 cache、`config.toml` 和 `marketplace.json` 三个数据源；marketplace 缺 SDD 条目是“显示未安装”的直接原因。
  - 0 字节或损坏配置未必能恢复用户表；当前恢复链主要恢复供应商路由、模型目录和少量共同偏好。
  - 启动恢复结果目前只写日志，没有用户可见反馈。
- 第三版新增输入：`docs/diagnostics/sdd-codex-user-mcp-overwrite-explore-2026-08-23.md`（状态 `DONE_WITH_CONCERNS`）。要点：
  - 用户级 MCP 覆盖在 `origin/main`（`9b0fd548`）已存在，不是当前未提交分支的回归；`mcp/codex.rs`、`services/mcp.rs`、`database/dao/mcp.rs`、`import_export_sync.rs` 与 `origin/main` 完全一致。
  - 覆盖点是 `sync_enabled_to_codex` 的整表替换语义；单条路径语义与之矛盾但行为正确。
  - 七个触发路径（供应商切换/保存、接管切回、设置保存、SQL/云同步后置、会话开关、通用配置片段保存）都汇聚到整表投影；启动恢复当前不直接调用它。
  - 数据库和 `McpServer` 结构没有所有权字段；两个既有测试把删除用户级 MCP 固定为成功预期。

## 3. 基线测试

运行目录：`D:\CCSwitchMulti\ccswitchmulti\src-tauri`

| 时间 | 命令 | 结果 |
|---|---|---|
| 2026-08-23 | `cargo test --lib merge_provider_config_preserves_live_user_sections -- --nocapture` | PASS，1 passed，3288 filtered out |
| 2026-08-23 | `cargo test --lib codex_restore_from_backup_preserves_live_desktop_settings -- --nocapture` | PASS，1 passed，3298 filtered out |
| 2026-08-23 | `cargo test --lib restore_falls_through_to_ssot_when_backup_is_proxy_placeholder -- --nocapture` | PASS，1 passed，3298 filtered out |
| 2026-08-23 | `cargo test --lib merge_empty_official_config_clears_provider_fields_but_keeps_user_sections -- --nocapture` | PASS，1 passed，3298 filtered out |

既有失败记录：本阶段没有发现新的既有失败。上述四条基线全部通过。

既有失败记录（第三版补充，证据来自 MCP 探索报告）：

| 时间 | 命令 | 结果 |
|---|---|---|
| 2026-08-23 | `cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync sync_enabled_to_codex -- --nocapture`（worktree `codex-startup-config-preservation`） | FAIL，退出码 101；`codex_plugin_registry.rs` 编译错误：`enabled_codex_plugins` 未找到（386、494 行）、`RepairableCodexPlugin` 缺 `repair_action`（415、514 行），另有 17 行未使用导入警告 |

结论：MCP 测试门当前不可用；计划 Task 21 把编译修复列为 Phase M 准入第一步。

复验：2026-08-23 256 在 worktree 运行 `cargo check --manifest-path src-tauri/Cargo.toml --tests`，退出码 1，同样 4 个编译错误（`enabled_codex_plugins` 未找到 2 处、`repair_action` 缺失 2 处）依然存在，与探索报告记录一致。

## 4. 冲突与歧义裁定

第一版裁定（保留）：

- 裁定：`config_text=None` 是缺失输入，不是显式空配置 — 原因：缺失输入写空文件会造成数据丢失 — 判断错误的代价：`config.toml` 被写成 0 字节。
- 裁定：用户配置以当前 live 为基底 — 原因：live 是 Codex Desktop 最新写入的位置 — 判断错误的代价：数据库旧快照会回滚用户刚刚保存的桌面偏好和插件状态。
- 裁定：用户表默认保留，未知表也保留 — 原因：只保护白名单无法应对 Codex 新增配置表 — 判断错误的代价：未来新增用户表会再次被清空。
- 裁定：健康备份优先，SSOT 只在能提供完整配置时写回 — 原因：SSOT 当前只保证路由和模型目录，不保证完整 `config.toml` — 判断错误的代价：恢复日志会误报成功并覆盖现场。
- 裁定：普通切换先预检再更新当前供应商 — 原因：现有顺序会先把数据库指针切走 — 判断错误的代价：切换失败后数据库和 live 指向不同供应商。

第二版新增裁定：

- 裁定：SDD 插件登记纳入范围，按 cache、config、marketplace 三数据源一致性处理 — 原因：marketplace 缺条目是 UI 显示未安装的直接原因，只修 config 空写不能让 SDD 恢复显示 — 判断错误的代价：修复前必须验证 manifest 和路径边界，否则可能误写用户登记文件。
- 裁定：插件登记修复默认提供修复按钮，自动修复待世豪裁定 — 原因：marketplace 文件归 Codex 安装流程所有，静默改写有审计风险 — 判断错误的代价：世豪多点一次，但不会误登记版本或来源。
- 裁定：marker 语义分级为 `unclean_exit`、`confirmed_crash`、`planned_restart_or_update`、活跃旧实例，marker 单独存在不能触发破坏性恢复 — 原因：marker 残留不等于崩溃 — 判断错误的代价：正常双开或托盘常驻会反复触发恢复链。
- 裁定：15721 被占用时先探测 `/health` 与 `/status` 识别占用者，任何路径不杀进程 — 原因：占用者极可能是正在服务 Codex 的旧 CCSM 实例 — 判断错误的代价：需要给 `/status` 响应补充实例身份字段（新增字段，向后兼容）。
- 裁定：Codex live 写入采用乐观并发（指纹复检、有限重试 2 次、超限报 `ConcurrentModificationDeferred`），不引入文件锁 — 原因：双写风险已证实存在但具体冲突未捕获，锁协议属于 C 层 — 判断错误的代价：极端冲突下放弃写入并报告，而不是静默覆盖。
- 裁定：恢复结果必须显式命名、先持久化再发事件、可查询 — 原因：当前恢复只写日志，UI 可能错过启动事件 — 判断错误的代价：多维护一个 JSON 结果文件，换来用户可见的恢复反馈。
- 裁定：0 字节或语法损坏的 live 不视为可解析来源；用户表已不存在时只能恢复供应商字段并如实警告 — 原因：没有来源时不能凭空还原用户表 — 判断错误的代价：`ProviderOnlyRestored` 场景用户需要知道 `max` 不会自己回来。
- 裁定：本次上游贡献拆为配置防丢、启动识别、恢复结果 UX、插件登记四组新 Issue/PR — 原因：母仓库要求 PR 小而专注，历史 PR #35 已混入 20 个提交及 fork 发布变更 — 判断错误的代价：维护者无法独立审查或合并修复。
- 裁定：fork 发布分支与上游 PR 分支分离 — 原因：版本号、签名和发布标签只服务 `zhushihao/ccswitchmulti` — 判断错误的代价：把 fork 私有发布元数据带入母仓库，污染 diff 并阻塞合并。
- 裁定：不更新 PR #35/#37/#39/#41/#43/#45/#47/#49 — 原因：世豪明确要求只创建新 Issue 和新 PR — 判断错误的代价：历史 PR head 再次增长，产生新的依赖和冲突。
- 裁定：开工前、每个上游 PR 前、fork 打标签前都重新 fetch 母仓库 — 原因：所有补丁必须基于母仓库最新代码 — 判断错误的代价：PR 带入已解决代码或发布包落后于母仓库。
- 裁定：fork 版本号以母仓库最新稳定 Release 为根并追加递增后缀 — 原因：世豪要求 fork 版本清楚表达其母仓库基线 — 判断错误的代价：自动更新比较和用户识别版本时产生歧义。

待世豪决定的产品取舍：

- 插件登记修复是保持“检测到后给修复按钮”，还是升级为“启动时自动修复”。规格默认推荐修复按钮。
- 第三版 MCP 补充已获世豪批准；Phase M（计划 Task 21-26）已完成实现与聚焦验证。

第三版新增裁定（MCP 所有权与对账，完整文本见规格第 15 节）：

- 裁定：采用方案甲，数据库 `mcp_servers` 表的 id 集合即 Codex live MCP 所有权集合 — 原因：数据库行只能由显式动作产生，id 在库即所有权可证明，且无需 schema 迁移 — 判断错误的代价：managed id 的外部手工改动会在下次对账被数据库版本还原（已文档化）。
- 裁定：整表投影改为 live-as-base 对账，空数据库不清空 live 表 — 原因：空库只表示 CCSwitchMulti 没有管理任何 MCP — 判断错误的代价：全新安装后第一次切换就删除用户全部手工 MCP。
- 裁定：同名同内容幂等、同名不同内容数据库版本覆盖（所有权可证明）、live-only 永不触碰 — 原因：数据库必须保持权威，否则界面编辑会被 live 旧值顶掉 — 判断错误的代价：用户需在 CCSwitchMulti 内编辑 managed 条目，该行为写入文档。
- 裁定：显式禁用/删除即撤销所有权并移除 live 同 id；导入即建立所有权；SQL/云同步移除 DB 行后该 id 转为 live-only 保留 — 原因：显式动作与自动对账权限边界必须不同 — 判断错误的代价：删除若被手工改动阻断，界面删除会成为假按钮。
- 裁定：旧格式 `[mcp.servers]` 先迁移后清理，live-only 条目迁入 `[mcp_servers]` — 原因：旧格式内容同样是用户配置 — 判断错误的代价：存量旧格式用户仍丢失全部 MCP。
- 裁定：七个触发路径共用同一对账函数；启动恢复当前不直接调用整表投影，本轮不新增该接线 — 原因：语义不一致是缺陷的结构性原因 — 判断错误的代价：只修一条路径，其余路径继续删除用户级 MCP。
- 裁定：MCP 三个写入口走乐观并发写入器（指纹复检、最多重试 2 次、超限 `ConcurrentModificationDeferred`）；解析失败保持原文件字节不变 — 原因：Codex Desktop 是同一文件的并发写入者 — 判断错误的代价：必要时在 `codex_config.rs` 新增闭包形态 `pub(crate)` 更新 API。
- 裁定：MCP 日志、冲突提示、搜索只含 id 和字段名，不含 `env`/`headers`/`token` 值 — 原因：MCP 配置常携带密钥 — 判断错误的代价：一条日志就可能泄露 API key。
- 裁定：MCP 修复是独立提交边界和第五组上游 Issue/PR — 原因：缺陷在 `origin/main` 已存在，责任边界清晰 — 判断错误的代价：多一个小 PR，换来独立审查和回滚。
- 裁定：Phase M 准入先修 `codex_plugin_registry.rs` 编译错误并完成修改前快照（HEAD `6923a996`、25 个未提交条目、快照覆盖 tracked diff 与 3 个未跟踪文件、禁止 reset/clean） — 原因：测试门退出码 101 时任何 RED 都不可信 — 判断错误的代价：先做一步与 MCP 无关的编译修复。

## 5. 文件归属

本阶段创建：

- `docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`（第二版）
- `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`（第二版）
- `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation-ledger.md`（第二版）
- `.claude/napkin.md`

第三版修订（仅文档）：

- `docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`（第三版，新增第 15 节 MCP 补充，状态“待世豪批准”）
- `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`（第三版，新增 Phase M Task 21-26、上游 PR E、Phase M 简报）
- `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation-ledger.md`（本文件）

实现阶段允许修改（Phase M，待世豪批准第三版后生效）：

- `src-tauri/src/services/mcp.rs`
- `src-tauri/src/mcp/codex.rs`
- `src-tauri/src/codex_config.rs`（仅写入器可见性或闭包形态 API）
- `src-tauri/src/database/dao/mcp.rs`（仅新增只读方法；不加列、不做 schema 迁移）
- `src-tauri/src/services/provider/mod.rs`、`src-tauri/src/services/provider/live.rs`（仅收口绕过对账的写入点）
- `src-tauri/src/services/codex_plugin_registry.rs`（仅 Task 21 编译修复）
- `src-tauri/tests/mcp_codex_reconcile.rs`（新建）
- `src-tauri/tests/import_export_sync.rs`、`src-tauri/tests/provider_service.rs`（修正错误预期）
- Phase M 不新增 commands/API/UI（同名冲突可见提示默认不做，见未解决问题）

实现阶段允许修改（Phase A）：

- `src-tauri/src/codex_config.rs`
- `src-tauri/src/services/proxy.rs`
- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/tests/provider_service.rs`

实现阶段允许修改（Phase B）：

- `src-tauri/src/app_exit_monitor.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/proxy/types.rs`
- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/services/recovery_outcome.rs`（新建）
- `src-tauri/src/services/codex_plugin_registry.rs`（新建）
- `src-tauri/src/commands/mod.rs`（或实际注册 Tauri 命令的模块）
- `src/App.tsx`
- `src/hooks/useTauriEvent.ts`（仅在现有 hook 无法承载负载类型时）
- `src/lib/api/` 下的命令绑定

不得覆盖或回退：

- `PLAN-reasoning-narrow-wide.md`
- `docs/diagnostics/` 下已有文件
- 用户或其他人后续加入工作树的任何无关改动

## 6. 隔离与基线裁定

- 文档阶段在主工作树完成，因为本阶段只新增或修订 Markdown 文件。
- 实现阶段应使用独立分支或工作树，因为主工作树已有用户未跟踪文件。
- 上游交付分支必须从提交时最新的 `origin/main` 新建；不得从当前 `fix/issues-28-30-v3.19.2-15` 或任何历史 PR head 派生。
- fork 自动发布必须使用单独的新集成/发布分支；只向 fork 推送 `v*` 标签，不向母仓库创建 Release。
- 当前母仓库稳定 Release 是 `v3.19.2-16`，fork 已有 `v3.19.2-16.2`；若发布前母仓库不变，下一版候选为 `v3.19.2-16.3`，母仓库若升级则重新按新版本 `.1` 起算。
- 所有恢复测试必须使用临时 HOME 和内存数据库。
- 不得重启 Codex Desktop，不得杀任何进程。
- 测试不得写真实 `~/.codex` 或 `~/.agents`；插件登记修复的产品行为只能经 TempHome 测试验证。
- Phase A 必须先通过 Task 7 的验证门，Phase B 才能开始；两个阶段不共用一个提交边界。

## 7. 阶段状态

| 阶段 | 状态 | 证据 |
|---|---|---|
| Flash 探索 | DONE_WITH_CONCERNS | 探索报告含静态证据、日志证据和两次确定性最小复现；写竞争、插件登记、恢复边界和用户反馈四个遗留关注点已转入第二版规格 |
| 256 规格与计划（第二版） | DONE | 本账本、规格第二版和计划第二版已创建 |
| 世豪规格批准（第二版） | DONE | 世豪确认扩展方向、收到第二版文档后补充交付规则，并要求继续执行 |
| Luna 实现 | IN_PROGRESS | 按已批准规格执行；插件登记采用修复按钮 |
| Luna 相邻 bug 附录 | IN_PROGRESS | 世豪已授权 Task 14-20，覆盖删除投影、缺 routes、include 门控、reasoning 默认值、稳定方案选择和日志清理 |
| 256 交叉验证 | NOT_STARTED | Luna 尚未自测 |
| Sol 最终审核 | NOT_STARTED | 验证尚未开始 |
| Flash MCP 覆盖探索 | DONE_WITH_CONCERNS | `docs/diagnostics/sdd-codex-user-mcp-overwrite-explore-2026-08-23.md`；事实闭合，所有权模型与冲突语义交由 256 裁定 |
| 256 规格与计划（第三版 MCP 补充） | DONE | 规格第 15 节、计划 Phase M、本账本已更新；状态明确“待世豪批准” |
| 世豪规格批准（第三版） | DONE | 世豪明确批准，并补充“本质都是兼容性设计”的实现原则 |
| Luna Phase M 实现 | DONE_WITH_CONCERNS | Task 21-26 已完成；MCP/代理聚焦门全绿。第 7、8 项（prefix-only legacy migration、preview default-route 文案）仍待下一阶段规格化，未在本轮实现 |
| 替代 Sol 阶段 A 复核 | BLOCKED→已交回修复 | 发现 provider merge 入口 TOCTOU；原验证被暂停，RED 已记录 |
| Luna TOCTOU 修复与回归 | DONE_WITH_CONCERNS | merge/transform/projection 均移入每轮 live 快照闭包；provider 与 MultiRouter 竞态、连续冲突 auth 回滚、投影 catalog/cache/managed-agent 副作用回滚、非法 TOML 保护均 GREEN；待同一替代 Sol 复审 |
| Luna A-3 agent ownership 修复 | DONE | capture/restore 仅处理 managed marker 文件；用户同名 agent 并发编辑保留，原 managed/新增 managed/catalog/cache 回滚均有 GREEN；无新增未裁定契约 |

## 8. 给 Luna 的恢复简报

Luna 必须从规格文件和计划文件恢复上下文，不需要读取整段会话历史。Luna 先执行 Phase A Task 1 和 Task 2 的 RED 测试，再执行 Task 3 到 Task 7；Phase A 全绿后才进入 Phase B Task 8 到 Task 13。Luna 不能把 `None` 改成 `Some("")` 后直接写盘；只有调用方明确需要“清供应商字段、留用户表”时才能传 `Some("")`。端口探测任何路径不得杀进程；`CompatibleInstance` 要求 `/health` 成功且身份匹配。插件登记修复默认只做修复按钮，不得实现静默自动修复。Luna 发现规格矛盾时必须返回 `NEEDS_CONTEXT` 或 `BLOCKED`，不能自行改变契约。

Phase M 补充：Luna 在世豪批准第三版规格后，先执行 Task 21 准入门（worktree 外快照 + 编译修复），再按 Task 22-26 顺序实施 MCP 对账；核心语义是“数据库 id 集合即所有权、live-only 默认保留、只删在库未启用项、空库不清表、旧格式先迁移后清理、全部写入口走乐观并发复检”。Phase M 提交边界独立于其他阶段，对应上游第五组 Issue/PR E。

## 9. 未解决问题

- 插件登记本轮采用修复按钮；安全自动修复仍可作为后续产品升级。
- SDD marketplace 注册文件为什么没有在用户重新点击安装后写入 SDD，需要独立追踪 Codex 插件安装写路径。
- schema-v2 MultiRouter 是否需要完整非接管配置投影需要后续架构裁定（C 层）。
- 正式跨进程文件锁与 Codex `expectedVersion` 的协同属于 C 层。
- 第三版 MCP 补充已获世豪批准；本轮 Phase M 已完成实现，后续仅保留下一阶段规格化事项。
- MCP 同名不同内容时是否提供界面可见提示，默认本轮只写文档和日志。
- MCP provenance 列（方案乙）是否作为后续增强，留待下一轮 SDD 裁定。

## 10. Phase M 实现交接（2026-08-23）

- 实现 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`；HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`；修改前完整快照：`D:\CCSwitchMulti\snapshots\2026-08-23-codex-startup-config-preservation\`。
- MCP 对账已收口到 `McpService::sync_enabled_for_app` / `sync_all_enabled` 与 `mcp/codex.rs::sync_enabled_to_codex_with_ownership`：数据库全部 id 代表管理权，live-only 保留，空库不清表，只移除库内未启用 id；同语义条目使用 TOML 语义比较保留注释和格式；旧 `[mcp.servers]` 先迁移再清理。
- 对账、单条新增、单条删除和代理快照配置写入均经过 `codex_config` 乐观指纹复检、原子替换和最多两次重试；解析失败或并发超限保留原文件字节。
- 验证摘要（后续 TOCTOU 修复复跑）：全量 Rust 库 3335/3335（5 ignored；代理聚焦 90/90、`mcp` 68/68、mcp_commands 23/23）；MCP 对账 11/11；Provider 37/37；导入导出 26/26；fingerprint 13/13；MultiRouter projection 10/10、mutation 7/7；变换入口/副作用回滚回归 4/4；A-3 managed-agent ownership 回归 1/1；新增 provider 并发/回滚回归 3/3；`cargo check --lib`、前端 typecheck 通过，前端 144 个测试文件、1177 个测试通过。
- 格式基线：`git diff --check` 通过；Rust `cargo fmt --check` 仍受 HEAD 既有的 `proxy/handlers.rs`、`proxy/providers/codex.rs`、`proxy/providers/openai_compat.rs`、`services/provider/live.rs` 未格式化影响；前端 `format:check` 只剩未改动的 `src/components/providers/forms/CodexFormFields.tsx`。
- 依赖影响：为执行前端门使用 `pnpm install --offline --frozen-lockfile --ignore-scripts`，复用了本机 pnpm 缓存；仅生成被 `.gitignore` 忽略的 `node_modules`，未修改 lockfile、版本或真实用户目录。
- 结论：`DONE_WITH_CONCERNS`。第 7、8 项仍按审计报告留待下一阶段契约批准；本轮没有新增“同步 MCP”按钮、Tauri command、前端 API 或 UI。
- 2026-08-24 追加：替代 Sol 阶段 A 发现的 provider merge 入口 TOCTOU 已修复；RED 为 0/1，GREEN 为聚焦 1/1、连续三次冲突 auth 回滚 1/1、非法 live TOML 原字节保护 1/1。修复后全量 Rust 3330/0/5、前端 144/1177 全绿，保留既有 os error 32、React/MSW/Tauri 测试警告；交由同一替代 Sol 复审。
- 2026-08-24 追加：替代 Sol 阶段 A 继续发现的 force-transform/projection 入口 TOCTOU 已修复；RED 为 2 项各 0/1，GREEN 为变换入口 2/2、projection conflict/invalid-live 副作用回滚 2/2。修复后全量 Rust 3334/0/5，`cargo check --lib`、`pnpm typecheck`、`git diff --check` 全绿；保留既有 os error 32、React/MSW/Tauri 警告；交由同一替代 Sol 复审。
- 2026-08-24 追加：替代 Sol 阶段 A 发现的 agent 快照所有权问题已修复；RED 为 0/1，GREEN 为 A-3 1/1、projection 5/5、managed-agent 4/4。修复后全量 Rust 3335/0/5，保留既有 os error 32、React/MSW/Tauri 警告与 Rust 四文件格式基线；交由同一替代 Sol 复审。

## 最新交接（2026-08-24）：Luna 第四轮兼容性收口

状态：`DONE_WITH_CONCERNS`。Luna 已完成第四轮实现和本地验证；Sol 的独立复核仍为 `BLOCKED`，本节只记录 Luna 证据，不修改 Sol 的状态。

### 四项 RED/GREEN

1. **Attempt receipt 所有权**：旧实现会在写入后重读文件并把外部第三版本错误认领为本次输出。`committed_attempt_does_not_claim_external_write_after_replace_before_receipt` 的 GREEN 为 `1 passed / 0 failed`；receipt 现在从实际 candidate bytes 计算 fingerprint。
2. **Companion 条件提交与 deferred**：catalog、models cache、cache backup、managed agent 全部绑定 capture 时的 expected fingerprint，并以实际 writer 输出建立 after fingerprint。`provider_projection_cache_does_not_overwrite_external_update_before_companion_write` 为 `1/1`，`multirouter_projection_` 过滤器为 `6/6`，连续三次 cache 外部版本回归 `multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects` 为 `1/1`。本轮裁定 companion 竞态采用 `codex.live.concurrent_modification_deferred`，保留外部版本，禁止静默合并并宣称成功。
3. **Raw writer 边界**：两个 raw Codex 全文 writer 已收窄为 `#[cfg(test)] pub(crate)`；`raw_codex_fulltext_writer_is_not_reexported_to_application_callers` 和 `raw_codex_fulltext_writers_are_not_public_module_api` 共 `2 passed / 0 failed`。集成测试通过隔离 HOME seed，不再依赖公共 raw writer。
4. **Takeover backup MCP 来源**：backup 使用与普通 live 写入一致的 ownership 规则，保留当前 live 用户 MCP、common-config MCP，丢弃 provider snapshot-only MCP。`update_live_backup_drops_stale_provider_mcp_and_keeps_live_and_common_entries` 为 `1 passed / 0 failed`。

### 测试夹具与布局裁定

- `profile_roundtrip` 旧夹具已经升级到合法 schema v2，补齐 route policy 字段和 upstream catalog；migration guard 没有放宽。`profile_roundtrip` 最终为 `8 passed / 0 failed`，该项属于既有 fixture 修复，不属于生产行为修复。
- `import_export_sync` 的测试数量从旧记录 26 变为当前 24 是测试迁移而不是遗漏：两个公共 raw-writer persistence/rollback 测试移到 `codex_config.rs` crate-private 单元测试；`removes_servers_when_none_enabled` 改名为 `preserves_live_only_servers_when_none_enabled`，仍是一个测试。当前 `import_export_sync` 为 `24 passed / 0 failed`。

### 最终验证数字

| 验证门 | 结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| 全量 Rust library | `3348 passed / 0 failed / 5 ignored` |
| `mcp_codex_reconcile` | `11/11` |
| `provider_commands` | `10/10` |
| `provider_service` | `37/37` |
| `import_export_sync` | `24/24` |
| `profile_roundtrip` | `8/8` |
| 前端 Vitest | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules，11.54s |
| `cargo fmt -- --check` | PASS，已先执行 `cargo fmt` |
| `git diff --check` | PASS |

前端测试输出包含 5 类既有 warning：baseline-browser-mapping 过期、React `act(...)` 与 DOM 属性提示、MSW 未匹配 `tauri.local` 请求、jsdom 下 Tauri window API 错误日志，以及测试故意触发的错误日志。Renderer build 输出包含 4 类既有提示：baseline-browser-mapping 过期、Browserslist 过期、subscription 动静态导入提示和 chunk 大于 500KB 提示。typecheck 没有 warning 或 error。

### 当前工作树和本轮文件

当前实现 worktree `D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation` 有 38 个已修改文件和 4 个未跟踪 Rust 文件，Luna 未提交、未推送。第四轮核心实现集中在：

```text
src-tauri/src/codex_config.rs
src-tauri/src/services/provider/live.rs
src-tauri/src/services/provider/mod.rs
src-tauri/src/services/proxy.rs
src-tauri/tests/import_export_sync.rs
src-tauri/tests/profile_roundtrip.rs
src-tauri/tests/provider_commands.rs
src-tauri/tests/provider_service.rs
src-tauri/tests/support.rs
src-tauri/tests/mcp_codex_reconcile.rs
```

工作树完整状态以 `git status --short` 为准；累计清单为上述核心文件加上既有 Phase A/B/M 文件，合计 `38 M + 4 ??`。本轮没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex，也没有提交、推送或发布。Sol 仍需独立复核，状态保持 `BLOCKED`。

本轮实际 `git status --short` 快照如下：

```text
 M src-tauri/src/app_exit_monitor.rs
 M src-tauri/src/codex_config.rs
 M src-tauri/src/codex_multirouter/migration.rs
 M src-tauri/src/codex_multirouter/mutation.rs
 M src-tauri/src/codex_multirouter/projection.rs
 M src-tauri/src/commands/mod.rs
 M src-tauri/src/commands/proxy.rs
 M src-tauri/src/config.rs
 M src-tauri/src/lib.rs
 M src-tauri/src/mcp/codex.rs
 M src-tauri/src/mcp/mod.rs
 M src-tauri/src/proxy/handlers.rs
 M src-tauri/src/proxy/providers/codex.rs
 M src-tauri/src/proxy/providers/codex_reasoning.rs
 M src-tauri/src/proxy/providers/openai_compat.rs
 M src-tauri/src/proxy/server.rs
 M src-tauri/src/proxy/types.rs
 M src-tauri/src/services/mcp.rs
 M src-tauri/src/services/mod.rs
 M src-tauri/src/services/provider/live.rs
 M src-tauri/src/services/provider/mod.rs
 M src-tauri/src/services/proxy.rs
 M src-tauri/tests/import_export_sync.rs
 M src-tauri/tests/profile_roundtrip.rs
 M src-tauri/tests/provider_commands.rs
 M src-tauri/tests/provider_service.rs
 M src-tauri/tests/support.rs
 M src/App.tsx
 M src/components/codex/CodexRouterWorkspacePage.test.ts
 M src/components/codex/CodexRouterWorkspacePage.tsx
 M src/i18n/locales/en.json
 M src/i18n/locales/ja.json
 M src/i18n/locales/zh-TW.json
 M src/i18n/locales/zh.json
 M src/lib/api/index.ts
 M src/lib/api/settings.ts
 M src/lib/codexMultiRouterWizard.test.ts
 M src/lib/codexMultiRouterWizard.ts
?? src-tauri/src/commands/recovery.rs
?? src-tauri/src/services/codex_plugin_registry.rs
 ?? src-tauri/src/services/recovery_outcome.rs
 ?? src-tauri/tests/mcp_codex_reconcile.rs
 ```

## 最新交接（2026-08-24）：Luna 第五轮 auth receipt 与 force-repair receipt 收口

状态：`DONE_WITH_CONCERNS`。Luna 已完成第四轮 A-4/A-5 修复并完成本地验证；以下证据待 Sol 独立复核，不能改写验证报告中 Sol 既有的 `BLOCKED` 结论。

- 新增 typed `CodexAuthWriteAttempt`，一次捕获同时保存 auth 原始字节和 fingerprint；commit 前复检当前 fingerprint，written fingerprint 直接基于实际 candidate bytes 计算。
- auth 回滚只在 current fingerprint 仍等于本次写入 fingerprint 时发生。外部 auth 更新会被保留并传播 deferred/恢复未完成结果；普通 provider、catalog projection、snapshot projection 三条生产路径都使用该 receipt。
- `write_live_with_common_config_with_receipt`、`write_codex_live_snapshot_with_receipt` 和 `SwitchResult.codex_receipt` 让 force-repair 只使用真实 writer 的 config/auth/companion receipt；生产 `CodexProjectionSideEffectsAttempt::finish()`、写后重读认领逻辑和无用 `agents_dir` 已删除。
- companion restore 改为 `Result<bool, AppError>`，force-repair 聚合 config/auth/companion 条件恢复结果。任何 false 都报告“自动回滚恢复未完成/并发修改”，不得显示“已恢复原配置”。

第五轮新增真实 RED/GREEN：

| 回归 | 结果 |
|---|---:|
| `provider_auth_commit_defers_when_external_update_occurs_after_capture` | `1/1` |
| `provider_auth_rollback_preserves_external_update_after_commit` | `1/1` |
| `force_repair_does_not_claim_companion_update_before_finish` | `1/1` |
| `force_repair_reports_deferred_when_companion_restore_is_skipped` | `1/1` |

第五轮验证门：串行 `cargo test -- --test-threads=1` 通过；Rust library `3352 passed / 0 failed / 5 ignored`；`cargo check --tests`、`cargo fmt --all -- --check`、`git diff --check` 通过；前端 `pnpm test:unit` 为 `144 files / 1177 tests / 0 failed`；`pnpm typecheck` 和 `pnpm build:renderer` 通过（3342 modules）。一次并行测试曾出现既有 `codex_history_migration` SQLite `threads` 表竞态，立即串行单测和最终串行全量均通过，按共享测试环境串扰记录，不作为产品失败结论。

本轮仍未访问真实 `~/.codex` 或 `~/.agents`，未重启/结束 Codex，未提交、推送或发布。Sol 待独立复核 auth attempt ownership、companion receipt 透传和 deferred 可见性。

## 最新交接（2026-08-24）：Luna 第六轮逐项恢复与最终串行验证

状态：`DONE_WITH_CONCERNS`。世豪批准的第三版兼容性原则继续生效：当前 live 是事实来源，CCSwitchMulti 只认领自己确切写出的版本，外部新版本优先保留。以下是 Luna 的实现证据，等待 Sol 独立复核，不修改既有 `BLOCKED`。

- `CodexSwitchStateSnapshot::restore_files_if_unchanged` 改为逐项恢复 runtime 状态。外部 current provider 变化时不回写第三方 current，但仍恢复本次 attempt 独占的 proxy config、backup、派生 takeover 和 proxy running；config/auth/companions 仍按 receipt 指纹条件恢复。
- 三个特殊 Codex switch 分支均透传完整 `CodexSwitchReceipt`；普通 provider 分支使用精确 provider receipt。managed companion 的 after snapshot 保留 tombstone，认证删除以 `written_fingerprint = None` 证明“当前应为空”。
- 格式化后的串行全量测试没有新增失败：Rust library `3361 passed / 0 failed / 5 ignored`，所有 integration tests 通过；force-repair 聚焦 `15/15`；前端 `144 files / 1177 tests` 全部通过，`pnpm typecheck` 通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 与 `git diff --check` 通过。全量 `pnpm format:check` 仍只命中未修改的 `src/components/providers/forms/CodexFormFields.tsx`，本轮修改的前端文件单独 Prettier 检查通过。

本轮没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex，没有提交、推送或发布；Sol 仍需独立复核 receipt、companion deferred 和逐项 runtime 所有权边界。

## 最新交接（2026-08-24）：Luna 第七轮 MCP receipt 与最终门

状态：`DONE_WITH_CONCERNS`，等待 Sol 独立复核。世豪批准的第三版原则继续作为实现边界：当前 live 是事实来源；CCSwitchMulti 只认领自己确切写出的版本；外部新版本优先保留。

- 普通 provider switch 现在把 MCP reconcile receipt 接到最后 writer 链，provider/MCP/auth receipts 共同构成回滚边界。特殊 switch 的 `finish_codex_switch_result` 与 `finish_codex_switch_mutation_result` 只组装 receipts，不再通过 after-read 认领外部版本。
- MCP ownership 以数据库出现过的 id 为证明：live-only 用户级 MCP 永远保留；仅删除库内已禁用条目；provider 快照陈旧 MCP 不复活；legacy `[mcp.servers]` 先迁移后清理；语义相同条目不重写，保留用户注释和格式。
- 受控旧实现 mutation 的 RED 已闭合：hot-switch 外部第三版本误认领、普通 switch finish-boundary 外部版本、MCP 最后 writer 使 provider receipt 过期等回归均已 GREEN。

### 第七轮验证门

| 验证门 | 结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| 串行 Rust 全量 `cargo test -p cc-switch -- --test-threads=1` | library `3363 passed / 0 failed / 5 ignored`；integration binaries 全部 PASS |
| `force_repair_` | `16 passed / 0 failed` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| `pnpm test:unit -- --run` | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules，11.59s |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS |
| `git diff --check` | PASS |

静态审计重点：两个 `finish_*` block 内均无 `capture(state)`、`capture_after`、`read_codex_config_text` 或 `ExactCodexSnapshot::read`；after-state 由 typed receipts 与未修改 runtime-before 组装。全量前端格式检查仅命中未修改的 `CodexFormFields.tsx`，不属于本轮改动。实现 worktree 当前为 `39 M + 4 ??`，未提交、未推送；没有访问真实用户配置或重启 Codex。Sol 仍需独立复核 receipt 与 MCP ownership 结论。

## 最新交接（2026-08-24）：启动 outcome 单一职责与四语言兼容性收口

状态：`DONE_WITH_CONCERNS`。世豪批准的第三版原则仍是“当前 live 是事实来源、兼容优先”；以下是 Luna 实现证据，等待 Sol 独立复核，不修改 Sol 的 `BLOCKED`。

- 真实 RED：职责收敛前 `normal_startup_` 为 `1 passed / 1 failed`，旧 `ActivePreviousInstance` 在正常启动后仍残留。
- GREEN：`normal_startup_` `2/2`、`app_exit_monitor` `11/11`、`recovery_outcome` `6/6`、`port_probe` `4/4`、不兼容 listener E2E `1/1`、四语言翻译测试 `4/4`。
- 单一职责：`record_startup_report()` 只读证据、分类、写 marker；`persist_startup_recovery_outcome()` 唯一负责写入 Active/Planned 或按 kind 清理旧瞬态。`ProviderOnlyRestored` 等非瞬态 outcome 保留。清理在共享进程内锁下复读并比较原始字节，内容变化则不删除。
- 静态调用证据：生产 `lib.rs` 只有一次 `record_startup_report()` 和一次 `persist_startup_recovery_outcome(&startup)`；唯一生产清理调用位于后者内部，测试中的 direct clear 不属于生产路径。
- 最终串行 Rust library `3375 passed / 0 failed / 5 ignored`，integration binaries 全部通过；前端 `145 files / 1181 tests`、typecheck、renderer build（3342 modules）、`cargo check --tests`、fmt check、`git diff --check` 全部 PASS。
- 四语言补齐 `closeOtherInstanceOrInspectProcess`，测试禁止显示内部驼峰 key；没有新增 MCP 同步按钮/API。
- 残余边界：进程内锁不覆盖跨进程第二次读取到删除之间的竞态，正式跨进程锁仍是后续规格项。

结论仍为“Luna 已修复，待 Sol 复核”，不得把实现方 GREEN 改写成 Sol 已通过。

# SDD 探索报告：用户级 Codex MCP 被 CCSwitchMulti 覆盖

状态：`DONE_WITH_CONCERNS`

日期：2026-08-23

仓库：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`

执行方式：只读探索。未修改产品代码，未提交、未推送、未重启或结束 Codex，未访问真实 `~/.codex` 或 `~/.agents`。产品源文件唯一新增物是本报告；运行 `cargo` 仅可能在已被忽略的 `target` 目录产生构建缓存，未改动产品源码。

## 1. 结论摘要

马博士确认，用户级 Codex MCP 被覆盖不是当前未提交分支新引入的问题，它已经在 `origin/main` 中存在。问题发生在 Codex MCP 的“整表投影”路径，而不是单条新增、编辑、启用或删除路径。

当前代码存在两套互相不一致的语义：

1. **单条更新语义**：新增、编辑、启用、禁用、删除 MCP 时，`sync_single_server_to_codex` 和 `remove_server_from_codex` 只更新或删除一个 `id`，会保留 live 配置中数据库不知道的其他 MCP。
2. **整表投影语义**：供应商切换、供应商保存、SQL 导入、云同步/设置后置同步、统一会话开关保存等路径会调用 `sync_enabled_for_app` 或 `sync_all_enabled`，最终调用 `sync_enabled_to_codex`。该函数以数据库快照为准，把整个 `[mcp_servers]` 表替换成数据库里“启用 Codex”的条目；数据库没有的 live-only MCP 会全部删除，同名但内容不同的 live MCP 会被数据库版本覆盖。

所以，“用户手工加到 `~/.codex/config.toml` 但未导入数据库的 MCP”会在整表投影时消失。代理备份路径 `services/proxy.rs` 已经有保护 MCP 的合并逻辑，但 MCP 服务层的整表投影会随后把它清掉。

现有规格只给了一句“`mcp_servers` 是混合边界”，实现计划没有对应的 MCP 保留任务和测试。马博士把这一点作为本次新增的问题交给 256 裁定。

## 2. Git 状态与当前工作树

工作树分支：`fix/codex-startup-config-preservation`

当前 HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`

`origin/main`：`9b0fd548301b2734772c155d8475deb285352bba`

当前工作树有 25 个未提交条目：

```text
 M src-tauri/src/app_exit_monitor.rs
 M src-tauri/src/codex_config.rs
 M src-tauri/src/codex_multirouter/mutation.rs
 M src-tauri/src/codex_multirouter/projection.rs
 M src-tauri/src/commands/mod.rs
 M src-tauri/src/lib.rs
 M src-tauri/src/proxy/providers/codex_reasoning.rs
 M src-tauri/src/proxy/server.rs
 M src-tauri/src/proxy/types.rs
 M src-tauri/src/services/mod.rs
 M src-tauri/src/services/provider/mod.rs
 M src-tauri/src/services/proxy.rs
 M src-tauri/tests/provider_service.rs
 M src/App.tsx
 M src/components/codex/CodexRouterWorkspacePage.test.ts
 M src/components/codex/CodexRouterWorkspacePage.tsx
 M src/i18n/locales/en.json
 M src/i18n/locales/ja.json
 M src/i18n/locales/zh-TW.json
 M src/i18n/locales/zh.json
 M src/lib/api/index.ts
 M src/lib/api/settings.ts
 ?? src-tauri/src/commands/recovery.rs
 ?? src-tauri/src/services/codex_plugin_registry.rs
 ?? src-tauri/src/services/recovery_outcome.rs
```

与用户级 MCP 问题直接相关的文件：

- `src-tauri/src/services/mcp.rs`
- `src-tauri/src/mcp/codex.rs`
- `src-tauri/src/database/dao/mcp.rs`
- `src-tauri/src/database/schema.rs`
- `src-tauri/src/commands/mcp.rs`
- `src-tauri/src/commands/provider.rs`
- `src-tauri/src/services/provider/mod.rs`
- `src-tauri/src/services/provider/live.rs`
- `src-tauri/src/services/config.rs`
- `src-tauri/src/services/proxy.rs`
- `src-tauri/src/lib.rs`
- `src/components/mcp/UnifiedMcpPanel.tsx`
- `src/components/mcp/McpFormModal.tsx`
- `src/components/mcp/useMcp.ts`（实际路径为 `src/hooks/useMcp.ts`）
- `src/lib/api/mcp.ts`
- `src/lib/api/providers.ts`
- `src/lib/api/settings.ts`
- `src/hooks/useSettings.ts`
- `src/hooks/useImportExport.ts`
- `src/utils/postChangeSync.ts`
- `src/components/DeepLinkImportDialog.tsx`
- `src-tauri/src/deeplink/mcp.rs`

验证命令：`git diff --numstat origin/main -- src-tauri/src/mcp/codex.rs src-tauri/src/services/mcp.rs src-tauri/src/database/dao/mcp.rs src-tauri/tests/import_export_sync.rs` 输出为空。也就是说，这几个文件与 `origin/main` 完全一致，覆盖风险不在当前未提交分支中，而是在主线已有代码里。

## 3. 关键写入路径

### 3.1 单条 MCP 操作

用户可见动作和调用链如下：

| 用户动作 | 前端证据 | API / 命令 | 后端行为 | 对 Codex live 的影响 |
|---|---|---|---|---|
| 新增 MCP | `src/App.tsx:1792-1801` 按钮，点击后 `openAdd()`；`UnifiedMcpPanel.tsx:216-220` | `mcpApi.upsertUnifiedServer` -> `commands::upsert_mcp_server`（`commands/mcp.rs:172`） | `McpService::upsert_server`（`services/mcp.rs:19`）；若 Codex 启用则 `sync_single_server_to_codex`（`services/mcp.rs:121`） | 只 upsert 一个 `id`，其他 live MCP 保留 |
| 编辑 MCP | `McpFormModal.tsx:369-410` 提交；`UnifiedMcpPanel.tsx:210-214` | 同 `upsert_mcp_server` | 同 `McpService::upsert_server`；取消 Codex 勾选时走 `remove_server_from_codex`（`services/mcp.rs:34-36, 171`） | 单条删除/写入，其他 live MCP 保留 |
| 单应用启用/禁用 | `UnifiedMcpPanel.tsx:160-173`；`AppToggleGroup` 调用 | `mcpApi.toggleApp` -> `commands::toggle_mcp_app`（`commands/mcp.rs:187`） | `McpService::toggle_app`（`services/mcp.rs:72`） | Codex 启用走单条同步，禁用走单条删除 |
| 批量启用/禁用 | `UnifiedMcpPanel.tsx:175-208`；`useBulkToggle` | `useMcp.ts:32-50`，逐条调用 `toggleApp` | 同上 | 逐条单条操作 |
| 删除 MCP | `UnifiedMcpPanel.tsx:247-266` | `mcpApi.deleteUnifiedServer` -> `commands::delete_mcp_server`（`commands/mcp.rs:181`） | `McpService::delete_server`（`services/mcp.rs:57`），再 `remove_server_from_codex` | 只删除一个 `id` |
| 从应用导入 MCP | `App.tsx:1786-1791` 按钮，点击后 `openImport()`；`UnifiedMcpPanel.tsx:222-240` | `mcpApi.importFromApps` -> `commands::import_mcp_from_apps`（`commands/mcp.rs:199`） | `McpService::import_from_all_apps`（`services/mcp.rs:531`） -> `import_from_codex`（`services/mcp.rs:348`） | 只写数据库，不写 live |
| Deep Link 导入 MCP | `DeepLinkImportDialog.tsx:103-109`，`importFromDeeplink` | `commands/deeplink.rs` 的 `import_from_deeplink_unified` | `deeplink/mcp.rs:40` -> `McpService::upsert_server` | 单条 upsert，不整表投影 |

马博士没有找到前端独立“同步 MCP”按钮。`sync_enabled_for_app` 和 `sync_all_enabled` 在 `src` 中没有被前端直接调用，所有调用都来自后端供应商、设置和导入/同步流程。

### 3.2 整表投影路径

| 触发行为和证据 | 后端调用点 | 最终动作 |
|---|---|---|
| 切换供应商：`useSwitchProviderMutation` -> `providersApi.switch`（`src/lib/api/providers.ts:283-285`） -> `switch_provider`（`commands/provider.rs:227`） | `ProviderService::switch` 末尾 `McpService::sync_enabled_for_app`（`services/provider/mod.rs:5454`） | Codex 整表投影 |
| 编辑并保存当前供应商：`providersApi.update`（`src/lib/api/providers.ts:221-231`） -> `update_provider`（`commands/provider.rs:62`） | `ProviderService::update` 末尾 `McpService::sync_enabled_for_app`（`services/provider/mod.rs:4550`） | Codex 整表投影 |
| 从代理接管切回官方等特殊切换：`ProviderService::switch` 特殊分支 | `McpService::sync_all_enabled`（`services/provider/mod.rs:5078`） | 所有应用整表投影 |
| 设置保存：目录变化或 Claude 插件开关变化后，`useSettings.ts:431-455` 调用 `syncCurrentProvidersLiveSafe` | `settingsApi.syncCurrentProvidersLive` -> `sync_current_providers_live`（`commands/import_export.rs:63`） -> `ProviderService::sync_current_to_live`（`services/provider/live.rs:1520`） | `McpService::sync_all_enabled`（`services/provider/live.rs:1537`） |
| SQL 配置导入：`useImportExport.ts:85-105` 调用 import 后再调用 `syncCurrentProvidersLiveSafe` | 后端 `import_config_from_file` 先执行 `run_post_import_sync`（`commands/import_export.rs:51`、`commands/sync_support.rs:10-14`） | `ProviderService::sync_current_to_live`，同样执行 `sync_all_enabled` |
| 统一 Codex 会话历史开关变化：`CodexAuthSettings.tsx:110-112` 复选框 | `settingsApi.save` -> `save_settings`（`commands/settings.rs:106`） -> `reapply_current_codex_official_live`（`services/provider/mod.rs:59`） | `McpService::sync_enabled_for_app`（`services/provider/mod.rs:103`） |
| 通用配置片段保存：`configApi.setCommonConfigSnippet`（`src/lib/api/config.ts:44-48`） | `set_common_config_snippet`（`commands/config.rs:307`） -> `ProviderService::sync_current_provider_for_app`（`commands/config.rs:348`） | `sync_enabled_for_app`（`services/provider/live.rs:1408`） |

### 3.3 启动路径

`src-tauri/src/lib.rs:1236-1303` 的启动后台任务会执行：

1. `state.proxy_service.recover_from_crash().await`
2. `codex_plugin_registry::detect_codex_plugin_registration()`
3. `ProviderService::scrub_leaked_gemini_common_config`
4. `initialize_common_config_snippets`
5. `restore_proxy_state_on_startup`

马博士用 `rg` 检查了 `src-tauri/src/lib.rs`、`src-tauri/src/services/proxy.rs` 和 `src-tauri/src/commands`，启动恢复路径本身没有调用 `McpService::sync_all_enabled` 或 `McpService::sync_enabled_for_app`。因此当前启动恢复本身不会直接触发 MCP 整表投影；风险主要来自启动后的供应商切换、设置保存、SQL 导入和同步流程。若后续把启动恢复接入了 MCP 同步，需要重新评估。

## 4. 覆盖发生的确切位置

### 4.1 `McpService::project_servers_to_app`

文件：`src-tauri/src/services/mcp.rs:223-259`

关键逻辑：

```rust
if matches!(app, AppType::Codex) {
    let mut config = crate::app_config::MultiAppConfig::default();
    for server in servers.values().filter(|server| server.apps.codex) {
        config.mcp.codex.servers.insert(
            server.id.clone(),
            serde_json::json!({
                "enabled": true,
                "server": server.server,
            }),
        );
    }
    return mcp::sync_enabled_to_codex(&config);
}
```

它只把数据库中 `apps.codex == true` 的条目放入临时配置，然后调用 `sync_enabled_to_codex`。数据库没有的 live MCP 不在这个临时配置里。

### 4.2 `mcp/codex.rs::sync_enabled_to_codex`

文件：`src-tauri/src/mcp/codex.rs:286-347`

关键行为：

- 第 307-315 行：先清理旧格式 `[mcp.servers]`。
- 第 318-320 行：数据库中没有任何启用项时，删除整个 `mcp_servers` 表。
- 第 323-339 行：数据库有启用项时，新建一个 `Table`，只加入数据库条目，然后直接执行 `doc["mcp_servers"] = Item::Table(servers_tbl)`。

这等价于“以数据库快照为权威，整体替换 `mcp_servers`”。因此：

- 数据库空、live 有 `external-only`：`external-only` 被删除。
- 数据库有 `managed`、live 有 `external-only`：`external-only` 被删除，`managed` 被写回。
- 数据库有 `same-id`、live 的 `same-id` 是用户手工更新：数据库版本覆盖 live 版本。
- live 有旧格式 `[mcp.servers]`：整表投影会把它清掉。

### 4.3 单条路径的行为对照

文件：`src-tauri/src/mcp/codex.rs`

- `sync_single_server_to_codex`（第 419-466 行）读取当前配置，只 upsert 一个 `id`，然后写回。
- `remove_server_from_codex`（第 470-499 行）只删除一个 `id`。
- 第 447-455 行和第 390-417 行仍会清理旧格式 `[mcp.servers]`，但不会删除目标 `id` 之外的条目。

因此，如果只做单条 MCP 编辑，外部 live 条目可以保住；一旦触发整表投影，就会丢失。

## 5. 数据库与数据模型没有所有权

### 5.1 SQLite 表

`src-tauri/src/database/schema.rs:64-71`：

```sql
CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
    description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
    enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
    enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
    enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
    enabled_hermes BOOLEAN NOT NULL DEFAULT 0
)
```

历史迁移只增加了用户元数据和各应用启用列：

- `schema.rs:571-587`：增加 `description`、`homepage`、`docs`、`tags`、`enabled_codex`、`enabled_gemini`。
- `schema.rs:1073-1088`：v3 -> v4 增加 `enabled_opencode`。
- `schema.rs:1520-1538`：v14 -> v15 增加 `enabled_grokbuild`。

没有任何 `source`、`owner`、`managed`、`origin`、`provenance` 列。

### 5.2 Rust 结构

`src-tauri/src/app_config.rs`：

- `McpServer`（第 246-259 行）：只有 `id`、`name`、`server`、`apps`、`description`、`homepage`、`docs`、`tags`。
- `McpApps`（第 9-22 行）：只有布尔启用状态。
- `McpConfig`（第 261-267 行）：注释说旧结构的服务器定义“包含 enabled/source 等 UI 辅助字段”，但该字段没有进入统一 `McpServer` 结构。

`McpServer` 没有可持久化的所有权字段，因此当前产品无法区分“CCSwitchMulti 管理的 MCP”和“用户直接写在 Codex live 里的 MCP”。

### 5.3 前端类型

`src/types.ts:772-785` 的 `McpServer` 有：

```ts
source?: string;
[key: string]: any;
```

但 `source` 只被 `UnifiedMcpPanel.tsx:27-51` 用作搜索文本的一部分（第 42 行），没有传给后端、没有写入数据库，也没有参与任何所有权判断。它只是前端兼容旧字段。

## 6. 现有测试已经固定错误行为

以下测试文件是现有代码中的证据，马博士没有新增测试：

### 6.1 直接复现“数据库空，live MCP 被删除”

`src-tauri/tests/import_export_sync.rs:434-457`：

```rust
fs::write(
    &path,
    r#"[mcp_servers]
disabled = { type = "stdio", command = "noop" }
"#,
)
.expect("seed config file");
let config = MultiAppConfig::default();
cc_switch_lib::sync_enabled_to_codex(&config).expect("sync codex");
assert!(!text.contains("mcp_servers") && !text.contains("servers"));
```

这个测试把“无启用项时应删除 MCP 表”写成预期行为。

### 6.2 供应商切换时删除 live-only MCP 的直接用例

`src-tauri/tests/provider_service.rs:2495-2672`，函数名 `switch_codex_syncs_shared_keys_from_live_into_common_config`：

- 第 2508-2530 行：live 配置包含 `[mcp_servers.echo]` 和旧格式 `[mcp.servers.ghost-legacy]`。
- 第 2534-2570 行：创建数据库供应商，但没有向数据库写入任何 MCP。
- 第 2579 行：执行 `ProviderService::switch(&state, AppType::Codex, "b")`。
- 第 2613-2634 行：断言切换后 `live_after` 不再包含 `mcp_servers`，也不包含 `ghost-legacy`。

这正好复现“用户手工写在 live 里的 MCP 在供应商切换后被删除”。现有测试把删除行为作为成功条件，说明当前产品把它当作“清理过期投影”。

### 6.3 原有格式迁移

`src-tauri/tests/import_export_sync.rs:396-431` 证明 `[mcp.servers]` 会被清理并迁移到 `[mcp_servers]`。

### 6.4 非 Codex 路径保留未知条目

`src-tauri/tests/mcp_commands.rs:1185-1278` 的 `sync_all_enabled_removes_known_disabled_but_preserves_unknown_live_entries` 只针对 Claude，断言 Claude live 的 `external-only` 保留。它没有覆盖 Codex；相反，Codex 的 `import_export_sync` 测试和 `provider_service` 测试都固定了删除语义。

### 6.5 代理层已经尝试保留 MCP

`src-tauri/src/services/proxy.rs:3359-3426` 的 `merge_codex_user_config_with_provider_config` 会保留 live 中用户自有的 MCP；

`src-tauri/src/services/proxy.rs:8332-8403` 的 `update_live_backup_from_provider_preserves_codex_mcp_servers` 明确断言代理热切换备份更新后旧 MCP 保留。

这证明 loss 不在“供应商配置合并”或“代理接管备份”层，而在后续的 MCP 整表投影层。

## 7. 测试执行结果

马博士尝试运行现有回归测试，但没有成功：

```text
工作目录：D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation
命令：cargo test --manifest-path src-tauri/Cargo.toml --test import_export_sync sync_enabled_to_codex -- --nocapture
退出码：101
```

编译错误：

```text
error[E0425]: cannot find function `enabled_codex_plugins` in this scope
  --> src\services\codex_plugin_registry.rs:386:19

error[E0063]: missing field `repair_action` in initializer of `RepairableCodexPlugin`
  --> src\services\codex_plugin_registry.rs:415:25

error[E0425]: cannot find function `enabled_codex_plugins` in this scope
  --> src\services\codex_plugin_registry.rs:494:23

error[E0063]: missing field `repair_action` in initializer of `RepairableCodexPlugin`
  --> src\services\codex_plugin_registry.rs:514:25
```

另外有一个警告：

```text
warning: unused import: `value as toml_value`
  --> src\services\codex_plugin_registry.rs:17:17
```

因此，本次探索没有执行任何 MCP 测试。马博士没有修改 `codex_plugin_registry.rs`，也没有为了复现临时增加测试代码。上述行为结论来自现有测试源、静态代码路径和已确认的 `origin/main` 一致代码。

## 8. 敏感字段与日志风险

当前 MCP 配置可以携带 `env`、`headers` 等敏感值：

- 前端类型：`src/types.ts:744-757`。
- 搜索文本明确排除 `env` 和 `headers`：`src/components/mcp/UnifiedMcpPanel.tsx:45-50`。

马博士检查了相关日志入口：

- `mcp/codex.rs` 的导入和转换日志只记录 `id` 和字段名，不记录字段值（例如第 208-209、216-252、721-726 行）。
- `mcp/validation.rs` 的错误只描述字段名或类型，不回显完整配置（第 8-51 行）。
- `deeplink/mcp.rs` 日志只记录 `id` 和错误字符串，不记录完整配置（第 102、118、134、141 行）。
- `database/dao/mcp.rs` 只把 `server_config` 序列化进数据库，错误消息不包含配置值（第 118-145 行）。

当前没有发现直接打印 `env`、`headers`、token 值的日志路径。但数据库本身会持久化这些值，如果未来增加所有权字段、导入导出或审计日志，必须继续把 `env`、`headers` 排除在日志和搜索之外。

## 9. 原子性和并发机制

### 9.1 MCP 层当前写入

`mcp/codex.rs` 的三个写入口都使用通用文本写入：

- `sync_enabled_to_codex`：第 344-345 行 `crate::config::write_text_file`。
- `sync_single_server_to_codex`：第 462-463 行。
- `remove_server_from_codex`：第 495-496 行。

`crate::config::write_text_file`（`src-tauri/src/config.rs:319-323`）调用 `atomic_write`（第 327 行）：临时文件 `create_new`，Windows 下 `ReplaceFileW` 原子替换，但没有跨进程锁，也没有指纹复检。

### 9.2 worktree 已新增的指纹复检写入器

`src-tauri/src/codex_config.rs` 当前已经有：

- `CodexConfigFingerprint`（第 518-537 行）：长度 + DefaultHasher。
- `write_codex_live_config_optimistic`（第 551-591 行）：替换前复检，冲突时重读并合并，最多 2 次重试，超限返回 `ConcurrentModificationDeferred`。
- `write_codex_live_config_atomic`（第 614-624 行）和 `write_codex_live_atomic`（第 274-312 行）已接入该写入器。

但该写入器是私有函数，且 MCP 层没有调用它。马博士只记录事实，不做方案设计：MCP 整表投影即使改用指纹复检，也仍会以数据库快照构造候选内容；能否覆盖问题是“候选内容如何构造”的问题，不是单纯并发写入的问题。

## 10. 现有规格和计划覆盖情况

已批准规格：`docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`

已批准计划：`docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`

账本：`docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation-ledger.md`

规格第 110 行明确写出：

> `mcp_servers` 是混合边界：合并时不得用旧供应商快照覆盖 live 中较新的条目；后续 MCP reconcile 可以按数据库投影增删它明确管理的条目，但本次配置恢复不能直接清掉整张表。

但马博士用 `rg` 在计划文件中搜索，计划内没有任何 MCP 保留任务、MCP 所有权字段、MCP 同名冲突规则或 MCP 回归测试。规格第 106-110 行把 `mcp_servers` 列为混合边界，却没有给出“哪些条目算 CCSwitchMulti 管理、哪些算用户自有、同名冲突怎么处理、删除和禁用语义”的定义。

这导致两项缺口：

1. 当前实现没有所有权字段，无法执行“只删除明确管理的条目”。
2. 当前整表投影会删除所有未导入数据库的 live MCP，和规格第 110 行不一致，但没有任何 RED 测试捕获。

## 11. 交给 256 的决策问题

马博士按“不设计最终方案”的要求，只提出需要 256 在规格/计划前裁定的问题：

1. **所有权模型**：是否需要新增数据库列（如 `managed`、`source`、`owner`、`provenance`），还是用“live 文件中已知 ID 集合 + 数据库 ID 集合”的运行时对账，还是使用前端已有但未持久化的 `source` 字段？
2. **用户级 MCP 的保留语义**：整表投影时，live 中数据库不认识的 MCP 是“无条件保留”，还是只在“用户通过 Codex 手工新增”时保留？
3. **同名冲突语义**：数据库条目和 live 条目同名但内容不同，谁优先？是数据库优先、live 优先、还是先探测是否被用户修改？
4. **禁用/删除语义**：用户通过 UI 取消 Codex 启用或删除 MCP 时，是否仍要删除同名 live 条目；如果 live 条目是用户手工改过的，是否要先告警？
5. **旧格式迁移**：`[mcp.servers]` 被视为历史错误格式并清理的做法是否保留；如果保留，是否也要把其中用户级条目纳入保护范围？
6. **导入时机**：首次启动导入、用户点击“导入 MCP”、Deep Link 导入分别应如何标记所有权，避免后续整表投影误删。
7. **启动/恢复边界**：启动恢复、SQL 导入、云同步、设置后置同步、供应商切换、供应商保存和统一会话开关保存是否共用同一套 MCP 对账规则。
8. **测试门**：应由 256 定义至少四个 RED 用例——数据库空 + live-only 保留、数据库有条目 + live-only 保留、同名不同内容优先级、旧格式用户级条目保留。
9. **回归目标**：现有 `provider_service.rs:2495-2672` 把删除 live-only MCP 固定为成功条件；修复后这一测试必须从“断言删除”改为“断言保留或按所有权规则处理”，否则测试本身会阻止正确修复。

## 12. 未完成状态

马博士已完成事实探索和证据整理，但以下工作尚未完成：

- 当前 worktree 的 `codex_plugin_registry.rs` 编译失败，测试门无法通过。
- 用户级 MCP 覆盖问题没有进入已批准实现计划的独立任务。
- 没有所有权字段、冲突规则和对应回归测试。
- 没有运行 MCP 相关的实际测试命令（因编译失败）。

因此报告状态为 `DONE_WITH_CONCERNS`：探索事实已闭合，但产品文件和测试门需要 256 先裁定所有权限模型与冲突语义，再由 Luna 实现。

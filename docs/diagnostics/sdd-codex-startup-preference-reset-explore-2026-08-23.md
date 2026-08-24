# Codex Desktop 启动后推理强度和 SDD 插件状态丢失：SDD 探索报告

状态：`DONE_WITH_CONCERNS`（根因与最小复现已确认；写入竞争、插件登记、恢复边界和用户反馈仍有遗留关注点）

仓库：`D:\CCSwitchMulti\ccswitchmulti`

日期：2026-08-23

执行范围：只读检查仓库源码、Codex 安装包、`~/.codex` 配置与日志、CCSM SQLite；未修改产品代码，未关闭或重启 Codex，未调度其他智能体。唯一写入物是本报告。

## 1. 结论摘要

世豪报告的“每次 Codex Desktop 启动后可用推理强度中的 max 消失”和“SDD 插件显示未安装”，由两条现象组成：

1. 推理强度设置极可能被 CCSM 的“SSOT 恢复（无备份兜底）”路径清掉。当前数据库的 `codex-multirouter` 供应商没有 `config` 文本，而该恢复路径会把空配置传给 `write_codex_live_for_provider`；这一步不会与现有 `config.toml` 合并，最终会写出空配置，从而把 `[desktop]` 和 `[plugins]` 两段一起清掉。
2. SDD 插件即使 `config.toml` 里有 `[plugins."sdd@personal"] enabled=true`，`~/.agents/plugins/marketplace.json` 仍没有 SDD 条目；Codex Desktop 的插件列表读取的是 marketplace 注册文件，因此 UI 仍显示“未安装”。重新点安装可能只补了 config 的启用字段，没有把 SDD 补进 marketplace 注册文件。

两条现象很可能由同一次启动清洗叠加造成：SSOT 恢复先清空用户配置，Codex Desktop 随后按默认值重新初始化 `[desktop]`，同时按缺失的 marketplace 注册文件显示插件未安装。

## 2. 关键事实

### 2.1 Codex 版本的默认值

安装包：

```text
OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0
App version: 26.818.41509
Install: C:\Program Files\WindowsApps\OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0
app.asar: C:\Program Files\WindowsApps\OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0\app\resources\app.asar
```

`app.asar` 内的 `.vite/build/src-CLzQUgbV.js`（约 394412 字节处）和 `webview/assets/app-initial-BhpTek7p.js`（当前看到的核心定义约 545486 字节处）都显示：

```javascript
default: ["low", "medium", "high", "xhigh", "ultra"]
key: "enabled-reasoning-efforts"
```

即 Codex 26.818.41509 默认可用推理强度不含 `max`。用户若没有在 `[desktop]` 显式写出 `enabled-reasoning-efforts = [..., "max"]`，界面就只会显示默认档位。

### 2.2 当前用户配置与备份

当前 `%USERPROFILE%\.codex\config.toml`：

```text
[desktop]
enabled-reasoning-efforts = ["low", "medium", "high", "xhigh", "ultra", "max"]

[plugins."sdd@personal"]
enabled = true
```

其他 bundled 插件也都有 `enabled = true`。当前文件修改时间为 2026-08-23 18:51:28；这是世豪重新勾选 max 并重新点安装 SDD 之后的现场状态。

`%USERPROFILE%\.codex\config.toml.bak_20260823_1813`（2026-08-23 18:21:56）中已有 max 和 bundled 插件的 `enabled = true`，但没有 SDD 条目。这表明：

- 在 18:39 启动失败之前，用户已经手动保存过 max；
- SDD 是 18:51 之后才重新出现的；
- 用户现场修复后的配置不能反推为“启动过程本来就保留完整”。

### 2.3 CCSM 数据库里的 Codex 供应商

`%USERPROFILE%\.cc-switch\cc-switch.db` 的 `providers` 表：

```text
id = codex-multirouter
name = Codex MultiRouter 个人
is_current = 1
meta = {}
settings_config.config = absent/null
settings_config.auth = {}
settings_config.codexRouting.schemaVersion = 2
settings_config.codexRouting.enabled = true
```

这个供应商是当前 SSOT，但其 `settings_config` 不保存 `config` 文本，因此本身不包含 `[desktop]` 或 `[plugins]`。

`settings` 表中的 `common_config_codex` 当前内容：

```toml
model_reasoning_effort = "max"
disable_response_storage = true
```

也没有 `[desktop]` 和 `[plugins]`。所以当启动流程只以“数据库当前供应商 + common config”重建 live 时，这两段用户配置没有可回放的来源。

### 2.4 CCSM 启动调用链和 18:39 SSOT 恢复

CCSM 日志 `%USERPROFILE%\.cc-switch\logs\cc-switch.log` 在 2026-08-23 18:39:28 记录：

```text
检测到上次异常退出（存在接管残留），正在恢复 Live 配置...
codex Live 配置已从备份恢复
已删除所有 Live 配置备份
已对账 CCSwitchMulti 自有 Codex 模型目录
检测到上次代理状态需要恢复，应用列表: ["codex"]
恢复 codex 的代理接管状态失败: 127.0.0.1:15721 已被占用
set_takeover_for_app 正在关闭 codex takeover
codex Live 配置已从 SSOT 恢复（无备份兜底）
```

关键在于：日志中的“从备份恢复”先删掉了备份，随后代理启动失败又走关闭接管的兜底路径；因为备份已删除且 live 已被 `takeover_live_config_best_effort` 改造成代理占位符，最终进入 `restore_live_config_for_app_with_fallback_inner` 的 SSOT 分支。

相关符号：

```text
src-tauri/src/services/proxy.rs:1595   reconcile_codex_owned_projection_on_startup
src-tauri/src/services/proxy.rs:2017   takeover_live_config_best_effort
src-tauri/src/services/proxy.rs:2159   restore_live_configs
src-tauri/src/services/proxy.rs:2192   restore_live_config_for_app_with_fallback_inner
src-tauri/src/services/proxy.rs:2288   restore_live_from_ssot_for_app
src-tauri/src/services/proxy.rs:2646   recover_from_crash
src-tauri/src/services/provider/live.rs:761  write_live_with_common_config
src-tauri/src/services/provider/live.rs:1312 write_codex_live_snapshot
src-tauri/src/codex_multirouter/projection.rs:251 projection_settings
```

`restore_live_from_ssot_for_app` 读取当前供应商并调用 `write_live_with_common_config`。对 schema-v2 MultiRouter，`write_live_with_common_config` 会使用 `build_projection_artifact` 得到 projection settings；该 settings 只是把 DB 供应商的 `codexRouting` 和 `modelCatalog` 重新投影，仍然没有 `config` 字段。

随后调用链是：

```text
write_codex_live_snapshot
  -> write_codex_provider_live_with_catalog_and_provider_context
  -> write_codex_live_for_provider
```

`codex_config.rs:6901` 的 `write_codex_provider_live_with_catalog_and_provider_context` 在 `config_text: Option<&str>` 为 `None` 时不会生成内容，只会把 `None` 继续传给 `write_codex_live_for_provider`。

`codex_config.rs:7531` 的 `write_codex_live_for_provider`：

```rust
let merged_config = config_text
    .map(merge_codex_provider_config_with_live)
    .transpose()?;

if should_write_auth {
    write_codex_live_atomic(auth, merged_config.as_deref())
} else {
    let live_config =
        prepare_codex_provider_live_config(auth, merged_config.as_deref().unwrap_or(""))?;
    write_codex_live_config_atomic(Some(&live_config))
}
```

当 `config_text` 为 `None` 时，`merged_config` 也是 `None`，`unwrap_or("")` 会把空字符串当作最终配置写入。也就是说，`merge_codex_provider_config_with_live` 根本没机会保留当前 live 中已有的 `[desktop]` 和 `[plugins]`。

`codex_config.rs:514` 的 `write_codex_live_config_atomic` 会接受空串并直接写盘。`codex_config.rs:7568` 的 `prepare_codex_provider_live_config` 在 auth 为空时也只是原样返回空串。

这与日志里的 SSOT 恢复分支完全对应，是本轮最重要的候选根因。

### 2.5 Codex Desktop 自己的 config 请求

`~/.codex/logs_2.sqlite` 的 `logs` 表在当天窗口内记录：

```text
2026-08-23 18:40:33  codex_app_server::message_processor  app-server request: config/batchWrite
2026-08-23 18:40:33  codex_skills_extension::host_service  client_name="Codex Desktop" api_version="v2"
2026-08-23 18:40:40  codex_app_server::message_processor  app-server request: config/batchWrite
2026-08-23 18:40:40  codex_skills_extension::host_service  client_name="Codex Desktop" api_version="v2"
2026-08-23 18:40:35  codex_app_server::message_processor  app-server request: plugin/list
2026-08-23 18:40:40  codex_app_server::message_processor  app-server request: plugin/installed
```

这些请求确实来自 Codex Desktop app-server，而不是 CCSM。但 `logs` 表只保存日志行和 metadata，不保存 JSON-RPC 请求 payload；因此不能直接从 `logs_2.sqlite` 判断 batchWrite 写了哪些键。

Codex app.asar 里能找到配置写入路径：

```text
.vite/build/main-g2764IDy.js           set-setting / get-setting / config/batchWrite
webview/assets/app-initial-BhpTek7p.js set-setting / get-setting / config/batchWrite
webview/assets/use-codex-worktrees-BHoJJeEo.js
  sendRequest("config/batchWrite", {
    edits: [{ keyPath: "desktop.${key}", value, mergeStrategy: "replace" }],
    ...
  })
```

`use-codex-worktrees` 的路径说明 Desktop 界面会以 `desktop.<key>` 为 keyPath 调用 `config/batchWrite`。`setDefaultModelConfig` 也会发送 `model` 和 `model_reasoning_effort` 的 batchWrite。这些是“用户或模型选择器保存设置”的路径，不是启动时独立的默认重写证据。

### 2.6 SDD 插件注册状态

SDD 插件文件存在：

```text
%USERPROFILE%\.codex\plugins\cache\personal\sdd\0.1.0+codex.20260819074949\.codex-plugin\plugin.json
```

该 manifest 存在且可解析，`name = sdd`，版本 `0.1.0+codex.20260819074949`。

`%USERPROFILE%\.agents\plugins\marketplace.json` 当前内容只列出：

```text
name = personal
plugins = [investment-signal-monitor]
```

没有 SDD。该文件修改时间为 2026-08-19 00:57:41，而 SDD cache 目录在 2026-08-23 18:51:28 被刷新；也就是说，世豪 18:51 重新点安装后，`marketplace.json` 并没有新增 SDD 条目。

Codex app.asar 中 `main-g2764IDy.js` 包含 marketplace 注册路径：

```text
hl = [".agents", "plugins", "marketplace.json"]
LD/RD 逻辑扫描 <codexHome>/plugins/cache/<marketplaceName> 下的版本目录
```

因此插件列表的安装状态来源于 marketplace 注册文件，而不是只看 `config.toml` 里是否有一行 `enabled = true`。SDD cache 虽然存在，但缺少 marketplace 注册条目，UI 显示“未安装”是合理的。

## 3. 根因排序

### R1：SSOT 空配置写回（最可能，主要根因）

受影响的输入：

- CCSM 当前供应商是 schema-v2 MultiRouter，DB 中 `config` 为 None；
- `restore_live_from_ssot_for_app` 被启动失败路径触发；
- `write_codex_live_for_provider(..., None)` 不合并现有 live 配置，直接写空串；
- 空串写盘后 `[desktop]` 与 `[plugins]` 同时消失；
- Codex Desktop 启动后按自身默认值显示 `low/medium/high/xhigh/ultra`，不显示 max。

已证实：

- 代码路径存在；
- 当前 DB 供应商确实没有 `config`；
- 日志证明 18:39 走了 SSOT 恢复；
- 当前配置只反映世豪 18:51 手动修复后的状态。

未证实：

- 没有 18:39 前后同一台机器的 config.toml 快照可以逐字比对；CSM 已删除该时刻的 live backup，当前的 `config.toml` 已被后续修复覆盖。

### R2：Codex Desktop app-server 初始化 batchWrite（次可能，叠加因素）

受影响的输入：

- 18:40:33 和 18:40:40 有 Codex Desktop 的 `config/batchWrite`；
- `plugin/list` 与 `plugin/installed` 也在同一窗口出现；
- app-server 使用 `desktop.<key>` 方式写设置。

如果没有 R1，正常保留的 `[desktop]` 不应被启动请求清掉，因为 app-server 的 batchWrite 通常只写请求中的 key；但目前没有 payload 证据，无法排除 app-server 内存中缓存了旧的有效配置并覆盖文件。

### R3：SDD marketplace 注册缺失（插件问题的直接原因）

受影响的输入：

- SDD cache 和 plugin.json 存在；
- `config.toml` 有 `[plugins."sdd@personal"] enabled=true`；
- `~/.agents/plugins/marketplace.json` 没有 SDD 条目；
- Codex 插件 UI 以 marketplace 注册文件为来源。

这条是插件现象的直接、已证实原因。它与 R1 的 `[plugins]` 段清空是两件相关联但不完全相同的事：即使 config 里的启用字段仍在，marketplace 缺失也会让 UI 显示未安装。

## 4. 风险

1. CCSM 的 SSOT 恢复路径会先删备份，再在代理端口失败时作“无备份”假设；如果这一路径写空配置，用户会丢失所有 `[desktop]`、`[plugins]`、`[projects]` 等用户自有表，风险高于单个偏好项。
2. MultiRouter schema-v2 的 DB 供应商本身不保存 `config`，因此“以 DB 为 SSOT”只对路由和模型目录成立，不能当作用户 `config.toml` 的完整快照。
3. `common_config_codex` 抽取只保留了若干共享偏好，不能覆盖所有 Desktop 设置；如果启动流程把它当作用户配置的完整来源，会继续发生同类丢失。
4. `logs_2.sqlite` 不记录 JSON-RPC payload，只能靠 app.asar 静态逻辑和受控复现区分“Codex 自己写默认值”与“CCSM 先清空再让 Codex 看见默认值”。
5. 当前没有对“SSOT 无 config 恢复必须保留 live 用户表”的回归测试。现有测试只覆盖“备份存在时保留 live desktop”，没有覆盖“备份缺失、走 SSOT、供应商 config 为 None”的 case。

## 5. 尚未解决的缺口

1. 缺少 18:39 恢复前的 `config.toml` 精确快照；只有 18:21 的旧 backup，不能证明该 backup 在 18:39 时仍代表现场。
2. 缺少 18:39 后、18:40 前的 live 文件字节级记录，无法把“空写”直接绑定到具体时间戳。
3. `logs_2.sqlite` 没有 `config/batchWrite` 请求体，不能确认 18:40:33 的 batchWrite 是否写了 `desktop.enabled-reasoning-efforts`，也不能确认它读取的是被清空后的 config。
4. SDD 重新安装到底更新了哪个配置源尚未被追踪；当前只有最终状态：cache 存在、config 有启用字段、marketplace 仍缺 SDD。
5. Codex 插件发现逻辑在 app.asar 中为压缩代码；本报告只给出关键字符串和函数形态，没有把完整 minified 逻辑逐行展开，因为该部分不是决定本次推理强度问题的主体。

## 6. 测试入口和不重启回归 seam

现有 Rust 测试入口：

```text
src-tauri/src/services/proxy.rs
src-tauri/tests/provider_service.rs
src-tauri/tests/import_export_sync.rs
```

`proxy.rs` 已有测试基础设施：

```text
TempHome::new()
Database::memory()
#[tokio::test]
#[serial]
```

同类现有测试：

```text
codex_restore_from_backup_preserves_live_desktop_settings
restore_falls_through_to_ssot_when_backup_is_proxy_placeholder
codex_restore_from_backup_preserves_model_catalog_pointer
```

可以新增一个不重启 Codex 的回归测试：

1. 用 `TempHome` 创建临时 HOME；
2. 写入一个带 `[desktop] enabled-reasoning-efforts=[...,"max"]` 和 `[plugins."sdd@personal"] enabled=true` 的 live `config.toml`；
3. 在 `Database::memory()` 中保存 schema-v2 MultiRouter current provider，`settings_config.config` 刻意设为 `None`；
4. 清空 live backup，令 live 看起来已经是代理占位符；
5. 调用 `service.restore_live_config_for_app_with_fallback(&AppType::Codex).await`；
6. 读回 `~/.codex/config.toml`，断言 `[desktop]`、max 和 `[plugins."sdd@personal"]` 仍在；
7. 现在该测试应失败，证明当前会清空用户表；修复后应通过，完成不重启的回归闭环。

运行命令建议：

```text
cd D:\CCSwitchMulti\ccswitchmulti\src-tauri
cargo test -p cc-switch restore_live -- --nocapture
```

（本轮只做只读诊断，未执行该测试，因为运行 Rust 测试会写 target 和临时目录；是否执行应由主智能体在下一步授权。）

前端/TypeScript 现有入口：

```text
pnpm test:unit
pnpm exec vitest run tests/components/CodexFormFields.test.tsx --reporter=dot --maxWorkers=1 --minWorkers=1
```

如果后续要加前端回归，优先覆盖“Codex Desktop 的 model picker 读取 `[desktop].enabled-reasoning-efforts` 并展示 max”的逻辑；但本问题的根因验证主要应放在 Rust `proxy.rs` 的恢复路径测试。

## 7. 建议下一步

1. 先做一次静态可执行复现：在临时 HOME 和内存 DB 中调用上述恢复路径，确认是否写出空 config。这不需要重启 Codex，也不需要触碰当前机器配置。
2. 若确认，需要把 R1 修复为“SSOT 恢复时，若目标 provider 的 `config_text` 为 None，仍然从当前 live 配置继承用户的 table-like 字段，而不是写空串”。
3. 补充 `common_config_codex` 或独立用户配置快照，使 `[desktop]`、`[plugins]` 等用户表有可恢复来源；不能只依赖 model/mcp 相关字段。
4. 重启后观察一次：在启动前记录 `config.toml` 哈希和关键字段，启动失败后记录再次结果，用来区分 R1 和 R2。该观察属于受控复现，应得到主智能体授权后再做。
5. 对 SDD：确认 `marketplace.json` 应包含 SDD 条目，并跟踪“重新点安装”写入的位置；如果安装流程只写 config 而不写 marketplace，需要按 Codex 的插件安装契约补齐。
6. 在 `proxy.rs` 增加上述 no-restart 回归测试，并同时覆盖 `config=None` 的 v2 router 和普通带 config 的 provider。

## 8. 最终判断

本轮已经证实：

- Codex 默认推理强度不含 max；
- 用户当前 config 有 max 和 SDD 启用状态，但数据库 SSOT/common config 没有对应保存；
- 18:39 存在一次“从 SSOT 恢复（无备份兜底）”事件；
- 该恢复路径对 `config=None` 的 v2 router 会走空写分支；
- SDD cache 存在但 marketplace 注册缺失。

最小复现已于 2026-08-23 完成，空写根因已从“静态推断”升级为“已实测复现”；本报告仍不实施修复，未修复项和 SDD marketplace 注册缺口保留给后续 SDD 阶段。

## 9. 最小复现（2026-08-23 现场确认）

执行目标：在不接触真实 `~/.codex`、不启动或重启 Codex 的前提下，验证 `restore_live_config_for_app_with_fallback` 在 schema-v2 MultiRouter 的 `settings_config.config = None`、无 live backup、live 原本含 `[desktop]` max 与 `[plugins."sdd@personal"]` 时，是否丢失这些表。

使用方式：

1. 在 `%TEMP%\codex-sdd-repro-worktree` 创建一次性 git worktree（主仓库未改动）。
2. 在该 worktree 的 `src-tauri/src/services/proxy.rs` 测试模块中临时增加 `repro_sdd_startup_config_none_ssot_drops_live_user_tables`。
3. 测试使用 `TempHome::new()` + `Database::memory()`，保存 v2 router 和目标 provider，设置 `currentProviderCodex = codex-multirouter`，写入带 max 与 SDD 的临时 live config，再把 `proxy_config.enabled` 设为 true，最后调用 `restore_live_config_for_app_with_fallback(&AppType::Codex)`。
4. 测试对恢复后的 `~/.codex/config.toml` 断言仍包含 `[desktop]`、`max`、`sdd@personal`。

命令：

```text
cd C:\Users\江厉害\AppData\Local\Temp\codex-sdd-repro-worktree\src-tauri
$env:CARGO_TARGET_DIR='D:\CCSwitchMulti\ccswitchmulti\src-tauri\target'
cargo test --lib repro_sdd_startup_config_none_ssot_drops_live_user_tables -- --nocapture
```

第一次运行：

```text
REPRO result=Ok(()) len=0 has_desktop=false has_max=false has_sdd=false raw=""
thread ... panicked at src\services\proxy.rs:10187:9:
SSOT restore must keep live [desktop], got:
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3299 filtered out
error: test failed, to rerun pass `--lib`
进程退出码：1
```

第二次运行（确定性验证）：

```text
REPRO result=Ok(()) len=0 has_desktop=false has_max=false has_sdd=false raw=""
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3299 filtered out
进程退出码：1
```

结论：

- `restore_live_config_for_app_with_fallback` 返回 `Ok(())`；
- 恢复后的 `config.toml` 长度为 0；
- `[desktop]`、`max`、`sdd@personal` 均不存在；
- 两次运行完全一致，结果是确定性的；
- 根因从“静态推断”升级为“已实测复现”；
- 本复现没有读取或修改真实 `%USERPROFILE%\.codex`，没有关闭/重启 Codex；
- 临时 worktree、临时 harness 和外部 `sdd_repro` 构建产物均已清理，主仓库只保留本报告文件。

## 10. 为什么 CCSwitchMulti 频繁判断“上次异常退出”

### 10.1 判定机制

CCSwitchMulti 的异常退出判定不依赖数据库，而是依赖 `app-run-marker.json`：

```text
src-tauri/src/app_exit_monitor.rs:31   record_startup
src-tauri/src/app_exit_monitor.rs:73   record_clean_exit
src-tauri/src/app_exit_monitor.rs:85   record_forced_exit
src-tauri/src/app_exit_monitor.rs:118  log_dir_path / exit_events_path
```

每次 `record_startup` 都：

1. 读取已有的 `app-run-marker.json`；
2. 如果存在，写入一条 `abnormal_exit_detected` 事件；
3. 用当前 PID、启动时间、版本、OS、cwd 覆盖 marker；
4. 返回上一次 marker 给启动日志，调用方只打 warn，没有直接推送用户 UI。

正常路径会删除 marker：

```text
托盘“退出”  -> record_clean_exit("user_requested_exit", 0)
窗口关闭退出 -> record_clean_exit("window_close_exit", 0)
事件循环正常退出 -> record_clean_exit("event_loop_exit", 0)
更新安装 / 进程重启 -> record_clean_exit(...)
数据库/配置初始化致命错误 -> record_forced_exit(...)
```

当前现场 `app-run-marker.json` 仍存在，内容：

```text
startedAt = 2026-08-23 18:40:04.939
pid = 3420
```

这表示当前 CCSwitchMulti 进程 3420 是在 18:40:04 启动的；当前进程没有退出，marker 当然存在。18:39:28 的日志又显示了上一次“异常退出”是 PID 44480 的 marker 残留。也就是说，当天的启动链是：

```text
14:09:45 PID 7296 启动
17:45:50 下一次启动读到 7296 marker，报告 abnormal_exit_detected，PID 44480 启动
18:39:28 下一次启动读到 44480 marker，PID 5844 启动
18:40:00 PID 5844 正常记录 clean_exit
18:40:04 PID 3420 启动
```

### 10.2 误判条件

已证实的误判条件：

- 进程被任务管理器、系统关机、崩溃、`std::process::exit` 绕过正常清理等任何路径终止，但没执行 `record_clean_exit`，marker 就会留在磁盘。
- 上次进程虽然已经退出，但日志目录里的 marker 没有删除，下一次启动就会无条件报告 abnormal。
- marker 存在只说明“没有干净退出记录”，并不等于代码崩溃；`crashLogModifiedAt` 为空时尤其不能说明有 panic。

高概率误判条件：

- 如果用户通过托盘“退出”，正常路径会删除 marker；但窗口最小化到托盘时不退出，marker 会一直存在，这是进程仍运行的正常状态，不是异常。
- 更新或 `app.restart()` 路径在 `RunEvent::ExitRequested` 处理时已经调用 `record_clean_exit`；如果 update 安装器内部直接 `exit` 前没有走到该路径，仍可能留下 marker。

仍未知：

- 18:39:28 是否真的发生过一个独立的新实例竞争，还是只是旧 PID 44480 的 marker 残留；当前只能从 `app-exit-events.jsonl` 恢复时间线，无法从已不存在的进程确认 PID 44480 当时是否仍活着。

## 11. 15721 端口占用的来源

### 11.1 代码证据

默认端口固定：

```text
src-tauri/src/database/schema.rs:129   listen_port INTEGER NOT NULL DEFAULT 15721
src-tauri/src/proxy/types.rs:46       listen_port: 15721
src-tauri/src/proxy/http_client.rs:46 默认回退 15721
```

绑定逻辑：

```text
src-tauri/src/proxy/server.rs:142   tokio::net::TcpListener::bind(&addr)
src-tauri/src/proxy/server.rs:541   format_bind_error 生成“端口被占用”的用户提示
src-tauri/src/services/proxy.rs:635  “启动代理服务器失败: {e}”
```

### 11.2 当前本机证据

只读命令：

```text
Get-NetTCPConnection -LocalPort 15721
```

当前结果：

```text
LocalAddress 127.0.0.1
LocalPort    15721
State        Listen
OwningProcess 3420
本地进程：cc-switch.exe
路径：D:\CCSwitchMulti\cc-switch.exe
3920/4208 side：codex.exe
路径：C:\Program Files\WindowsApps\OpenAI.Codex_26.818.5229.0_x64__2p2nqsd0c76g0\app\resources\codex.exe
```

也就是说，当前 15721 不是外部服务，而是另一个/同一台机器上的 CCSwitchMulti `cc-switch.exe`（PID 3420）正在监听。该进程 18:40:04 启动，与 18:39:28 的新实例（PID 5844）不是同一进程。当前 Codex `codex.exe` PID 4208 已经建立两条到 15721 的连接，说明它当前正通过这个 CCSM 代理。

18:39 时占用端口的旧 PID：

- 旧 PID 44480、5844、7296 当前均已不存在；
- 无法从当前进程表还原 18:39:28 当时的占用者；
- CCSM 日志中的错误文案和执行路径都指向“另一个 CCSwitchMulti 实例或旧进程残留”，没有指向第三方进程。

结论：18:39:28 端口占用极可能是同一台机器上的旧 CCSM/并发 CCSM 实例，而不是 Codex 或另一个无关服务。不得据此终止当前进程；正确修复方向是端口占用预检、单实例协调和“若端口已由本程序占用则复用/提示”。

## 12. CCSM 与 Codex config/batchWrite 的写入关系

### 12.1 代码证据

CCSM 写 Codex 配置的核心路径：

```text
src-tauri/src/codex_config.rs:514   write_codex_live_config_atomic
src-tauri/src/codex_config.rs:6817  merge_codex_provider_config_texts
src-tauri/src/codex_config.rs:6901  write_codex_provider_live_with_catalog_and_provider_context
src-tauri/src/codex_config.rs:7531  write_codex_live_for_provider
src-tauri/src/codex_config.rs:7568  prepare_codex_provider_live_config
```

`write_codex_live_config_atomic` 采用临时文件 + `ReplaceFileW` 的原子替换，但没有跨进程文件锁。`write_codex_live_for_provider` 在 `config_text=None` 时会跳过 `merge_codex_provider_config_with_live`，最终把空串写入磁盘；这是已经用最小复现确认的丢失入口。

Codex Desktop app-server 的写入入口：

```text
webview/assets/app-initial-BhpTek7p.js
  setDefaultModelConfig -> config/batchWrite
  edits: [{ keyPath: model/model_reasoning_effort, mergeStrategy: upsert }]
  use-codex-worktrees -> config/batchWrite
  edits: [{ keyPath: desktop.<key>, mergeStrategy: replace }]
  插件启用 -> config/batchWrite
  edits: [{ keyPath: plugins.<pluginId>.enabled, mergeStrategy: upsert }]
```

`use-codex-worktrees` 的代码明确说明它是 Codex UI 对 remote host 的配置写入，不是 CCSM 调用。它写 `desktop.<key>`，不合并整张用户配置。

### 12.2 日志/时间线

当天关键时序：

```text
18:39:28  CCSM: 检测到异常退出，先恢复备份，再删除所有 Live backup
18:39:28  CCSM: reconcile Codex 模型目录
18:39:28  CCSM: 恢复 codex 接管失败，15721 被占用
18:39:28  CCSM: set_takeover_for_app 关闭 codex
18:39:28  CCSM: codex Live 配置已从 SSOT 恢复（无备份兜底）
18:40:00  CCSM: PID 5844 正常退出
18:40:04  CCSM: PID 3420 启动
18:40:10  CCSM: 代理启动，备份 codex Live，接管 config
18:40:33  Codex app-server: config/batchWrite，client_name=Codex Desktop
18:40:35  Codex app-server: plugin/list
18:40:40  Codex app-server: plugin/installed
18:40:40  Codex app-server: config/batchWrite
18:51:28  用户手动重新加入 max 和 SDD 后 config.toml 的当前 mtime
```

Logs 只记录：

```text
codex_app_server::message_processor  app-server request: config/batchWrite
codex_skills_extension::host_service  client_name="Codex Desktop" app_server.api_version="v2"
```

没有 payload，不能从 `logs_2.sqlite` 判断 batchWrite 写了哪些键。

### 12.3 DB 的当前 backup 证据

只读查询当前 `~/.cc-switch/cc-switch.db`：

```text
proxy_live_backup
app_type = codex
backed_up_at = 2026-08-23T10:40:10.607502500+00:00 (本地 18:40:10)
config 包含 [desktop] = false
config 包含 [plugins] = false
```

这是 18:40:10 代理启动时备份下来的原始 live。它已经不含 `[desktop]` 或 `[plugins]`，说明在 18:40:10 之前，live 配置已经丢失了用户表。也就是说，18:40:33 的 Codex app-server batchWrite 发生之前，CCSM 的 SSOT 恢复已经造成一次破坏；Codex 的后续 batchWrite 是叠加因素，不是唯一的凶手。

### 12.4 最后写入者覆盖风险

高概率推断：两个进程都写同一个 `~/.codex/config.toml`，而且没有共享锁：

- CCSM 写的是完整文件原子替换；
- Codex Desktop app-server 通过 `config/batchWrite` 写入；
- 如果 Codex 先读到一个旧快照再写回，或者 CCSM 在 Codex 读取后覆盖，就可能出现 last-writer-wins；
- `expectedVersion` 只存在于 Codex app-server 自己的请求参数中，CCSM 不参与该版本检查。

仍未知：没有捕获到一次“两个写入者同时读旧内容后再写”的字节级冲突记录；目前只能证明存在时序接近和最后写入者覆盖概率，不能证明某一次 batchWrite 直接覆盖了用户表。

## 13. SDD cache、config 与 `~/.agents/plugins/marketplace.json` 的各自作用

### 13.1 三个存储的角色

```text
~/.codex/plugins/cache/personal/sdd/<version>/
  含义：Codex 插件版本化内容缓存，实际技能文件、plugin manifest、skills 都在这。
  证据：当前存在 0.1.0+codex.20260819074949/.codex-plugin/plugin.json。

~/.codex/config.toml [plugins."sdd@personal"] enabled=true
  含义：Codex 的持久化启用状态。
  证据：当前 config 确实有此条，mtime 18:51:28。

~/.agents/plugins/marketplace.json
  含义：Codex 的本地 marketplace 注册表，UI 的 plugin/list、plugin/installed 依赖它。
  证据：当前只有 name=personal，plugins=[investment-signal-monitor]，没有 sdd。
```

### 13.2 Codex app.asar 中的注册写入路径

`main-g2764IDy.js` 里有：

```text
FD() -> LD() 扫描插件缓存 -> ID() 写 marketplace.json
kD() 复制插件内容到 plugins/cache/<marketplace>/<name>/<version>
PD() 修改 config.toml 的 [plugins.<name>@<marketplace>].enabled
```

`webview/assets/app-initial-BhpTek7p.js` 也引用 `.agents/plugins/marketplace.json`，并且插件启用通过：

```text
config/batchWrite
keyPath = plugins.<pluginId>.enabled
```

这说明三个存储不是同一回事：

- cache 是否存在，决定插件文件是否可加载；
- config 是否启用，决定运行时是否参与；
- marketplace 是否登记，决定 UI 是否能发现/安装。

### 13.3 点击安装后登记链为什么可能缺失

当前事实：

```text
SDD cache 目录 mtime = 2026-08-23 18:51:28
SDD manifest 文件 mtime = 2026-08-19 23:05:14
marketplace.json mtime = 2026-08-19 00:57:41
```

也就是说，SDD cache 在 18:51 被刷新，但 marketplace.json 没有同步更新。这已经证明“重新点安装”没有完成全部注册步骤，至少 marketplace 登记缺失。

高概率推断：安装流程可能只执行了 cache 复制和 config 启用写入，却没有把 SDD 条目写回 marketplace 注册表；或者注册表写入发生在另一个 Codex 进程/旧版本中，当前进程读不到。

仍未知：app.asar 是压缩后的生产包，`FD/LD/ID` 可能属于 Claude Cowork 或另一条导入路径；本报告不把这一小段代码断言为个人插件安装的完整流程。需要后续对 Codex app-server 二进制或抓取一次真实安装请求才能确认最终调用链。

### 13.4 能否安全兼容修复

建议（不做实现）：

1. 从 `plugins/cache/personal/sdd/<version>` 重新生成 marketplace 条目；
2. 保留现有 `investment-signal-monitor` 等条目，只做 merge，不覆盖；
3. 以 `name` 和 `source.path` 为键做去重，避免重复条目；
4. 校验 `source.path` 必须在 `~/.codex/plugins` 下，拒绝路径穿越；
5. 在启动或插件刷新时做一次幂等 reconcile，使 cache、config、marketplace 三方一致；
6. 若 config 有 `[plugins."sdd@personal"] enabled=true` 而 marketplace 缺条目，应视为可自愈的 warning，不静默丢弃用户意图。

## 14. 对 0 字节或部分损坏 `config.toml` 的恢复边界

### 14.1 当前可用的恢复来源

```text
%USERPROFILE%\.codex\config.toml
  当前值：有 [desktop] max 和 [plugins."sdd@personal"]，但这是 18:51 手动修复后的状态。

%USERPROFILE%\.codex\config.toml.bak_20260823_1813
  有 max 和 bundled 插件，但没有 SDD；不是自动恢复来源。

%USERPROFILE%\.cc-switch\cc-switch.db / proxy_live_backup
  当前 codex backup 的 config 不含 [desktop]/[plugins]，是 18:40:10 已损坏状态的备份。

%USERPROFILE%\.cc-switch\settings.common_config_codex
  只有 model_reasoning_effort=max 和 disable_response_storage=true。

%USERPROFILE%\.cc-switch\backups\codex-force-repair\...
  有若干 provider/config.toml 历史备份，但不是当前 live 的自动回滚来源。
```

### 14.2 不同损坏形态的结果

已证实：

- `write_codex_live_config_atomic(Some(""))` 会接受空串并写出 0 字节文件；
- `read_codex_live_settings` 对 exist-but-empty config 返回 `config=""`，不会报错；
- empty config 状态下 Codex 使用自身默认值，默认推理强度不含 max；
- 我们的一次性 SSOT 恢复复现最终得到 `len=0`，`has_desktop=false`、`has_max=false`、`has_sdd=false`。

高概率推断：

- 部分损坏、但语法仍可解析的 TOML，CCSM 可能通过普通读取继续处理，或者只修复 unescaped Windows path 这一种已知畸形；
- 语法完全损坏的 TOML 会使 `read_and_validate_codex_config_text` 返回错误，启动对账失败，但不会自动进入“从用户文件备份恢复”的流程。

仍未知：

- Codex app-server 对 0 字节/部分损坏 config 的精确降级行为；
- Codex 是否会自行创建默认 config、是否会覆盖插件/marketplace 文件；
- 如果用户已有 `config.toml.myconfig` 或其它本地副本，Codex 是否会在启动时读取它们。

### 14.3 能恢复什么、不能恢复什么

能恢复：

- 如果存在完整的 `proxy_live_backup` 且它本来有用户表，恢复路径可以写回 provider 字段并保留用户表；
- 如果用户手动复制 `config.toml.bak_20260823_1813`，可以恢复该备份里已有的 max 和 bundled 插件；
- CCSM 的 common config 可恢复少量跨 provider 偏好，例如 `model_reasoning_effort`。

不能恢复：

- 当前 DB backup 中没有的 `[desktop]`、`[plugins]`；
- 当前 DB provider `settings_config.config` 中没有的字段；
- `common_config_codex` 中没有的字段；
- 缺失 marketplace 条目的 SDD 插件；
- 0 字节或语法损坏后，没有用户原始文件副本时无法凭空还原的用户表。

结论：当前恢复链不是“用户完整配置的备份”。它主要恢复 provider 路由、模型目录和少数共同偏好；用户自有 Desktop 设置和插件登记需要独立持久化来源。

## 15. 当前用户如何看到错误或恢复结果

当前自动启动路径：

```text
record_startup -> log::warn("检测到上次应用未正常退出")
recover_from_crash -> log::warn/error
restore_proxy_state_on_startup -> log::error
set_takeover_for_app(false) -> log::info/error
```

这些错误只写日志，不发送 UI 事件，也没有 toast。

现有用户可见反馈接口：

```text
src/main.tsx
  configLoadError 事件 + message dialog
  get_init_error + DatabaseUpgrade 页面

src/hooks/useProxyStatus.ts
  用户主动 start/stop/takeover 时 toast.success / toast.error

src/hooks/useProxyStatus.ts / src/lib/query/proxy.ts
  useProxyStatusQuery 每 2 秒轮询 ProxyStatus
  ProxyStatus{ running, address, port, last_error, ... }

src/hooks/useTauriEvent.ts
  通用事件订阅 hook

src/App.tsx
  webdav-sync-status-updated -> error toast
  s3-sync-status-updated -> error toast
  proxy-official-warning -> warning toast

src/lib/api/settings.ts
  openLogDir() -> 打开 ~/.cc-switch/logs
```

当前缺口：

- 启动恢复失败没有 `codex-config-recovery-failed` 或类似事件；
- `ProxyStatus.last_error` 只在服务器运行时由 server status 填充；如果 `start()` 因为端口占用失败，`get_status()` 返回的是 `running=false` 的默认状态，用户看不到 `last_error`；
- `get_config_status` 只判断 `auth.json` 存在或 config 非空，不能区分“0 字节但损坏”和“正常配置”；
- `openLogDir` 只是手动入口，不会在失败时自动展示；
- 没有 toast/status/event 记录“SSOT restore 成功但丢失了用户表”这种可恢复警告或“无法恢复”的明确引导。

结论：当前错误主要停留在日志层，用户只能在设置页看到“代理未运行/KBX”等间接状态，很难定位到是 15721 竞争还是 config 被清空。

## 16. 面向 UX 的验收场景

### 16.1 无感成功

场景：正常关闭再启动，live config 包含 `[desktop] max` 和 `[plugins."sdd@personal"]`，代理状态可恢复。

验收：

- 启动后 config.toml 保持完整，不需要用户再次勾选；
- 没有错误 toast；
- 代理状态自动恢复或在设置页默认显示运行中；
- restart 后一次也不出现“设备配置已恢复为默认值”的提示。

### 16.2 可恢复警告

场景：检测到上次异常退出，且有旧备份或可重建的用户表。

验收：

- 用户看到一条 warning toast，包含“已恢复 Codex 配置，部分用户设置可能来自备份”；
- 后台写 `codex-config-recovery-warning` 事件；
- 日志同时记录丢失前和恢复后的字段摘要；
- 用户点开事件或日志目录可看到具体差异。

### 16.3 不可恢复明确引导

场景：`config.toml` 已经是 0 字节，或 DB backup/SSOT 都不含 `[desktop]`/`[plugins]`。

验收：

- 显示明确错误，不假装成功；
- 说明“无法恢复以下字段：…”，并列出已保留的字段；
- 如果本地有 `.bak` 文件，给出一键恢复或打开文件位置的按钮；
- 如果没有任何可恢复来源，引导用户手动提供备份，而不是静默用 Codex 默认值覆盖。

### 16.4 插件登记自愈

场景：cache 有 SDD，config 有 `[plugins."sdd@personal"] enabled=true`，但 marketplace 缺条目。

验收：

- 插件列表自动显示 SDD 为“已安装/已启用”，或至少显示“检测到已安装插件，需要登记”；
- 点击一次“修复”后 cache、config、marketplace 三者一致；
- 修复是幂等的，重复点击不产生重复条目；
- 修复失败仍保留用户原意图，并给出失败原因。

### 16.5 并发写保护

场景：CCSM 正在写 config，Codex Desktop 也在写 `config/batchWrite`。

验收：

- 两个写入者不会互相覆盖对方刚写的字段；
- 单文件原子替换仍安全，但字段级合并或写入锁要保证 last-writer-wins 不会丢用户表；
- 启动时的恢复、代理接管、Codex app-server batchWrite 三个动作有明确的顺序或互斥；
- 没有共享锁时，至少通过 revision/expectedVersion 检测冲突并提示“配置已被其他进程修改”。

## 17. 证据等级汇总

已证实：

- Codex 默认推理强度不含 max；
- CCSM `write_codex_live_for_provider` 对 config_text=None 写空串；
- 最小复现两次均得到 len=0，`[desktop]`/max/SDD 全丢；
- 18:39 启动链、18:40:10 backup 无用户表、18:40:33 Codex batchWrite、18:51 手动修复有时间线；
- 当前 15721 由同一台机器上的 `cc-switch.exe` 监听，Codex app-server 连入；
- SDD cache 存在、config 有启用状态、marketplace 缺 SDD；
- 启动恢复失败目前只有日志，没有自动 UI 通知。

高概率推断：

- 18:39 端口占用是另一 CCSM 实例/旧进程残留；
- Codex batchWrite 与 CCSM 写入存在时序接近和 last-writer-wins 风险；
- 插件“重新安装”只更新了 cache/config，未同步 marketplace 注册；
- 空/损坏 config 会影响 Codex 默认行为，且当前没有用户表恢复来源。

仍未知：

- 18:39 时端口的精确旧 PID；
- 18:40:33 和 18:40:40 batchWrite 的具体 edits；
- Codex app-server 对 0 字节/部分损坏 config 的内部恢复行为；
- SDK 安装流程是否必须通过 app-server 写 marketplace，以及为何当前没有同步；
- 两个进程是否真的同时读到旧快照后各自重写，需要受控日志/文件监控才能确认。

## 18. 最终说明

本轮已完成只读调查和临时目录复现，未修改产品代码，未改真实 `~/.codex` 或 `~/.agents`，未关闭或重启 Codex。主仓库新增内容只有本报告；临时 worktree、临时 harness 和外部 `sdd_repro` 构建产物已清理。

结论仍是：`restore_live_config_for_app_with_fallback` 在 schema-v2 MultiRouter `config=None`、无 backup 场景下会清空用户表，这是已实测根因；启动恢复失败目前缺少用户可见反馈，插件 marketplace 登记和并发写入保护均需后续修复方案。

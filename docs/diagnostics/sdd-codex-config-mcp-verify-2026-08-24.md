# SDD Codex 配置与 MCP 独立验证报告

日期：2026-08-24  
状态：`PASS`（Task 1–9 的阶段 A、B、C 均通过；允许恢复独立 Sol 最终审核）  
验证 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`  
基线 HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`

最新标准结论以文末“Sol 对 Task 8/9 及 outcome 生命周期修复的独立复核”为准；前文 `BLOCKED`、`NOT_STARTED` 和“待 Sol 复核”均为按时间保留的历史记录。

## 流程偏离记录

- 2026-08-24：256 提供方额度耗尽。世豪明确授权由 Sol 临时代替 256 执行本轮 `sdd-verify`。
- 本轮 Sol 只承担替代验证，不承担产品实现，也不承担最终收尾审核。
- 本轮严格按阶段 A（规格符合性）→阶段 B（代码质量）→阶段 C（交叉与广泛回归）执行。阶段 A 发现重要问题后，按验证门规则停止，没有进入阶段 B 或阶段 C。

## 阶段 A：规格符合性复核

结论：`BLOCKED`。主体 Codex 配置保护的乐观并发契约尚未满足。用户级 MCP 定向集成测试已经独立复跑通过，但 attempt 所有权、companion 条件提交、raw API 私有化和 takeover backup 的 MCP 来源边界仍有重要规格不符合项，不能进入阶段 B 或阶段 C。

2026-08-24 顺序修复复审：A-1 已清零。Luna 已把 provider 合并移入 `write_codex_live_config_reconcile()` 的每轮 live 快照闭包，并为官方 auth 写入补充失败回滚。替代验证 Sol 独立复跑 3 个新增回归，全部退出码 0、各 1 passed / 0 failed / 3334 filtered out。继续扫描阶段 A 全部写入路径时发现新的重要问题 A-2，因此阶段 A 仍为 `BLOCKED`。

2026-08-24 第二次顺序修复复审：A-2 的两条 read-transform-write 入口已经移入 reconcile 每轮闭包；替代验证 Sol 独立复跑 4 个新增回归，全部退出码 0、各 1 passed / 0 failed / 3338 filtered out。继续复核 projection side-effect 回滚时发现新的重要所有权问题 A-3，因此阶段 A 仍为 `BLOCKED`。

2026-08-24 第三次顺序修复复审：A-3 已限制为只捕获和恢复带 CCSwitchMulti managed marker 的 agent 文件；替代验证 Sol 独立复跑 targeted 回归，退出码 0、1 passed / 0 failed / 3339 filtered out。继续完成所有 atomic writer 生产调用者扫描时发现 takeover 与 force-repair 仍有预生成全文窗口 A-4，因此阶段 A 仍为 `BLOCKED`。

### 重要问题 A-1：provider 写入的首次指纹晚于 live 读取与合并，仍可覆盖并发用户配置

严重度：重要。

证据：

- `src-tauri/src/codex_config.rs:7059-7061` 的 `merge_codex_provider_config_with_live()` 先调用 `read_codex_config_text()`，再基于这次无指纹快照完成 provider 合并。
- `src-tauri/src/codex_config.rs:7719-7728` 的 `write_codex_live_for_provider()` 把已经合并好的候选文本交给 `write_codex_live_atomic()` 或 `write_codex_live_config_atomic()`。
- `src-tauri/src/codex_config.rs:551-575` 的 `write_codex_live_config_optimistic()` 到候选文本已经产生以后才首次在 559 行记录 live 指纹；当 559 行与 564 行指纹相同，575 行直接替换文件。
- 因此，如果 Codex Desktop 在 7060 行读取之后、559 行首次记录指纹之前写入新的 `[desktop]`、`[plugins]`、`[mcp_servers]` 或未知用户表，559 行和 564 行都会观察到同一个新指纹，写入器不会重读，575 行会用基于旧快照的候选覆盖新内容。
- `src-tauri/src/codex_config.rs:11982-11995` 的现有测试只通过测试钩子在 559 行记录指纹以后、564 行复检以前修改文件，所以它没有覆盖上述入口前窗口。

这违反已批准主规格的裁定 9、行为契约 8.8 和 A 层验收标准：读取原始字节与指纹必须属于同一次快照，合并后再复检；任何并发修改都不能被旧快照覆盖。

复核命令：

```powershell
$p='src-tauri/src/codex_config.rs'
$lines=Get-Content -LiteralPath $p
foreach($range in @(@(547,590),@(673,683),@(7058,7062),@(7702,7729),@(11980,12026))){
  for($i=$range[0]-1;$i -lt [Math]::Min($range[1],$lines.Count);$i++){
    '{0,5}: {1}' -f ($i+1),$lines[$i]
  }
}
```

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib write_codex_live_config_atomic_rechecks_fingerprint_before_replace -- --nocapture
```

测试结果：退出码 0；1 passed，0 failed，3331 filtered out。该结果只证明“首次指纹之后”的冲突会重试，不能证明“provider 合并之后、首次指纹之前”的冲突安全。

预期结果：provider 写入器在同一轮中读取 live 原始字节和对应指纹，基于该快照合并 provider 字段，并在原子替换前复检；任一时点发生并发修改都重新读取并重新合并，超过重试上限则保持最后外部内容并返回 `ConcurrentModificationDeferred`。

实际结果：provider 路径先无指纹读取并合并，随后才启动指纹复检。入口前窗口内的并发修改不会被识别，仍可能丢失用户配置。

修复建议：让 provider 写入路径也通过 `write_codex_live_config_reconcile()` 的闭包形态完成“每轮读取、合并、复检”，或者让底层写入器接收 provider delta 而不是预合并全文。新增一个 RED 测试，在 provider live 读取/合并完成后、写入器首次指纹前写入外部用户表，并证明修复后该表保留或冲突超限时最后外部字节保持不变。

A-1 复审结果：`RESOLVED`。

### 重要问题 A-2：MultiRouter 目录发布与切回内建 OpenAI 仍保留相同的入口前竞态

严重度：重要。

证据：

- `src-tauri/src/codex_config.rs:6417-6423` 的 `publish_codex_multirouter_projection()` 先读取 live 配置，再基于该无指纹快照生成完整 `prepared` 文本，之后才调用 `write_codex_live_config_atomic()`。
- `src-tauri/src/codex_config.rs:7040-7043` 的 `force_codex_builtin_openai_live_provider()` 同样先读取 live、生成完整变换结果，之后才调用 `write_codex_live_config_atomic()`。
- `write_codex_live_config_atomic()` 使用的首次指纹仍发生在候选文本已经生成以后。若 Codex Desktop 在上述读取/变换结束以后、写入器首次指纹以前修改用户表，首次指纹和替换前复检会同时观察到新文件，旧候选会直接覆盖该并发修改。
- 生产调用点包括 `src-tauri/src/codex_multirouter/projection.rs:104`、`src-tauri/src/codex_multirouter/mutation.rs:39`、`src-tauri/src/commands/provider.rs:290`、`src-tauri/src/services/provider/mod.rs:4904` 和 `:4908`。

这仍违反主规格裁定 9、行为契约 8.8 与“所有 Codex live 写入路径”的 A 层目标。MultiRouter 目录发布属于 CCSwitchMulti 拥有的目录投影，切回内建 OpenAI 属于明确的 provider 字段清理；两者都必须保留并发产生的用户配置。

复核命令：

```powershell
$p='src-tauri/src/codex_config.rs'
$lines=Get-Content -LiteralPath $p
foreach($range in @(@(6401,6424),@(7039,7044),@(675,709))){
  for($i=$range[0]-1;$i -lt [Math]::Min($range[1],$lines.Count);$i++){
    '{0,5}: {1}' -f ($i+1),$lines[$i]
  }
}
rg -n "publish_codex_multirouter_projection\(|force_codex_builtin_openai_live_provider\(" src-tauri/src
```

预期结果：目录投影和切回内建 OpenAI 都在 `write_codex_live_config_reconcile()` 的每轮闭包中读取当前 live、生成候选、复检指纹；冲突时重新执行变换，超限时保持最后外部字节。

实际结果：两条路径在乐观写入器外部读取和变换 live；入口前并发修改不会触发重试。

修复建议：把两条 read-transform-write 操作分别移入 reconcile 闭包，并各新增一个“变换完成以后、写入器首次指纹以前发生外部写入”的 RED 测试。目录发布还必须明确 catalog/cache side effect 与 config 写入冲突失败之间的次序和恢复语义，避免 config 延迟写入后留下看似成功的投影副作用。

A-2 入口复审结果：`RESOLVED`。目录副作用的用户文件所有权问题转为 A-3。

### 重要问题 A-3：projection 失败回滚会覆盖既有用户 agent 文件的并发修改

严重度：重要。

证据：

- `src-tauri/src/codex_config.rs:6450-6465` 的 `CodexProjectionSideEffectsSnapshot::capture()` 会读取 `agents` 目录中的所有普通文件，没有使用 `codex_agent_file_is_cc_switch_managed()` 限定 CCSwitchMulti 所有权。
- `src-tauri/src/codex_config.rs:6504-6506` 的 `restore()` 会把 `agent_files` 中的每个原文件按快照字节写回，同样没有所有权检查。
- 只有删除 projection 运行期间新建的文件时，`src-tauri/src/codex_config.rs:6495-6499` 才检查扩展名和 CCSwitchMulti managed marker。这个检查不能保护捕获时已经存在的用户 agent 文件。
- 因此，如果 projection 在 config 冲突或非法 live TOML 后回滚，同时用户修改了一个既有自定义 agent 文件，回滚会把用户新内容覆盖成 projection 开始前的旧快照。
- 当前新增测试 `multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects` 与 `multirouter_projection_invalid_live_toml_keeps_catalog_and_cache_bytes` 只断言 catalog/cache，没有证明用户 agent 文件不会被回滚。

预期结果：失败回滚只恢复 CCSwitchMulti 明确拥有并可能由 projection 修改、创建或删除的 managed agent 文件。用户 agent 文件只能参与名称占用判断，任何回滚都不得重写或删除它。

实际结果：快照和恢复覆盖 agents 目录中的所有普通文件，包含 CCSwitchMulti 不拥有的用户文件。

修复建议：capture 阶段只收集 `codex_agent_file_is_cc_switch_managed()` 为真的文件；restore 也只回写这些已证明所有权的路径。新增 RED：预存无 managed marker 的用户 agent，在 projection transform 后模拟用户改写并制造连续 config 冲突；最终用户新内容必须保留，同时原 managed agent、catalog、models cache 和 backup 按契约恢复。

A-3 复审结果：`RESOLVED`。普通用户 agent 不再进入回滚快照；新建文件删除也复检 managed marker。符号链接会被 `Path::is_file()` 跟随，但只有目标内容同时满足严格 managed marker、同名 `name`、非空 `model` 和 CCSwitchMulti router provider 时才会进入 managed 集合；该边界本轮未发现可复现的重要用户文件覆盖。

### 重要问题 A-4：takeover 与 force-repair 仍在 optimistic writer 外部预生成整份 live 配置

严重度：重要。

证据：

- Takeover 生产路径在 `src-tauri/src/services/proxy.rs:2000-2028`、`:2082-2091`、`:2160-2196` 和 `:3605-3622` 先读取当前 live 并变换完整配置；统一出口 `:4072-4125` 再投影和准备完整文本，最后调用 `write_codex_live_config_atomic()`。读取/变换结束以后到首次 fingerprint 之间的外部用户配置修改仍会被旧候选覆盖。
- 代理活跃时的供应商同步在 `src-tauri/src/services/proxy.rs:500-526` 先把一次旧 live 快照的用户表复制进 effective settings，再进入相同 takeover 写入口，同样保留旧快照窗口。
- 显式 force-repair 在 `src-tauri/src/services/provider/mod.rs:5152-5203` 先读取并修复完整 live，然后调用 atomic writer；finalize 在 `:5231-5239` 再次先读取、组合并修复完整文本，随后调用 atomic writer。
- force-repair 失败回滚 `src-tauri/src/services/provider/mod.rs:5175-5191` 直接把操作开始时的 `original_live` 整份写回，没有 fingerprint 或字段所有权限制，会覆盖操作期间 Codex Desktop 产生的用户修改。

预期结果：takeover 和 force-repair 的每次“从 live 变换配置”都必须发生在 `write_codex_live_config_reconcile()` 每轮当前快照闭包中。失败回滚只能撤销本次 owned/provider 字段，或者在 fingerprint 仍匹配本次写入结果时精确恢复；不得无条件覆盖外部新内容。

实际结果：两组生产路径仍把预生成全文交给只从更晚时点开始记录 fingerprint 的 atomic writer；force-repair 回滚还绕过该 writer 直接覆盖。

修复建议：为 takeover 构建一个接收当前 live 文本的纯变换闭包，把代理字段、目录投影和 provider-owned 字段每轮重做；为 force-repair 的初写、finalize 和回滚建立同样的 current-live/owned-field reconcile 契约。新增 RED 覆盖 takeover transform 后并发更新 `desktop/plugins/mcp_servers/未知表`，以及 force-repair 初写、finalize、失败回滚三个外部修改时点。

### 停止逐点修复后的结构性只读审计

世豪要求暂停逐点修复后，替代验证 Sol 穷举了生产代码中的 Codex live 写入和回滚入口。以下分类只描述当前实现，不表示阶段 A 已通过。

#### 分类结论

- 只看 `config.toml` 提交边界：A 类安全路径 6 组，B 类危险路径 5 组，C 类“已证明安全的整份精确快照恢复”0 组。
- 把 catalog、models cache、cache backup 和 managed agent 纳入同一事务边界后，另有 2 组 B 类副作用问题。因此本轮共记录 7 组结构性危险面。
- `src-tauri/src/services/proxy.rs:602` 的热切换回滚虽然意图是恢复本次操作前的精确快照，但它没有携带 expected fingerprint，也没有证明文件仍等于本次操作写入的结果，因此归 B 类，不归 C 类。
- `src-tauri/src/services/provider/live.rs:1121-1148` 的 `LiveSnapshot::restore()` 也会直接写回或删除整份 Codex config；当前生产代码没有调用它，只有测试调用，因此不计入生产调用点数量，但它是一个应当收窄或删除的潜在绕过 API。

#### A 类：每次重试都从当前 live 计算变更

| 组 | 生产入口 | 调用路径与裁定 |
|---|---|---|
| A-S1 | `src-tauri/src/mcp/codex.rs:422-440`、`:530-551`、`:556-578` | MCP 全量对账、单条写入、单条删除都在 `write_codex_live_config_reconcile()` 闭包内重新读取当前 live，并且只对数据库明确拥有的 MCP id 做增删。 |
| A-S2 | `src-tauri/src/codex_config.rs:411-429`、`:7937-7958` | 普通 provider config-only 与 auth+config 路径都在 current-live 闭包内重新合并 provider 字段；auth 写入若 config 提交失败会恢复旧 auth。调用者包括 `services/provider/live.rs:1332-1349`、`services/config.rs:174`、`services/proxy.rs:3281-3289`、`:4033-4036`、`:4057-4065` 和 `:4450-4451`。 |
| A-S3 | `src-tauri/src/codex_config.rs:7342-7382` | 退出接管后的 config-only provider 写入，无配置时显式只清理 owned 字段，有配置时按当前 live 合并。目录副作用的提交问题另列 B-X1。 |
| A-S4 | `src-tauri/src/codex_config.rs:6546-6577` | MultiRouter projection 的 `config.toml` 候选在每轮 current-live 闭包内重新生成。它的副作用回滚仍有独立问题 B-X2。 |
| A-S5 | `src-tauri/src/codex_config.rs:7193-7201` | 切回内建 OpenAI 每轮只从当前 live 删除 CCSwitchMulti/provider-owned 字段。 |
| A-S6 | `src-tauri/src/services/proxy.rs:4270-4379`、`:4382-4469` 中 `merge_existing_config=true` 的分支 | 健康备份、SSOT 恢复和普通 verbatim restore 仍以当前 live 为底，只叠加备份的 provider 字段；当前 OAuth 被保留。`merge_existing_config=false` 不属于本组。 |

#### B 类：预生成全文、旧全文回滚或跨文件副作用没有 attempt 所有权

| 组 | 生产入口 | CCSwitchMulti 实际拥有的字段 | 可能丢失或错配的数据 | 失败与回滚语义 |
|---|---|---|---|---|
| B-1 takeover 全文出口 | 统一出口 `src-tauri/src/services/proxy.rs:4072-4125`；入口 `:500-526`、`:2000-2028`、`:2082-2091`、`:2160-2196`、`:3605-3622` | 本地 router 的 `model/model_provider/model_catalog_json`、当前 CCSM provider 表、local `base_url`、`wire_api`、proxy bearer token、系统代理策略和目录投影 | transform 后到 writer 首次 fingerprint 前写入的 `desktop`、`plugins`、`projects`、`memories`、`mcp_servers`、marketplace 或未知表可被整份旧候选覆盖 | proxy placeholder 分支保留 `auth.json`，但 catalog、models cache、backup、managed agent 已在 config 提交前改变；config 冲突失败时没有统一恢复。启动接管虽然会尝试从 DB backup 恢复 live，但该恢复本身不能撤销所有副作用。 |
| B-2 force-repair 初写 | `src-tauri/src/services/provider/mod.rs:5152-5203` | 仅旧 `agents.max_threads` 到 canonical 字段的迁移、已知 Windows notify 路径规范化，以及随后 provider 修复所需字段 | 从 `original_live` 读取以后产生的任何用户表或未知字段修改 | 目标 Provider DB 行先在 `:5194-5201` 保存为 repaired 版本，再写预生成的整份 live。写入失败后调用旧全文回滚。 |
| B-3 force-repair finalize | `src-tauri/src/services/provider/mod.rs:5230-5239` | 最终 provider/router 字段和已知 repair 字段 | `final_live` 读取以后产生的任何用户修改；预切换补回逻辑只列出 `mcp_servers/projects/plugins/memories`，不是一般用户字段所有权证明 | finalize 失败后仍调用同一个旧全文回滚；此前 normal switch 或 takeover 产生的 auth、catalog、cache、agent、MCP 和状态副作用没有成组撤销。 |
| B-4 force-repair 失败回滚 | `src-tauri/src/services/provider/mod.rs:5175-5191`，调用点 `:5205`、`:5217`、`:5242` | 只应撤销本次 repaired Provider 行、current-provider 指针和本次 owned live delta | 操作期间 Codex Desktop 写入的全部用户数据都可能被 `original_live` 无条件覆盖 | 回滚顺序是 Provider DB 行、DB current、local settings、旧 live 全文。它不恢复 `auth.json`，不恢复 outgoing provider backfill/common config，不恢复 catalog/cache/cache backup/managed agent，不关闭可能已启用的 takeover，也不恢复 takeover DB backup 或代理 enabled 状态。 |
| B-5 hot-switch preparation 回滚 | 快照捕获 `src-tauri/src/services/proxy.rs:3230-3238`；失败调用 `:3296-3339`；回滚写入 `:576-608`；raw 分支 `:4382-4455` 的 `merge_existing_config=false` | 只应撤销同一次 hot-switch 的 provider/auth/live 变更 | 快照捕获后的 `desktop/plugins/projects/memories/未知表` 更新；当前非 OAuth auth 更新；同时可能错配 catalog/cache/agent | DB live backup 在 `:589-596` 先恢复，随后旧 live 快照写回。false 分支只把当前 live 的 MCP 合并到旧 snapshot，其他用户表仍以旧 snapshot 为准；没有 expected fingerprint，所以不能证明是精确回滚。 |
| B-X1 provider/restore 的目录副作用先于 config 提交 | `src-tauri/src/codex_config.rs:7298-7333`、`:7359-7382` 和 `src-tauri/src/services/proxy.rs:4414-4423` | CCSM catalog、owned cache 投影、cache backup、managed agent，以及 live 中的 catalog 指针 | config 因非法 live 或连续冲突未提交时，catalog/cache/agent 仍可能已经切到新 Provider；cache 是 Codex 与 CCSM 共用文件，不能当作无条件整文件所有权 | 普通 provider 的 config/auth 路径只回滚 auth，不回滚 catalog/cache/backup/agent。restore 的 inline `modelCatalog` 分支也在进入 current-live writer 前产生这些副作用。 |
| B-X2 projection 副作用旧快照回滚 | 捕获 `src-tauri/src/codex_config.rs:6450-6497`；恢复 `:6499-6539`；调用 `:6559-6576` | generated catalog 与 CCSM managed agent；models cache 只在带 CCSM etag 时拥有部分投影语义；backup 是跨 attempt 恢复材料 | config 冲突期间 Codex 写入的新 cache 元数据，或另一 CCSM attempt 写入的新 catalog/cache/backup/managed agent 可能被旧快照覆盖 | `files` 中的 catalog、cache、backup 都无 fingerprint 地整份恢复或删除。A-3 已保护普通用户 agent，但 managed agent、catalog、cache 和 backup 仍没有“只回滚本 attempt 写入结果”的条件。 |

#### C 类：允许的显式精确快照恢复

当前生产调用点为 0 组。一个调用点只有同时满足以下条件才可进入 C 类：快照在本次事务开始时捕获；提交记录本次写入后的 fingerprint；回滚时 current fingerprint 仍等于该写入结果；auth 与每个 companion file 也分别满足同样条件。当前 `write_codex_live_snapshot(..., false)` 只满足“有旧快照”，不满足后面三个条件。

#### 绕过点核查

- 活跃生产代码对真实 Codex `config.toml` 的直接 `write_text_file` 绕过只发现 force-repair 回滚 `services/provider/mod.rs:5188-5191`。
- `codex_config.rs:685` 与 `:745` 是两个底层 writer 自身的最终原子替换，不是额外调用者。
- `services/provider/mod.rs:1092`、`:1161` 以及 `services/proxy.rs:4677` 之后、`codex_config.rs:8202` 之后的直接写入都在测试模块内。
- `services/provider/live.rs:1121-1148` 的 dormant `LiveSnapshot::restore()` 没有生产调用者，但保留了无指纹的全文写入/删除能力。
- `src-tauri/src/lib.rs:49` 仍公开 re-export `write_codex_live_atomic`，使 raw 全文 API 的可调用面大于实际需要；结构修复应一并收窄。

### 重要问题 A-5：跨文件 projection 副作用没有并发所有权与条件回滚

严重度：重要。

证据：

- `prepare_codex_config_text_with_model_catalog_impl()` 在 `src-tauri/src/codex_config.rs:6411-6418` 直接写 catalog、models cache 和 managed agent，然后调用者才尝试提交 `config.toml`。
- 普通 provider 和 restore 调用者没有 side-effect snapshot；因此 config 因非法 live 或连续冲突失败时，新 catalog/cache/agent 可以保留在磁盘上。
- MultiRouter projection 虽然在 `:6450-6497` 捕获快照，但 `:6500-6527` 无条件整份恢复旧 catalog/cache/backup/managed agent。该恢复没有确认 current 文件仍是本次 attempt 写入的内容。
- `models_cache.json` 会被 Codex 自己刷新；当前逻辑只在读取时判断 CCSM etag，不能证明 restore 时文件仍归本次 attempt 所有。catalog、backup 和 managed agent 即使属于 CCSM，也可能由另一并发 CCSM attempt 更新。

预期结果：projection 先生成纯计划，不在 reconcile 闭包内直接产生可见副作用。每个文件都记录变更前 fingerprint 和本 attempt 写入后的 fingerprint；提交失败时只恢复仍等于本 attempt 输出的文件，遇到外部变化就保留现场并标记 `projection_pending`。

实际结果：config 的 current-live 语义已经修复，但 companion files 仍可能在 config 未提交时残留，或者在失败回滚时覆盖更新后的共享文件。

修复建议：引入纯 `CodexProjectionPlan` 与 attempt id。先生成并验证所有目标字节，再按 expected fingerprint 提交 companion files，最后提交 config；任一步失败只对 fingerprint 仍匹配本 attempt 输出的文件做补偿。无法安全补偿时保留外部内容并持久化 pending/recovery outcome，不能强写旧快照。

### 统一最小架构边界

1. 普通更新只能提交 delta/reconciler。调用者传入“要改哪些 owned 字段”，writer 每次重试都从同一轮 current live 重新应用变更。
2. `write_codex_live_config_atomic()`、`write_codex_live_atomic()` 和任何 raw 全文 restore API 必须私有化。普通 takeover、repair、provider、MCP、恢复路径不能调用它们。
3. 精确恢复必须使用显式类型，例如 `ExactCodexSnapshot { bytes, fingerprint }` 和 `CommittedAttempt { after_fingerprint }`。只有 current fingerprint 等于本 attempt 的 after fingerprint 时才允许恢复旧字节。
4. auth、catalog、models cache、cache backup、managed agent 和 DB backup 都必须具有独立 expected fingerprint 或 attempt ownership。回滚只能撤销本 attempt 仍然拥有的结果。
5. projection 函数必须拆成纯计划与提交两步。纯计划不得写文件；提交失败时记录 `projection_pending` 或明确 recovery outcome，不能用旧全文强行制造表面一致。

### 必须先补的 RED 矩阵

| 范围 | 外部修改注入点 | 必须证明的结果 |
|---|---|---|
| takeover 全局、strict、best-effort、active provider sync、pool-policy reprojection | 每个入口完成 transform 后、writer 第一次读取 fingerprint 前 | 当前 `desktop/plugins/projects/memories/mcp_servers/未知表` 保留；冲突超限时最后外部字节不变。 |
| takeover 统一出口 | catalog/cache/agent 已准备、config 连续冲突 | config 保持外部版本；catalog/cache/backup/managed agent 恢复或保持新外部版本；不得残留伪成功投影。 |
| force-repair initial | `original_live` repair 完成后、初写 writer 入口前 | 并发用户表保留；非法 live 时 Provider DB 行和所有文件保持原状态。 |
| force-repair finalize | `final_live` 读取与组合完成后、final writer 入口前 | finalize 每轮基于当前 live；`desktop/marketplaces/未知表` 等不在四项白名单中的用户表同样保留。 |
| force-repair failure rollback | 初写、normal switch、takeover 或 finalize 之后分别修改 live/auth/catalog/cache/backup/agent | 回滚只撤销本次 owned delta；外部修改不被旧 `original_live`、旧 auth 或旧 companion file 覆盖；DB/settings/proxy takeover 状态回到一致状态。 |
| hot-switch exact-snapshot 边界 | direct write 后、settings 或 DB current 提交失败前修改非 MCP 用户表与 auth | 没有 expected fingerprint 时拒绝全文回滚；有匹配 fingerprint 时才恢复；不匹配时保留外部内容并报告 deferred。 |
| 普通 provider 与健康备份/SSOT restore | catalog/cache/agent 写入后让 config reconcile 非法或连续冲突 | config 失败不能留下不对应的 catalog/cache/agent；当前 OAuth 和用户表保持。 |
| projection snapshot rollback | capture 后由另一方更新 catalog、models cache、backup、managed agent | rollback 不覆盖另一方更新；共享 cache 的未知顶层元数据保留。 |
| API 边界 | 编译期或结构测试枚举 raw writer 调用者 | 除 typed exact-restore 模块外，生产代码无法调用 raw 全文 API；新增调用会使测试失败。 |

## 阶段 B：代码质量复核

结论：`NOT_STARTED`。阶段 A 的重要问题尚未清零，按 `sdd-verify` 规则不得进入阶段 B。

## 阶段 C：交叉与广泛回归

结论：`NOT_STARTED`。阶段 A 的重要问题尚未清零，按 `sdd-verify` 规则不得进入阶段 C。

本轮只运行了用于固定问题证据的单条测试：

| 命令 | 退出码 | 通过数 | 失败数 | 说明 |
|---|---:|---:|---:|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --lib write_codex_live_config_atomic_rechecks_fingerprint_before_replace -- --nocapture` | 0 | 1 | 0 | 只覆盖首次指纹之后的注入点；不能证明完整并发契约 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib write_codex_live_for_provider_preserves_change_after_provider_merge_before_writer_entry -- --nocapture` | 0 | 1 | 0 | A-1 修复复审通过；3334 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_write_defers_after_bounded_merge_conflicts_and_rolls_back_auth -- --nocapture` | 0 | 1 | 0 | 连续冲突时保持最后外部 config，并回滚 auth；3334 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_write_rejects_invalid_live_toml_without_touching_config_or_auth -- --nocapture` | 0 | 1 | 0 | 非法 live TOML 时 config 原字节保持、auth 回滚；3334 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib force_builtin_openai_preserves_change_after_transform_before_writer_entry -- --nocapture` | 0 | 1 | 0 | A-2 切回 OpenAI 入口复审通过；3338 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib multirouter_projection_preserves_change_after_transform_before_writer_entry -- --nocapture` | 0 | 1 | 0 | A-2 projection 入口复审通过；3338 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects -- --nocapture` | 0 | 1 | 0 | 仅证明 catalog/cache 回滚；未覆盖用户 agent；3338 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib multirouter_projection_invalid_live_toml_keeps_catalog_and_cache_bytes -- --nocapture` | 0 | 1 | 0 | 仅证明非法 TOML 时 catalog/cache 保持；未覆盖用户 agent；3338 filtered out |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib multirouter_projection_conflict_preserves_user_agent_edit_and_owned_agent_rollback -- --nocapture` | 0 | 1 | 0 | A-3 用户 agent 所有权复审通过；3339 filtered out |

既有失败：本轮未运行广泛回归，未重新分类既有格式失败。Luna 报告中的既有 `cargo fmt --check` 和 `pnpm format:check` 基线尚未由本轮独立复核。

新失败：A-1、A-2、A-3 已修复并通过范围复审；结构性只读审计确认 A-4 与 A-5 两组重要规格不符合项。A-4 覆盖 takeover、force-repair 和 hot-switch rollback 的全文写入/回滚；A-5 覆盖 provider/projection 的 catalog、cache、backup 和 managed-agent 副作用。两组都尚无完整 RED 矩阵实现。

## Luna 第三版 A-4/A-5 修复证据（待 Sol 独立复核）

本节记录 Luna 提交后的实现与定向证据，尚未由 Sol 独立复核；因此不改变上文阶段 A 的 `BLOCKED` 结论，也不宣称 A-4/A-5 已通过。实现目标仍是“兼容性优先、统一 current-live reconcile、跨文件条件回滚”。

### provider_commands 失败、根因与最小修复

首次运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test provider_commands -- --test-threads=1
```

失败断言为 `src-tauri/tests/provider_commands.rs:439`：`live file must replace stale provider MCP entries with the DB projection`。根因是统一 provider 合并已经以 current live 为底，但 provider 快照里的旧 `[mcp_servers.latest]` 被递归合并进候选；后续 ownership-aware MCP 对账把它误判为 live-only，于是旧供应商 MCP 被保留。其余测试随后因测试互斥锁被首个 panic 污染，出现 `PoisonError`，不是独立产品失败。

首次最小修复是在 `merge_codex_provider_config_texts()` 进入合并前剥离 provider 快照的根级 `mcp_servers` 与历史 `[mcp.servers]` 容器（新增 `strip_codex_provider_mcp_tables()`）；该尝试随后暴露了公共配置片段新增 MCP 被误删的问题。最终实现撤销通用 merge 的无条件剥离，改为只在 `build_effective_settings_with_common_config()` 的 provider 快照克隆上剥离，再应用公共配置片段；provider 快照仍原样保存在数据库，live-only MCP 继续由 current live 与数据库 ownership 对账。由于剥离后可能出现只有 MCP 的空 provider 配置，provider projection writer 先与最新 live 合并，再应用 bearer token，保持旧快照不复活且兼容空 provider 配置。

Luna 定向复跑记录为：`provider_commands` 10/10；两个公共配置 MCP 回归各 1/1。以上结果仍待 Sol 独立复跑确认。

这属于本轮统一接线后的兼容性回归：它不是无关功能变更，而是“provider 兼容合并”和“用户级 MCP 保护”两条契约在同一入口的交界问题。

### A-4/A-5 待 Sol 复核的 GREEN 证据

| 范围 | 命令结果 |
|---|---|
| 完整 Rust 单元回归 | `cargo test --manifest-path src-tauri/Cargo.toml --lib`：3342 passed，0 failed，5 ignored |
| force-repair initial/finalize/rollback | 6 passed，0 failed |
| takeover 入口竞态 | 1 passed，0 failed |
| hot-switch/direct rollback | 1 passed，0 failed |
| models cache 写入竞态 | 1 passed，0 failed |
| projection conflict side effects | 1 passed，0 failed |
| invalid restore companion 残留 | 1 passed，0 failed |
| restored backup catalog write error | 1 passed，0 failed |
| `provider_service` integration | 37 passed，0 failed |
| `provider_commands` integration | 10 passed，0 failed |
| `import_export_sync` integration | 26 passed，0 failed |
| `mcp_codex_reconcile` integration | 11 passed，0 failed |

第 9 组 projection conflict 测试采用 config-before-companion 提交顺序，断言的是“config 冲突时不提交 companion，并保留外部 `external_third_value`”，不是 companion 已提交后再回滚旧快照。

`profile_roundtrip` 仍有一个既有基线失败：`codex_profile_reapplies_same_multirouter_after_takeover_cleanup` 返回 `codex_multirouter_migration_required`。在未改动的主目录基线同样复现，因此不归因于本轮配置/MCP 兼容性修复；其余 7 项通过。

### API、warning、格式和前端边界

- raw writer 结构扫描显示 `services/provider/mod.rs` 与 `services/proxy.rs` 的剩余调用全部位于 `#[cfg(test)] mod tests`；生产调用数为 0。`src-tauri/src/lib.rs` 已不再根级 re-export raw writer。integration seed 明确从 `cc_switch_lib::codex_config` 引用测试 writer。
- 本轮不再使用的两个旧 prepare API 与无调用测试 helper 已删除；删除后没有新增 dead-code warning。
- `cargo check --manifest-path src-tauri/Cargo.toml --tests` 通过。
- Rust `cargo fmt --check` 仍被工作树既有格式差异阻断；没有对四个既有基线文件或整仓做机械重排。`git diff --check` 通过。
- `pnpm test:unit`：144 files / 1177 tests 通过；`pnpm typecheck` 通过；`pnpm build:renderer` 通过。`pnpm format:check` 仅报告未在本轮改动列表中的 `src/components/providers/forms/CodexFormFields.tsx`，未修改该文件。

本轮仍未访问真实用户 `~/.codex`、`~/.agents`，未结束或重启 Codex，未提交、推送或发布。

## Sol 对 Luna 第三版的独立复核结论

结论：`BLOCKED`。Luna 报告的定向 GREEN 数字可以独立复现，但 GREEN 矩阵没有覆盖“本次写入完成以后、记录 after fingerprint 以前”的窗口，也没有证明 companion 提交本身具备 capture 时的 expected fingerprint。A-4 与 A-5 因此没有清零。Sol 没有进入阶段 B 或阶段 C，也不允许进入独立 Sol 最终审核。

### A-4 复审：attempt 会把写后外部更新误认成本 attempt 输出

严重度：重要。复审结果：`NOT_RESOLVED`。

证据：

- `src-tauri/src/codex_config.rs:901-905` 先写 candidate，再重新读取 current 文件，并把这次读取的 fingerprint 记为 `after_fingerprint`。如果 Codex Desktop 在 901 行写入完成以后、902 行读取以前更新文件，当前 attempt 会把 Codex Desktop 的新内容误认成自己的输出。后续 `restore_if_unchanged()` 在 `:775-785` 看到 fingerprint 相等后，会把外部新内容覆盖成旧 `before` 快照。
- `CommittedCodexAttempt::from_before_and_current()` 在 `src-tauri/src/codex_config.rs:764-772` 没有接收或验证本次预期输出，只把调用时的 current 文件认作本 attempt 输出。生产调用包括 hot-switch 的 config/auth（`src-tauri/src/services/proxy.rs:3372-3388`）和 force-repair 的 auth/finalize（`src-tauri/src/services/provider/mod.rs:5397-5400`、`:5433-5438`）。force-repair 还在 `:5388-5395` 手工使用同一种“事后读取即归本 attempt”构造。
- `CodexProjectionSideEffectsAttempt::finish()` 在 `src-tauri/src/codex_config.rs:6814-6839` 等所有 companion 写入结束以后逐个读取 current fingerprint。较早写完的 companion 如果在 `finish()` 读取以前被 Codex Desktop 或另一个 CCSM attempt 更新，也会被错误认领，随后可能被旧快照回滚。
- 当前测试钩子只覆盖 writer 首次 snapshot 前、transform 后、fingerprint 复检前、companion 写入前和回滚前；没有“candidate/companion 已写完、after fingerprint 尚未记录”的注入点。通过的 force-repair、hot-switch 和 projection 测试不能证明该窗口安全。
- force-repair 的 `restore_original_state()` 在 `src-tauri/src/services/provider/mod.rs:5343-5350` 丢弃 config/auth `restore_if_unchanged()` 的 `false` 返回值，调用方随后仍返回“已恢复原配置”。即使条件回滚正确拒绝覆盖外部更新，错误结果也没有向调用者说明 deferred 状态。

预期结果：writer 必须从自己将要写入的确切 candidate bytes 计算并返回 after fingerprint，不能通过写后重读来推断所有权。复合 config/auth/companion 写入也必须由实际 writer 返回每个文件的 attempt proof。回滚遇到 fingerprint 不匹配时必须明确传播 deferred/recovery outcome，不能声称已经完整恢复。

### A-5 复审：companion 只有条件回滚，没有条件提交

严重度：重要。复审结果：`NOT_RESOLVED`。

证据：

- `write_codex_projection_plan_reconciled()` 在 `src-tauri/src/codex_config.rs:6699` capture companion，在 `:6701-6708` 提交 config，然后在 `:6713` 才调用 `commit_codex_projection_plan()`。capture 与 companion commit 之间，另一个 CCSM attempt 可以更新相同文件。
- `commit_codex_projection_plan()` 在 `src-tauri/src/codex_config.rs:6664-6677` 对 catalog 使用无 expected fingerprint 的 `write_json_file()`，对 managed agent 调用普通写入/删除路径，对 cache restore/backup delete 也没有 expected fingerprint。
- models cache 主文件在 `:6449-6501` 每轮重读并复检，能够保留复检前出现的未知顶层字段；但 backup 准备仍在 `:6400-6416` 使用无条件 `write_json_file()` 或 `fs::copy()`。退出投影在 `:6519-6535` 无条件恢复或删除 cache/backup。
- managed agent 在 `:6006-6032` 使用普通 `write_text_file()`，prune 在 `:6108-6125` 使用普通 `delete_file()`。managed marker 只能证明 CCSM 类型所有权，不能证明文件仍属于本 attempt；另一个 CCSM attempt 在 capture 后写入的新版 managed agent 仍可被覆盖或删除。
- `multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects` 在 config 连续冲突时于 companion commit 前返回，因此只证明 companion 没有提交。测试中的 cache 外部更新发生在 config 冲突期间，不是“companion 已提交后发生外部更新再触发回滚”。

预期结果：catalog、cache、backup 和每个 managed agent 的提交都必须绑定 capture 时的 expected fingerprint，且 writer 返回其确切 candidate fingerprint 作为 attempt proof。无法安全提交或补偿时必须保留外部版本并记录 `projection_pending`/recovery outcome。

### A-6：raw 全文 writer 仍是公共 API

严重度：重要。结果：`OPEN`。

- `src-tauri/src/codex_config.rs:399` 的 `write_codex_live_atomic()` 和 `:943` 的 `write_codex_live_config_atomic()` 仍是 `pub fn`；`src-tauri/src/lib.rs:8` 又公开 `pub mod codex_config`。因此外部调用者仍可使用 `cc_switch_lib::codex_config::write_codex_live_atomic`，并不需要根级 re-export。
- 生产代码扫描确认两个 raw writer 在 `services/provider/mod.rs` 和 `services/proxy.rs` 的剩余调用都位于各自 `#[cfg(test)] mod tests` 中，生产调用数为 0。这个结果只证明当前没有生产调用者，不能证明 API 已私有化。
- `raw_codex_fulltext_writer_is_not_reexported_to_application_callers` 只检查 `src/lib.rs` 的文本中没有函数名。该测试在公共模块仍暴露函数时也会通过，不能落实统一最小架构边界第 2 条和 RED 矩阵的编译期边界。

预期结果：raw writer 至少收窄为私有或 `pub(crate)`，integration seed 使用单独的 test fixture/helper。结构测试必须证明 crate 外部无法编译调用 raw API，而不只是检查根级 re-export 文本。

### A-7：takeover backup 仍会保存并复活 provider 快照里的 stale MCP

严重度：重要。结果：`OPEN`。

- 正常 live 写入在 `src-tauri/src/services/provider/live.rs:751-777` 先从 provider clone 剥离 `[mcp_servers]`/历史 `[mcp.servers]`，再应用 common config；这个顺序可以同时做到 stale provider MCP 不复活、common config 新 MCP 不丢失。
- takeover backup 却在 `:738-749` 调用 `build_effective_settings_with_common_config_inner(..., false)`，明确绕过 provider MCP 剥离。`src-tauri/src/services/proxy.rs:3127-3133` 使用这个专用 helper 重建 hot-switch backup。
- `update_live_backup_from_provider_keeps_new_codex_mcp_entries_on_conflict` 在 `src-tauri/src/services/proxy.rs:9182-9268` 明确把 provider 快照中的 `shared/latest` MCP 当作新定义，并要求它覆盖或进入 backup。退出接管时 `restore_live_config_for_app_with_fallback_inner()` 在 `:2417-2449` 直接恢复该 backup，没有随后执行 DB MCP ownership reconcile；未知 id 会继续被当成 live-only 保留。
- 因此，历史 provider 行里残留但已不属于 MCP DB SSOT 的条目，可以经“provider → takeover backup → restore”重新进入 live。现有 `provider_commands` 10/10 只覆盖普通 provider switch，不能覆盖该 backup 绕过路径。

预期结果：provider-derived backup 与普通 live 写入采用相同来源顺序：先剥离 provider 快照 MCP，再应用 common config，然后只从当前 live/已有健康 backup 合并真正的 live-only MCP。恢复 backup 后仍应按 DB ownership 对账，不能让 provider 快照条目伪装成 live-only 条目。

### Sol 独立定向测试

| 命令/过滤器 | 真实结果 | 边界说明 |
|---|---:|---|
| `cargo test --manifest-path src-tauri/Cargo.toml --lib force_repair_ -- --nocapture` | 6 passed / 0 failed | 3341 filtered out；未覆盖写后、after capture 前窗口 |
| `takeover_global_rebases_on_user_write_before_raw_writer_snapshot` | 1 passed / 0 failed | 3346 filtered out；只覆盖 writer snapshot 前 |
| `codex_direct_live_write_rolls_back_when_provider_commit_fails` | 1 passed / 0 failed | 3346 filtered out；回滚前注入，不覆盖错误认领窗口 |
| `provider_projection_cache_does_not_overwrite_external_update_before_companion_write` | 1 passed / 0 failed | 3346 filtered out；只覆盖 cache 写前注入 |
| `multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects` | 1 passed / 0 failed | 3346 filtered out；config 失败发生在 companion commit 前 |
| `codex_restore_invalid_live_does_not_leave_projection_companions_after_config_failure` | 1 passed / 0 failed | 3346 filtered out |
| `provider_switch_with_restored_codex_backup_propagates_catalog_write_errors` | 1 passed / 0 failed | 3346 filtered out |
| `codex_multirouter_effective_settings_start_from_common_config` | 1 passed / 0 failed | 3346 filtered out；证明 common config MCP 可物化 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test provider_commands -- --test-threads=1` | 10 passed / 0 failed | 普通 provider MCP 对账通过；不覆盖 takeover backup 绕过 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_codex_reconcile -- --test-threads=1` | 11 passed / 0 failed | live-only/DB ownership 定向集成通过；不覆盖错误 attempt ownership |

本轮没有运行 Rust lib 全量、`profile_roundtrip`、格式检查或前端回归，因为阶段 A 已由静态证据阻塞。Luna 报告的 3342/0/5、既有 `profile_roundtrip` 失败和格式基线没有在本次独立复核中重新分类。

## 交接裁定

- A-4 与 A-5 仍未清零；raw API 私有化 A-6 和 takeover backup MCP 来源边界 A-7 也处于 `OPEN`。下一轮实现必须一次性修复“writer 返回确切 attempt proof、companion 条件提交、raw API 收窄、provider/common/live-only MCP 来源分层”，不能继续按单一调用点打补丁。
- 修复前不允许进入阶段 B、阶段 C，也不允许进入独立 Sol 最终审核。下一轮必须补“写后、after fingerprint capture 前”以及“companion capture 后、commit/finish 前”的 RED 注入矩阵。
- 本轮没有修改产品代码，没有访问真实 `~/.codex` 或 `~/.agents`，没有重启进程，没有提交、推送或发布。

## Luna 第四轮修复证据（待 Sol 独立复核）

本节记录 Luna 在 Sol 阶段 A 阻塞后完成的第四轮修复和本地验证。它不改变本报告上方的 `BLOCKED` 状态，也不替 Sol 宣称独立复核已经通过。第四轮按第三版“本质是兼容性设计”的裁定，补齐了 attempt 所有权、companion 条件提交、raw API 边界和 takeover backup 的 MCP 来源边界。

### 四项 RED/GREEN

| 项目 | RED（上一轮缺陷） | GREEN（Luna 本轮证据） |
|---|---|---|
| attempt receipt 错误认领写后外部版本 | Sol A-4 指出 writer 写入后重读 current 并把外部版本当成 after fingerprint | `committed_attempt_does_not_claim_external_write_after_replace_before_receipt`：`1 passed / 0 failed`；after fingerprint 来自 candidate bytes |
| companion 缺少 capture 时条件提交 | Sol A-5 指出 catalog/cache/backup/agent 只有条件回滚，没有 capture fingerprint 的条件提交 | `provider_projection_cache_does_not_overwrite_external_update_before_companion_write`：`1/1`；`multirouter_projection_`：`6/6`；连续三次外部 cache 更新后 `multirouter_projection_conflict_rolls_back_catalog_and_cache_side_effects`：`1/1` |
| raw 全文 writer 是公共 API | Sol A-6 指出两个 raw writer 可以从公共模块使用 | `raw_codex_fulltext_writer_is_not_reexported_to_application_callers` 与 `raw_codex_fulltext_writers_are_not_public_module_api`：`2 passed / 0 failed`；生产 writer 为 `#[cfg(test)] pub(crate)` |
| takeover backup 复活 provider snapshot-only MCP | Sol A-7 指出 takeover backup 绕过普通 MCP 来源剥离 | `update_live_backup_drops_stale_provider_mcp_and_keeps_live_and_common_entries`：`1 passed / 0 failed`；当前 backup 只保留 live 用户 MCP 和 common-config MCP |

### Companion deferred 裁定

Luna 本轮明确采用 deferred 语义。配置已经提交后，如果 catalog、cache、backup 或 managed agent 在 capture 后发生外部变化，提交器不会静默合并，也不会返回成功；它会返回 `codex.live.concurrent_modification_deferred`，保留外部第三版本，并只在 after fingerprint 仍属于本次 attempt 时执行补偿。配置仍由本次 attempt 所有时才恢复旧配置，外部变化时不覆盖外部字节。该裁定覆盖正常投影、失败回滚和 takeover 相关 companion 路径。

### Fixture 与测试布局复核

- `profile_roundtrip` 的 `codex_profile_reapplies_same_multirouter_after_takeover_cleanup` 夹具已经升级为合法 schema v2，加入 `targetProviderId`、`modelSelection`、`authPolicy` 和独立 upstream catalog；生产 migration guard 没有放宽。最终 `profile_roundtrip` 为 `8 passed / 0 failed`，这是既有 fixture 修复。
- `import_export_sync` 当前为 `24 passed / 0 failed`，旧记录为 26 的原因是两个公共 raw-writer persistence/rollback 测试迁入 `codex_config.rs` crate-private 单元测试；`removes_servers_when_none_enabled` 仅改名为 `preserves_live_only_servers_when_none_enabled`，没有少测一项 MCP 行为。

### Luna 本地最终验证

| 命令/测试 | 实际结果 |
|---|---:|
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | `3348 passed / 0 failed / 5 ignored` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| `provider_commands` | `10 passed / 0 failed` |
| `provider_service` | `37 passed / 0 failed` |
| `import_export_sync` | `24 passed / 0 failed` |
| `profile_roundtrip` | `8 passed / 0 failed` |
| `pnpm test:unit` | `144 files / 1177 tests passed / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules，11.54s |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS（本轮已执行 `cargo fmt`） |
| `git diff --check` | PASS |

### Warning 记录

前端 `pnpm test:unit` 没有失败，但输出有 5 类既有 warning：baseline-browser-mapping 过期、React `act(...)`/DOM 属性提示、MSW 未匹配 `tauri.local` 请求、jsdom 下 Tauri window API 错误日志和测试故意触发的错误日志。Renderer build 没有失败，但输出有 4 类既有提示：baseline-browser-mapping 过期、Browserslist 数据过期、subscription 动静态导入提示和 chunk 大于 500KB 提示。`pnpm typecheck` 没有 warning 或 error。Rust `cargo fmt --check` 与 `git diff --check` 均通过。

### 工作树边界

当前实现 worktree 为 `D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`，累计 `38` 个已修改文件和 `4` 个未跟踪 Rust 文件。第四轮核心文件为 `src-tauri/src/codex_config.rs`、`src-tauri/src/services/provider/live.rs`、`src-tauri/src/services/provider/mod.rs`、`src-tauri/src/services/proxy.rs` 及对应 Rust 回归测试。Luna 没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束 Codex，没有提交、推送或发布。

本轮实际 `git status --short` 快照为：

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

Sol 仍需独立复核本节四项 GREEN，当前报告总状态继续保持 `BLOCKED`。

## Sol 对 Luna 第四轮的独立复核结论

结论：`BLOCKED`。第四轮的 config candidate receipt、raw API 和 takeover backup MCP 来源三项实现与定向 GREEN 可以独立复现；companion 主提交路径也已经接入 capture fingerprint。但是 A-4/A-5 的统一边界仍有两个生产绕过：组合 auth 写入仍无 expected fingerprint/条件回滚，force-repair 仍通过 `finish()` 写后重读来推断整组 companion 所有权。阶段 A 因此没有清零。Sol 没有进入阶段 B 或阶段 C，也不允许进入独立 Sol 最终审核。

### 四项第四轮修复的独立裁定

| 第四轮项目 | Sol 裁定 | 独立证据 |
|---|---|---|
| config writer receipt 从 candidate bytes 计算 | `RESOLVED_WITHIN_CONFIG_WRITER` | `CommittedCodexAttempt::from_before_and_bytes()` 在 `src-tauri/src/codex_config.rs:790-796` 直接计算 candidate fingerprint；reconcile writer 在 `:925-931` 不再写后读取。写后第三版本回归 1/1。 |
| catalog/cache/backup/managed-agent capture fingerprint 条件提交 | `PARTIAL` | `commit_codex_projection_plan()` 在 `:6794-6815` 全部经过 attempt；`write_bytes_if_unchanged()`/`delete_if_unchanged()` 在 `:6996-7050` 复检 capture/前次 after fingerprint，并从实际 bytes 记录 receipt。config 后 companion 冲突回归 1/1。但 force-repair 外层仍绕过该 receipt，见下方 A-5。 |
| raw writer crate 外不可用 | `RESOLVED` | 两个函数在 `src-tauri/src/codex_config.rs:425-426`、`:968-969` 均为 `#[cfg(test)] pub(crate)`；生产模块调用扫描为 0，integration tests 不再调用该 API。raw 边界回归 2/2。 |
| takeover backup 丢弃 stale provider MCP | `RESOLVED` | `build_effective_settings_with_common_config_for_backup()` 在 `src-tauri/src/services/provider/live.rs:738-748` 与普通路径一样先剥离 provider MCP，再应用 common config；`services/proxy.rs:3124-3159` 再从健康 backup/current live 补用户配置。stale provider、live-only、common managed 回归 1/1。 |

### A-4 仍未清零：组合 auth 写入没有 attempt ownership

严重度：重要。结果：`OPEN`。

证据：

- 普通 auth+config provider 写入在 `src-tauri/src/codex_config.rs:479-505` 先读取旧 `auth.json`，随后 `write_json_file()` 无条件写入新 auth；config reconcile 失败时，`:496-501` 又无条件写回旧 auth。Codex Desktop 如果在旧 auth 读取以后更新登录态，新写会覆盖它；如果在本次 auth 写入以后、config 失败以前刷新 token，旧 auth 回滚也会覆盖它。
- 带 catalog 的 provider receipt 路径在 `src-tauri/src/codex_config.rs:7860-7875` 同样先读旧 auth、再无条件写新 auth；projection 失败时 `:7899-7907` 无条件恢复旧字节。成功返回的 `auth_attempt` 虽然从实际 candidate bytes 建立 receipt，但这个 receipt 只用于更外层的后续回滚，不能保护写入前竞态或本函数失败分支。
- snapshot projection 在 `src-tauri/src/codex_config.rs:7973-7987` 和 `:8000-8008` 保留相同的无条件 auth 写入/旧字节回滚。生产调用包括 `services/proxy.rs:4502-4508`、`:4569-4572`；普通 provider 入口还从 `codex_config.rs:8618` 进入无 receipt helper。
- 当前没有 auth 写入前、auth 写入后/config 失败前的外部 mutation hook。`provider_write_defers_after_bounded_merge_conflicts_and_rolls_back_auth` 只证明没有外部 auth 更新时可以恢复旧 auth，不能证明 OAuth 刷新不会丢失。

这仍违反统一最小架构边界第 4 条“auth 必须有独立 expected fingerprint 或 attempt ownership”。预期修复是 auth writer 自己捕获 coherent before snapshot、按 expected fingerprint 提交，并从实际 candidate bytes返回 receipt；config/companion 失败时只在 current auth 等于本 attempt candidate fingerprint 时恢复，遇到外部版本必须保留并传播 deferred outcome。

### A-5 仍未清零：force-repair 继续写后重读 companion 并忽略 deferred

严重度：重要。结果：`OPEN`。

证据：

- `CodexProjectionSideEffectsAttempt::finish()` 在 `src-tauri/src/codex_config.rs:7053-7082` 扫描 capture 后出现的 managed agent，并在 `:7079-7080` 对所有文件重新读取 current fingerprint，直接当作本 attempt 的 after fingerprint。外部 managed agent 或 companion 如果在内层写入后、`finish()` 前更新，仍会被错误认领；capture 后由另一个 CCSM attempt 新建的 managed agent还会被记录为 `before=None`，失败回滚可能删除它。
- `finish()` 的生产调用只剩 force-repair：`src-tauri/src/services/provider/mod.rs:5378-5400` 在 `Self::switch()` 外再 capture companion，switch 失败或成功后都调用 `finish()`。内层 projection writer 已经产生精确 `CodexProjectionCommitReceipt`，但 `ProviderService::switch()` 没有把 receipt 传回 force-repair，外层只能重新推断所有权，重新打开了第四轮声称关闭的写后认领窗口。
- force-repair 的 `restore_original_state()` 在 `src-tauri/src/services/provider/mod.rs:5338-5346` 继续丢弃 config/auth `restore_if_unchanged()` 的 `false` 返回值。调用方在 `:5384-5396`、`:5415-5427` 仍可能返回“已恢复原配置”，即使 config/auth 因外部变化实际没有恢复。companion `restore_if_unchanged()` 也只写 warning 后返回 `Ok(())`，没有把逐文件 deferred 传播给调用者。
- 现有 6 个 `force_repair_` 测试没有“内层 companion 写完后、outer finish 前外部更新”注入，也没有“条件恢复返回 false 时禁止显示已完整恢复”的断言。

预期修复是删除生产 `finish()` 推断路径，由实际执行 switch/projection/auth 写入的 writer 向 force-repair 传回 typed receipts；force-repair 只能回滚这些 receipts。任一文件不再匹配本 attempt 输出时，返回值和 recovery outcome 必须明确说明 deferred/部分恢复，不能宣称完整恢复。

### A1–A5、MCP 与第 7/8 项独立定向结果

| 范围 | 独立结果 |
|---|---:|
| 第四轮 config receipt 新回归 | 1 passed / 0 failed |
| 第四轮 config-after-commit companion 冲突新回归 | 1 passed / 0 failed |
| raw API 边界 | 2 passed / 0 failed |
| takeover backup MCP 来源 | 1 passed / 0 failed |
| A-1 provider merge/连续冲突/非法 TOML | 3 passed / 0 failed |
| A-2 OpenAI 与 MultiRouter transform 入口 | 2 passed / 0 failed |
| A-3 用户 agent ownership | 1 passed / 0 failed |
| force-repair | 6 passed / 0 failed |
| takeover/hot-switch/cache/restore/catalog-error 定向 | 5 passed / 0 failed |
| `multirouter_projection_` | 6 passed / 0 failed |
| `provider_commands` | 10 passed / 0 failed |
| `provider_service` | 37 passed / 0 failed |
| `mcp_codex_reconcile` | 11 passed / 0 failed |
| 第 7 项 prefix-only migration | 6 + 1 + 1 passed / 0 failed |
| 第 8 项 wizard + workspace page | 2 files / 77 tests passed / 0 failed |

第 7 项继续保持 prefix-only 冻结当前目录、未来模型不自动扩大、混合选择兼容；第 8 项继续保持新计划不生成 `defaultRouteId`、旧 V2 只读保留且运行时 fail-closed。MCP 11/11 继续证明数据库 managed id 对账、live-only 保留、空库不清表、legacy 迁移和语义幂等没有回归。

### 阶段门裁定

- 阶段 A：`BLOCKED`。A-1、A-2、A-3、raw API 和 takeover backup MCP 来源可以清零；A-4 的 auth attempt ownership 与 A-5 的 force-repair outer `finish()`/deferred 传播仍是重要问题。
- 阶段 B：`NOT_STARTED`。阶段 A 未通过，未进入代码质量门。
- 阶段 C：`NOT_STARTED`。没有独立运行 `profile_roundtrip`、`import_export_sync`、Rust lib 全量、前端全量、build、typecheck 或格式门。Luna 声称的合法 V2 fixture 8/8、import 26→24 测试迁移和广泛回归数字，本轮因阶段门未独立确认，不能改写为 Sol 结论。
- 最终审核：`NOT_ALLOWED`。A 阶段修复并由同一替代 Sol 复核通过以前，不允许进入独立 Sol 最终审核。

本轮没有修改产品代码，没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束进程，没有提交、推送或发布。

## Luna Task 8/9 修复输入（待 Sol 独立复核）

以下是第三版新增启动恢复与端口所有权兼容性范围的实现方交接。它追加在既有验证历史之后，不把 Luna 的自测当作 Sol 的独立裁定，也不改写文档中任何已有的 `BLOCKED` 记录。对新增范围只能记录为“Luna 已修复，待 Sol 复核”。

### Task 8：启动恢复分类的时序契约

生产入口现在先读取旧 marker、`clean_exit`、`panic` 和 `crash.log` 修改时间并完成分类，最后才写入新 marker，不再固定传入 `planned=false`。分类按以下优先级执行：

- 活跃旧 PID 优先，返回 `ActivePreviousInstance`。
- `panic` 或 `crash.log` 晚于旧 marker，且晚于最新 clean exit，返回 `ConfirmedCrash`。
- clean exit 晚于旧 marker，且其后没有更新的 panic/crash，返回 `PlannedRestartOrUpdate`。
- stale marker 且没有更新证据，返回 `UncleanExit`；没有旧 marker，返回 `NoPreviousRun`。

Active/Planned outcome 通过现有 `record_recovery_outcome` 原子持久化并发出事件。Rust outcome、TypeScript 联合类型、四语言 i18n 和 `App.tsx` 已同步；Active 显示 warning，Planned 静默处理。

### Task 8 受控 RED/GREEN

临时把生产分类固定为 Planned 后运行：

```text
cargo test --lib app_exit_monitor --no-default-features -- --nocapture
```

结果为退出码 101，3 项失败；Active 与 Confirmed 实际被错误分类为 Planned。恢复“先读证据、按时序分类、最后写 marker”后，`app_exit_monitor` 为 `9 passed / 0 failed`。这是一组真实的受控 RED/GREEN，不是只修改断言后的绿灯。

### Task 9：端口所有权的 same-major SemVer 契约

`/health` 与 `/status` 使用真实 HTTP 探测。只有以下条件全部满足才允许兼容：应用名属于允许的 CCSwitchMulti 别名；远端、本地版本均为合法 SemVer；major 相同；PID 大于 0。`instance_id` 缺失继续兼容，空或纯空白值拒绝；预发布和 build metadata 后缀不改变 same-major 规则。跨 major、非法/未知版本、错误应用名、无效 PID、空白 `instance_id` 或未知所有者统一返回带 `PORT_OWNERSHIP_GUARD` 前缀的错误。

启动恢复遇到该错误时必须停止 Codex takeover、保留状态，不调用破坏性关闭清理。实现新增直接 `semver = "1"` 依赖，lockfile 只增加现有 semver 包的直接引用。

### Task 9 受控 RED/GREEN 与不兼容 listener E2E

临时恢复“版本非空即兼容”后运行：

```text
cargo test --lib port_probe --no-default-features -- --nocapture
```

跨 major listener 被错误判为 `CompatibleInstance`，而回归期望 `UnknownOwner`，退出码 101。恢复合法 SemVer 且 major 必须相同的规则后，`port_probe` 为 `4 passed / 0 failed`。

不兼容 listener E2E 为 `1 passed / 0 failed`：跨 major listener 被记录为 `PortOwnedByUnknownOwner`；Codex takeover 未启用；本实例未绑定该端口；原 listener 仍能响应，证明恢复流程没有杀掉原进程。

### 新增范围的实现方验证输入

| 验证门 | Luna 实际结果 |
|---|---:|
| `app_exit_monitor` | `9 passed / 0 failed` |
| `recovery_outcome` | `2 passed / 0 failed` |
| `port_probe` | `4 passed / 0 failed` |
| 不兼容 listener E2E | `1 passed / 0 failed` |
| takeover 聚焦回归 | `1 passed / 0 failed` |
| `cargo check --tests` | PASS |
| 串行 Rust `cargo test --tests -- --test-threads=1` | library `3371 passed / 0 failed / 5 ignored`；所有 integration binaries PASS；MCP reconcile `11/11`、MCP commands `23/23`、import/export `24/24`、provider commands `10/10`、provider service `37/37`、profile roundtrip `8/8` |
| 前端 Vitest | `144 files / 1177 tests / 0 failed` |
| TypeScript typecheck | PASS |
| renderer build | PASS，3342 modules |
| Rust fmt | PASS |
| `git diff --check` | PASS |

这些结果仅是 Luna 的实现方输入，尚未完成新增 Task 8/9 的 Sol 独立复核。新增范围不得提前标记为 PASS；当前应标注“Luna 已修复，待 Sol 复核”。本轮未提交、未推送、未访问真实 `~/.codex` 或 `~/.agents`，未重启或结束 Codex。

## Luna 第七轮实现证据（待 Sol 独立复核）

Luna 已在实现 worktree 完成特殊 switch typed receipt 与普通 MCP 最后 writer receipt 收口。以下内容只作为待复核输入，不改写本报告此前的 `BLOCKED` 阶段门：Sol 仍需独立复跑并确认 A-8/A-9、MCP ownership 和所有权边界。

### 本轮兼容性收口

- takeover→official、自动启用本地代理和 taken-over hot-switch 三个特殊分支现在都返回 typed config/auth/companion receipts；`finish_codex_switch_result` / `finish_codex_switch_mutation_result` 只组装 receipts，不再在 finish 阶段读取文件推断 after-state。
- 普通 provider switch 把 MCP reconcile receipt 接入最后 writer 链，避免 provider receipt 在 MCP 写入后过期。MCP 规则是“数据库 id 集合证明所有权”：live-only 用户 MCP 保留，库内禁用项才删除，provider 快照陈旧 MCP 不复活，legacy `[mcp.servers]` 先迁移后清理，语义相同条目不重写。
- 受控旧实现 mutation 已有真实 RED：`force_repair_hot_switch_restores_backup_when_finalize_fails`、普通 switch finish-boundary 与 MCP projection 失败回滚回归曾把外部第三版本错误认领；当前实现均 GREEN。
- 静态审计确认两个 `finish_*` block 内不存在 `capture(state)`、`capture_after`、`read_codex_config_text` 或 `ExactCodexSnapshot::read`，after-state 由 typed receipts 与未修改 runtime-before 组装。

### Luna 本地验证输入

| 验证范围 | 结果 |
|---|---:|
| `force_repair_`（含 A-8 三特殊分支、A-9 auth cleanup、普通 MCP receipt） | `16 passed / 0 failed` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| 串行 Rust 全量 `cargo test -p cc-switch -- --test-threads=1` | library `3363 passed / 0 failed / 5 ignored`；integration binaries 全部 PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | PASS |
| 前端 Vitest | `144 files / 1177 tests / 0 failed` |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS，3342 modules，11.59s |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS |
| `git diff --check` | PASS |

全量前端格式检查仍只报告未修改的 `src/components/providers/forms/CodexFormFields.tsx`；测试/构建中的浏览器数据、React act、MSW/Tauri mock、动态导入和大 chunk 提示均为既有 warning。实现 worktree 当前为 `39 M + 4 ??`，未提交、未推送；本轮没有访问真实用户配置，也没有重启 Codex。上述内容等待 Sol 独立复核后，才能更新阶段 A 和最终审核状态。

## 当前标准状态（第六轮）

- 总状态：`BLOCKED`。
- 阶段 A：`BLOCKED`；A-9 已清零，A-8 的特殊 switch writer→receipt capture 错误认领窗口仍有 1 组重要问题。
- 阶段 B：`NOT_STARTED`。
- 阶段 C：`NOT_STARTED`。
- 独立 Sol 最终审核：`NOT_ALLOWED`。

详细证据见上方“最新结论：Sol 对 Luna 第六轮的独立复核”。

## 最新结论：Sol 对 Luna 第六轮的独立复核

结论：`BLOCKED`。第六轮已经让三个特殊 Codex switch 分支都返回 `CodexSwitchReceipt`，并修复了 A-9 的 auth-only snapshot 与 stale-auth cleanup 两个具体绕过；相关定向测试可以独立复现。但是特殊分支的文件/companion 所有权仍由 switch 完成后的 `after` 重读推断，不是由实际 writer 返回的 candidate receipt。外部第三版本如果出现在特殊分支写入完成后、`after` 捕获前，仍会被错误认领并在 force-repair 回滚时覆盖。A-8 因此仍有一组重要问题，阶段 A 没有清零。Sol 没有进入阶段 B 或阶段 C，也不允许进入独立 Sol 最终审核。

### 第六轮已独立确认的修复

| 项目 | Sol 裁定 | 独立证据 |
|---|---|---|
| 三个特殊 switch 不再返回空 receipt | `RESOLVED_FOR_RETURN_PLUMBING` | `src-tauri/src/services/provider/mod.rs:6312-6316` 在分支前捕获 before；official-exit、auto-takeover、hot-switch 分别在 `:6348`、`:6366-6375`、`:6397-6406` 调用 `finish_codex_switch_result()`；normal switch 也在 `:6410-6411` 统一收口。A 类 finalize-error 回归三条独立 `3 passed / 0 failed`。 |
| state 条件恢复与 deferred 可见性 | `RESOLVED_AFTER_RECEIPT_CAPTURE` | `CodexSwitchStateSnapshot` 覆盖 config/auth/companions、app proxy config、live backup、由 `proxy_config.enabled` 派生的 global takeover、DB/local current 和 proxy running。`restore_files_if_unchanged()` 在 `:544-598` 对每个 after 状态做相等检查并聚合结果；config/current 第三版本回归独立 `2 passed / 0 failed`。 |
| A-9 auth-only snapshot | `RESOLVED` | `src-tauri/src/services/proxy.rs:4513-4525` 在操作开始时捕获 exact auth snapshot；auth-only 分支 `:4612-4621` 调用 `CodexAuthWriteAttempt::write_if_snapshot_unchanged()`。capture 后 OAuth 回归独立 `1 passed / 0 failed`。 |
| A-9 stale-auth cleanup | `RESOLVED` | `src-tauri/src/codex_config.rs:1310-1337` 复检 capture fingerprint 后条件删除，并用 `written_fingerprint=None` 表示 owned tombstone；`services/provider/mod.rs:6801-6818` 把 optional receipt 接入 switch result。cleanup capture 竞态、finalize 失败恢复、外部新 auth deferred 三类回归独立 `3 passed / 0 failed`。 |

### 重要问题 A-8 仍未清零：特殊 switch 的 after snapshot 继续写后重读认领

严重度：重要。结果：`OPEN`。

`CodexSwitchReceipt` 的结构范围已经完整，但特殊分支的所有权证明仍不成立：

- `finish_codex_switch_result()` 在 `src-tauri/src/services/provider/mod.rs:6422-6435` 等特殊分支的全部写入结束后，调用 `CodexSwitchStateSnapshot::capture(state)` 重读 config、auth、companions、proxy config、backup、current 和 proxy running，并把这次读取直接保存为 `after`。
- normal switch 的 config/auth/companion 可以在 `restore_files_if_unchanged()` 的 `:507-523` 使用真实 `CodexProviderWriteReceipt`，但三个特殊分支的 `provider_receipt=None`，只能进入 `:524-541` 的 fallback。fallback 用 `self.config.fingerprint()`、`self.auth.fingerprint()` 和 post-switch companion snapshot 构造 ownership proof；这些 fingerprint 只证明“finish 读取时文件是什么”，不能证明“本次特殊 switch 实际写出了什么”。
- 时序仍然是 before=A → 特殊 switch 写入 B → Codex Desktop 或另一 CCSwitchMulti 实例写入 C → finish 捕获 after=C。finalize 失败时，如果 current 仍为 C，`:525-540` 会判断 current 与 after 相等并恢复 A，从而覆盖外部 C。这与第四轮已经否决的生产 `finish()`/写后重读错误认领完全同型。
- companions、DB backup 和 proxy/current 状态也存在相同的“写入完成后到 after capture 前”窗口。进程内 switch lock 只能串行当前实例的 provider/takeover 操作，不能证明 Codex Desktop 或另一进程没有写文件，也不能把写后读取升级为 attempt receipt。

第六轮 B 类测试的注入点在 force-repair 取得 `Self::switch()` 返回值以后：`src-tauri/src/services/provider/mod.rs:6601-6607` 才修改 companion/config/current/auth。此时 `finish_codex_switch_result()` 已经在 `Self::switch()` 内完成 after capture，因此这些测试只证明“receipt 捕获后的第三版本”会 deferred，不覆盖“特殊 writer 写完后、receipt capture 前”的错误认领窗口。当前没有 before-receipt hook 或等价回归。

预期修复：official-exit、auto-takeover 和 hot-switch 的实际 config/auth/companion writer，以及 proxy config、backup、current 和 proxy running 的状态变更器，必须返回各自基于实际 candidate/committed state 构造的 typed receipt；外层只组合这些 receipts，不能通过写后重读 current 推断所有权。至少新增一个特殊分支 RED，在实际写入后、`finish_codex_switch_result()` 捕获前注入 config 第三版本，并证明 finalize failure 保留该版本且报告 deferred；companion/backup 等隐藏写入也需要同类所有权覆盖。

### 第六轮 Sol 独立定向结果

| 范围 | 独立结果 |
|---|---:|
| A-8 三个特殊分支 finalize-error A 类 | 3 passed / 0 failed |
| A-8 config/current 第三版本 B 类 | 2 passed / 0 failed |
| A-9 auth-only capture 后 OAuth | 1 passed / 0 failed |
| A-9 stale cleanup capture 竞态 | 1 passed / 0 failed |
| A-9 cleanup 后 finalize rollback/deferred | 2 passed / 0 failed |
| `force_repair_` | 15 passed / 0 failed |
| config receipt / companion / raw API / takeover MCP | 1 + 1 + 2 + 1 passed / 0 failed |
| A-1 provider merge/连续冲突/非法 TOML | 3 passed / 0 failed |
| A-2 OpenAI 与 MultiRouter transform | 2 passed / 0 failed |
| A-3 用户 agent ownership | 1 passed / 0 failed |
| takeover/hot-switch/cache/restore/catalog-error 定向 | 5 passed / 0 failed |
| `multirouter_projection_` | 6 passed / 0 failed |
| `provider_commands` | 10 passed / 0 failed |
| `provider_service` | 37 passed / 0 failed |
| `mcp_codex_reconcile` | 11 passed / 0 failed |
| 第 7 项 prefix-only migration | 6 + 1 + 1 passed / 0 failed |
| 第 8 项 wizard + workspace page | 2 files / 77 tests passed / 0 failed |

上述测试全部退出码 0。前端只出现 `baseline-browser-mapping` 数据过期提示。测试结果证明第六轮已有覆盖范围没有回归，但不能替代缺失的“特殊 writer 写后、receipt capture 前”所有权证明。

### 第六轮阶段门裁定

- 严重问题：未发现。
- 重要问题：1 组，A-8 特殊 switch after snapshot 错误认领窗口仍为 `OPEN`。
- 未裁定小问题：未发现需要单独裁定的小问题。
- 阶段 A：`BLOCKED`。A-9 清零；A-8 只完成 receipt 返回接线和 receipt 捕获后的条件恢复，writer 到 receipt capture 的窗口尚未清零。
- 阶段 B：`NOT_STARTED`。阶段 A 未通过，未进入代码质量门。
- 阶段 C：`NOT_STARTED`。阶段 A 未通过，未运行 `profile_roundtrip`、`import_export_sync`、Rust lib 全量、`cargo check --tests`、前端全量、build、typecheck 或格式门。Luna 报告的广泛回归数字仍只是实现证据。
- 最终审核：`NOT_ALLOWED`。A-8 修复并由同一替代 Sol 独立复核通过以前，不允许进入独立 Sol 最终审核。

本轮没有修改产品代码，没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束进程，没有提交、推送或发布。

以下第五轮内容为历史记录；当前标准状态以此前的“当前标准状态（第六轮）”为准。

## Luna 第五轮实现后待 Sol 独立复核

Luna 已在实现 worktree 完成 A-4 auth attempt ownership 与 A-5 force-repair companion receipt 修复，并报告本地验证通过；Sol 尚未独立复核，本验证报告的既有 `BLOCKED`、阶段门和“最终审核不允许”结论保持不变。

Luna 报告的新增回归为 4/4：auth capture 后外部更新、auth commit 后外部更新、force-repair companion 外部更新、companion 条件恢复 deferred。其串行 Rust 全量、前端 144/1177、typecheck、renderer build、fmt 和 diff 门也已通过。上述数字只作为待复核输入，不是 Sol 的独立验证结论。

## Sol 对 Luna 第五轮的独立复核结论

结论：`BLOCKED`。第五轮确实修复了第四轮报告中的两处具体实现：三条组合 auth 写入路径已经使用 typed `CodexAuthWriteAttempt`，普通直连 force-repair 也已经删除外层 `finish()`、使用真实 writer receipt 并传播 companion deferred。但是阶段 A 的统一边界仍有两组重要生产绕过：`SwitchResult.codex_receipt` 没有贯穿三个 Codex 特殊切换分支，另外仍有 auth-only snapshot 写入和 stale-auth 删除绕过 attempt ownership。阶段 A 因此没有清零。Sol 没有进入阶段 B 或阶段 C，也不允许进入独立 Sol 最终审核。

### 第五轮已独立确认的修复

| 第五轮项目 | Sol 裁定 | 独立证据 |
|---|---|---|
| 组合 auth 写入的 attempt ownership | `RESOLVED_WITHIN_THREE_COMBINED_PATHS` | `CodexAuthWriteAttempt::capture_and_write()` 在 `src-tauri/src/codex_config.rs:842-879` 从 coherent before snapshot 捕获旧字节和指纹，commit 前复检，written fingerprint 从 candidate bytes 计算；`:882-898` 的失败恢复会传播 deferred/rollback failed。普通 provider、catalog provider 和 snapshot projection 分别在 `:516-535`、`:7906-7957`、`:8024-8056` 接入。两条新增 auth 回归独立 `2 passed / 0 failed`。 |
| 普通直连 force-repair companion receipt | `RESOLVED_WITHIN_NORMAL_SWITCH_PATH` | 生产 `CodexProjectionSideEffectsAttempt::finish()` 已删除；`restore_if_unchanged()` 在 `src-tauri/src/codex_config.rs:7141-7158` 返回 `Result<bool, AppError>`。`src-tauri/src/services/provider/mod.rs:5525-5568` 聚合 config/auth/companion 的 `false`，`:5598-5659` 使用 `SwitchResult.codex_receipt`。两条新增 force-repair 回归独立 `2 passed / 0 failed`。 |

### 重要问题 A-8：force-repair 的 receipt 没有贯穿 Codex 特殊 switch 分支

`SwitchResult` 在 `src-tauri/src/services/provider/mod.rs:334-341` 定义内部 `codex_receipt`。普通 `switch_normal()` 在 `:5779-5788` 调用 `write_live_with_common_config_with_receipt()` 并保存回执；三个 Codex 特殊分支仍返回默认回执：

- takeover 中切回不支持代理的 official：`:5400-5428` 先关闭接管、更新 current provider、写 config-only 并同步 MCP，最后返回 `SwitchResult::default()`；
- 需要转换或多路由时自动开启本地代理：`:5431-5451` 已执行 takeover 和 provider switch，返回值仍通过 `..Default::default()` 留空回执；
- 已接管时 hot-switch：`:5454-5478` 已更新代理目标和投影，返回值仍留空回执。

`force_repair_and_switch_codex_provider()` 在 `:5598` 直接调用公开的 `Self::switch()`，没有前置条件排除上述状态。切换成功以后，`:5616-5629` 才发现 `codex_receipt == None` 并返回错误；该分支只调用 `restore_original_state(Some(&initial_attempt), None, None)`。它可以恢复初始 config attempt、目标 Provider 行和 current-provider 指针，但不能撤销已经发生的 takeover disable/enable、hot-switch live 状态、代理目标、DB live backup 或特殊分支中的投影副作用。因此函数可能向用户报告失败，同时留下与恢复后的 current-provider 指针不一致的代理和备份状态。

现有 force-repair 测试全部使用普通 custom provider。第五轮两个新增 companion 测试也只覆盖 `switch_normal()` 有回执的路径；当前没有 force-repair + official-exit、auto-takeover 或 taken-over hot-switch 的回归。

预期修复：force-repair 只能调用一个保证返回完整 typed receipt 的受控切换入口，或者三个特殊分支必须各自返回足以恢复 config/auth/companions、takeover/proxy target 和 DB backup 的事务回执。任何已经改变状态却不能安全恢复的分支必须在改变状态前拒绝执行，不能在成功切换后才用 `None` 回执报错。

### 重要问题 A-9：生产 auth 写入/删除仍有 attempt ownership 绕过

第五轮实现声称三条生产 auth 路径都使用 typed receipt，但穷举生产调用点后仍有两个 live `auth.json` 入口不在该边界内：

- `src-tauri/src/services/proxy.rs:4487-4591` 的 snapshot restore 在 `:4496-4498` 读取一次 live OAuth 状态；当备份只有 `auth`、没有 `config` 时，`:4585-4591` 直接调用 `write_json_file(&auth_path, auth)`。健康备份恢复、接管关闭和故障恢复会进入 `write_codex_live_snapshot()`，auth-only 备份是允许的生产形状。Codex Desktop 如果在 OAuth 检查后刷新或创建登录态，直接写会覆盖该外部版本。
- `src-tauri/src/codex_config.rs:1274-1291` 的 stale-auth cleanup 在读取并判断旧第三方 key 后直接 `delete_file()`，没有在删除前复检 expected fingerprint。普通 switch 在 `src-tauri/src/services/provider/mod.rs:5797-5811` 已经产生 provider receipt 以后才执行该删除，因此删除本身也没有进入 `SwitchResult.codex_receipt`。force-repair 切到无登录材料的 official provider且发生 backfill 时，如果 finalize 随后失败，`:5646-5659` 得不到该 auth 删除的 attempt，无法恢复它；读取与删除之间出现的新 OAuth 也可能被删除。

这两处仍违反统一最小架构边界“auth 必须有独立 expected fingerprint 或 attempt ownership”。预期修复：auth-only snapshot 必须使用同一个 typed auth attempt writer；stale-auth cleanup 必须以 coherent before snapshot 和 expected fingerprint 条件删除，并把删除 receipt 贯穿 `SwitchResult` 和 force-repair 回滚。外部版本不匹配时必须保留并传播 deferred。

### 第五轮 Sol 独立定向结果

| 范围 | 独立结果 |
|---|---:|
| 第五轮 auth capture/rollback 新回归 | 2 passed / 0 failed |
| 第五轮 force-repair companion receipt/deferred 新回归 | 2 passed / 0 failed |
| config candidate receipt | 1 passed / 0 failed |
| config-after-commit companion 冲突 | 1 passed / 0 failed |
| raw API 边界 | 2 passed / 0 failed |
| takeover backup MCP 来源 | 1 passed / 0 failed |
| A-1 provider merge/连续冲突/非法 TOML | 3 passed / 0 failed |
| A-2 OpenAI 与 MultiRouter transform | 2 passed / 0 failed |
| A-3 用户 agent ownership | 1 passed / 0 failed |
| `force_repair_` | 8 passed / 0 failed |
| takeover/hot-switch/cache/restore/catalog-error 定向 | 5 passed / 0 failed |
| `multirouter_projection_` | 6 passed / 0 failed |
| `provider_commands` | 10 passed / 0 failed |
| `provider_service` | 37 passed / 0 failed |
| `mcp_codex_reconcile` | 11 passed / 0 failed |
| 第 7 项 prefix-only migration | 6 + 1 + 1 passed / 0 failed |
| 第 8 项 wizard + workspace page | 2 files / 77 tests passed / 0 failed |

上述测试全部退出码 0。前端只出现 `baseline-browser-mapping` 数据过期提示，没有测试失败。通过结果证明既有覆盖范围没有回归，但不覆盖 A-8 的三个特殊 switch 分支，也不覆盖 A-9 的 auth-only snapshot 与条件删除竞态。

### 第五轮阶段门裁定

- 严重问题：未发现。
- 重要问题：2 组，A-8 与 A-9，均未清零。
- 未裁定小问题：未发现需要单独裁定的小问题。
- 阶段 A：`BLOCKED`。原第四轮 A-4/A-5 的具体修复在普通路径内有效，但统一 receipt/auth ownership 边界仍不完整。
- 阶段 B：`NOT_STARTED`。阶段 A 未通过，未进入代码质量门。
- 阶段 C：`NOT_STARTED`。阶段 A 未通过，未运行 `profile_roundtrip`、`import_export_sync`、Rust lib 全量、`cargo check --tests`、前端全量、build、typecheck 或格式门。
- 最终审核：`NOT_ALLOWED`。A-8 与 A-9 修复并由同一替代 Sol 复核通过以前，不允许进入独立 Sol 最终审核。

本轮没有修改产品代码，没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束进程，没有提交、推送或发布。

## 当前标准状态（第七轮 Luna 实现，待 Sol 独立复核）

Luna 已完成 A-8 特殊 switch typed receipt、普通 switch MCP 最后 writer receipt 和相关 ownership 收口；以下仍是实现方证据，不能替代 Sol 独立复核。因此总状态继续为 `BLOCKED`，阶段 A 尚未宣布通过，阶段 B/C 与最终审核仍保持原门控。

| 范围 | Luna 输入 |
|---|---:|
| A-8 三个特殊分支 finalize-error 回归 | `3 passed / 0 failed`（包含 takeover→official、自动启用代理、taken-over hot-switch） |
| A-9 auth cleanup / deferred 回归 | `3 passed / 0 failed` |
| 普通 switch finish-boundary 与 MCP 最后 writer 回归 | `2 passed / 0 failed` |
| `force_repair_` 总组 | `16 passed / 0 failed` |
| `mcp_codex_reconcile` | `11 passed / 0 failed` |
| Rust 串行全量 | library `3363 passed / 0 failed / 5 ignored`；integration binaries 全部 PASS |
| 前端 Vitest | `144 files / 1177 tests / 0 failed` |
| `cargo check --tests`、`pnpm typecheck`、renderer build、Rust fmt、diff check | 全部 PASS |

静态复核重点：`finish_codex_switch_result` 和 `finish_codex_switch_mutation_result` 不再 `capture(state)`、`capture_after` 或读取 live 文件认领 after-state；普通 provider 的 MCP receipt 已作为最后 writer 进入回滚链。实现 worktree 为 `39 M + 4 ??`，未提交、未推送；没有访问真实用户配置或重启 Codex。Sol 需要在此基础上独立复跑并决定是否解除阶段 A 的 `BLOCKED`。

## 当前标准状态（第七轮）

结论：`PASS`。阶段 A、B、C 均已通过。允许进入独立 Sol 最终审核。

### A 阶段

- A-8 已清零。三个特殊 Codex switch 分支由底层 provider/config/auth/backup/proxy writer 返回 `CodexSwitchMutationReceipt`；`finish_codex_switch_mutation_result()` 在 `src-tauri/src/services/provider/mod.rs:6777-6801` 只组合 receipts，`finish_codex_switch_result()` 在 `:6739-6774` 只消费普通 provider、最后 MCP writer 和 auth cleanup receipts。两个 finish 块没有 `capture()`、`capture_after()`、`read()` 或 `ExactCodexSnapshot::read()`。
- 普通 switch 的最后一次 MCP config writer 已由 `McpService::sync_all_enabled_with_codex_receipt()` 在 `src-tauri/src/services/mcp.rs:203-229` 返回，并在 `services/provider/mod.rs:6666-6668`、`:6747-6766` 进入同一回滚链。normal finish-boundary 外部第三版本会保留并报告 deferred；无外部变化时 finalize failure 会恢复 provider 与 MCP 两次写入前的状态。
- A-9 auth-only snapshot 与 stale cleanup receipt 继续成立。A-1 至 A-7、MCP mixed ownership 也没有发现新的严重、重要或未裁定小问题。
- 第 7 项没有缩小：prefix-only migration 当前目录冻结 `6/6`，未来 catalog 不自动扩大 `1/1`，混合 models+prefix 兼容 `1/1`。动态未来 prefix/上游发布仍属于明确非目标。
- 第 8 项没有缩小：wizard + workspace `77/77`，覆盖新计划不生成 `defaultRouteId`、旧 V2 只读保留、运行时 fail-closed 和可见诊断。

### B 阶段

`PASS`。typed receipt 的接口深度、错误传播、特殊分支一致性和注释与实现一致性通过复核。没有发现新的严重、重要或未裁定小问题。

### C 阶段独立结果

| 范围 | 结果 |
|---|---:|
| `force_repair_` | 17 passed / 0 failed |
| Rust library 串行全量 | 3363 passed / 0 failed / 5 ignored |
| `provider_commands` | 10 passed / 0 failed |
| `provider_service` | 37 passed / 0 failed |
| `mcp_codex_reconcile` | 11 passed / 0 failed |
| `import_export_sync` | 24 passed / 0 failed |
| `profile_roundtrip` | 8 passed / 0 failed |
| 其余 integration binaries | 全部通过；包括 app config/type、deeplink、Hermes、MCP commands、proxy commands、skill sync、support safety |
| 前端第 7/8 定向 | 2 files / 77 tests passed |
| 前端 Vitest 全量 | 144 files / 1177 tests passed |
| `cargo check --tests` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS；3342 modules |
| `git diff --check` | PASS |

前端仍有既有 baseline-browser-mapping、Browserslist、React `act(...)`/DOM 属性、MSW 未匹配请求、动态导入和大 chunk 提示，没有测试、typecheck 或 build 失败。

## 最新结论：Sol 对 Task 8/9 及 outcome 生命周期修复的独立复核

结论：`PASS`。Task 8/9 的阶段 A、B、C 均已通过；严重问题、重要问题和未裁定小问题全部为 0。此前 B 阶段发现的“旧 Active warning 在后续正常启动重复显示”和“Active 下一步缺少四语言文案”已经独立复核为 `RESOLVED`。允许恢复独立 Sol 最终审核。

### B 阶段修复复审

- `persist_startup_recovery_outcome()` 是生产启动 outcome 的唯一 owner。`record_startup_report()` 只读取证据、分类并写 marker，不承担 outcome 清理。
- `NoPreviousRun`、`UncleanExit` 和 `ConfirmedCrash` 只会条件清理旧 `ActivePreviousInstance` 或 `PlannedRestartOrUpdate`；`ProviderOnlyRestored` 等非瞬态恢复结果继续保留。
- outcome 写入、读取和瞬态清理由同一进程内 `Mutex` 串行。清理会读取、解析、复读并比较原始字节，再执行条件删除；新的进程内 writer 不会被旧清理覆盖。
- Rust `RecoveryOutcomeKind` 的 camelCase 序列化、TypeScript 联合类型和 `App.tsx` 分支一致。英文、中文、繁中和日文均提供 `closeOtherInstanceOrInspectProcess` 的用户文案，不再回退显示内部键。
- Task 9 继续使用标准 `semver::Version` 判断同主版本；`commands/misc.rs` 的本地 parser 只服务 updater 排序，职责边界不同，不构成 Task 9 的重复兼容性实现。
- 阶段 B 裁定：严重 0、重要 0、未裁定小问题 0，`PASS`。

正式跨进程文件锁仍是已批准的非目标。另一个进程仍可能在清理的第二次读取与 `remove` 之间写入 outcome；当前字节复读只能降低该 TOCTOU 风险，不能把进程内锁升级为跨进程所有权证明。该残余风险已经明确记录，不阻塞本轮范围。

### C 阶段独立结果

| 范围 | 独立结果 |
|---|---:|
| `app_exit_monitor` | 11 passed / 0 failed |
| `recovery_outcome` | 6 passed / 0 failed |
| `port_probe` | 4 passed / 0 failed |
| 不兼容真实 listener E2E | 1 passed / 0 failed |
| `takeover` 过滤 | 49 passed / 0 failed |
| 四语言 outcome 文案 | 4 passed / 0 failed |
| `force_repair_` | 17 passed / 0 failed |
| 第 7 项 prefix-only migration | 6 passed / 0 failed；未来目录和混合兼容性仍由 Rust 全量覆盖 |
| 第 8 项 default-route warning | 1 passed / 0 failed；wizard + workspace 77 passed / 0 failed |
| typed receipt 过滤 | 1 passed / 0 failed；完整 receipt/ownership 矩阵由 Rust 全量与 force-repair 覆盖 |
| Rust library 串行全量 | 3375 passed / 0 failed / 5 ignored |
| 全部 integration binaries | 全部 PASS；`mcp_codex_reconcile` 11、MCP commands 23、provider commands 10、provider service 37、import/export 24、profile roundtrip 8，其余集成测试全部通过 |
| 前端相关测试 | 3 files / 81 tests passed（含第 8 项 77 与 outcome 四语言 4） |
| 前端 Vitest 全量 | 145 files / 1181 tests passed |
| `cargo check --tests` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `pnpm typecheck` | PASS |
| `pnpm build:renderer` | PASS；3342 modules |
| `git diff --check` | PASS |

前端测试只出现既有 baseline-browser-mapping、React `act(...)`、DOM 属性、MSW 未匹配 Tauri 请求、jsdom window API 和故意触发的错误日志。renderer build 只出现既有 baseline-browser-mapping、Browserslist 数据过期、subscription 动静态导入和大 chunk 提示。Rust 检查没有 warning；`git diff --check` 只提示 Cargo.lock 工作副本未来可能由 LF 转换为 CRLF。以上提示均未造成失败。

### 最终准入

- 总状态：`PASS`。
- 阶段 A：`PASS`。
- 阶段 B：`PASS`。
- 阶段 C：`PASS`。
- 严重问题：0。
- 重要问题：0。
- 未裁定小问题：0。
- 独立 Sol 最终审核：`ALLOWED`。

本轮只更新了本独立验证报告，没有修改产品代码、实现报告或 ledger；本轮没有访问真实 `~/.codex` 或 `~/.agents`，没有重启或结束进程，没有提交、推送或发布。

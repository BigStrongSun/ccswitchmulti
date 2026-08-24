# Codex 启动配置保护与恢复设计（SSOT 配置保留扩展版）

日期：2026-08-23（同日第三次修订，新增用户级 MCP 覆盖补充）

状态：世豪已批准（第三版，含 MCP 所有权与对账补充；实现必须以兼容性设计为核心）

输入证据：

- `docs/diagnostics/sdd-codex-startup-preference-reset-explore-2026-08-23.md`（状态 `DONE_WITH_CONCERNS`）
- `docs/diagnostics/sdd-codex-user-mcp-overwrite-explore-2026-08-23.md`（状态 `DONE_WITH_CONCERNS`，第三版新增输入）

## 1. 问题与预期结果

Codex Desktop 启动恢复时，CCSwitchMulti 可能进入“无备份，改用数据库当前供应商恢复”的路径。当前供应商是 schema-v2 MultiRouter 时，数据库记录可以合法地没有 `settings_config.config` 文本。现有写入链把这个缺失值一路传成 `None`，最后在 `write_codex_live_atomic` 或 `write_codex_live_config_atomic` 中变成空字符串并写盘。最小复现已经两次确认：恢复后 `config.toml` 长度为 0，`[desktop]`、`[plugins]` 全部消失。

实际结果是 `~/.codex/config.toml` 被写成 0 字节。用户自己的 `[desktop]`、`[plugins]`、`[projects]`、`[marketplaces]` 和其他 CCSwitchMulti 不管理的配置会一起消失。Codex Desktop 随后按自己的默认设置显示推理强度，所以 `max` 从“可用推理强度”中消失；SDD 的启用字段也可能随 `[plugins]` 一起消失。

扩展后的探索报告补充了四条证据链，说明本次故障不是单一空写：

1. `app-run-marker.json` 只能证明“上次进程没有留下正常退出记录”，不能直接证明崩溃。当天 14:09、17:45、18:39 三次启动都读到残留 marker；托盘常驻、系统关机、强制结束都会产生同样现场。
2. 18:39:28 启动时 15721 端口被占用。当前 15721 由本机 `cc-switch.exe` 监听，18:39 的占用者极可能是旧 CCSM 实例或并发实例，但精确 PID 已不可考。端口占用触发“关闭接管”，关闭接管又把恢复推向 SSOT 空写路径。
3. CCSM 和 Codex Desktop 都会写同一份 `~/.codex/config.toml`，两边没有共享锁。探索报告只证明了时序接近和 last-writer-wins 风险存在，没有捕获到一次具体的双写冲突，因此本规格只能写“风险已证实存在，具体冲突尚未捕获”。
4. 启动恢复的所有结果只写日志，用户看不到“恢复了什么、丢了什么”。SDD 插件显示未安装的直接原因是 `~/.agents/plugins/marketplace.json` 缺少 SDD 条目，而 cache 目录和 `config.toml` 启用字段都存在；插件安装状态涉及 cache、`config.toml`、`marketplace.json` 三个数据源。

预期结果分三层交付：A 层先堵住数据丢失；B 层让启动恢复自动完成并且结果对用户可见，世豪不再需要在每次启动后重新勾选 `max`、重新点安装 SDD；C 层是后续长期兼容性工作，不在本次实现范围。

## 2. 交付分层

### A 层：数据防丢（最小可交付）

- 禁止 `None` 空写。
- live-as-base 合并保留用户表。
- 损坏分级和恢复来源判定。
- 跨进程乐观并发：读取原始字节和指纹；解析、字段级合并、校验；替换前复检指纹；冲突时重新读取并有限重试；超限后保持原文件并报告冲突。

### B 层：启动自动恢复与可见反馈（世豪验收必须包含）

- 修正 marker 语义，区分 `unclean_exit`、`confirmed_crash`、`planned_restart_or_update` 和活跃旧实例；marker 单独存在不能触发破坏性恢复。
- 15721 端口安全识别：先探测占用者，能安全复用就复用，不能复用就退出接管并明确提示。
- 新增可查询的“最近一次恢复结果”，并发送事件和 toast：无感成功、已恢复但有警告、无法恢复需要用户处理。
- 插件三数据源一致性检测与修复。
- 最终必须实现：`max` 不再消失；SDD 不再错误显示未安装，或者能够自动修复、明确引导；启动后不需要重复手工操作。

### C 层：深层兼容（后续范围，不在本次实现）

- 完整跨进程协调或正式锁协议。
- 更完整的用户配置快照与历史恢复。
- 旧 schema、新 schema-v2 MultiRouter 和插件安装协议的长期迁移。
- 首个补丁不能无限扩张到本层，但 A、B 两层的测试矩阵必须预留这些契约。

## 3. 目标

A 层目标：

- `settings_config.config` 缺失时，任何 Codex live 写入路径都不能把 `None` 转成空字符串写盘。
- 启动恢复、备份恢复和普通供应商切换都必须保留当前 live 中的用户自有配置。
- 无备份且数据库无法提供完整配置时，恢复流程要失败到“清理接管占位符”的安全路径，不能声称 SSOT 已成功重建整份配置。
- Codex 的 `auth.json` 与 `config.toml` 的所有权边界保持清楚；本设计不扩大认证写回范围。
- Codex live 写入增加乐观并发复检，写盘前发现文件已被其他进程修改时不覆盖现场。

B 层目标：

- 启动时的异常判定基于分类证据，不再把“marker 残留”直接等同于“需要恢复”。
- 15721 被占用时，先识别占用者身份，再决定复用、退出接管或提示；任何路径都不得仅凭端口占用杀进程。
- 每次启动恢复产出一个结构化结果，持久化最近一次结果，提供查询命令，并按严重度发送事件和 toast。
- 插件 cache、config 启用状态、marketplace 登记三者不一致时，能被检测到，并给出修复能力或明确引导。

## 4. 非目标

- 本设计不修改 Codex Desktop 的安装包、模型目录或推理能力目录。
- 本设计不把 `[desktop]`、`[plugins]` 等完整用户表复制进 CCSwitchMulti 数据库，也不把 common config 扩展成完整用户配置快照。
- 本设计不改变健康备份优先于 SSOT 的恢复顺序。
- 本设计不改变普通供应商已有 `config` 文本时的模型、端点和认证合并规则。
- 本设计不实现 C 层的正式跨进程文件锁、完整用户配置历史快照和 schema 长期迁移。
- 本设计不声称双写冲突已经发生；并发防护按“风险已证实存在、具体冲突尚未捕获”的口径设计。

## 5. 参与者、职责和边界

- CCSwitchMulti Provider 记录：负责供应商认证、模型、路由、端点和 CCSwitchMulti 投影字段。
- CCSwitchMulti common config：只负责用户明确共享的少量偏好，不能当作整份 `config.toml` 快照。
- Codex live `config.toml`：是当前机器上用户配置的最新来源。恢复时可以读取它并保留用户自有表，但不能把接管占位符固化进数据库。
- Codex live `auth.json`：保存 Codex Desktop 当前登录材料。缺少 `config.toml` 时不能为了写认证而顺手清空配置。
- Codex Desktop 与 app-server：消费 live 配置，并通过 `config/batchWrite` 写自己的 `desktop.*`、`plugins.*` 等键。它是 `config.toml` 的并发写入者；CCSM 不应重写这些用户表，也不得假设自己是唯一写入者。
- `app_exit_monitor`（`src-tauri/src/app_exit_monitor.rs`）：负责 marker、退出事件和 panic 摘要。它提供证据，不负责直接触发恢复动作。
- CCSM 代理服务器（`src-tauri/src/proxy/server.rs`）：监听 15721，暴露 `/health` 与 `/status`。端口占用判定可以使用这两个端点识别占用者是否为兼容 CCSM 实例。
- `~/.agents/plugins/marketplace.json`：Codex 插件安装登记数据源，归 Codex 安装流程所有。CCSM 只能在通过验证后做结构化合并修复，不能整文件覆盖。
- `~/.codex/plugins/cache/<marketplace>/<name>/<version>`：插件内容缓存，是插件登记修复的事实来源之一。

## 6. 核心裁定

### 裁定 1：`config_text=None` 表示“缺少供应商配置”，不表示“用户要清空配置”

决定：Codex provider live 写入边界必须拒绝 `config_text=None`，并在写任何文件前返回明确错误。低层原子写入器也必须拒绝 `None`，不能再把 `None` 规范化为 `String::new()`。

原因：数据库中的 schema-v2 MultiRouter 可以没有物化 `config`，但 live 写入器需要完整配置。把缺失值写成空文件会销毁用户配置，属于不可逆的数据丢失。

判断错误的代价：如果继续允许空写，任何启动恢复、同步或供应商写入异常都可能清空整份 Codex 配置。

### 裁定 2：用户配置以当前 live 为准，不新增独立数据库快照

决定：本次修复不创建新的用户配置快照表。恢复和切换都以当前 live 配置为用户自有配置的基底；数据库和 common config 只提供供应商或共享偏好字段。

原因：探索报告已经证明现有 `merge_codex_provider_config_texts` 能以 live 为底保留用户表。新增数据库快照会引入同步时机、陈旧数据、敏感字段和迁移问题，超出本次故障的最小修复范围。

判断错误的代价：如果只依赖数据库快照，Codex Desktop 最近一次写入的桌面偏好和插件状态可能被旧快照回滚。

### 裁定 3：用户自有表采用“默认保留”，供应商字段采用“明确拥有才可替换”

决定：至少把以下顶层表视为用户自有：`desktop`、`plugins`、`projects`、`marketplaces`、`memories`。CCSwitchMulti 不认识的顶层表也默认保留。只有供应商明确拥有的字段和表才能被替换或清理。

供应商或 CCSwitchMulti 明确拥有的字段包括：`model`、`model_provider`、`model_catalog_json`、`openai_base_url`、`base_url`、`wire_api`、`experimental_bearer_token`、当前活动的 `model_providers.<id>`，以及 CCSwitchMulti 生成的接管路由和目录投影。

`mcp_servers` 是混合边界：合并时不得用旧供应商快照覆盖 live 中较新的条目；后续 MCP reconcile 可以按数据库投影增删它明确管理的条目，但本次配置恢复不能直接清掉整张表。

原因：用户配置的来源和供应商配置的来源不同。默认保留未知表能避免未来 Codex 新增配置表时再次被误删。

判断错误的代价：如果只保护四个已知表，Codex 下次新增用户表时仍会复现同类丢失。

### 裁定 4：三条写入路径使用同一所有权契约，但失败动作不同

#### 健康备份恢复

- 备份存在且不含接管占位符时，备份仍是供应商字段的来源。
- 当前 live 仍是用户自有表的来源。
- 写入结果必须应用备份中的供应商字段，同时保留 live 中较新的用户自有表。
- 当前 live 的 Codex OAuth 登录材料不能被旧备份中的空认证或旧令牌覆盖。

#### 无备份或备份是接管占位符时的 SSOT 恢复

- 先检查数据库当前供应商在应用 common config 后是否能提供完整的 `config` 字符串。
- 能提供时，使用现有 live-as-base 合并逻辑写回供应商字段，并保留用户自有表。
- 不能提供时，不调用 Codex provider live 写入器；恢复流程应落入现有的接管占位符清理路径。
- 占位符清理只能移除本地代理地址、`PROXY_MANAGED` 令牌、CCSwitchMulti 接管路由和 CCSwitchMulti 生成的目录指针；必须保留 `[desktop]`、`[plugins]`、`[projects]`、`[marketplaces]` 和未知用户表。

#### 普通 Provider 切换

- 普通切换在更新当前供应商指针前先做 Codex live 写入预检。
- 预检要求目标供应商在应用 common config 后具有对象形态的 `auth` 和字符串形态的 `config`。
- 缺少 `config` 时返回明确错误，live 文件和当前供应商指针都保持不变。
- 代理接管路径可以把缺失的数据库 `config` 作为输入，因为接管构建器会生成一份明确的接管 `config`；最终交给低层写入器的仍必须是 `Some(config)`。

原因：三条路径的风险不同，但都不能把缺失配置解释为空配置。普通切换必须先失败，避免数据库已经指向新供应商而 live 仍是旧供应商。

判断错误的代价：如果把所有 `None` 都当成可恢复输入，调用方会继续在错误层级做隐式猜测；如果只改启动路径，普通切换仍会复现空写。

### 裁定 5：`Some("")` 与 `None` 必须保持不同语义

决定：`Some("")` 可以继续表示“明确不提供供应商字段，清除 CCSwitchMulti/provider 拥有的字段并保留用户自有表”。`None` 必须表示“调用方缺少必要输入”，必须失败。

原因：现有合并逻辑已经为空 official 配置定义了安全语义；真正危险的是把“缺失”静默改成“显式为空”。

判断错误的代价：如果禁止所有空字符串，官方内置供应商和显式清理路径会失去现有能力；如果允许 `None` 变成空字符串，本次数据丢失会继续存在。

### 裁定 6（修订）：SDD 插件登记按三数据源一致性处理，修复方式待世豪裁定

决定：本修复除了保证 `[plugins."sdd@personal"] enabled=true` 不因 CCSM 恢复而丢失，还要覆盖插件三数据源的一致性检测。自愈或修复前必须读取并验证：cache 中的 `.codex-plugin/plugin.json`、插件名称、版本、marketplace 来源；解析出的 canonical 路径必须位于允许的插件缓存目录内，拒绝路径穿越。`marketplace.json` 的更新必须结构化解析、合并、按 `name` 和 `source.path` 去重、原子替换，并保留已有条目（例如现有 `investment-signal-monitor`）。`config.toml` 的启用状态不能被擅自改写。

如果仓库中没有可靠的安装登记 API，最小交付只能提供“检测到可修复插件 + 修复按钮或打开安装入口”的引导，不能宣称自动修复完成。“启动时自动修复”与“检测到后给修复按钮”是产品取舍，默认推荐修复按钮（世豪明确点一次），是否升级为自动修复由世豪决定。

原因：Codex 插件界面以 marketplace 注册文件判断安装状态，`config.toml` 只保存启用偏好。三个数据源不一致时，静默替用户改登记文件有误登记版本、来源或安装状态的风险。

判断错误的代价：如果把 marketplace 更新写成整文件覆盖，用户已有的其他插件条目会丢；如果只修 config 空写不处理登记，SDD 会继续显示未安装，世豪仍要每次手动点安装。

### 裁定 7（新增）：marker 语义分级，marker 单独存在不能触发破坏性恢复

决定：启动时的异常判定必须区分四类情况：`unclean_exit`（有 marker 残留且没有 panic 证据）、`confirmed_crash`（有 panic 记录或崩溃日志更新）、`planned_restart_or_update`（更新或重启路径已经记录 clean_exit 后又出现新 marker 的正常时序）、活跃旧实例（marker 中的 PID 当前仍在运行）。只有 `confirmed_crash` 和确无活跃实例的 `unclean_exit` 才允许进入恢复流程；发现活跃旧实例时当前实例不得执行接管恢复。

原因：探索报告证明 marker 残留可以由托盘常驻、系统关机、强制结束等造成；把“没有干净退出记录”直接当成崩溃会让正常启动也触发接管关闭和 SSOT 恢复。

判断错误的代价：如果继续把 marker 等同于崩溃，每次正常双开或托盘常驻都会触发一次恢复链，配置丢失会反复发生。

### 裁定 8（新增）：15721 端口占用先识别占用者，再决定动作

决定：接管启动遇到 15721 被占用时，不得仅凭端口占用杀进程。必须先探测占用者：请求 `http://127.0.0.1:15721/health` 与 `/status`，判断是否为兼容 CCSM 实例、版本是否兼容、实例身份是否属于本机 CCSM、健康状态是否正常。可安全复用时复用该实例并跳过本实例接管；占用者不可识别、版本不兼容或不可连接时，当前实例退出接管流程并给出明确提示，由用户决定结束哪个进程。

原因：18:39 的端口占用极可能是旧 CCSM 实例或并发实例；直接杀进程可能杀掉正在服务 Codex 的代理，直接忽略又会把恢复推向 SSOT 空写路径。

判断错误的代价：如果误判占用者，要么杀掉正常代理造成 Codex 请求中断，要么继续走空写兜底造成配置丢失。

### 裁定 9（新增）：Codex live 写入使用乐观并发，不使用强制锁

决定：Codex live 写盘流程改为：读取原始字节并计算指纹；解析、按字段级规则合并、校验；原子替换前复检文件指纹；指纹变化时重新读取、重新合并，并有限重试；超过重试上限后保持原文件不变，报告 `ConcurrentModificationDeferred`。本次不引入跨进程文件锁，正式锁协议属于 C 层。

原因：CCSM 和 Codex app-server 都写同一份 `config.toml`，已证实存在时序接近和 last-writer-wins 风险。乐观并发能在不依赖 Codex 配合的情况下避免“读到旧快照后覆盖对方刚写的字段”。

判断错误的代价：如果没有复检，CCSM 恢复可能在 Codex 刚写入桌面偏好后用旧快照覆盖整份文件，用户表再次丢失。

### 裁定 10（新增）：损坏分级与恢复结果必须显式命名

决定：每次恢复必须产出下列结果之一，并持久化最近一次结果：

- `HealthyBackupRestored`：健康备份完整恢复。
- `LivePreservedProviderRepaired`：live 可解析，保留用户表，只修供应商字段。
- `ProviderOnlyRestored`：用户表已经不存在，只能恢复供应商字段，必须带警告。
- `UserBackupCandidateFound`：发现 `.bak` 等候选文件，等待用户确认或提供安全的一键恢复。
- `UnrecoverableUserTables`：没有任何可恢复来源，不能假装成功。
- `ConcurrentModificationDeferred`：复检发现并发修改，有限重试后停止，原文件保持不变。
- `PluginRegistrationRepairAvailable`、`PluginRegistrationRepairCompleted`、`PluginRegistrationRepairFailed`：插件登记修复的三个状态。
- `PortOwnedByCompatibleInstance`、`PortOwnedByUnknownOwner`：端口占用识别的两个结果。

0 字节或语法损坏的 live 配置不能视为可解析来源；解析失败时不得用新空文件覆盖现场。用户表已经不存在时，恢复只能到 `ProviderOnlyRestored` 或 `UnrecoverableUserTables`，必须如实告知，不能凭空还原用户表。

原因：探索报告证明 0 字节或损坏配置未必能恢复用户表；把每种结果显式命名，UI 和日志才能说清“恢复了什么、丢了什么”。

判断错误的代价：如果继续用“成功或失败”二值表达恢复，用户会在 `ProviderOnlyRestored` 场景误以为 `max` 和插件还会自己回来。

### 裁定 11（新增）：恢复结果必须可查询、持久化并可见

决定：最近一次恢复结果持久化到本地（数据库或日志旁的结果文件），提供查询命令；启动恢复完成后发送事件，前端按严重度显示 toast：无感成功不打扰、已恢复但有警告显示 warning、无法恢复显示 error 并给出下一步（打开日志目录、一键恢复 `.bak`、修复插件登记）。持久化必须先于事件发送，避免 UI 错过启动事件后查不到结果。

原因：当前启动恢复只写日志；世豪只能在设置页看到“代理未运行”这类间接状态，无法定位是端口竞争还是配置被清空。

判断错误的代价：如果结果只发事件不持久化，启动早于前端订阅完成时用户永远看不到这次恢复发生了什么。

## 7. 方案取舍

### 方案甲：把用户表复制进数据库或 common config

优点：live 文件丢失后仍有恢复来源。

缺点：需要新迁移、新同步时机和陈旧快照策略；可能复制用户路径、插件偏好和其他敏感配置；不能解决当前 live 比数据库更新的问题。

结论：不采用为本次修复方案。

### 方案乙：保持 `None` 到空字符串的现有行为

优点：实现最小。

缺点：已经把真实 `config.toml` 复现写成 0 字节，风险不可接受。

结论：拒绝。

### 方案丙：缺失配置时失败，并以 live 为用户配置基底，叠加乐观并发复检

优点：改动集中在 Codex 写入边界、启动恢复和普通切换预检；复用现有合并逻辑；能直接覆盖本次复现场景；复检不依赖 Codex 配合。

缺点：配置缺失的供应商会从“静默写空”变成“明确报错”；个别依赖旧错误行为的调用需要显式改成 `Some("")` 或接管配置生成器；乐观并发在极端高频冲突下会放弃写入并报告。

结论：采用。

### 插件登记修复的两个候选

- 自动修复：启动 reconcile 时直接补齐 marketplace 条目。优点是世豪零操作；缺点是一旦 manifest 验证规则有缺口，会在用户不知情时写登记文件。
- 修复按钮：检测到不一致时提示“检测到已安装插件 SDD 需要登记”，用户点一次完成修复。优点是动作明确、可审计；缺点是多一次点击。

结论：默认采用修复按钮；是否升级为自动修复由世豪在批准规格时裁定。

### 端口冲突的两个候选

- 直接结束占用进程：实现简单，但可能杀掉正在服务 Codex 的旧代理，拒绝。
- 探测后复用或退出接管：多一次 HTTP 探测，换来不中断正常实例；采用。

## 8. 行为契约

### 8.1 Codex 原子写入器

- `write_codex_live_atomic(auth, None)` 必须返回错误，并且不能写 `auth.json` 或 `config.toml`。
- `write_codex_live_config_atomic(None)` 必须返回错误，并且不能写 `config.toml`。
- `Some(text)` 仍按现有逻辑规范化、校验 TOML 并原子写入。
- 空字符串只能以 `Some("")` 的形式进入；调用方必须有意选择这个语义。
- 写盘采用裁定 9 的乐观并发流程；冲突超限时返回 `ConcurrentModificationDeferred`，两个目标文件保持原样。

### 8.2 Codex provider live 写入

- `write_codex_live_for_provider(category, auth, None)` 必须返回错误。
- 错误必须发生在认证写回、目录投影、托管 agent 文件同步和 `config.toml` 写入之前。
- `write_codex_provider_live_with_catalog_and_provider_context` 与无 provider context 的对应函数必须在准备目录前检查 `config_text`。
- `write_codex_provider_config_only_with_catalog_and_provider_context` 如果需要“只清理供应商字段”，必须显式调用“清理供应商字段并保留 live 用户表”的 helper，不能用 `unwrap_or("")` 隐藏输入缺失。

### 8.3 启动异常恢复

- 进入恢复前先按裁定 7 分类；发现活跃旧实例或属于计划内重启时，不执行接管关闭和 SSOT 恢复。
- 恢复顺序保持为：健康备份、SSOT、占位符清理。
- 备份是接管占位符时不能写回。
- SSOT 配置缺失时不能写回，也不能返回“已从 SSOT 恢复”。
- 占位符清理成功时，最终配置可以没有本地代理路由，但必须保留用户自有表。
- 占位符清理失败时，错误必须向调用方暴露，不能用一个新空文件覆盖现场。
- 每次恢复结束必须产出裁定 10 的命名结果，并按裁定 11 持久化和上报。

### 8.4 普通供应商切换

- 普通 Codex 切换先预检目标供应商的 live 写入输入。
- 预检失败时，不更新 settings 当前供应商，不更新数据库当前供应商，不写 live。
- 预检成功后才允许回填旧供应商、更新当前供应商和写 live。
- common config 应用后产生了完整 `config` 的供应商可以通过预检。

### 8.5 用户表保留

- `[desktop].enabled-reasoning-efforts` 中的 `max` 必须在恢复后仍存在。
- `[plugins."sdd@personal"].enabled = true` 必须在恢复后仍存在。
- `[projects]` 与 `[marketplaces]` 必须在恢复后仍存在。
- 未知顶层表必须在恢复后仍存在。
- 接管字段 `PROXY_MANAGED`、`http://127.0.0.1:<port>` 和 CCSwitchMulti router 路由必须按现有清理规则移除。

### 8.6 marker 分类

- 启动时读取 marker、panic 事件和崩溃日志修改时间，输出裁定 7 的四类判定之一。
- 判定为活跃旧实例时，当前实例不得执行接管恢复，并在日志和恢复结果中记录。
- 判定逻辑本身不得删除或改写 marker；marker 生命周期仍由 `record_startup`、`record_clean_exit`、`record_forced_exit` 管理。

### 8.7 端口占用处置

- 绑定 15721 失败时，先探测 `/health` 和 `/status`。
- 探测确认是兼容 CCSM 实例且健康时，当前实例复用该实例，不再尝试接管，恢复结果记为 `PortOwnedByCompatibleInstance`。
- 探测失败、占用者不是 CCSM、或版本不兼容时，当前实例退出接管，恢复结果记为 `PortOwnedByUnknownOwner`，toast 或状态页提示用户结束占用进程或改监听端口。
- 任何路径都不得主动终止占用进程。

### 8.8 乐观并发写入

- 写入前记录 live 文件字节指纹（长度加内容哈希）。
- 合并和校验完成后、原子替换前复检指纹。
- 指纹变化时重新读取并重新合并，最多重试有限次数（建议 2 次）。
- 超限后保持原文件，返回 `ConcurrentModificationDeferred`，由上层决定提示或稍后重试。

### 8.9 损坏分级

- live 可解析且含用户表：恢复后必须保留用户表，结果为 `LivePreservedProviderRepaired` 或 `HealthyBackupRestored`。
- live 可解析但用户表已缺失：只能恢复供应商字段，结果为 `ProviderOnlyRestored`，必须带警告。
- live 为 0 字节或语法损坏：不视为可解析来源；发现合格 `.bak` 候选时报 `UserBackupCandidateFound`；无候选时报 `UnrecoverableUserTables`。
- 任何分级下都不得用新空文件覆盖现场。

### 8.10 恢复结果上报

- 结果包含：命名状态、保留的字段摘要、丢失的字段摘要、建议下一步、发生时间。
- 结果先持久化，再发送 `codex-config-recovery-*` 事件；前端订阅事件并按严重度 toast。
- 前端打开时可通过查询命令补拉最近一次结果，错过事件也能看到。

### 8.11 插件登记一致性

- 检测：cache 存在有效 manifest、config 有启用字段、marketplace 缺条目时，报 `PluginRegistrationRepairAvailable`。
- 修复：验证 manifest 的 `name`、版本、marketplace 来源和 canonical 路径；路径必须位于 `~/.codex/plugins` 下；结构化合并进 `marketplace.json`，保留并去重已有条目，原子替换。
- 修复成功报 `PluginRegistrationRepairCompleted`，失败报 `PluginRegistrationRepairFailed` 并保留现场。
- config 启用状态不被修复流程改写。

## 9. 可观察验收标准

A 层验收：

- 新增的 SSOT 恢复回归测试使用临时 HOME、内存数据库、schema-v2 MultiRouter、`config=None`、无备份和带用户表的 live 配置；测试通过后，恢复结果不是空文件，并且包含 `desktop`、`max`、`sdd@personal`、`projects`、`marketplaces` 和未知用户表。
- 同一恢复结果不包含 `PROXY_MANAGED`，也不包含本地代理地址。
- 原子写入器的 `None` 测试失败信息明确说明缺少 Codex 配置，且两个目标文件内容不变。
- 乐观并发测试模拟写盘期间文件被外部修改；写入器重试并最终基于新内容合并，或在超限后保持原文件并报告 `ConcurrentModificationDeferred`。
- 普通 Codex 供应商切换缺少 `config` 时返回错误，旧当前供应商和旧 live 配置保持不变。
- 已有健康备份恢复测试、备份占位符回退测试、空 official 配置合并测试和用户表合并测试继续通过。

B 层验收（世豪视角）：

- 正常关闭再启动后，`config.toml` 保持完整，`max` 仍在可用推理强度中，不需要再次勾选。
- SDD 在插件列表中显示已安装，或出现一次性的“检测到已安装插件需要登记”提示，点一次修复后不再出现。
- 启动后不需要重复手工操作；出现异常时 toast 能说明“恢复了什么、丢了什么、下一步做什么”。
- marker 残留但旧实例仍活跃时，不发生接管恢复，也不清空配置。
- 15721 被兼容 CCSM 实例占用时，当前实例复用并提示；被未知进程占用时，当前实例退出接管并明确提示，不杀进程。

通用约束：

- 本修复的所有测试不产生对真实 `~/.codex`、真实 `~/.agents` 或 Codex Desktop 进程的写入。

## 10. 测试矩阵

| 层 | 场景 | 输入 | 预期 |
|---|---|---|---|
| A | schema-v2 MultiRouter，无备份，live 有用户表 | `settings_config.config=None`，live 含接管路由、`desktop` max、`plugins` SDD、`projects`、`marketplaces`、未知表 | 不写空；清理接管字段；用户表保留 |
| A | schema-v2 MultiRouter，无备份，live 无接管痕迹 | 当前供应商缺少配置，live 为空或不存在 | 恢复不创建或重写空配置；返回成功但不改变文件状态 |
| A | 普通 Provider，完整配置 | 目标供应商有 `auth` 和 `config` | 使用现有合并规则；用户表保留；供应商字段生效 |
| A | 普通 Provider，缺少配置 | 目标供应商无 `config` | 切换前失败；当前供应商和 live 不变 |
| A | common config 补齐配置 | 目标供应商无 `config`，共享片段应用后产生 `config` | 允许写入；用户表保留 |
| A | 健康备份恢复 | 备份有供应商字段，live 有较新用户表 | 备份供应商字段生效；live 用户表保留 |
| A | 备份是代理占位符，SSOT 有完整配置 | 备份和 live 都含本地代理占位符 | 跳过备份；使用 SSOT 供应商字段；用户表保留 |
| A | 备份是代理占位符，SSOT 缺少配置 | 当前供应商无 `config` | 跳过备份和 SSOT；清理接管字段；用户表保留 |
| A | 低层写入器收到 `None` | live 已存在 | 返回错误；文件内容不变 |
| A | 写入期间文件被并发修改 | 合并完成后、替换前指纹变化 | 重新读取合并或超限后保持原文件，报告 `ConcurrentModificationDeferred` |
| A | live 为 0 字节 | live 存在但长度为 0 | 不视为可解析来源；不得用新空文件覆盖 |
| A | live 语法损坏 | live 含非法 TOML | 报错或分级到候选恢复；不覆盖现场 |
| B | marker：正常退出后启动 | 无 marker 残留 | 不进入恢复 |
| B | marker：托盘常驻期间第二次启动 | marker 中的 PID 仍活跃 | 判定活跃旧实例；不执行接管恢复 |
| B | marker：计划内重启或更新 | 有 clean_exit 记录且时序正常 | 判定计划内重启；不进入破坏性恢复 |
| B | marker：强制退出，无 panic 证据 | marker 残留，`crashLogModifiedAt` 为空 | 判定 `unclean_exit`；允许恢复但结果带警告 |
| B | marker：有 panic 记录 | marker 残留且有 panic 事件 | 判定 `confirmed_crash`；进入恢复 |
| B | 端口：兼容 CCSM 实例占用 | `/health` 正常、`/status` 身份匹配 | 复用；记 `PortOwnedByCompatibleInstance` |
| B | 端口：未知进程占用 | 探测无响应或身份不匹配 | 退出接管并提示；记 `PortOwnedByUnknownOwner`；不杀进程 |
| B | 端口：占用但不可连接 | 连接超时 | 退出接管并提示；不杀进程 |
| B | 恢复结果：无感成功 | 健康备份完整恢复 | 持久化结果；无错误 toast |
| B | 恢复结果：有警告 | `ProviderOnlyRestored` | warning toast；结果列出丢失字段摘要 |
| B | 恢复结果：无法恢复 | 无备份、live 损坏 | error toast；给出打开日志或提供备份的引导 |
| B | UI：错过启动事件 | 前端晚于事件启动 | 查询命令能返回最近一次结果 |
| B | 插件：三方一致 | cache、config、marketplace 都有 SDD | 不提示 |
| B | 插件：marketplace 缺条目 | cache 和 config 有 SDD，marketplace 缺 | 报 `PluginRegistrationRepairAvailable`；修复按钮可点 |
| B | 插件：修复成功 | 有效 manifest | marketplace 合并后含 SDD；保留 `investment-signal-monitor`；幂等 |
| B | 插件：manifest 路径越界 | `source.path` 指向插件缓存目录外 | 拒绝修复；报 `PluginRegistrationRepairFailed` |
| B | 插件：重复修复 | 修复后再次执行 | 不产生重复条目 |

## 11. 风险与缓解

- 风险：某些调用方依赖 `None` 被写成空配置。缓解：用编译搜索和测试找出动态 `None` 调用点；需要空配置语义的调用方必须显式传 `Some("")`。
- 风险：schema-v2 MultiRouter 的普通切换缺少物化配置。缓解：普通切换给出明确错误；代理接管路径继续先生成明确接管配置；后续架构任务可以再设计完整投影配置。
- 风险：live 已损坏时无法读取用户表。缓解：解析失败直接报错并分级，不能覆盖现场；发现 `.bak` 候选时引导用户确认。
- 风险：乐观并发在高频冲突下放弃写入。缓解：有限重试加明确结果；UI 提示“配置正被其他程序修改，请稍后重试”。
- 风险：端口探测把非 CCSM 服务误判为可复用。缓解：要求 `/health` 与 `/status` 双重匹配实例身份，任一不匹配都按未知占用者处理。
- 风险：插件登记修复误写 marketplace。缓解：manifest 验证、路径边界检查、结构化合并、幂等去重、原子替换；默认只提供修复按钮。
- 风险：启动事件早于前端订阅。缓解：结果先持久化再发事件，前端可查询补拉。

## 12. 回滚

实现应以小步提交或清晰补丁边界完成。A 层回滚只撤回 Codex 写入边界、启动恢复预检、乐观并发复检和对应测试；B 层回滚按 marker 分类、端口探测、恢复结果上报、插件登记四个独立边界分别撤回。不要回退本设计文档、探索报告或用户工作树中的无关文件。回滚后必须重新运行账本中的基线测试，确认旧行为没有带来额外文件变化。

## 13. Git、上游贡献与 fork 发布契约

- `BigStrongSun/ccswitchmulti` 是母仓库；`zhushihao/ccswitchmulti` 是世豪自己的 fork。产品安装包只由 fork 的 GitHub Actions 发布，不在母仓库创建发布标签或 Release。
- 母仓库贡献必须遵循“一项独立问题、一条从最新母仓库 `main` 创建的新分支、一个新 Issue、一个新 PR”。任何本次修复都不得继续推送到历史 PR #35、#37、#39、#41、#43、#45、#47 或 #49 的 head 分支。
- 上游 PR 分支不得包含 fork 专用的版本号、签名密钥、发布说明、README 徽章、自动更新地址或 Release 工作流调整。
- 本实现按可独立审查和回滚的责任边界拆成四组上游贡献：配置防丢与安全写入；启动 marker 与端口识别；恢复结果与界面反馈；插件登记一致性。每组先创建新的母仓库 Issue，代码验证通过后再创建对应的新 PR。
- fork 发布使用单独的新集成分支。集成分支只挑选已经验证的功能提交，再单独加入 fork 版本号和发布说明；`v*` 标签只推送到 fork，由现有 `.github/workflows/release.yml` 自动发布。
- 每次开始实现前必须 `git fetch origin --prune` 并记录新的 `origin/main`；每个上游 PR 创建前和 fork 发布前必须再次同步。若母仓库已经前进，必须在新的母仓库基线上重建干净分支并重新验证，不能把旧基线直接推送。
- fork 版本以母仓库最新稳定 Release 标签为基准，追加递增数字后缀：母仓库为 `vX` 时，fork 依次发布 `vX.1`、`vX.2`。如果 fork 已发布到 `vX.2` 且母仓库仍是 `vX`，下一版是 `vX.3`；如果母仓库升级为 `vY`，fork 从 `vY.1` 重新开始。
- 历史 PR 只作为证据来源。本任务不更新其提交、描述、base、head 或发布标签，避免扩大 diff 和制造无法合并的历史依赖。

## 14. 未解决问题

- “检测到后给修复按钮”是否升级为“启动时自动修复插件登记”，需要世豪在批准规格时裁定。
- SDD marketplace 注册文件为什么没有在世豪重新点击安装后写入 SDD，需要独立追踪 Codex 插件安装写路径；本次只做一致性检测与修复，不改 Codex 安装流程。
- schema-v2 MultiRouter 是否需要一份可直接落盘的完整非接管配置，应由后续 MultiRouter 投影设计裁定（C 层）。
- 正式跨进程文件锁与 Codex `expectedVersion` 的协同属于 C 层。
- 18:39 端口占用者的精确 PID 已不可考；本设计用端口探测契约覆盖该类场景，不再追求还原历史 PID。

## 15. 第三版 MCP 补充：用户级 MCP 覆盖问题

### 15.1 问题与证据

世豪在 2026-08-23 确认把“用户级 Codex MCP 会被覆盖”纳入本轮解决。探索报告 `docs/diagnostics/sdd-codex-user-mcp-overwrite-explore-2026-08-23.md` 证明：

- 该问题在 `origin/main`（`9b0fd548`）已经存在，不是当前未提交分支引入的回归。
- 覆盖发生在整表投影路径：`mcp/codex.rs::sync_enabled_to_codex` 以数据库快照为权威，整体替换 `[mcp_servers]` 表，数据库不认识的 live 条目被删除。
- 单条路径（新增、编辑、启用、禁用、删除）只操作一个 id，会保留数据库不认识的 live 条目；两套语义互相矛盾。
- 前端不存在独立的“同步 MCP”按钮；`sync_enabled_for_app` 和 `sync_all_enabled` 全部由供应商切换/保存、设置保存、SQL 导入、云同步后置、统一会话开关、通用配置片段保存等后端流程触发。
- 启动恢复路径当前不直接调用 MCP 整表投影；本轮不新增启动到 MCP 投影的接线。
- 数据库 `mcp_servers` 表和 `McpServer` 结构没有任何所有权/来源字段。

### 15.2 “用户级 MCP”的可观察定义

定义：在 Codex live `config.toml` 的 `[mcp_servers]` 表（含旧格式 `[mcp.servers]` 迁移来源）中，id 不存在于 CCSwitchMulti 数据库 `mcp_servers` 表的条目，称为用户级 MCP（live-only 条目）。

契约：live-only 条目在任何自动对账路径中默认保留，永不删除、永不改写。删除或改写一个 live 条目必须满足“所有权可证明”：该 id 当前存在于数据库 `mcp_servers` 表。

### 15.3 所有权模型候选方案

方案甲：数据库 id 集合即所有权，live-as-base 对账（推荐）

- 规则：数据库 `mcp_servers` 表的 id 集合就是 CCSwitchMulti 对 Codex live MCP 的所有权集合。对账从当前 live 文档出发：upsert 所有“数据库中启用 Codex”的条目；只删除“在数据库中但未启用 Codex”的 id；live-only 条目原样保留。
- 优点：无需 schema 迁移，兼容全部已有数据库行；所有权随显式操作自然建立（新增/导入）和撤销（删除）；与单条路径语义完全一致，消除两套矛盾语义。
- 缺点：用户手工修改一个“数据库拥有”的 id 后，下一次对账会用数据库版本还原；这需要在行为契约和文案中明确。

方案乙：数据库新增 provenance 列（`source`/`managed`）

- 规则：为每条数据库行记录来源（界面新增、应用导入、Deep Link、SQL/云同步），对账按来源细分权限。
- 优点：将来可以在界面展示来源徽标，支持更细的“导入但暂不接管”状态。
- 缺点：需要 v16 -> v17 schema 迁移和已有行的回填规则；但 live-only 条目的保护仍然只能靠“id 不在数据库”判断，provenance 列不解决本次覆盖问题本身，属于增量信息而非必要条件。

方案丙：投影状态表（记录 CCSwitchMulti 上一次写入 live 的 id 集合和内容指纹）

- 规则：只有“上次由 CCSwitchMulti 投影”的 id 才允许对账删除或覆盖；能检测外部对 managed id 的手工修改并按内容指纹决定保留或还原。
- 优点：同名冲突语义最细，可以区分“用户手工改过”。
- 缺点：引入新的持久化状态、同步时机和指纹陈旧问题；SQL/云同步清空数据库后状态表与 live 的关系需要额外裁定；复杂度明显超出本次故障的最小修复。

结论：采用方案甲。方案乙作为后续可选增强，方案丙不采用。

### 15.4 裁定（第三版新增）

裁定 12：数据库 id 集合即 Codex live MCP 所有权集合，live-only 默认保留

决定：CCSwitchMulti 只修改或删除它明确拥有的 MCP id——即当前存在于数据库 `mcp_servers` 表的 id。live 中数据库不认识的条目是用户级 MCP，任何自动对账都不得删除或改写。

原因：数据库行只能由显式动作产生（界面新增/编辑、应用导入、Deep Link 导入、SQL/云同步导入），因此 id 在数据库中即构成可证明的所有权。

判断错误的代价：继续整表替换会把世豪手工写在 `config.toml` 里的 MCP 在每次供应商切换时清空。

裁定 13：整表投影改为 live-as-base 对账，空数据库不得清空 live 表

决定：`sync_enabled_to_codex` 改为对账语义：以当前 live 文档为底，upsert 数据库中启用 Codex 的条目；仅当某 id 在数据库中但 Codex 未启用时才从 live 删除该 id；数据库为空时 owned 集合为空，对账不删除任何 live 条目，也不删除整张 `[mcp_servers]` 表。

原因：空数据库只表示“CCSwitchMulti 没有管理任何 MCP”，不能推出“用户想清空 live MCP”。

判断错误的代价：全新安装或 SQL 导入清空数据库后，第一次供应商切换就会删除用户全部手工 MCP。

裁定 14：同名冲突的确定规则

决定：

- 同名同内容：不动 live（对账幂等）。
- 同名不同内容且 id 在数据库中：数据库版本覆盖 live。所有权可证明，不视为“静默覆盖”；该行为写入文档和界面提示，引导用户在 CCSwitchMulti 内编辑 managed 条目。
- 同名不同内容且 id 不在数据库中：不可能发生（live-only 条目不参与同名判定，对账不触碰）。
- 用户外部手工修改 managed id 的 live 内容：下一次对账用数据库版本还原。这是方案甲的已知取舍，作为确定性行为记录，不做内容级合并猜测。

原因：数据库行可证明所有权；对无法证明所有权的 live 内容（live-only）则绝对不覆盖。

判断错误的代价：如果反过来让 live 优先，界面上的编辑会在下次切换后被 live 旧值顶掉，数据库失去权威意义。

裁定 15：显式禁用与删除即撤销所有权

决定：用户在界面取消某 MCP 的 Codex 启用，或删除某 MCP，即显式撤销 CCSwitchMulti 对该 id 的 Codex 所有权：单条路径继续只移除该 id 的 live 条目。对账路径遇到“数据库中未启用 Codex”的 id 时同样移除。live 中该 id 的手工改动不阻断显式删除，因为对账删除的是“数据库拥有的 id”，用户在界面发出的是明确删除意图。

原因：显式动作和自动对账的权限边界必须不同：显式删除可以移除 owned id，自动对账只能移除“owned 且未启用”的 id，任何路径都不能碰 live-only id。

判断错误的代价：如果删除时因为 live 被手改而拒绝，界面删除会变成永远失败的假按钮。

裁定 16：导入即建立所有权

决定：从应用导入（`import_from_codex` 等）、Deep Link 导入、界面新增，在写入数据库行时建立对该 id 的所有权。导入时若 live 已有同 id 条目，以导入进数据库的内容为准，下一次对账按 managed 条目处理。SQL/云同步导入的数据库行同样获得所有权；SQL/云同步从数据库中移除的 id 自动失去所有权，其 live 条目转为 live-only 并被保留（不因对账消失）。

原因：导入是用户显式发起的“把这条配置交给 CCSwitchMulti 管理”的动作。

判断错误的代价：如果导入不转移所有权，导入后的条目在下次对账又变回“不认识”，界面显示和 live 内容会长期不一致。

裁定 17：旧格式 `[mcp.servers]` 的用户条目迁移后保留

决定：清理旧格式时先迁移再删除：`[mcp.servers]` 中 id 不在数据库的条目迁入 `[mcp_servers]` 作为用户级条目保留（live 同 id 已存在时保留 live 版本）；id 在数据库的条目按数据库版本写入 `[mcp_servers]`；然后才移除 `[mcp.servers]` 表。任何路径不得在未迁移的情况下直接清空旧格式内容。

原因：旧格式里的条目同样是用户配置；当前实现把整表直接删掉，与本次修复的用户级 MCP 是同一类数据丢失。

判断错误的代价：只修新格式不修旧格式，使用旧格式的存量用户仍会在第一次对账丢失全部 MCP。

裁定 18：所有触发路径共用同一对账实现；启动恢复本轮不接 MCP 投影

决定：供应商切换、供应商保存、设置保存（`sync_current_providers_live`）、SQL 导入后置同步、云同步后置流程、统一 Codex 会话开关保存、通用配置片段保存，全部经由 `sync_enabled_for_app` / `sync_all_enabled` 汇聚到同一个 Codex MCP 对账函数，禁止任何路径绕过对账直接整表替换。事实记录：启动恢复当前并不直接调用 MCP 整表投影，本轮不新增该接线；若未来接入，必须复用同一对账函数。

原因：七个触发路径语义不一致正是本次缺陷的结构性原因。

判断错误的代价：只修其中一条路径，其余路径继续删除用户级 MCP。

裁定 19：MCP 写入必须 live-as-base、原子替换、指纹复检、有限重试

决定：`mcp/codex.rs` 的全部三个写入口（对账、单条 upsert、单条删除）改用裁定 9 的乐观并发写入：读取原始字节和指纹；在内存文档上应用 MCP 变更；替换前复检指纹；冲突时重新读取并重新应用变更，最多重试 2 次；超限保持原文件字节不变并返回 `ConcurrentModificationDeferred`。解析失败时同样保持原文件字节不变。若现有 `write_codex_live_config_optimistic` 只接受预合并文本，则在 `codex_config.rs` 新增一个接收文档变更闭包的 `pub(crate)` 更新 API，MCP 层不得自行拼装“先读后写”的非复检流程。

原因：Codex Desktop 是同一文件的并发写入者；对账本身依赖 live 内容，只有在写入点复检才能保证“保留 live-only”的承诺在并发下成立。

判断错误的代价：没有复检时，对账读到的 live-only 条目可能在写入前被 Codex 改掉，CCSwitchMulti 用旧快照写回，等于换一种方式覆盖用户配置。

裁定 20：MCP 敏感字段不进入日志、冲突提示或搜索

决定：日志、恢复结果、冲突提示、前端搜索文本只允许出现 MCP 的 `id` 和字段名，不得出现 `env`、`headers`、`token`、`Authorization` 等值。对账冲突（同名不同内容）的用户可见提示只展示 id 和“已被 CCSwitchMulti 管理”的事实。

原因：MCP 配置常携带密钥；本次新增对账结果和提示文案不能引入新的泄露面。

判断错误的代价：一条错误日志或 toast 就可能把 API key 写进日志文件或屏幕截图。

### 15.5 行为契约（第三版新增）

8.12 Codex MCP 对账

- 输入：数据库全部 MCP id 集合（含未启用）、数据库中启用 Codex 的条目内容。
- 以当前 live 文档为底；live 文件不存在时按空文档处理；live 语法损坏时报错并保持原文件字节不变。
- upsert：数据库启用项逐个写入 `[mcp_servers]`；同名同内容跳过。
- 删除：仅删除“id 在数据库且 Codex 未启用”的 live 条目。
- 保留：live-only 条目、未知顶层表、注释和未触及区域的格式。
- 旧格式：按裁定 17 先迁移后清理。
- 写盘：按裁定 19 的乐观并发流程原子替换。
- 空数据库：不删除任何 live 条目，不删除 `[mcp_servers]` 表。

8.13 触发路径一致性

- `ProviderService::switch`、`ProviderService::update`、接管切回官方分支、`sync_current_to_live`、SQL/云同步后置同步、`reapply_current_codex_official_live`、通用配置片段保存后的 `sync_current_provider_for_app`，对 Codex 的 MCP 影响必须全部经过 8.12 的对账函数。
- 单条 upsert/删除路径（界面新增、编辑、启用、禁用、删除、Deep Link 导入）维持单条语义，并与对账共用同一套所有权判定和写入器。

8.14 既有错误预期测试的修正

- `src-tauri/tests/import_export_sync.rs:434-457` 当前把“无启用项时删除 MCP 表”写成成功条件，必须改为“空投影时 live-only 条目保留”。
- `src-tauri/tests/provider_service.rs:2495-2672`（`switch_codex_syncs_shared_keys_from_live_into_common_config`）当前断言供应商切换后 `mcp_servers` 与 `ghost-legacy` 消失，必须改为“live-only 的 `echo` 保留；旧格式 `ghost-legacy` 迁移到 `[mcp_servers]` 后保留”。
- 这两个测试在新实现前必须先改成正确预期并观察 RED；不允许带着旧预期通过。

### 15.6 测试矩阵（第三版新增，全部 TempHome + 内存数据库）

| 场景 | 输入 | 预期 |
|---|---|---|
| 空 DB + live-only | 数据库无 MCP；live 有 `external-only` | 对账后 `external-only` 保留，`[mcp_servers]` 表保留 |
| DB 启用项 + live-only | DB 有 `managed` 启用 Codex；live 有 `external-only` | 两者都在；`managed` 内容来自 DB |
| 同名同内容 | DB 与 live 的 `same-id` 内容一致 | live 文件语义不变（幂等） |
| 同名不同内容 | DB 与 live 的 `same-id` 内容不同 | DB 版本覆盖；`id` 出现在日志，值不出现 |
| 外部手改 managed id | live 的 `same-id` 被手工改过 | 对账还原为 DB 版本 |
| 显式禁用 | DB 有 `managed`，界面取消 Codex 启用 | live 中 `managed` 被单条移除，其他 live 条目保留 |
| 显式删除 | DB 有 `managed`，界面删除 | 数据库行删除，live 中 `managed` 移除，其他保留 |
| 导入后所有权 | live 有 live-only `foo`，用户执行从应用导入 | 导入后 `foo` 归 CCSwitchMulti 管理；后续对账按 managed 处理 |
| SQL 导入移除 DB 行 | 对账后 SQL 导入清掉 DB 的 `managed` | live 中 `managed` 转为 live-only 并保留 |
| 旧格式迁移 | live 有 `[mcp.servers]` 含用户条目 `ghost-legacy` 和 DB 拥有的 `managed` | 迁移后 `[mcp_servers]` 含两者（`managed` 用 DB 版本），`[mcp.servers]` 被移除 |
| 无效 TOML | live 语法损坏 | 报错；原文件字节不变 |
| 并发修改 | 对账合并完成后、替换前 live 被外部修改 | 重新读取并重新对账；超限报 `ConcurrentModificationDeferred`，原文件保持最后外部内容 |
| 供应商切换 | 切换 Codex 供应商，live 有 live-only MCP | 切换成功且 live-only 保留（修正 `provider_service.rs:2495` 的旧预期） |
| 设置保存后置同步 | `sync_current_providers_live` 触发 | live-only 保留 |
| 敏感字段 | 对账处理含 `env`/`headers` 的条目 | 日志与提示只含 id，不含任何值 |

### 15.7 回滚与交付边界（第三版新增）

- MCP 修复是独立提交边界，独立于第二版的四组上游贡献；对应新的第五组上游 Issue/PR（MCP 所有权对账），不与配置防丢、启动识别、恢复结果 UX、插件登记任何一组混合。
- 回滚 MCP 修复只撤回 `services/mcp.rs`、`mcp/codex.rs`、`codex_config.rs` 的对账与写入器改动和对应测试；不回滚第二版任何内容。
- 实现准入第一步：先修复当前 worktree 未跟踪文件 `src-tauri/src/services/codex_plugin_registry.rs` 的编译错误（缺失 `enabled_codex_plugins`、`RepairableCodexPlugin.repair_action`），使测试门恢复可运行；探索报告已记录 `cargo test` 退出码 101 的既有失败。
- 修改任何代码前必须完成修改前快照：当前 HEAD 为 `6923a996`，工作树有 25 个未提交条目（22 个已修改、3 个未跟踪模块）；快照必须同时覆盖 tracked diff（`git diff` 输出）和 3 个未跟踪文件的完整副本，存放到 worktree 之外；禁止 `git reset`、`git clean` 或任何丢弃性命令。

### 15.8 未解决问题（第三版追加）

- 同名不同内容时是否在界面提供一次性的“该 id 已被 CCSwitchMulti 管理”提示，默认本轮只写文档和日志，不加新 UI；若世豪要求可见提示再追加 commands/API/UI 任务。
- 方案乙（provenance 列）是否作为后续增强，留待下一轮 SDD 裁定。

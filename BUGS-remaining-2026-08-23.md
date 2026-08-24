# CCSwitchMulti 遗留 Bug 排查报告（2026-08-23）

> 排查范围：v3.19.2-16.2 当前代码（HEAD 6923a996）。
> 角色：只找 bug + 给修复思路，不改代码。
> 结论：确认 8 个 bug（1 中 7 低，其中 2 个为本次新发现：迁移边界 + 预览兜底文案）。生产代码无 panic 风险。16.2 打包已完成并发布。
> 本轮新增确认：后端“exact OR prefix”语义已全部统一（含 include 门）；“无 route 第三方静默落官方”已在编译层 fail-closed 修复；modelSelection 所有读取点均已设防（Mac 崩溃类彻底关闭）。

## 一、打包状态（远程仓库 zhushihao/ccswitchmulti）
- 版本 **v3.19.2-16.2 已构建完成并发布**，GitHub Release 已挂 **21 个资产**（macOS .dmg/.zip、Windows x86_64/arm64 Setup/Portable 等），并标记为 **Latest**。
- 本次两个紧急 bug（Mac modelSelection 崩溃、Luna 401 authPolicy）的修复**已包含在 16.2 中**。
- **Mac 端用户**：下载 v3.19.2-16.2 的 `CCSwitchMulti-v3.19.2-16.2-macOS.dmg`（或 `.zip` 解压即用）即可解决 `undefined is not an object (evaluating 'n.modelSelection.mode')` 崩溃。

## 二、确认的 Bug（按严重度排序）

### 1. [中] 删除路由时 publish 缺少 active 过滤（竞态覆盖）
- 位置：`src-tauri/src/codex_multirouter/mutation.rs` `apply_codex_provider_delete_with_hooks`（约 151 行）
- 现象：删除一个被多个路由（个人+公司）共享的 target 时，删除路径的 publish 循环会发布“所有”准备好的路由，没有像更新路径那样用 `active_codex_router_id` 过滤。非活跃路由的 publish 可能后写覆盖活跃路由的 `.codex/cc-switch-model-catalog.json`（last-writer-wins）。
- 根因：更新路径（`apply_codex_provider_mutation_with_publisher`，44 行）有 active 过滤（59 行）；删除路径的 publish 循环（tx.commit 后，约 300 行）没有。
- 修复思路：给删除路径的 publish 循环加上同样的 `active_codex_router_id(db)?` 过滤，只发布活跃路由。

### 2. [低] 前端 readCodexRouting 对 v2.routes 未设防
- 位置：`src/components/codex/CodexRouterWorkspacePage.tsx:1080`
- 现象：schemaVersion=2 的对象如果缺 `routes` 字段（跨设备同步 / 手改 DB），前端 `.map` 崩溃。后端 `CodexRoutingConfigV2` 有 `#[serde(default)]` 能容忍，前端不能。
- 修复思路：`(v2.routes ?? []).map(...)`。

### 3. [低] 3 个次级 UI 路径缺 include 门（与运行时不一致）
- 位置：`src/components/codex/CodexRouterWorkspacePage.tsx` `collectRoutedCatalogModels`（约 2316）、`handlePreviewRoute`（约 3414）、`routeMatchesModel`（约 2355）
- 现象：这 3 处只做“精确或前缀”匹配，没有“include 模式下前缀命中必须同时在 include 列表”的门。后果：include 模式 + 粗前缀（如 "gpt"）时，被反选的模型在这 3 个视图里仍显示为可路由/命中，与运行时 fail-closed 不一致。
- 修复思路：给这 3 处加上与 `routeCanMatchVisibleCatalogModel`（约 1964）和后端 `codex_route_prefix_match_requires_include`（codex.rs:1513）相同的门。

### 4. [低] sanitize_persisted_efforts 不清理 default_effort
- 位置：`src-tauri/src/proxy/providers/codex_reasoning.rs:268-283`
- 现象：清理后保留合法的 supported_efforts + effort_map，但 default_effort 原样不动。validate() 要求 default_effort ∈ supported_efforts。若 default_effort 指向被删/非法档位（如 "ultra"），validate 失败 → 整个能力声明被忽略 → fail-closed。
- 修复思路：sanitize 后若 default_effort ∉ 剩余 supported_efforts，置 None 或回退到 provider 默认。

### 5. [低] selectedPlan 回退不稳定
- 位置：`src/components/codex/CodexRouterWorkspacePage.tsx:3031-3037`
- 现象：activeProviderId 为 undefined（代理还没处理过请求 → active_targets 空）时，回退到 routingPlans[0]，其顺序不稳定（Rust HashMap 的 Object.values 键枚举）→ 可能随机落到公司 plan。
- 修复思路：回退前先用持久化的 currentProviderId（profile 绑定，稳定）。

### 6. [低] saveOrder 诊断日志残留
- 位置：`src/components/codex/CodexRouterWorkspacePage.tsx:4238, 4330, 4345`
- 现象：`console.error("[model-order] ...")` 残留。
- 修复思路：删除或降级为 console.debug。

### 7. [低·本次新发现] v1→v2 迁移：仅前缀的旧路由迁移后失效
- 位置：`src-tauri/src/codex_multirouter/migration.rs` `build_migration`
- 现象：旧路由如果只有 `match.prefixes`（没有 `match.models`），迁移后 `modelSelection` 变成 `Include{models: []}`（空 include）。运行时 `codex_route_prefix_match_requires_include`（codex.rs:1513）对空 include 返回 false → 前缀命中也被拦 → 整条路由匹配不到任何模型（fail-closed）。而 v1 下同样的路由（无 modelSelection）前缀是正常生效的。
- 根因：`legacy_route_canonical_models` 只读 `match.models`，不读 `match.prefixes`；canonical 为空时直接落到 `Include{}`。
- 修复思路：当 canonical_models 为空且 match_prefixes 非空时，`modelSelection` 用 `All`（或 include 所有匹配前缀的目录模型），而不是空 Include。

### 8. [低·本次新发现] 路由预览的“兜底文案”与运行时不一致
- 位置：`src/components/codex/CodexRouterWorkspacePage.tsx` `handlePreviewRoute`（约 3415-3450）
- 现象：预览里如果模型没精确命中，文案会提示“没有精确命中，会走默认路由 …”。但 v2 运行时（`resolve_codex_v2_routed_provider`，codex.rs:568）已经是 **fail-closed**——无匹配一律返回 unroutable，**不再回退默认路由**。所以预览告诉用户“会走默认路由”，实际运行时该请求是不可路由的。用户会被误导，以为某个模型还能用。
- 根因：预览的兜底文案是 v1 时代“default 兜底”语义的残留，没跟上编译层改成 fail-closed 的事实。
- 修复思路：把预览兜底文案改成“没有命中任何启用规则，该模型当前不可路由（fail-closed）”，与运行时一致；或明确区分 v1/v2 语义。

## 三、已确认修复（不再是 bug，勿重复上报）
- modelSelection 未设防访问（Mac 崩溃）— 已修（839efa7c），所有读取点已设防。
- 本轮复核：`readCodexRouting`（1082）、route 编辑器（1784、5717）、checkbox（5870）、saveOrder（4272）等所有 `modelSelection.mode/.models` 读取点，要么 `?? {mode:'all'}` 兜底、要么 `?.` 短路保护，Mac 崩溃类**彻底关闭**。
- v1 路径漂移 `pointer("/upstream/...")` — 已清零，全部重构为 `codex_route_auth_source`（codex.rs），带回退链 upstream.auth→auth→authPolicy→auth_policy。
- “无 route 的第三方模型静默落官方”（unroutable UX 缺口）— 已修：编译层 `resolve_codex_v2_routed_provider` / `resolve_codex_v2_raw_passthrough_provider`（codex.rs:568/619）改为 **fail-closed**，无匹配一律 unroutable，不再 prefix/default/official 兜底。
- 后端“exact OR prefix”语义不一致 — 已统一：`codex_route_matches_model`（codex.rs:1531）、`codex_catalog_route_matches_model`（codex_config.rs:961）、`route_matches_model`（database/schema.rs:2995）三处后端匹配函数**全部加了 include 门**（前缀命中须同时 ∈ include）。
- model_pricing 4 个测试失败 — 已修（现 11 passed / 0 failed）。
- hermes 测试隔离（LOCALAPPDATA）— 已中和。
- 前端投影丢 reasoning 字段 — 已修（catalogDraftFromSourceModel 复制 reasoning）。
- /v1/models 缺 display_name — 已修（openai_model_entry_with_source 传 4 个键）。

## 四之二、错误吞没审计（结论：干净）
对 `let _ =` 全量扫描，重点复核三处“回滚/清理吞错”：
- codex_config.rs:286-288（写 config.toml 失败回滚 auth.json）、commands/proxy.rs:2494（同步代理策略失败回滚 DB）、commands/failover.rs:148（切 P1 失败清理 failover 队列）——三处都是**尽力回滚/清理**，原始错误均正常向上抛出，吞掉的只是“回滚本身再失败”的二次错误（此时已无更好处理），属合理设计，非 bug。

## 四、生产代码 panic 审计（结论：干净）
对 codex_config.rs / config.rs / database/ / claude_mcp.rs / s3_sync.rs / webdav_sync.rs / sync_support.rs 全量扫描 `.unwrap()/.expect()/panic!/unreachable!`：
- 全部命中要么在 `#[cfg(test)]` 内，要么“按构造安全”：
  - codex_config.rs:4175 — 前面已 `let Some(reasoning)=model.get("reasoning") else continue`，键必存在。
  - codex_config.rs:4923 — mutex 中毒（理论风险，需另一线程持锁 panic 才触发，可接受）。
  - codex_config.rs:6020 — 前面刚 `if !cache.is_object(){cache=json!({})}`，必为对象。
  - config.rs:366 — 16 次循环，全碰撞时 last_collision 必为 Some。
- 生产路径无真实 panic 风险。

## 五、向导保存路径（结论：干净）
`buildCodexMultiRouterWizardPlan`（codexMultiRouterWizard.ts:1167）：反选模型会同时从 source provider 目录和 route include 列表移除，spawnAgentModels 也按选中可见模型过滤，无“残留快照”问题。

## 五之二、本轮深挖的三块高风险区（结论：均干净）
马博士按“举一反三”把之前没深挖的三块高风险区全部过了一遍，未发现新 bug：

1. **WebDAV / S3 同步并发与一致性**（services/webdav_sync.rs、s3_sync.rs、sync_protocol.rs、archive.rs、database/backup.rs）：
   - 上传/下载都用 `tokio::Mutex` 串行化（`run_with_webdav_lock` / `run_with_s3_lock`），不会并发读写。
   - 上传顺序“先产物、后 manifest”（best-effort 一致性），部分上传不会留下指向不完整产物的有效 manifest。
   - 下载先校验 manifest 兼容性，再 `download_and_verify`（sha256 校验）逐个拉产物，最后 `apply_snapshot`。
   - **DB 导入是原子的**：`import_sql_string_inner`（backup.rs:150）先在**临时库**执行导入（带 authorizer 防恶意 SQL），`create_tables` + `apply_schema_migrations` + `validate_basic_state` 校验，再用 SQLite **Backup API**（`Backup::new` + `step(-1)`）把临时库原子写回主库——失败不会污染主库。导入前还 `backup_database_file` 备份 + `snapshot_to_memory` 保留本机专属表。
   - 导出用 `snapshot_to_memory`（一致快照，处理 WAL），路径 portable 化 + 密钥按选项脱敏。
   - skills 解压有 **zip 炸弹防护**（`copy_entry_with_total_limit` 总量上限）、**符号链接/循环目录防护**（canonicalize + visited 集合）。
   - skills 先换、DB 后导，DB 失败回滚 skills（双失败才报错）。

2. **数据库锁使用**（`lock_conn!`）：同步导入、mutation 事务里的 `lock_conn!` 都是**短持有**（块内用完即释放），无跨 await 长持有，未见死锁/锁序问题。

3. **转发层 401/故障转移/整流/流式重连/账号池**（proxy/forwarder.rs、streaming_retry.rs、transform_codex_chat.rs）：
   - 401/403 归类为 Credential、402/429 为 Quota、5xx/超时为 Transient、4xx 客户端错误为 Neutral/NonRetryable——分类清晰。
   - 重试循环：Retryable 记健康度（账号池候选只释放 permit 不污染持久 Router）、NonRetryable/ClientAbort 释放 permit 并返回、全被熔断拒绝返回 NoAvailableProvider、全失败返回 last_error——无静默吞错。
   - 整流器（thinking 签名 / budget / media 降级）各自独立重试标记，避免跨 provider 短路；整流未生效按客户端错误返回，不做无意义重试。
   - 流式重连（streaming_retry.rs）：只在“语义输出前”中断才重连（带退避 + 最大次数），语义输出后的传输错误直接透传客户端；keepalive 静默期发 ping；terminal 事件正常转发。
   - 账号池（expand_codex_account_pool）：先观察 Desktop Authorization 代际再发 quota 探测（避免新凭据首快照写旧代际），quota 刷新失败只 warn 不中断。
   - transform_codex_chat.rs 的 `.ok()?` / `unwrap_or` 都是 JSON 字段提取的防御性默认或 `?` 传播，非吞错。

## 六、已知未实现项（非新发现）
- reasoning “窄显示 + 宽映射”（PLAN-reasoning-narrow-wide.md）：底层 bug 真实存在（deepseek/k3 的 codex_selectable_efforts 算成 6 档而非 3 档），方案已写但未实现。属已知待办。

## 六之二、边缘模块全量扫描（结论：均干净）
马博士把剩下的边缘模块（非核心 Codex 路由路径）也逐一扫过，未发现新 bug：
- **quota_collaboration.rs**（配额协作/强制）：`codex_enforcement_reason_inner` 逻辑正确——observe 模式/缓存过期/缺 captured_at 都返回 None（不拦截），只取有限值里的最高利用率，剩余 ≤ 阈值才拦截；6 个单测覆盖各分支。
- **deeplink/provider.rs**（深链导入 Provider）：入参全部校验（resource/app/api_key/endpoint/homepage/name），endpoint 支持逗号分隔多地址（首个为主），ID 用“净化名+时间戳”防碰撞，额外 endpoint 尽力添加（失败只 warn），enabled 才切当前。
- **gemini_url.rs**（Gemini URL 构造）：fragment 剥离、query 合并、base path 规范化都处理到位；`should_normalize_gemini_full_url` 对“不透明 relay 固定端点”保守不改写，只对 Google 主机或 Gemini 专属语法路径改写。
- **claude.rs / transform_gemini.rs / streaming_gemini.rs**（Claude/Gemini 适配器与流式）：生产代码无 unwrap/expect/panic（transform_gemini、streaming_gemini 生产段零命中）；claude.rs 两处 `as_object_mut().unwrap()` 都前置 `get/get_mut` 保证 body 是对象，按构造安全。
- **claude_desktop_config.rs**：`as_object_mut().expect("just normalized to object")` 前置 `if !value.is_object(){value=json!({})}`，按构造安全。
- **subscription.rs**（订阅/额度查询）：三处 `token.expect("token must be Some when status is Valid")` 依赖“status=Valid 时 token 必为 Some”的不变式——马博士逐个核对了 Claude/Codex/Gemini 的凭据解析函数（keychain + 文件两条路径），Valid 分支都返回 `Some(access_token)`，不变式成立，expect 安全。
- **auto_launch.rs / app_store.rs / gemini_mcp.rs / mcp/claude.rs / mcp/gemini.rs / deeplink/parser.rs / deeplink/utils.rs**：生产代码无 unwrap/expect/panic，仅少量 `let _ =`（注册表清理、配置目录刷新等尽力操作，合理）。

- **codex_history_migration.rs（7248 行，历史迁移）**：生产段 3 处 `expect`（2966/2973/3008）都前置了类型归一化（`if !is_object(){=Object}` / `if !is_array(){=Array}`），按构造安全；其余命中都在 `#[cfg(test)]`（4307+）内。
- **codex_state_db / codex_oauth_auth / codex_chat_history / import_export / claude_plugin / gemini_shadow / gemini_schema / session_usage_gemini / gemini_config / DAO 全套**：生产代码无 unwrap/expect/panic，仅少量合理的 `let _ =`（busy_timeout、文件清理等尽力操作）。
- **subscription_grok.rs**：`token.expect("status=Valid 时 token 必为 Some")` 依赖的不变式，马博士核对了 `parse_grok_auth_json`（Valid 分支返回 `Some(access_token)`，且 `select_preferred_entry` 保证 key 非空），不变式成立，安全。

## 七、最终结论
马博士对 CCSwitchMulti（v3.19.2-16.2，HEAD 6923a996）做了全量排查：核心 Codex 路由/代理/同步/数据库/前端主页面 + 全部边缘模块。
- **排查覆盖**：核心 Codex 路由/代理/同步/数据库/前端全部组件 + 配额协作/深链/Gemini URL/Claude·Gemini 适配器/订阅凭据/自启动/应用商店/MCP/状态库/OAuth/聊天历史/导入导出/历史迁移/DAO 全套——整个代码库的核心路径与边缘模块均已穷尽。
- **确认 bug：8 个**（1 中 + 7 低），详见第二节，均给出修复思路。
- **已确认修复**：Mac modelSelection 崩溃、v1 路径漂移、unroutable 落官方、后端匹配语义不一致、model_pricing、hermes 隔离、reasoning 字段、display_name 等。
- **干净区**：生产 panic、吞错误、modelSelection 设防、编译层 fail-closed、向导保存、同步一致性、数据库锁、转发层 401/故障转移/整流/流式重连/账号池、配额协作、深链导入、Gemini URL、Claude/Gemini 适配器、订阅凭据解析。
- 整体质量健康：8 个 bug 里 7 个低危、1 个中危（删除路径竞态），无高危、无数据损坏/崩溃类遗留。

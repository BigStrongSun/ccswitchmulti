# 2026-08-29 Kimi k3-256k "401 supports only 256K context" 根因（抓包确证）

## 结论

- ~~直接触发不是 API Key 失效，而是 Kimi 的上下文预检查：所有 ≥262144 字节的 k3-256k 请求被拒……边界与"256KiB 字节检查"精确吻合。~~ **[2026-08-29 晚间修正：精确 256KiB 字节边界的推断是错的]** 控制变量实验（对抓包真实请求体逐级削减后经 15721 重放）实测：327,860 B / 70,958 实际 input tokens → **200 正常回答**；401,087 B / 86,850 tokens → 200；413,611 B → 401；449,610 B → 401。阈值在 401,087–413,611 字节（实际 tokens 86,850–约 9 万，262,144/3=87,381 落在区间内，疑似 Kimi 守卫按"实际 token×3"或"字节×0.21"量级预估上下文后卡 262,144）。方向不变：**负载主要由 Codex 工具定义构成、请求超阈值即被 401 拒绝**，但拒绝边界不是 256KiB 字节，且实际 token 远低于 262,144。
- 该检查不是 token 制：probe 实测 Kimi 对工具 JSON 约 3.9 bytes/token，431KB 请求折算约 12 万 input token，远低于 262,144 token，仍被拒。且 Kimi 把这一超限错误伪装成 HTTP 401 Unauthorized，是"看起来像 Key 失效"的直接原因。
- 请求膨胀的构成：通过本地中继抓到的完整 Codex 请求体 432,022 bytes 中，`tools` 数组 30 项占 417,946 bytes（96.7%），input 仅 30,485 bytes。空对话"你好"的会话 rollout 全文只有 ~89KB，与请求体差异全部来自 Codex 请求时注入的工具定义（不写入 rollout）。
- 注入大户是用户级 `mcp__codex_apps__*` 应用连接器（CLI 在全新空目录运行同样携带，证明不分工作区）：alpha_vantage 121,756 / github 77,334 / linear 71,773 / sites 32,444 / atlassian_rovo 29,130 / alphaxiv 18,224 / patent_literature_search_oauth 12,347 / plugin_management 7,570 等，合计约 381KB；`agents`(multi-agent v2) 10,967、`mcp__kimi_cu` 8,850、`mcp__node_repl` 2,441。config.toml 里的 mcp_servers 均为小头。Desktop 0.150 比 CLI 0.146 多约 119KB（browser/documents/pdf/spreadsheets/presentations 等 Desktop 专属插件）。
- Kimi 对 Codex 专有结构的接受度（均已实测）：`namespace` 工具类型 HTTP 200 接受（probe B2，332 input tokens）；`tool_search` 工具类型 400 拒绝（15:20/15:21 紧凑模式请求 `tools.16: tool_type "tool_search" is not supported`）；`parallel_tool_calls: false` 400 拒绝（Codex 每个请求都带，是独立于本次 401 的必现 400 隐患）。
- 官方 gpt-5.6-luna 同刻请求仅 173,575 bytes 的原因：官方模型走紧凑工具模式、第三方未知模型全量内联。15:20→15:23 的 216KB→155KB→664KB 序列与"紧凑模式被 Kimi 400 拒绝后 Codex 回退全量内联"一致（推断，Codex 侧机制未读源码确证）。

## 证据与可复现实验

- 日志：`~/.cc-switch/logs/codex-router.log`，失败 trace `c97c1f90`(15:20,216,058→400 tool_search)、`35cfbf32`(15:21,154,862→400 tool_search)、`6de92e23`(15:23,664,842→401)、`ccdf7d23/e1983dd4/2b0be8d0/10b032f5/0d70ed64`(15:25,f01e 空对话 551,284×5→401)、`9ceb9d69`(16:48,CLI 复现 431,473→401)。同刻 luna 200 对照：`17c83c97`(40,230)、a86f(173,575)。
- 抓包：临时 Python 中继 127.0.0.1:15999→15721（纯观察、未改任何配置），`codex exec --skip-git-repo-check -c model_providers.codex_model_router_v2.base_url=…15999… "你好"` 复现，请求体存 `/tmp/k3probe/capture-1.bin`（authorization 头已抹除，无任何密钥；可随时删除）。字段分解与 tools 明细见上。
- 探测（均经 15721 正常路由、由 CC Switch 注入 Kimi Bearer，未接触任何 Key）：无工具基线 HTTP 200（87 input tokens）；单 namespace 工具 HTTP 200（332 tokens）；`parallel_tool_calls:false` HTTP 400。探测在 router log 中 session-id 为 `probe-ccsm-*`。
- k3-256k 当前档案 `experimental_supported_tools=[]`、`max_context_window=262144`、`isDefault=true`（`~/.codex/config.toml` 顶层默认模型即 k3-256k）；目录中所有模型该字段均为空，与请求体积差异无关联。

## 边界

- 未修改任何代码、配置、数据库；未重启 CCSM 进程、15721 或 Codex Desktop。router log 中新增了 4 条 probe-ccsm-* 探测与 2 次 codex exec 复现（401）记录。
- Codex Desktop 15:20 前( compact/tool_search 模式触发条件) 与 15:21→15:23 之间的用户操作未完全还原；"紧凑被拒后回退内联"为最自洽解释。
- k3-256k 作为子代理角色的可用性未单独验证（当日两个 k3 子代理 rollout 存在但无 upstream_send 记录）。

## 后续修复方向（按优先级）

1. 用户侧立即可用：在 Codex Desktop 关闭重度 codex_apps 连接器（至少 alpha_vantage/github/linear；全部 codex_apps 约 381KB），或对 Codex 主会话改用 1M 档（glm-5.3-flash / deepseek-v4-flash-vision-exp）或 Kimi 1M 模型；k3-256k 留给小工具集场景。
2. CC Switch 请求规范化（挂点 `forwarder.rs::prepare_upstream_request_body`，所有出站请求已必经）：对声明不支持的上游过滤/降级 `tool_search` 类型工具（可转等价 function）、移除或翻转 `parallel_tool_calls`。既有同类先例：DeepSeek vendor catalog `supports_search_tool=false`（codex_config.rs:7880）。
3. 过大请求预检：出站前按模型上下文预算（Kimi 实际按字节）比较 `request_bytes`，超限快速失败并给出 top 工具占用诊断，替代上游误导性 401。
4. 错误语义映射：把 Kimi `401 + supports only 256K context` 识别为上下文超限而非认证失败，避免用户按"Key 失效"排查。
5. 交接文档中的 Key 轮换建议仍然有效（本次全程未读取、未复制任何 Key）。

## 2026-08-29 补充：修复方向 2 已实现（TDD，源码态）

- 范围：仅实现上面第 2 条（能力声明驱动的请求规范化）；第 1/3/4 条仍未实现。注意本修复不解决 401 本身——432KB 请求被规范化后仍会因超过 Kimi 256KiB 字节预检查而 401；它消除的是 `tool_search`/`parallel_tool_calls` 两类 400，为紧凑工具模式和干净报错铺路。
- 能力声明链路（全部复用既有字段，不新增用户可编辑项）：DB 模型条目 `supportsSearchTool/supportsParallelToolCalls`（camel/snake 双写法）→ `compiler.rs::effective_capability_summary` 解析进 `CodexModelCapabilitySummary`（新增 `supports_search_tool` 字段）→ route resolver 写入 `codexResolvedCapabilities`（codex.rs 物化时随 contextWindow 一起拷贝）。未显式声明时，forwarder 侧按内置 `codex_native_responses_template.json` 的类别默认兜底：第三方 native-Responses 上游=不支持，官方 OAuth 上游与协议转换上游=完全支持。k3-256k 的 DB 条目虽未显式声明，经模板默认即命中规范化。
- 规范化实现：`codex.rs` 新增 `codex_upstream_tool_wire_capabilities`（三级解析：resolved caps > 目标 provider modelCatalog 条目 > 类别默认）与 `normalize_codex_responses_body_for_upstream_tool_capabilities`（`tool_search` 工具→等价 function（复用 transform_codex_chat 的 `TOOL_SEARCH_PROXY_NAME` 语义与 query/limit 参数形状，Responses function 扁平结构）；历史 `tool_search_call/tool_search_output`→`function_call/function_call_output`（arguments 规范化为字符串）；`parallel_tool_calls` 字段整体移除——Codex 的 `false` 只是对模型侧限制的重复强调，上游默认即等价行为，选移除而非翻转）。forwarder 在 `prepare_upstream_request_body` 与 local-proxy overrides 之后、仅对 Codex /responses 且未经任何协议转换的透传路径应用；重复执行幂等。
- TDD：RED 先行（编译期缺符号确认）。新增 9 个测试：compiler 解析 1（camel/snake/未声明）、resolver 5（第三方默认/官方全支持/resolved caps 显式覆盖 camel+snake/modelCatalog 覆盖/chat 上游不套默认）、normalizer 2（转换+移除+计数/全支持 no-op）、forwarder 端到端 1（内存 DB+TCP mock 上游捕获真实出站 body：resolved-route provider 经完整 forward_with_retry 后 mock 收到的 body 已无 `parallel_tool_calls`、`tool_search` 已转 function、历史项已映射）。
- Fresh 验证：Rust lib 全量 `3528 passed / 0 failed / 6 ignored`（基线 3519+新增 9）；`cargo check --tests --no-default-features` 通过；rustfmt clean；`git diff --check` 通过；`cargo clippy --lib` 17 条警告与既有基线逐一比对，全部位于未触碰代码，本次零新增。
- 边界：未构建安装包、未替换 3.19.2-18 安装态、未重启 CCSM/15721/Codex Desktop；`codex_config.rs`（Issue #74 preview 回归测试）与未跟踪 `src/components/codex/diag-projection.test.ts` 属另一并行工作，未纳入也不属于本次修改。真实 k3-256k 上游验证需下一版 canary。

## 2026-08-29 补充 3：控制变量实验修正阈值结论 + alpha_vantage 单关即可用 + v2 编译失败事件

- 用户质疑后对抓包真实请求体（/tmp/k3probe/capture-1.bin，Codex 全量 30 工具）做逐级削减重放（均经 15721、无密钥、session-id=probe-e*/bisect-*）：**tools=[] 全保留 input → 400 parallel_tool_calls（过上下文检查）**；去 alpha_vantage 单个 → 327,860 B / 70,958 tokens → **200**；alpha 只留 10 子工具+其余全量 → 337,860 B / 73,130 tokens → 200；alpha 全量改名 → 449,609 B → 401；alpha 单独全量 → 153,370 B / 33,959 tokens → 200；bisect-80（401,087 B / 86,850 tokens）→ 200；bisect-95（413,611 B）→ 401。
- 结论修正：(1) 拒绝不是 alpha_vantage 特定内容、不是工具个数（bisect-80/95 同为 30 个顶层工具、一过一拒）、不是精确 256KiB 字节——是**请求总规模阈值**（40–41 万字节 / 实际 8.7–9 万 tokens 之间），与 Kimi 以 262,144 为准的预估守卫一致；(2) **用户侧立即可用修复：在 Codex Desktop 只关 alpha_vantage 一个连接器（-122KB）即可让 k3-256k 在 Codex 里正常工作**（实测 200 带真实回答），但单关后余量仅约 1.6 万 tokens，长对话会再次触顶；建议连 github/linear 一起关（-271KB → 约 3.8 万 tokens，余量充足）；(3) 400 parallel_tool_calls 在请求小于阈值时必然出现，证明规范化修复（已实现未安装）是必要的。
- 凌晨"能用"的最自洽解释收敛为：当时请求在阈值下（连接器集更小或入口不同）。15:20:45 的 216KB 请求（未超阈值）当年 400 于 tool_search；15:21-15:23 用户在 Codex 里开关连接器使请求 216→664KB（有实证）越过阈值后持续 401。
- v2_route_forbidden_field 事件（用户 17:5x 粘贴的错误）：`route[2] contains forbidden inherited field apiFormat`。取证：live DB codexRouting 现为 5 条干净路由（official/kimi/deepseek/qwen/基元律动），无任何路由带 FORBIDDEN 字段；前端 workspace/wizard/enable 三条保存路径的序列化器（serializeCodexRouteV2 等）均只输出合法字段；router log 零条记录（编译失败发生在 route_resolved 之前、直接抛给客户端）。判定为 17:47-17:58 之间路由文档被手工/外部加入 v1 继承字段 `apiFormat` 的瞬态状态，之后已被移除；CC Switch 应用 17:59:04 重启、18:01:02 正常退出，当前 15721 无进程属正常（应用未运行）。产品改进候选：保存路径对 FORBIDDEN 继承字段做规范化（剥离+警告）而非请求期 fail-closed，且错误信息应给出修复指引。

## 2026-08-29 补充 4：v2_route_forbidden_field 复发取证、清库与保存边界防线

- 用户报告"保存子智能体时每次都报错"并怀疑本次会话改坏。澄清：本会话全部改动仅在源码树（未构建安装，运行中进程为 /Applications/CCSwitchMulti.app 3.19.2-18 安装版，PID 3161），且对 DB 的写入仅限一次显式清洗（有备份）；此前所有 DB 访问均为 `mode=ro`。
- 复查 live DB：18:05 时 route[2]（DeepSeek）字段干净，随后再次出现 `apiFormat:"openai_responses"` + 顶层 `match:{models:[deepseek-v4-flash]}`（v1 形态）。后端 `validate_and_collect_affected_router_ids` 在持久化前解析路由文档并 fail closed，因此前端提交的脏文档无法落库；用户看到的保存报错是"保存→投影重建/编译→解析已有毒数据失败"的症状，毒数据由绕过校验的写入路径（确切来源未定，可能为手工 JSON 编辑/旧版导入/外部工具）先前写入。已审计 workspace/wizard/enable 三条前端序列化器与后端 migration/mutation：均不会生成路由级 apiFormat。
- 应急处置：备份 `cc-switch.db.bak-route-clean-20260829-182319` 后，用 SQLite 事务清洗两个 router 的全部路由（剥离 FORBIDDEN 十键 + 遗留 `match`），读回验证 7 条路由全部干净；需重启 CC Switch 使 UI 状态重新加载。
- 永久防线（TDD，源码）：`schema.rs` 新增 `strip_v2_route_inherited_fields`（schemaVersion==2 时剥离路由对象上的 FORBIDDEN 继承键，v1 文档、modelCatalog 模型级 `apiFormat` 等合法声明不受影响，返回是否剥离），FORBIDDEN 列表提升为常量与解析拒绝共用；`dao/providers.rs::save_provider_with_protocol_profiles_for_related_providers` 在持久化前对 codex 应用该剥离并记 warn 日志——覆盖所有写入方（前端、同步、导入、手工）。新增 3 测试（schema 2 + dao 1）；`cargo test --lib` 全量 3535 passed / 0 failed / 6 ignored；rustfmt、diff clean；clippy 基线无新增（38 行含汇总，与 stash 前基线一致）。
- 注意：并行 Issue #74 会话已把 tool 线格式规范化提交为 `7a482d43`、别名等价修复为 `81162ee3`，分支被 rebase（原 c7cecce7 → 26a3a283）；本轮 sanitize 防线尚未提交。防线属运行时修复，需下一版 canary 安装后才在用户机器生效。

## 2026-08-29 补充 5：v2_route_forbidden_field 真正写入者定位——DeepSeek 幂等修复迁移污染 v2 路由

- 用户重启应用（新 PID 7828，18:30:07 启动）后保存子智能体仍报同一错误，且 18:23 清洗过的 route[2] 再次出现 `apiFormat:"openai_responses"` + `match:{models:["deepseek-v4-flash"],prefixes:[同]}`（无 upstream 键）。逐字段溯源锁定写入者：`database/schema.rs::repair_deepseek_native_responses_on_conn`（每次启动经 `database/mod.rs:145` 运行的旧版升级迁移）中的 `canonicalize_deepseek_route`——`match.models/prefixes` 写 `[model]`、路由无 `upstream` 键时写顶层 `route["apiFormat"]`，与毒数据完全吻合。触发条件：provider 包含 deepseek+api.deepseek.com，且路由文本含 deepseek、target 指向官方 DeepSeek provider；该迁移无 schemaVersion 守卫，对 v2 文档照样回写，而 v2 解析对 apiFormat/upstream fail closed。
- 闭环解释所有现象：应用每次启动→迁移"幂等修复"→把字段写回（清库被抵消）→子智能体保存/任意路由编译失败。18:05 读到干净是因为迁移在该次启动的写回时机/条件与读取时机错开（细节未完全还原，不影响结论）；清库必须配合迁移修复才稳定。
- 修复（TDD，源码）：`repair_deepseek_native_responses_on_conn` 对 `codexRouting.schemaVersion==2` 的 provider 整体跳过（迁移只面向 legacy 路由；v2 的协议/模型事实在目标 Provider 目录）。新增回归 `repair_deepseek_native_responses_leaves_schema_v2_routes_untouched`（RED 先行：修复前测试失败，确认迁移确实污染 v2）。加上此前的 save 边界剥离，两层防线齐备。全量 `cargo test --lib` 3535 passed / 0 failed / 6 ignored，fmt/diff clean，clippy 基线无新增。
- 运行时状态：已再次清洗 live DB（route[2] 干净）。当前运行中的实例（7828）后端保存从 DB 现读，理论上不重启即可保存；但**下一次应用启动仍会复现**（安装版 3.19.2-18 含此迁移 bug），必须构建并安装含修复的 canary 才能根治。旧版升级场景的实际影响：v2 下协议事实由 Provider 目录承载，DeepSeek 官方 provider 已是 openai_responses，跳过迁移无实际功能损失。

## 2026-08-29 补充 6：母仓库归属确认（Issue #76）与孤儿子智能体清理

- 母仓库归属：remote upstream = BigStrongSun/ccswitchmulti（origin = zhushihao fork）。两个冲突提交均在 upstream/main：迁移 `4d62e500`（fix(codex): preserve DeepSeek Pro Chat route ownership）与 v2 禁令 `0167dd1d`（feat(codex): define declarative MultiRouter schema v2，FORBIDDEN 列表含 apiFormat/upstream）；upstream 的 database/schema.rs 无任何 schemaVersion 守卫。**判定为母仓库自身 bug，非本地引入**。
- 已提 issue：BigStrongSun/ccswitchmulti#76（含现象、触发条件、机理、最小复现种子、建议修复：迁移对 schemaVersion==2 跳过 + 持久化边界剥离兜底）。
- 孤儿子智能体：subagentV2.profiles 共 11 个，其中 4 个孤儿（模型已不在任何目录）——`glm-5.3-flash`（同名 UUID 别名键 `glm-5.3-flash-4faa657f-…` 的活跃孪生并存，孤儿为 disabled 旧键）、`k3`（已移除的 Kimi 1M 档）、`deepseek-v4-flash`、`deepseek-v4-pro`。不自动清除的原因：SyncCatalog 设计为只增不改（profile 问卷是用户数据），81162ee3 的等价改名迁移在"目标身份已被占用"时不迁移（UUID 键孪生占位），孤儿遂长期滞留且 UI 无法配置。
- 处置：备份 `cc-switch.db.bak-orphan-clean-*` 后删除 4 个孤儿 profile，保留全部目录内模型（含用户停用的 terra/vision-exp/qwen3.8）。产品建议：SyncCatalog 附带 prune（disabled 且无等价目录模型的 profile 提供一键清理），或 UI 对 unroutable profile 显示过期标记。
- UUID 别名成因（用户追问后定位）：Bitto(4e063208) 与基元律动(4faa657f) 都提供上游模型 `glm-5.3-flash`，`resolveWizardModelNameCollisions` 对非官方源改名消歧（官方源才保持原名，`isCanonicalModelSource=isOfficialCodexSource`）；后缀规则 `providerNameSuffix` 把 provider 名称做 ASCII 清洗，**纯中文名"基元律动"清洗后为空，退化成完整 36 位 UUID**，于是可见名变成 `glm-5.3-flash-4faa657f-…`。消歧必要、后缀退化粗糙。改进方向：空清洗时改用 id 前 8 位或允许自定义后缀（需迁移存量别名），子 Agent 列表对编译后无路由对象的 profile 显示不可路由徽标+一键清理。
- 注意：下次应用启动，#76 迁移仍会重新污染 route[2]（安装版未含修复）；孤儿清理不受该迁移影响（它只写 DeepSeek 路由的 match/apiFormat，不动 subagentV2）。

## 2026-08-29 补充 7：18.5 三项改进实现、API 推送与打包

- 已实现（用户拍板全做）：① 别名后缀改拼音——`providerNameSuffix` 清洗为空时转无声调拼音（pinyin-pro，ü→v，「基元律动」→ `glm-5.3-flash-jiyuanlvdong`），再不行回退 provider id 前 8 位；新增 `cleanAliasSegment` 助手。② 存量长别名收敛——`normalizeCodexRoutesForVisibleModelAliases` 的 aliases 合并改为"过期键剔除"（不在修复后可见名集合里的旧键丢弃），工作台重新保存一次路由规则即把全 UUID 旧别名收敛为新规则，无需后端迁移。③ 子 Agent 编辑器——未启用且模型不在当前路由目录的 profile 显示「已过期：模型不在当前路由目录」徽标 + 「删除」按钮（`removeProfile` 走既有草稿脏检查），仅展示层判断，编辑/启用仍按后端 status，避免 v28 死锁回归。
- 新增依赖 pinyin-pro 3.29.3。环境坑：node_modules 原为 pnpm 12.0.0-rc（store v11）所建，与 12.0.0 正式版/11.x（store v10）不兼容——已用 12.0.0 正式版 `pnpm install` 重链到 v10（15s 硬链接），随后 `pnpm add` 正常。corepack 会按 packageManager 把 npx pnpm 钉到 10.12.3，绕过用 `COREPACK_ENABLE_STRICT=0 npx -y pnpm@12.0.0`。
- 新增测试：`codexMultiRouterWizard.test.ts` 2 例（拼音后缀 + 假名回退 id 前 8）。vitest 该文件 7/7；CodexSubagentV2ProfileEditor.test.tsx 127/127；typecheck 唯一报错在并行会话未跟踪的 diag-projection.test.ts（非本次文件）。
- 推送：github.com:443 被重置（api.github.com 可用、无代理配置、SSH 无密钥），改走 **GitHub Git Data API** 重建两个提交推送：远端分支 fix/codex-subagent-alias-equivalence = `3739b892`（fix: #76 迁移跳过+保存剥离）→ `5cfefe09`（feat: 拼音别名+孤儿清理+18.5 版本）。本地同名提交 `96fd4b79`/`4ed883f9` 内容与远端逐字节一致但 sha 不同——网络恢复后 `git pull --rebase` 会识别为空补丁自动对齐。已移除临时 http.version=HTTP/1.1 仓库配置。
- 版本 3.19.2-18.5 四处同步（package.json/Cargo.toml/Cargo.lock/tauri.conf.json）+ `docs/release-notes/v3.19.2-18.5-zh.md`。`pnpm build:renderer` 通过；`pnpm tauri build` 已后台启动，产物待验证（注意本机无 TAURI_SIGNING_PRIVATE_KEY，updater 签名步骤可能如 3.19.2-18 时一样失败，但应用包产物会先行生成）。

## 2026-08-29 补充 2：对"凌晨还能请求 Kimi"质疑的取证（结论：质疑的链路不存在）

用户质疑"今天凌晨同样配置可以请求 Kimi"。四个独立证据源一致证伪"凌晨在 Codex+CCSwitch 链路上用过 Kimi"：
1. router log（覆盖 2026-07-27 起全量）：`api.kimi.com` 上游首次出现 = 2026-08-29 15:20:45，共 107 次（94 次 /coding/v1/responses + 13 次 /coding/responses[15:46-15:51 用户手动去 /v1 试验，全部 404]），此前为零。
2. app log（cc-switch.log + 8/28 轮转日志）：首个 kimi.com 请求目标同为 15:20:45；kimi 相关历史流量仅 2026-07-27/28 的 `kimi-k2.6`，走 `llmapi.bilibili.co`（另一套 provider，model_provider="openai"，Chat Completions，请求 69-133KB，全部成功）。
3. Codex 全部 359 个 rollout 扫描：kimi 模型会话只有 7/27-28 的 kimi-k2.6；今天凌晨（00:28）到 15:20 的会话全部 `model_provider=codex_model_router_v2`，模型为 qwen3.8/gpt-5.6-luna/deepseek-v4-flash-vision；k3-256k 的第一个会话 = 15:25:40。
4. DB：`proxy_request_logs` 中 Kimi For Coding provider（4c556560）成功请求记录为零（历史成功量恒等于 0）；`usage_daily_rollups` Kimi 记录止于 7/28；协议探测档案首条 = 15:45（k3 与 k3-256k 两个模型都探过，partial；k3[1M 档]已从当前 modelCatalog 移除，现存 k3-256k）。
- 结论：凌晨可用的"Kimi"不在这台机器的 Codex+CCSwitch 链路上——最可能是 Kimi For Coding 自有入口（官方客户端/网页/CLI，请求小、自带协议）或对 7/27-28 kimi-k2.6 的记忆。这不削弱反而佐证根因：账号/Key/额度/服务端全程正常（小请求探测 200），唯一失败的变量是 Codex 注入 418KB 工具的超大请求打到 256K 档的 k3-256k。若用户能指认凌晨具体客户端，可再查该路径。


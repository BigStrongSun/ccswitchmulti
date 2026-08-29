# 2026-08-29 Kimi k3-256k "401 supports only 256K context" 根因（抓包确证）

## 结论

- 直接触发不是 API Key 失效，而是 Kimi 的上下文预检查：所有 ≥262144 字节的 k3-256k 请求被拒（431,473 / 551,284 / 664,842 均返回 401 `k3-256k supports only 256K context`），154,862 / 216,058 字节的请求则通过该检查（随后因其他原因 400）。262,144 = 256×1024，边界与"256KiB 字节检查"精确吻合。
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

## 2026-08-29 补充 2：对"凌晨还能请求 Kimi"质疑的取证（结论：质疑的链路不存在）

用户质疑"今天凌晨同样配置可以请求 Kimi"。四个独立证据源一致证伪"凌晨在 Codex+CCSwitch 链路上用过 Kimi"：
1. router log（覆盖 2026-07-27 起全量）：`api.kimi.com` 上游首次出现 = 2026-08-29 15:20:45，共 107 次（94 次 /coding/v1/responses + 13 次 /coding/responses[15:46-15:51 用户手动去 /v1 试验，全部 404]），此前为零。
2. app log（cc-switch.log + 8/28 轮转日志）：首个 kimi.com 请求目标同为 15:20:45；kimi 相关历史流量仅 2026-07-27/28 的 `kimi-k2.6`，走 `llmapi.bilibili.co`（另一套 provider，model_provider="openai"，Chat Completions，请求 69-133KB，全部成功）。
3. Codex 全部 359 个 rollout 扫描：kimi 模型会话只有 7/27-28 的 kimi-k2.6；今天凌晨（00:28）到 15:20 的会话全部 `model_provider=codex_model_router_v2`，模型为 qwen3.8/gpt-5.6-luna/deepseek-v4-flash-vision；k3-256k 的第一个会话 = 15:25:40。
4. DB：`proxy_request_logs` 中 Kimi For Coding provider（4c556560）成功请求记录为零（历史成功量恒等于 0）；`usage_daily_rollups` Kimi 记录止于 7/28；协议探测档案首条 = 15:45（k3 与 k3-256k 两个模型都探过，partial；k3[1M 档]已从当前 modelCatalog 移除，现存 k3-256k）。
- 结论：凌晨可用的"Kimi"不在这台机器的 Codex+CCSwitch 链路上——最可能是 Kimi For Coding 自有入口（官方客户端/网页/CLI，请求小、自带协议）或对 7/27-28 kimi-k2.6 的记忆。这不削弱反而佐证根因：账号/Key/额度/服务端全程正常（小请求探测 200），唯一失败的变量是 Codex 注入 418KB 工具的超大请求打到 256K 档的 k3-256k。若用户能指认凌晨具体客户端，可再查该路径。


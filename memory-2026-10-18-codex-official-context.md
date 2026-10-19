# Codex 官方模型上下文设置

## 2026-10-20 自定义目录配置方法核对

- 用户本轮要求核对正确配置方法，不是授权切换现役配置。内置 Web 重新读取官方 config reference/advanced 及 openai/codex 当前 main 源码；独立 Matrix 搜索仍 HTTP521。没有改现役 TOML、认证、DB、缓存，没有安装、重启、推送或发布。
- 官方配置参考确认 `model_catalog_json` 是启动时加载的 JSON 目录路径，可由单独 profile 配置覆盖。完整目录解析成 `ModelsResponse`，要求非空 `models`，模型条目还有非默认必需字段，不能只拿几个窗口字段组成最小 JSON 替换整份目录；保留原模型工具、instructions、v2、modalities 等元数据。来源：`https://learn.chatgpt.com/docs/config-file/config-reference`、`https://learn.chatgpt.com/docs/config-file/config-advanced`。
- 关键调用链纠正：本地 `codex-source-latest/codex-rs/model-provider/src/provider.rs:396` 等入口遇到 `config_model_catalog=Some` 直接选择 `StaticModelsManager`；官方当前 main 的 `ConfiguredModelProvider::create_models_manager` 也相同。静态 manager 返回本地 models，忽略刷新策略，`refresh_if_new_etag` 无操作，不会被远端 `/models` 再覆盖。不能用未选中的 `OpenAiModelsManager` 刷新逻辑推导静态目录会被覆盖。来源：`https://github.com/openai/codex/blob/main/codex-rs/model-provider/src/provider.rs`、`https://github.com/openai/codex/blob/main/codex-rs/models-manager/src/manager.rs`。
- 独立 Codex 的候选配置：只对目标 `gpt-6.1-sol` 的完整条目设 `context_window=1000000`、`max_context_window=1000000`、`effective_context_window_percent=95`、`auto_compact_token_limit=900000`，顶层 `model_catalog_json` 指向此独立完整目录。若另设顶层窗口/compact，分别为1000000/900000，并指定或保持默认 `model_auto_compact_token_limit_scope="total"`。这是客户端预算覆盖方法，不是证明账号服务端容量的能力声明，不应把本地 override 反写成可信发现 max。
- `with_config_overrides` 仍按 catalog max 限幅，故只填 TOML 的1M而目录max872K时仍受限；静态目录提高max才改变该客户端计算。1M raw、95% effective对应950000有效预算；total scope自动compact为 `min(configured, raw*90%)`，故900000，不是950000。`body_after_prefix` 是不同计数口径，可能直接用显式配置值，不应把90%结论无条件套到所有scope。来源：官方 `models-manager/src/model_info.rs`、`protocol/src/openai_models.rs`；本地 `core/src/session/context_window.rs:31`、`turn_context.rs:309`。
- 实际读取 `https://github.com/openai/codex/issues/41325`：这是2026-08-28 GPT-5.6 Sol报告；作者仅提高 bundled max，远端目录随后替换bundled，未测试静态 `model_catalog_json`，没有执行被服务端拒绝的1M推理。因此标题的 remote cap 不等于服务端推理硬上限，也不能直接套到6.1。响应usage路径使用 `turn_context.model_context_window()` 更新本地token状态，未找到stream返回metadata重写模型窗口的路径；服务端仍可独立拒绝过长请求。
- CCSM 已有受管 catalog 且按Provider逐模型投影context/max，保存门禁仍只信任发现上限。当前3.20.4-7并未开放这项本地实验性max覆盖；正确后续改法是在Official目标模型上区分可信发现能力与显式本地override，保留其他模型和元数据，经受管投影生效，不另挂全局catalog、不全局写1M污染MultiRouter。先验客户端resolved预算，再做目标账号长请求接受验收；本轮只完成文档/源码交叉核对，未执行隔离binary或真实长请求测试。

## 2026-10-19 近月1M配置关键词检索

- 用户明确要求搜索“为codex配置1m上下文”，近月范围为2026-09-19至2026-10-19。内置Web使用中文原词、英文config与明确日期过滤；独立Matrix search返回HTTP521。搜索recency标签会混入8月旧教程，必须以正文发布日期核对，不能把近期crawl当近期发布。
- 日期核验后的线索：2026-10-12 `https://www.allagent.wiki/blog/codex-day-7-context-500k-no-long-context-fee/` 转述6.1可手动提高到1M并介绍config窗口1M/compact900K，但没有证明当前客户端未被catalog限幅；2026-10-08项目维护者原始答复 `https://github.com/BigPizzaV3/CodexPlusPlus/issues/905` 明确需选中模型catalog条目生效、顶层数字不一定覆盖外部catalog；10月原始运行报告 `https://github.com/openai/codex/issues/52567` 记载配置1M而有效828400。三者分开使用，不把教程标题当作实测证明。
- 新检索还找到 `https://docs.xhyapi.com/codex/one-million-context` 自定义model_catalog_json方案：复制完整目录并修改目标条目context/max字段以提高客户端预算。该页无可核实日期，不计入近月清单；来源是教程服务商而非OpenAI，且只用/status显示证明配置，不能证明服务端接受同长度请求。这一路与只改TOML是不同方案，值得独立隔离验证；当前872K是显式目录限幅，不应把它直接等同服务端物理上限。未实施自定义max、未写缓存或现役配置。

## 2026-10-19 订阅长上下文计费核对

- 后续多方核对找到直接官方依据，替代下方仅有社区转述的阶段：`https://help.openai.com/en/articles/20001275-chatgpt-work-and-codex`（2026-10-19读取，页面标4天前更新）明确 GPT-6.1 Sol 在 Work/Codex 默认500,000；订阅 included allowance 无额外 long-context surcharge，但长任务仍计额度；purchased credits、Enterprise PAYG、API继续各自原价规则。不能再把6.1的订阅计费记为“只有转述，无法确认”，也不能套用5.4历史2倍额度。
- OpenAI 2026-03-05 GPT-5.4 发布说明明确：Codex 实验性支持 1M，通过 `model_context_window` / `model_auto_compact_token_limit` 配置；超过标准 272K 的请求按正常速率 2 倍消耗 usage limits。来源：`https://openai.com/pa-IN/index/introducing-gpt-5-4/`（Availability and pricing）。这是该型号当时的订阅额度规则，不是全部新型号的通用规则。
- 当前 `https://learn.chatgpt.com/docs/pricing` 明确 API token prices 与 subscription usage 分离，不能用 API 价估算订阅任务数；上下文、推理、工具、缓存共同影响消耗。设置窗口上限与实际请求超过计费阈值也不是一回事。
- 官方社区 `https://community.openai.com/t/28-days-of-shipping-at-openai/1403897` 的 Day 7 聚合转述 2026-10-12 Tibo 公告：GPT-6.1 Sol 在 Codex/Work 默认从272K升至500K、仍可设到1M、从订阅额度扣用量时不收长上下文额外费用。原始链接 `https://x.com/thsottiaux/status/2109430343346581527` 与补充 `https://x.com/thsottiaux/status/2109430894604026039` 本轮均403，未直接读取原帖。默认500K和订阅无附加费已由上方Help Center独立确认；“可设到1M”仍只有转述且受实际目录限幅，不能作为本机账号1M运行证明。
- Enterprise token-based USD rate card 的 >272K 输入/缓存2倍、输出1.5倍适用于对应企业合同，不能混成 Plus/Pro 通用订阅规则；该页另明确 Astra 在 Codex 无额外长上下文倍率。来源：`https://help.openai.com/en/articles/20001415-chatgpt-rate-card-enterprise-token-based-pricing`。
- 本轮使用内置 Web 检索并读取官方页面，独立 Matrix search 仍HTTP521。当前所见272K/872K是目录来源的观测值，不足以证明 GPT-6.1 Sol 永久只支持872K；继续追踪公共目录/账号目录/本地缓存来源，不用 API 文档覆盖可信 max。
- 本机只读结构化核对：`.cc-switch/codex-official-models-cache.json` source=`https://github.com/openai/codex`、fetched_at=`2026-10-19T03:25:17.467576300Z`，Luna/Sol/6.1 Sol三型均显式default272000/max872000、无独立input/output。`.codex/models_cache.json` 的etag=`cc-switch-model-catalog`，为CCSM生成的投影，不是独立官方账号证据。解析分别取context_window/max_context_window，没有default→max换算；872K的直接来源是公开快照。此轮未重取账号OAuth目录、未发送推理请求；fresh public raw请求超时。
- 官方Codex配置参考 `https://learn.chatgpt.com/docs/config-file/config-reference` 区分窗口、auto compact阈值、compact scope；官方源码 `codex-rs/models-manager/src/model_info.rs` 把配置窗口按server max取min，`protocol/src/openai_models.rs` 的compact最多为raw resolved window的90%。95% effective window另算，不能将其再乘90%当阈值。于是目录max872000时，即使填写1000000，仍只得到raw872000、95%有效828400、自然compact上限784800；500000对应有效475000、自然compact450000。这是源码计算，不是本机实际长请求验证。
- 独立一手报告旁证但不当官方保证：`https://github.com/openai/codex/issues/53336` 作者控制账号/endpoint/client_version仅改originator时得到272K/500K，正常切账号后另一账号也可收500K，报告中max仍872K；`https://github.com/openai/codex/issues/52567` 作者填写1M后记录有效828400，并在后续恢复测试报告802805输入成功。不能据此认定本机账号已接受对应容量，不部署originator冒充或修改trusted max。
- 可落地于CCSM的方案仍是OpenAI Official Provider逐模型的6.1窗口：选择500000匹配公告默认，或选择该次可信目录的max（本次快照872000）；保存并重新应用对应Provider/路由，新会话再读实际model_context_window。不要在MultiRouter全局写1M/compact而污染其他模型；native独立配置示例与CCSM管理目录模式不是同一个写入口。真1M输入/有效窗口尚无本机账号运行证明。

## 2026-10-19 3.20.4-7本地构建交付

- 源提交 `b70dec331d8b0bcca5182715b2f6c08ee0192f58`，16:50:42启动local-release pipeline、17:08:19完成原子导出至主项目 `release/local-v3.20.4-7`。helper编译8m05s，renderer production build和typecheck通过；执行会话收尾输出未取回（session已消失），不伪称拿到pipeline退出码。导出metadata、全部产物及源版本已实查，构建进程已结束，隔离Cargo target与lock均已清理。
- 安装包13,982,572 bytes，SHA256 `11E8F669F27C05B8C3D1B11390EDA5B43814FF10AFF0EC2D00881CCA815EA7FA`；portable ZIP16,514,036 bytes，SHA256 `31EC91341FF8C982F1371D1C8079B8E42F0871CE852DB39AF8563284482BB3A0`；raw/ZIP内EXE44,886,016 bytes，SHA256 `80D50C4C01EDEFE81A8617617FD21460196F5F06A07EC10A18F403644FAB9C7D`。
- 16/16 manifest哈希、PE ProductName=CCSwitchMulti、FileVersion/ProductVersion=3.20.4-7、metadata源提交、portable/raw一致、latest版本及signature文本一致通过。复用NSIS派生校验函数得到installed EXE hash `90C6A2598FDECD9F97FAC2731D7F6DE6343AABBABACDCA964DB74132B6675C0D`，与导出声明相符；签名文本一致不是额外公钥验签，派生hash不是安装验收。
- 未安装/重启/发布/push，现役认证、Provider配置与DB不变。源码组件验收Vite4179已停止。此包修正文档输出规格展示和未知max的UI校验，不代表已修复公共目录默认漂移或证明真实1M运行。

## 2026-10-19 官方文档规格与 Codex 目录来源分离

- 用户安装 3.20.4-6 后指出输出一直显示“目录未提供”。根因不是缺输出导致上下文保存失败，而是 UI 只展示 Codex 目录字段、未接入官方文档参考；该目录通常没有独立 input/output 字段，不能由字段缺失推断模型没有公开规格。
- 内置 Web 独立检索并打开 `https://developers.openai.com/api/docs/models/gpt-6-luna`、`gpt-6-sol`、`gpt-6.1-sol`，三个精确型号均声明 API context 1,050,000、max output 128,000。Matrix 独立检索返回 HTTP 521，不能称第二检索链交叉验证成功。文档参考必须标来源和核验日期，不可写回可信 Codex max、账号发现缓存、DB 或运行目录来虚构账号支持 1M。
- 当前本地 Codex 源码的 Config/ModelInfo/Responses 请求没有模型输出预算配置入口；不要混淆工具执行的 `max_output_tokens`。CCSM ProviderMeta.maxOutputTokens 仅在 Codex→Anthropic 协议转换时注入，不适用于 Official。API 最大输出是模型能力规格，不能制作一个在原生 Official 下无效的输出设置控件。
- 本批按真实 upstreamModel 精确匹配已核验的 API 文档参考：目录输出优先，缺失时另标 API 文档的 128K；1.05M API 上下文与 Codex 可设置上限分开。文档不会将目录 872K 抬高，也不推算独立最大输入。上下文设置不以输出字段是否存在为前提。
- 只读进程复核：用户已安装运行 3.20.4-6，不能继续沿用上轮“运行 3.20.4-4”的旧结论。本批版本递增为 3.20.4-7，沿用公开 CCSM 集成 worktree，不改 AgentMesh、不 push 或发布，不自动安装/重启或写现役配置。
- 本批源码验证：CodexFormFields 48/48、TypeScript typecheck、diff check、严格UTF-8/no BOM/no U+FFFD通过；独立审查无阻断。Playwright真实组件/合成IPC在390x844和1440x1000验证三型号128K参考、正确官方链接、只有上下文可写、872K仍可设置且API文档不解锁1M，pageerror=0、横向overflow=0。证据在 `release/acceptance/codex-provider-context-visual/evidence/docs-reference-v7/`；不是安装态或真实长请求E2E。
- 下方旧记录的“default>=1M而max缺失”前后端不一致已在本批修复：排除>=1M快捷档位，手输标invalid，清空override恢复默认仍允许。已补组合回归；未放宽后端可信max门禁。

## 2026-10-19 上下文可调范围与输入输出分栏

- 用户指出旧界面只有手输框和上限，无法知道哪些模型可提高以及输入/输出各是多少。根因是发现 DTO 仅有 default/max context，UI 未区分可提高、默认即上限与未知能力；872K 上限的模型也没有快捷档位。现拆出 `CodexOfficialContextRow`，按可信目录展示默认窗口、可设置上限、独立最大输入、最大输出，后两者只透传显式正整数字段，缺失显示“目录未提供”。不由上下文减法猜输出，不把 API 产品规格用于解锁 Codex 上限。
- 档位包含跟随官方默认（清除 override）、已知默认、有效常用值和真实上限；1M 仅在 ceiling 允许时出现。未知 max 时只允许已知 default 或更低；连 default 都未知时禁用输入。手输超限即时提示，原后端可信 max 保存校验继续生效。恢复默认由现有保存规范化清除空 contextWindow，不制造全局覆盖。
- 2026-10-19 只读公共缓存：Astra/Sol/Luna、5.6 三型、daybreak-blue 和 auto-review 的 default=272000/max=872000；daybreak-red 为372000/372000，5.5为272000/272000。公共目录不是当前账号授权证明，UI不硬编码这些值。当前公共缓存未给独立 input/output 上限，证据不足，不能称这些 Codex 账号都可1M输入或128K输出。
- 内置 Web 打开 OpenAI GPT-5.4 API 文档，官方规格为1,050,000 context/128,000 max output；本地官方 Codex 源码确认 context override 由 max_context_window限幅。独立 Matrix search 返回HTTP521，第二搜索链不可用，不能声称双链一致。API 总规格与 Codex 当前来源的可设置上限分开。
- 验证：前端 CodexFormFields 45 + ProviderForm 21，共66 tests；Rust model_fetch 48 + codex_oauth_models 18，共66 tests（含2个新显式输入/输出解析回归）通过；TypeScript、rustfmt、renderer production build通过。初轮测试因测试翻译mock未加载新key及查询多匹配失败，修改测试mock/精确dd查询后通过；typecheck修正row值string/number转换和测试类型后通过。
- Playwright真实源码组件、合成IPC在390x844和1440x1000验证1M/872K档位、未知字段、清除默认、所选账号、失败不静默fallback，pageerror=0、横向overflow=0。截图/结果在 `release/acceptance/codex-provider-context-visual/evidence/`，不是安装态或长请求E2E。独立只读审查无新增阻断；generic第三方parser既有max_input_tokens兼容上下文语义不属于官方专用解析链，未扩展改动。
- 本轮版本统一为3.20.4-6，沿用原集成worktree、本地提交并绕过自动release hook。不push、不发布、不安装、不改现役配置/认证/DB。本地打包结果与哈希待构建完成后追加；此前3.20.4-5包保持不可变，本版保留近5分钟tok/s统计。
- 构建已完成：源提交 `eb2ed2397`，12:40:20开始、12:59:24导出成功，helper release 8m19s、main release 9m28s。产物目录为 `C:/Users/sunda/Documents/LLMservice/cc-switch/release/local-v3.20.4-6`，流水线退出0并finally清理本次3.2GiB隔离target。
- 安装包13,984,096 bytes，SHA256 `2362BA7EE2A6E1BFC39421B597824EB46A6BC4ECE798453F129207F6C713ABC2`；portable ZIP16,506,806 bytes，SHA256 `AE021C44689B7F40EF709FC5865E07DB148B20A79A37A80756F88A0824FC6F43`；raw/ZIP内EXE44,885,504 bytes，SHA256 `A06E6BFB1D4B5AB51283ED1EB8FD3C1ABB926CC898B4BE0DA3A16031B3E09557`。16/16 manifest哈希、metadata源提交、PE CCSwitchMulti/3.20.4-6、portable/raw一致性、latest.json版本与signature文本一致性通过。签名检查是元数据一致性，不是另行公钥验签。
- 初次验收把安装态EXE hash直接与raw比较失败；随后复读既有 `Write-NsisInstalledExeHash`，确认Tauri打包时把raw的UNK标记临时改为NSS，故二者本来不同。复用 `Get-TauriNsisInstalledExeSha256` 核对安装态派生hash `842AD54A5A6790FD53DC52C60A1D36FFDFE20CFBA748C2E1A74303FE8A56A48A` 通过；没有执行安装、没有把派生核对当安装运行证明。
- 追加只读审查记录一个当前公共目录无法触发的边界：若未来目录default>=1M但缺显式max，Row按default产生的高档位/手输可能被后端>=1M可信max门禁拒绝。后端仍fail-closed，不会越权写入；当前default272K/372K不触发。本轮未为未来大default且缺max放宽后端，后续处理应让前端preset/invalid与该门禁一致，并补该组合测试，不要伪造max。这项不计入当前目录范围的通过证明。

## 2026-10-19 本地新版构建

- 用户要求立即本地构建新版本；四处版本元数据统一为 `3.20.4-4`，功能基线为 `d649eb5dc`。沿用现有 local-release pipeline，产物写入主项目忽略目录 `release/local-v3.20.4-4`，不覆盖现役安装，不推送或发布。
- 本阶段只变更版本和记录，复用上一批有效测试、类型检查和独立审查；构建结果与产物哈希以随后流水线和实际文件核验为准。版本递增和本地打包属于机械操作，不需要新的外部事实检索。
- 从 clean source `5ab3e514cedc5c33832090e6bd6760b81d5c3039` 构建，2026-10-19 09:58:54 开始、10:17:47 成功导出；helper release 8m27s，Tauri main release 9m24s。流水线退出 0，finally 清理本次隔离 target（3.2 GiB）。没有安装、重启或发布。
- 导出目录 `C:/Users/sunda/Documents/LLMservice/cc-switch/release/local-v3.20.4-4`：`SHA256SUMS.txt` 16/16 重算匹配，metadata source commit/version 正确，raw PE ProductName=CCSwitchMulti、FileVersion/ProductVersion 均为 `3.20.4-4`。portable ZIP 内 EXE 与 raw EXE SHA-256 均为 `35DF10EDF9A3B94B111B0D79E3DAD6B64669EC2A88425C32A362C9318191C18E`。
- NSIS `windows/installer/CCSwitchMulti_3.20.4-4_x64-setup.exe` SHA-256 为 `376BE890F6BD3414FEB15E9909E47814EA990E76C5BA0E01CB57AE9AA6BA8032`；本地 updater metadata 版本及 signature 与 `.sig` 一致，这是一致性核验，不是另行公钥验签。真实服务端 1M 请求和安装态验收仍未执行。

## 当前 Provider 方案（替代下方历史全局方案）

- 用户后续明确要求入口放在 OpenAI Official Provider，复用其他供应商的逐模型上下文编辑与模型发现；不再提供全局 1M 设置。下面 `ead9246f7` 的全局方案从未部署，本批删除其 AppSettings/Settings UI、编译前覆盖、native 1M 注入及归属 marker，不能再按旧方案实现或恢复。
- 根因之一是官方 Provider 的模型目录 portal 没有 mount target，而且保存路径排除了 official catalog 和映射 meta；不是模型编辑组件缺失。修复须同时打通显示、探测、Provider 源存储和 live 投影，不能只增加按钮。
- 官方探测 DTO 的 default/max 原先混用。现在 `FetchedModel.contextWindow` 与 `maxContextWindow` 分开；只接受官方显式 max 字段作上限，不按型号、用户 catalog 声明或 API 产品页猜测当前 Codex 上限。
- 后端 Provider 写入前校验变更的官方逐模型窗口：1M 或更大必须有可信 max，且不得超出该 max；有可信 max 的较小设置同样不能超限。不自动修改任何模型窗口。旧窗口未改、上游及认证绑定未改时允许无关编辑，避免旧记录阻断改名等操作；第三方逻辑不变。
- 托管 OAuth 探测走已选账号的 `backend-api/codex/models`，成功结果按本地 account_id 存进程内限额快照（32 账号、每账号最多 2048 模型、15 分钟），不存 Token。该账号明确缺失 max 也能覆盖公共目录的较大声明；失败不刷新快照，不串账号。Desktop 当前登录/账号池入口的公共目录发现不等同于账号可用性证明。
- 另一根因是 direct Official 的旧顶层 `model_context_window` / `model_auto_compact_token_limit` 在 live merge 后仍会覆盖逐模型窗口。官方写入边界在 merge 后清理这两个覆盖，仅当本次 provider 配置声明 CCSM-owned catalog；无目录、外部目录、非官方 provider 保持原行为。源 DB 不注入全局请求值，认证写入边界保持原样。
- 代理接管 placeholder writer 不经过普通 Provider merge，是额外写入边界；它也在投影/认证对齐之后复用官方受管目录清理。删除 Provider 后的无类别 verbatim backup 恢复不猜测官方身份。
- 本轮 Rust：context validator 4 passed；OAuth parser/账号快照 17 passed；全库 lib 4343 passed、9 ignored。独立审查随后指出 takeover 绕行并修复，新增隔离 writer 测试核验两模型 1M/272K 及 auth 不变，再跑 `services::proxy` 110 passed。该增量发生在 full lib 之后，不能把 full lib 数字说成包含新增测试。初次新增测试因缺失 Path import 编译失败，改为 std::path::Path 后通过。下方数字只属于历史提交。
- 最终 Rust Clippy `-D warnings` 和格式通过。前端独立审查发现官方身份退出未作废在途结果、visible alias匹配错误，均修正；视觉验收再发现空 upstream 字符串遮蔽 visible fallback 造成重复导入，从共用模型身份 helper 修复并加测试。
- Playwright 使用真实 CodexFormFields 源码、合成 IPC，在 1440x1000 与 390x844 验证只有显式 max=1050000 行可设1M，max=872000/未知行保持272K；探测传所选accountId，失败不调用公共fallback，无pageerror及document横向溢出。证据在忽略目录 `release/acceptance/codex-provider-context-visual/evidence/`。这是源码UI和隔离写入验收，不是安装态或服务端1M长请求E2E。
- 前端最终聚焦 4 文件共 82 tests 通过，覆盖 ProviderForm 官方目录/投影保存、探测与上下文交互，以及原全局入口删除回归。类型检查曾发现新增测试 initialData 多传 id，移除该非法字段后最终 `pnpm typecheck` 通过；不涉及运行行为修改。
- 最终 Prettier、`git diff --check`、30 个变更文本严格 UTF-8 无 BOM/U+FFFD 检查通过；renderer production build 在最终 i18n 后重跑通过，Playwright 两尺寸也重跑通过。构建仅有既有浏览器数据陈旧、动态/静态导入和大 chunk 提示。复用同批 Rust/独立审查结果，不重复执行全库测试。
- 本批本地提交包含源码、测试和本项目记录，绕过自动 release hook；不 push、不发布、不安装，现役配置和认证未改。视觉验收临时 4179 服务在收尾停止；现有集成 worktree 由未结束的 4.0 同步任务继续持有。

## 历史方案（已撤除，保留溯源）

## 范围

- 用户要求 CCSM 最新代码增加 Codex 官方模型 1M 上下文设置。复用 `bigstrongsun/ccsm-4.0-integration`，基线 `c2f925e87`，包版本保持 `3.20.4-3`。不改内部 AgentMesh，不改上游 PR #8403，不自动发版、安装、重启或写现役配置/数据库。
- 主 checkout `bigstrongsun/te-provider-type` 的原 dirty 保持原样；本批没有新增 worktree。

## 官方事实与检索

- OpenAI 配置参考：https://learn.chatgpt.com/docs/config-file/config-reference 。`model_context_window` 是上下文请求配置，`model_auto_compact_token_limit` 是自动压缩阈值，未设置时采用模型默认。
- GPT-5.4 API：https://developers.openai.com/api/docs/models/gpt-5.4 ，API 声明 1,050,000 context；超过 272K 的长上下文有计费差异。API 能力不等同于当前 Codex 模型目录上限。
- 本机官方源码 `codex-source-latest/codex-rs/models-manager/src/model_info.rs`：原生 context override 按 `max_context_window` 取 min；`protocol/src/openai_models.rs`：自动压缩阈值为显式值与窗口 90% 的较小值。不能修改 trusted max 来伪造解锁。
- 实时公共目录 `https://raw.githubusercontent.com/openai/codex/main/codex-rs/models-manager/models.json` 与本地旧目录存在漂移。本轮公共目录多个 GPT 模型 default 272000、max 872000；UI 不硬编码这个数字，始终从 CCSM 现有可信缓存选择链读取。打包/runtime cache 可能使具体模型结果不同。
- 内置 Web 搜索并打开官方文档；独立 Matrix search 返回 `fetch failed`，未获得第二链结果。文档、公共源码与本地实现交叉核对，但不宣称 Matrix 成功或所有账号已实测 1M。

## 实现与归属

- `AppSettings.codexOfficialContext1M` 默认 false，设置页 Codex 区有跟随官方、请求 1M、恢复默认及目录计算表。只保存偏好，不在保存过程中重写 auth/config；用户下一次重新应用/切换供应商配置后新建会话。
- 纯函数 `codex_context` 按可信官方模型 ID/upstreamModel 匹配：effective=min(1000000, declared max)，无 max 时保守使用声明 default；缺失 max 仍展示未知，不伪造成已知能力。已有 max、模型 ID、第三方条目和 DB 原记录不变。
- schema-v2 compiler 在编译前对官方 target 的 clone 应用偏好，因此投影、角色 context、预览及 dependency fingerprint 同源；混合多路由不写全局 context/compact override。
- direct 官方配置使用原生 `model_context_window=1000000`，如有 CCSM 目录同时投影逐模型窗口；Codex 按每个模型的 native/catalog max 限幅。外部 `model_catalog_json` 不接管。显式用户 auto compact 配置不覆盖，UI 是官方目录计算预览而非实际运行态或用户 override 汇总。
- 发现 live merge 会保留 provider 未声明的全局字段，直接写 1M 会在切第三方或关闭后残留。native 写入现在使用该字段的 TOML 注释 marker 保存原 item；重新应用幂等，切换/回填/共享配置提取先恢复归属。只恢复仍等于注入值的字段，用户已手改的数字保留；损坏/未知 marker 不猜测覆盖。独立审查指出原回填错误仅记录后仍返回 live；现恢复失败就返回 DB 原配置，禁止把损坏的 live 值固化。
- `modelCatalog` 已有 DB-SSOT 回填保护继续复用，不把派生长窗口保存为源配置。

## 验证

- 前端最终聚焦 3 文件 20 tests、typecheck、Prettier 通过；新增保存成功而预览刷新失败的独立提示回归。初轮 19 tests 后审查新增了该用例。
- Rust 首轮 helper 编译暴露 toml_edit RawString API 使用错误；修正后幂等测试发现恢复 original item 多加空格，已从 TOML 重建边界修正。最终 helper 12/12 通过。
- 全库 `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1 --quiet`：4354 passed、9 ignored、0 failed。追加损坏 marker 回填保护后，`services::provider::` 聚焦：140 passed、1 ignored、0 failed。该追加用例是在全库之后加入，不把前一份全库计数伪称为包含追加用例。
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`、格式检查和 renderer build 通过；测试仅有既有 Moonshot fixture unused 与故意无效 UTF-8 literal 两项警告。
- Playwright Chromium 以源码真实组件、合成 IPC 验证 1440x900 / 390x844，无 document 横向溢出或 page errors；请求 1M、恢复和保存失败交互通过。证据在忽略目录 `release/acceptance/codex-context-visual/evidence/`，不是安装态或真实服务端 1M 验收。临时 Vite 4178 已停止。
- 本地提交绕过自动 Release hook；不 push、不发布、不安装，不改变现役认证/配置/数据库。已有 4.0 集成 worktree 继续由未完成的迁移任务持有，不为本功能重复创建或删除。

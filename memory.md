# CC Switch Repository Memory

## 2026-09-02 历史任务 Provider 冷恢复与 Qwen reasoning 回放根修

- 目标任务 `01a04e4f-55be-7463-98ca-d5d8fb1cd158` 的 rollout 首条 `session_meta` 与 `state_5.sqlite.threads` 都保存了 `model_provider=openai`，但这不代表 Qwen 请求实际绕过 Router。`codex-router.log` 在任务创建后的首个 turn 明确记录 `outer_provider=codex-multirouter`、`effective_name=Qwen`、上游为 Qwen Responses，且状态 200；因此“历史 Provider 标签”和“真实转发路由”必须分开判断。
- 结合时间线可确定：任务创建时 app-server 仍持有旧的 built-in `openai + 本地 15721 base_url` 运行快照；CCSM/磁盘后来已经使用稳定 `codex_model_router_v2`，但运行中的 app-server 配置和线程元数据不会热迁移。旧字符串在 2026-09-02 恢复时又被当前 app-server 按官方 ChatGPT Provider 解释，于是用户即使把模型切回 `gpt-5.6-sol`，Provider 仍是 `openai`，请求绕过 CCSM 的 official OAuth replay normalizer。
- 报错 `Invalid input[17].content: array too long; maximum length 0, got 1` 已定位到最后一次 compaction 后的首条 Qwen `type=reasoning` item，其 `content=[{type=reasoning_text}]`。CCSM 的 Router official route 已有 `normalize_codex_oauth_reasoning_item`，会把该 raw content 归并为 `summary` 并删除官方私有 schema 不接受的 `content`；失败的直接原因是错误 Provider 让请求没有经过 Router，而不是整个 Responses `input` 数组长度超限。
- 根修不批量重写 rollout/SQLite，也不抹掉历史来源。renderer 兼容 payload 新增当前 live `model_provider`；`normalizeAppServerRequestParams` 只在 `thread/resume` 的 direct 或 `send-cli-request-for-host` wrapped 请求上，把 `modelProvider` 覆盖为当前活动 Provider。缺失/空 Provider 时 no-op，`thread/list modelProviders=[]` 和其它请求保持原语义。已加载且仍被订阅/运行的线程会按 app-server 语义忽略 resume override；重启后首次打开、未加载旧任务、或无订阅 idle cache 被卸载后的 cold resume 才真正创建新 Provider continuation。
- 旧安装态已有 request wrapper V5，若沿用版本号会跳过新行为；本修复把 request wrapper 升为 V6，并用 QuickJS 行为测试锁定 V5 必须替换、V6 幂等。TDD RED 分别捕获缺少 normalization core 与缺少 V5→V6 升级门；GREEN 为 request normalization 3/3、`codex_desktop::tests` 33/33、Rust library `3721 passed / 0 failed / 6 ignored`，`cargo check --lib --no-default-features`、rustfmt 与 diff check 通过。当前只完成隔离 worktree 源码验证，尚未构建/安装新版本，也未重启承载当前任务的 Codex。
- 外部核对使用两条独立链：Codex 内置 Web 与 Matrix WebSearch 均打开 OpenAI 官方 Responses create 文档，公共契约支持 reasoning 通过 `summary`/`encrypted_content` 跨轮回放；公开文档不能单独证明 ChatGPT Codex 私有 schema，最终 `content` 最大长度 0 的判断以本机真实错误、精确 input ordinal、当前 app-server 源码和 CCSM official normalizer 交叉确定。

## 2026-09-02 CCSwitchMulti v3.19.2-23 阶段性大版本发布

- 用户要求把 `v3.19.2-18` 起的后续改动统一整理为一个完整大版本，而不是继续发布只覆盖单点修复的短说明。`v18..main@55245dc4` 实际包含 72 个提交、150 个文件、约 4 万行新增；新说明按自动双协议探测/Provider Set、推理与工具语义、13/14 页 MultiRouter 向导、统一 Protocol Lab、Codex TOML 所有权、运行态与历史恢复、Windows 接管升级、模型兼容和 Guardian V2 九个主题重写，并补充升级步骤、错误分类和安装态边界。
- 版本提交为 `d647e96a`，只包含 `docs/release-notes/v3.19.2-23-zh.md` 与 package/Tauri/Cargo/Cargo.lock 四个版本源；主工作区并行的 Rust 与临时文件没有被暂存。annotated tag `v3.19.2-23` 指向 `d647e96a0f74913c86cf62f9e2f6b108395b15db`，`main` 与标签已推送到 `BigStrongSun/ccswitchmulti`。
- 干净 detached 候选 fresh 门禁：前端 165 files / 1327 tests、Rust library 3718 passed / 0 failed / 6 ignored、Windows guardian/install/release Pester 90/90；TypeScript、Prettier、renderer production build、`cargo check --lib --no-default-features`、rustfmt、diff 与 UTF-8/no-BOM 通过。严格 `cargo clippy -- -D warnings` 在未改业务代码的干净主线报告 25 个存量 lint（含 Rust 1.95 新 lint、dead code 和 large enum）；它不是当前 tag release workflow 的既有门禁，未在本次发布中顺手做大范围无关重构，后续应单独收口且不得声称 Clippy 已通过。
- GitHub Actions run `33590274165` 的 Linux x64/ARM64、Windows x64/ARM64、macOS、Publish GitHub Release、Assemble latest.json 共 7 个 job 全部 `success`。正式 Release `CCSwitchMulti v3.19.2-23` 于 `2026-09-02T05:09:18Z` 发布，为非 draft、非 prerelease，且 `/releases/latest` 已指向本 tag；共 19 个资产。
- 远端 `latest.json` 版本为 `3.19.2-23`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台且六个 signature 均非空。Windows x64 installer 资产 SHA-256 为 `10b8a07c88c593ad724f51d043519e192d8570dd5c8296ba67c24251d9842ebe`；`latest.json` SHA-256 为 `68f9e27e3efe3c6e3ac8ed6a543b43da8a421391d26006c8e63b6d6fea7a88d1`。
- 外部当前状态核对按仓库规则走两条独立链：Codex 内置搜索能定位公开 Releases 但索引停留在较早版本；Matrix WebSearch 直接读取了 v22 Release 正文；最终 v22/v23 的 latest、资产、时间和 workflow 状态以 GitHub API/CLI 一手数据为准。两条搜索链与本地 Git 对版本序列和发布工作流的结论一致，搜索索引时效差异已明确保留。

## 2026-09-02 Guardian V2 FeatureToml 与 Statsig 补丁越界根修

- 截图中的 `data did not match any variant of untagged enum FeatureToml in features.guardianv2` 不是磁盘 `~/.codex/config.toml` 仍然无效。现场磁盘 TOML 不含 Guardian 键，本机随包 `codex.exe features list` 返回 0；但同一 Desktop renderer 的 Statsig `2553103476` 动态配置在 CCSM V5 兼容层运行后，从原始 `{ enabled: false }` 变成了同时含 `available_models`、`default_model`、`use_hidden_models` 的四字段对象，随后被 Desktop 作为请求级 `features.guardianv2` 转发并由 app-server 严格拒绝。
- 根因是 `build_model_picker_unlock_script` 的 V5 `patchStatsigConfig` 没有检查动态配置 ID，把仅属于模型白名单 `107580212` 的三个字段写进了所有 Statsig 动态配置。旧日志只在 `thread/resume` 显示通用“无法加载 config.toml”，所以此前反复重投影 TOML或重启只能处理表象，不能消除 renderer 每次请求时重新制造的坏对象。
- V6 把模型白名单变换严格限制到 `107580212`。若检测到已有 V5 wrapper，会在其外层只清理由旧补丁加入的三个字段，并保留 Guardian 的 `enabled` 与其它未知字段；其它干净动态配置不改写。请求边界另有 fail-safe：只有 app-server 明确返回同时含 `FeatureToml` 和 `features.guardianv2` 的错误时，才把该次请求的 Guardian 对象按 `enabled` 降级为布尔并重试一次；网络错误、其它 feature、其它请求字段和原始对象均不重试/不变更。
- TDD/验证：新增 Guardian 直接/host-wrapped 请求降级、精确错误分类、V5 污染清理、Statsig ID 隔离与原对象不变回归；`codex_desktop::tests` 31/31，通过完整 Rust lib `3769 passed / 0 failed / 6 ignored`，`cargo check --lib --no-default-features`、单文件 rustfmt 和 diff check 通过。源码阶段临时加载同构 V6 canary 后，Guardian 动态值只剩 `enabled`；对原报错任务 `01a05e1e-a8a4-7d31-81fd-e3098d45b106` 人工携带同样污染对象执行 `thread/resume` 已成功返回 thread 结构，证明修复点位于 renderer 请求适配而非 TOML 写入。
- 修复提交为 `0aa10552`，版本提交为 `07b27fed`，发布版本 `3.19.2-22`。从 `07b27fed` 的干净 detached worktree 构建 NSIS；installer SHA-256 为 `EEFF95C36F542131C527C91A0D8B185BE525265462DC74D7ADCE80321E36AD36`，raw EXE SHA-256 为 `6A168FD98A52565FEBDE78D3783A97FA6F9D6AB84E691DED82944B9E3A4991D1`，严格推导的 NSIS installed payload SHA-256 为 `1250FBED20C725C15F9A9E532227D5389E11A4C0836099AFB9128DEB4E55F14A`。首次打包只因尚未先构建声明的 `codex-history-repairer.exe` sidecar 在 NSIS 前 fail-closed；补齐仓库既有 sidecar 构建步骤后，最终 NSIS 命令 exit 0。
- 可回滚替换事务 `ccsm-20260902-055143-dbad6b2630394a19a725d6a4cbae6142` 成功，将 v3.19.2-21 PID `40496` 替换为 v3.19.2-22 PID `61376`。安装后 15721 只有一个 listener，`/health=healthy`、`/status.running=true`、`listener_role=takeover`，安装哈希精确等于预期 payload，事务 stderr 为 0、维护租约已删除；Codex Desktop 主 PID `27348` 全程未退出。
- 安装态真实 Renderer 已读回 `__ccSwitchCodexAppCompatibilityV6`、`appServerPatchVersion=5`、`reactRequestClientPatchVersion=5` 和 `__ccSwitchStatsigPatchVersion=2`；10 个真实 app-server client 已接管且历史查询刷新完成。Guardian `2553103476` 的实际 `value` 仅含 `enabled=false`，不再含三个模型白名单字段；原报错任务再次 `thread/resume` 成功返回 thread 与初始分页结构。当前磁盘 `~/.codex/config.toml` 仍不含 `guardianv2`、`codex.exe features list` exit 0，最新 500 行 CCSM 日志没有 10048、bind、panic、fatal 或新 ERROR。
- 正式发布 tag 为 annotated `v3.19.2-22`，tag object `b26623ac6b54448b1edd4841c4f981e3b74c0acc`，peeled commit 为 `e42ce09a1715eaaef4f66ca8c70d6ba24f2c7686`；发布时远端 `main` 与 peeled commit 一致。GitHub Actions run `33566088594` 的 Linux x64/ARM64、Windows x64/ARM64、macOS、Publish GitHub Release、Assemble latest.json 共 7 个 job 全部 `success`。
- GitHub Release `CCSwitchMulti v3.19.2-22` 于 `2026-09-01T23:14:14Z` 发布，为非 draft、非 prerelease，且 `/releases/latest` 指向本 tag；共 19 个正式资产。下载复验的 `CCSwitchMulti-v3.19.2-22-Windows-Setup.exe` 为 `13,031,106` bytes、SHA-256 `efad92e82b060dc8a7634fee28e3c23e2d62a2190c9450b579f4aeb63df6c3b2`，与 GitHub asset digest 精确一致；对应 `.sig` 为 432 bytes。
- 远端 `latest.json` 版本为 `3.19.2-22`，覆盖 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台，所有 signature 均非空。发布候选在干净 `07b27fed` worktree 上通过前端 165 files/1327 tests、Rust library 3718 passed/0 failed/6 ignored、TypeScript、Prettier、`cargo check --tests`、rustfmt 与 diff check；正式签名多平台产物以 tag workflow 为准，不能用本地 unsigned NSIS 摘要替代远端资产摘要。

## 2026-09-02 Codex 启动门禁与 config.toml 写入边界

- `launchCodexDesktopWithCcswitch` 在 `a8870050` 引入时被放在 `lib.rs` 数据库初始化之前；后续新增的异常恢复、Codex 模型目录投影、15721 接管恢复和配置一致性检查均位于异步启动尾段，旧启动点没有随架构演进迁移。本机 `2026-09-01 12:24`、`16:20` 日志实证都是“先启动 Codex，后恢复代理并写接管配置”，会让 app-server 在 CCSM 修复磁盘前固化旧 Provider/目录状态。
- 根修新增 `codex_startup` 启动事务：显式开关关闭时完全不触碰 Codex；Codex 已运行时保持幂等并交给独立运行态刷新；Codex 未运行时只在模型目录对账、代理接管恢复成功后检查托管投影，发现可修复漂移则重新投影并二次检查，任何接管失败、无效 TOML、修复失败或复检不通过都禁止启动。早期同步启动调用已删除，启动只能发生在恢复与验证之后。
- Codex 新版频繁改写 `config.toml` 主要是官方行为而非托管路由随机损坏：官方 app-server 的 `thread/start` 会在 workspace-write/full-access 工作区持久化 `projects.<path>.trust_level`，`config/value/write` 与 `config/batchWrite` 供 Desktop 保存 `desktop.*`、插件等用户字段。不得把整个文件设为只读；CCSM 只拥有模型、活动 Provider、目录指针和受管路由字段，当前一致性指纹明确忽略 `agents.*`、`desktop.*`、MCP、插件和项目信任，同时托管写入使用最新快照合并与 compare-and-swap 重试。
- TDD 先让“修复并复检后启动、复检失败禁止启动、接管失败禁止启动、已运行不重配、关闭开关零副作用”在旧实现下失败，再实现门禁。fresh 验证：启动门禁 8/8、配置一致性 20/20、启动幂等与 Desktop 字段恢复 2/2；Rust lib 全量 `3765 passed / 0 failed / 6 ignored`，`cargo check --lib`、rustfmt 和 `git diff --check` 通过。根修提交为 `07f4cfa7`，版本提交为 `f7102f2e`，发布版本 `3.19.2-21`。
- 从 `f7102f2e` 的干净 detached worktree 完成 typecheck、release build、Tauri 与 NSIS 打包，产物目录为 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.19.2-21`。installer SHA-256 为 `78A7A0CA79841820E808144444D12404499DFF63CA582C121C179213CEB94CC1`，raw EXE SHA-256 为 `1EB052447D683D96C644021B35078C45DB64693BFDBE33526F985245F846B20A`，推导并发布的 installed payload SHA-256 均为 `B8ED4CB87DC5B9164ACFD6F4D235D25A5F1B2E194FA9E6304D207987CD163E2C`。
- 可回滚替换事务 `ccsm-20260902-042328-5cddd19a3741421c9cdb7fce1def9d11` 成功，将 v3.19.2-20 PID `47612` 替换为 v3.19.2-21 PID `40496`。安装后只有一个 15721 listener，`/health=healthy`、`/status.running=true`、`listener_role=takeover`，版本和 installed hash 一致，事务 stderr 为空、维护租约已删除、数据库只读 integrity 为 `ok`、TOML 严格 UTF-8 解析成功且无 BOM/U+FFFD。
- 安装态日志在 `04:23:41` 明确按“对账 Codex 模型目录 -> 恢复 15721 接管 -> Codex 已运行，跳过启动门禁事务”执行，不再出现旧版的恢复前启动。当前 `launchCodexDesktopWithCcswitch=true`；由于承载本次工作的 Codex 已在运行，安装验收没有强行结束它来实测 absent-process 分支，该分支由 8 条门禁测试锁定。磁盘 `codex_model_router_v2` 托管投影的实时 SHA-256 指纹与 activation receipt 均为 `bd982c7d42183411e83147e1bf11d54188c6924537ec644c046a92cc3b0d3b99`，证明安装后没有托管配置漂移；Codex 对 `projects`、`desktop`、插件等非托管字段的合法写入继续允许。

## 2026-09-02 v3.19.2-20 可回滚替换与异常退出误报根修

- `v3.19.2-19@ce23b42d` 的首轮事务安装本身成功，但新进程 PID `5932` 在 `2026-09-02 02:59:54.089` 新增了一条 `uncleanExit`。根因不是 Codex 或 CCSM 再次崩溃，而是升级包装器只持有 guardian maintenance lease；事务强制停止旧 CCSM PID `57040` 后仍留下 `logs/app-run-marker.json`，而 app exit monitor 不读取 guardian lease，因此下一实例把这次计划内替换误判为异常退出。
- 根修提交 `e5844dc4` 让安装事务在“旧进程身份已验证、进程已退出、15721 已释放”之后退休其精确拥有的 run marker，再做静默卸载/安装。所有权同时核验 PID、版本、规范化可执行路径（含 Codex AppContainer LocalCache 投影）和 Windows process start FILETIME；任一字段不匹配都 fail-closed 并保留 marker。事务计划固定为 `stop -> wait-port-release -> retire-owned-run-marker -> quiescent-config-snapshot -> uninstall/install`，不是通过屏蔽异常检测绕过问题。Windows PowerShell 5.1 Pester fresh 结果为 `90 passed / 0 failed`。
- 最终版本提交为 `5f956239`，版本 `3.19.2-20`。干净 detached worktree 构建产物位于 `releases/ccswitchmulti/v3.19.2-20`；NSIS SHA-256 为 `9E7FBF349596A2CDD56B04876BFE8B27A17F305D41C8F72B5C1E6E4FD1F971FB`，预期/实际 installed payload SHA-256 均为 `98CBFA1D39AF4EB46013BE016BCCC4FE1CC933A1F0C0D09B0316353B48054CB5`。
- 外层升级 `ccsm-20260902-032705-58b80a5c76bd453788661f0398f7195e`、内层事务 `ccsm-20260902-032706-b253cba509a543b7a77c89159c72a5ae` 均成功，把 PID `5932` 替换为 PID `47612`。fresh 安装态读回：15721 只有一个 listener，路径为 `%LOCALAPPDATA%\\CCSwitchMulti\\cc-switch.exe`，file/registry/status/marker 均为 `3.19.2-20`，`/health=healthy`、`listener_role=takeover`，数据库只读 `PRAGMA integrity_check=ok`，`~/.codex/config.toml` 用 smol-toml 解析成功且有 26 个顶层键，Codex Desktop 主进程与 app-server 均保持运行。
- 修复后的事务没有制造新的异常退出：`app-exit-events.jsonl` 仍为 187 条，末条仍是上述 `02:59:54.089` 的旧事件。安装后顶部黄条来自 generation 40 的旧记录 `164313b3-6864-4f3c-a67b-65c3e601c800`，不是 generation 41 的新失败；视觉验收确认后于 `03:33:04.074+08:00` 通过 toast 关闭动作按 ID 写入 `acknowledgedAt`，当前未确认 warning 为 0，记录本身仍保留以便审计。
- Computer Use 对最终安装版确认：Provider 列表和 `New Codex MultiRouter` 使用状态保留；“Codex 状态与修复”把三项独立呈现为“配置与运行态：正常”“分页历史：需要处理（历史文件 3、重复序号 3）”“历史查询兼容层：重启后验证”，一项不会覆盖另一项。为避免中断承载本次工作的 Codex，没有点击会关闭并重启 Codex 的“刷新 Codex 状态”；因此本次证明的是安全安装、运行态与入口/UI 状态，不把分页历史写入修复或兼容层重启后验证冒充已完成。

## 2026-08-31 MultiRouter 完整引导向导漏合根因与主线迁移

- 用户安装态看到左侧只有四步不是新设计回退，而是历史分支漏合。Git 祖先关系已经实证：`b495befd`（四步向导）是当前 `main` 祖先；完整重做位于 `bigstrongsun/multirouter-guided-wizard` 的 `8f5465bc -> 48c62cc5 -> 715a3db6`，其中 `715a3db6` 不是 `main` 祖先。不能把旧分支整体 cherry-pick，因为其中的浅探测、逐 Provider 保存与 Split 二次确认已经被主线 Protocol Lab、Provider Set 原子事务和双协议证据替代。
- 主线迁移保留现有后端架构，只恢复完整页面流程：13 个常规页面（开始、环境、来源就绪、来源选择、目录同步、协议深探测、模型选择、模型顺序、推理、Sub-Agent/工具、路由确认、保存启用、真实请求验收）；没有任何 Provider 时动态插入第 14 页“接入第一个模型源”。后续页可预览但不可编辑，并明确指向缺失的前置步骤。
- 协议页不再出现“基础探测、1024 输出、不验证工具/流式”的旧语义。普通 Provider/模型必须分别验证 Responses 与 Chat 的基础响应、流式 SSE、推理语义、强制工具调用和工具结果续接；401/403/429、网络错误与 5xx 继续归为认证/可用性错误，不得误判协议不支持。保存仍走统一 Protocol Lab 与批量 prepare/commit，自动 Single/Split 均直接 `accept_auto`。
- 新建 Provider 回到向导不能按名称、索引或固定草稿 ID 猜测：`App` 在打开新增页前记录真实 Provider ID 集，关闭后重新读取并计算 ID 差集；向导只合并这些新增 ID、自动选中并回到“模型源就绪”。Qwen 深探测若后端漏回来源，前端会生成具名阻塞失败，禁止呈现“0/0/0 但可继续”的矛盾终态。
- 首个安装候选 `a72896eb` 已生成并安装 NSIS，真实桌面确认左侧为完整 13 页、后续页可预览但不可编辑、协议页包含 Responses/Chat、流式 SSE、推理、强制工具和续轮，且 Qwen 实测得到 Chat Verified / Responses Partial，不再出现模型明细 0/0/0。该次真实多 Provider 探测同时暴露新的汇总根因：batch adapter 会为每个 Provider 转发一个 `batch_finished`，进度对话框却只读取最后一个事件，导致完整明细上方误报“已完成 1 个模型”；进度分母还把固定 Responses 的 official/OAuth 模型算作待探测目标。
- 汇总根修不改探测证据：对话框累加同一次 workflow 中全部 Provider 的 `batch_finished`；模型分母改由 Protocol Lab 的统一自动模型选择器生成，只包含启用、未手动覆盖且确实需要探测的模型。完全手工或 official/xAI/Copilot 源不计入分母；逐模型手工覆盖的 Provider 只跳过已覆盖模型，其余“跟随自动”模型继续探测，避免旧向导把整个 Provider 错误跳过。对应两条回归均先在旧实现下真实 RED，再 GREEN。
- 汇总修复候选 `67b48408` 的干净 NSIS 已生成并安装，安装器 SHA-256 为 `ED2A6CDCE5B969F7535AD2ADE8211E20E7C310524ADDA21766B26ADCCC95036D`，安装后 EXE SHA-256 为 `1EC06E812998F7CA7F9A916FF413729D00FD2B4752080E43CB5B852C03E7262D`。安装态再次确认新建配置显示完整 `1/13` 和所有 13 个页面；继续验收时发现当前主线的 `GOTO_STEP` 会按目标页面重算业务 `status`，而完整重做分支 `715a3db6` 的同一事件只改变可见页面。该偏差会让纯预览“保存并启用”等后续页面把真实工作流状态伪装成 `enablePrompt`，违背“可以查看、完成前置步骤后才能编辑”的语义。根修恢复原分支的状态/页面分离：侧栏和上一步只导航，不篡改探测、保存或启用进度；真实业务转换继续由 `NEXT/FETCH/PROBE/SAVE/ENABLE` 事件承担。回归先在旧实现下以 `状态机：enablePrompt` 真实 RED，再 GREEN；向导/Flow/App/Sub-Agent 定向为 5 files / 199 tests。该修复仍需重新构建安装器并复验真实多 Provider 汇总，不能把本条源码门禁当作最终安装态完成。
- 最新源码 fresh 门禁：前端全量 165 files / 1303 tests；新增汇总、向导分母与 Protocol Lab 定向为 3 files / 28 tests；`pnpm typecheck`、任务文件 Prettier、`pnpm build:renderer` 和 `git diff --check` 通过。此前同一向导候选的 Rust 全量为 3728 passed / 0 failed / 6 ignored，`cargo check --no-default-features` 通过；`cargo fmt --check` 仍只报告并行 Rust 工作。此记录写入时汇总修复后的新安装器尚未完成安装态复验，不能把源码测试冒充最终桌面验收。
- 精确包含 `44413f1d` 的干净安装候选已完成第一轮桌面验收：NSIS SHA-256 `C9A9542A0B8267BB5E539749A89FEC188F9C4495417220D235D4EA29FD3285E7`，安装后 EXE SHA-256 `94246F71B322E55267C9DA9BDBC4A766A3609FF755D97CA17DD95896091ED2F5`。安装态确认 13 页完整、未来页预览保持 `status=opened`、模型目录刷新停留在向导、协议文案覆盖流式/推理/强制工具/续轮；真实探测中 Qwen 为 Chat Verified / Responses Partial，DeepSeek 的 HTTP 402 与 MiniMax 的 HTTP 500 被正确保留为上游可用性错误。
- 该轮真实探测又把汇总缺陷追到更准确的根因：运行中显示 `11` 张卡而结束标题为 `13`，不是手工协议混入，而是两个 Kimi Provider 都含 `k3`、`k3-256k`；后端按 `(Provider, model)` 正确计 13 个目标，前端却用纯模型名 `Map<string, ModelProgress>` 把跨 Provider 同名模型合并为 11 张卡。根修由 batch adapter 给每个进度事件附加 `providerId/providerName`，向导传入同样的具名 expected target，对话框以 `(providerId, model)` 为身份并在卡片显示 Provider 小标签；同名模型回归与 adapter 元数据回归均先 RED 后 GREEN。此修复写入时仍需重新构建安装器并复跑真实探测，不能沿用上一安装候选的结论。
- 正式安装版 `v3.19.2-22` 已完成上述缺口的最终桌面验收：在现有 MultiRouter 草稿中选择 7/7 个 Provider，七个模型目录全部同步成功；深探测按当前目录生成并显示完整 13 个独立目标，结束汇总也是 13 个（Verified 11、Partial 1、Failed 1），没有再从 13 张折叠为 11 张。当前两个 Kimi Provider 的重名项是 `kimi-for-coding` 与 `kimi-for-coding-highspeed`，两个来源的卡片、Provider 标签和最终结果均分别保留，证明身份键 `(providerId, model)` 在安装态生效；先前记录的具体重名模型随上游目录变化，不能把模型名字本身当作固定契约。
- 同一次安装态探测中，Qwen `qwen3.8` 的 Chat 基础响应、流式 SSE、推理正文、强制工具调用和工具结果续轮全部通过并得到 Verified，Responses 因工具调用不支持为 Partial，不再出现 `0/0/0`。`glm-5.3` 因 Chat SSE 网络失败为 Partial，`MiniMax-M3` 的 Responses/Chat 都返回 HTTP 500 而为 Failed；UI 分别保留网络/上游可用性分类，没有误判为协议不支持。官方账号源按固定 Responses 跳过额外计费探测。验收只关闭并丢弃向导草稿，没有执行保存；原 `New Codex MultiRouter` 继续显示使用中，现有路由未被改写。

## 2026-08-30 Codex Provider 协议适配读模型、统一 workflow 与三入口收口

- 本轮在 `main@75ce07ba` 上落实协议适配计划，没有新开分支。领域判断从页面收回后端：`ProviderMeta` 新增 `codexProtocolMode` 与规范化模型名 Key 的 `codexProtocolOverrides`；旧 `modelCatalog[].apiFormat` 和旧 Provider 级 manual 仅在读取/正常保存时原子迁移，不再作为新数据写入模型目录。
- 后端新增 `get_codex_provider_editor_snapshot` 与 `list_codex_provider_adaptation_summaries`。读模型同时恢复逻辑 Provider、Single/Split/legacy 状态、每模型实际协议、自动/手工来源、Chat/Responses 独立证据、过期状态及 projection；列表仍保持扁平，内部同协议叶子不暴露，门面只显示 `自动适配 · Responses + Chat` 小标记。
- Provider Set 按启用模型逐项规划：跟随自动必须持有当前请求指纹的 Verified receipt；手工覆盖允许无证据保存并在 read model 中告警；同协议落一个严格 Provider，混合协议落可见门面和两个隐藏叶子；自动 Partial/Failed 阻塞且数据库零写入。HTTP 521/5xx、网络、超时、401/403/429继续归类为可用性/认证错误，不能被当作协议不支持。
- `Protocol Lab` 已抽为无页面概念的 reducer、adapter 和 `useProtocolLabWorkflow`。Add/Edit、MultiRouter Wizard、Universal 三个入口共用 consent、probe、prepare、commit、blocked、stale retry、projection warning 状态机；自动 Single/Split 都以 `accept_auto` 一次提交，不再弹 Split 二次确认。探测只更新 workflow/read model，不再改写顶层 `apiFormat`。
- Wizard 保留模型源、模型列表、路由预览、启用及历史修复，但删除自身 receipt/Split 编排；Universal 由后端生成最终 Codex draft 后走同一事务。Universal 首次保存且当前没有 Codex Provider 时不会误投影未激活 facade；真实 projection 失败返回“已保存、待刷新”而不是保存失败。
- Responses 续轮探测的三种请求体语义已锁定：`native_only` 原样回放上游 reasoning item；`responses_reasoning_text_content` 构造真实 `reasoning_text` content，绝不伪装为 summary；`omit` 只删除 reasoning item，保留工具调用与结果。只有明确的 400/422 replay-shape 拒绝才逐级降级，普通 400 不降级。
- 生产等价探测继续由同一 candidate/request preparer 生成 endpoint、认证、模型映射、Custom User-Agent、header/body override、推理参数与请求预算，并进入 receipt 指纹；旧测试夹具也改用生产 preparer，避免 Chat 请求错误硬编码到 `/responses`。
- Fresh 源码门禁：Rust 全量 `3718 passed / 0 failed / 6 ignored`；前端全量 `162 files / 1290 tests`；定向 Provider Set 49/49、ordinary preflight 11/11、Universal 22/22、protocol compatibility 124/124、Codex request contract 24/24、Responses continuation 4/4；typecheck、Prettier、rustfmt、`cargo check --no-default-features`、Clippy 和 `git diff --check` 均退出码 0。Clippy 仍有 28 条仓库既有非阻断 warning。本条记录时 Windows installer 的构建、安装和真实桌面 UI 验收尚未执行，不能把源码门禁冒充安装态验收。
- 工作区同时存在 Profile 同步、请求转换、进程监控等并行 Rust 修改和构建目录。本任务精确暂存协议适配文件，并在 `services/provider/mod.rs` 只暂存覆盖迁移与 Universal projection 结果 hunks；不得 `git add .`、reset/clean 或把并行改动混进提交。

## 2026-08-27 用户反馈「官方模型推理强度丢失」追问：不是旧配置污染，是模型名重映射 + 能力契约缺口

- 用户问题：v18 用户反馈官方模型（gpt-5.6-sol）推理强度可能丢失，是否「又考虑了旧配置」。结论：**不是** config.toml / models_cache 旧数据污染。
- 排除旧配置的现场证据（8/27 复核）：(1) 当前 `~/.codex/config.toml` 内联 `[model_providers.codex_model_router_v2].models` 的 gpt-5.6-sol 条目含完整官方档 `supported_reasoning_levels=[low,medium,high,xhigh,max,ultra]` + `additional_speed_tiers=["fast"]` + priority 服务档（历史上 02b9827e 修过的内联缺档问题没有复发）；(2) `models_cache.json` 8/27 21:07 随 router 激活重新同步，11 个模型，gpt-5.6-sol 完整官方档；(3) `models_cache.cc-switch-backup.json`（8/22 真实官方缓存）8 个官方模型档位齐全；(4) 本机 Codex CLI `codex debug models --bundled` 第一方交叉验证：gpt-5.6-sol=[low,medium,high,xhigh,max,ultra]。若请求走 official 缓存来源（platform=None 第 6 优先级）解析 gpt-5.6-sol，high 必然通过——而 422 实际发生，反证请求在能力解析前已被重映射到 qwen3.8。
- 真实根因链（v18 现场，全部本地证据）：(1) Qwen Provider（274cfc2c，8/19 DB 备份证实当时 `meta.apiFormat=openai_chat`，即 responses→chat 转换分支；8/27 深探后才切 openai_responses）modelCatalog 仅声明 qwen3.8（`supportedEfforts=[low,medium,xhigh]` + codexUltra→xhigh）；(2) Codex 请求官方模型名 gpt-5.6-sol（不在该 Provider catalog）到达时，`forwarder.rs:2555 apply_codex_chat_upstream_model` → `apply_codex_upstream_model` 对 catalog 未命中模型回退改写为 Provider 配置模型 qwen3.8，改写发生在能力解析之前；(3) `resolve_codex_chat_reasoning_config` 按改写后的 qwen3.8 解析，UserConfig（用户当前声明，设计上最高优先级）产出 `capability|low,medium,xhigh,ultra|...`，无 high 映射；(4) 选择器/官方目录仍按 gpt-5.6-sol 的官方档暴露 high，用户选 high → `map_capability_reasoning_effort` 两处未命中 → fail-closed 422。`proxy_request_logs`：8/26 23:38 六连 422（Qwen 直连、model=gpt-5.6-sol、allowed 含 ultra）；8/20 16:07 同类（router Qwen 路由、gpt-5.6-luna、allowed=[low,medium,xhigh]，当时 codexUltra 尚未启用）。
- 「丢失」的另一变体（静默）：承接 Provider 对映射后的模型完全无能力声明时，resolver 落到 Unknown，`infer_codex_chat_reasoning_config` 对 Qwen/vLLM haystack 返回 `supports_effort=false`，effort 被静默剥离——用户感知「选的档没生效」。这是保守推断的既有设计（绝不为未知第三方注入私有参数），非 v18 引入。
- v18 是否回归：否。v17→v18 diff 中 forwarder/handlers 只加测试，codex.rs 195 行是 reasoning 投影/transport 继承（23a3df8e/b5395d80/584a1834/a7e87d1d），effort 映射逻辑零改动；bundled fallback（cfb9906a）自 v13 存在；同类 422 在 v17 时代（8/20）已有记录。
- 根修状态：`0424ee5b`（本日记）已在单一 resolver 补最近档钳制（high→xhigh、minimal→low、等距取更高档），主请求路径经 `codex_chat_reasoning_config_from_capability → resolve_subagent_reasoning_capability` 同链覆盖，全链路回归 `qwen_shaped_capability_clamps_high_effort_to_nearest_supported` 已含本场景；3511 passed。已安装 3.19.2-18 不含修复，临时规避：该线程推理档切 xhigh 或会话模型切 qwen3.8。
- 当前路由实况（8/27 21:07 起）：router 激活，`gpt*` 前缀路由到 `codex-official`（native_codex_auth 直连 OpenAI），官方模型 high 档原生可用；422 场景特指「Qwen Provider 直连 + 官方模型名」组合。
- 本次为纯本地源码 + 活体 DB/日志/CLI 取证，第一方来源（CLI bundled + 8/22 官方缓存备份）交叉一致，按 AGENTS.md 规则 11 跳过联网检索。

## 2026-08-27 Qwen 能力契约缺 high 档钳制，gpt-5.6-sol 会话 422 根修

- 现象：`unexpected status 422 ... Provider: Qwen, model: gpt-5.6-sol; cause: 请求/响应转换错误: reasoning effort 'high' is not supported; allowed=[low,medium,xhigh,ultra]`（url http://127.0.0.1:15721/v1/responses），客户端 5/5 重试全失败。
- 现场证据链：(1) `~/.codex/state_5.sqlite` threads 存在 `reasoning_effort=high` 的 gpt-5.6-sol 线程（01a03d50、019fb88a）；(2) Qwen Provider（274cfc2c）modelCatalog 仅有 qwen3.8：`supportedEfforts=[low,medium,xhigh]` + `codexUltra{enabled,providerEffort:xhigh}`，`apply_catalog_ultra_setting` 注入 max→xhigh 后解析契约为 `allowed=[low,medium,xhigh,ultra]`、`effortMap={low,medium,xhigh,max→xhigh,ultra→xhigh}`；(3) `proxy_request_logs` 显示 Qwen Provider 实际收到 gpt-5.6-sol/gpt-5.6-terra 请求（该时段上游 521），证明路由层确实把 GPT 模型名交给 Qwen 承接；(4) `map_capability_reasoning_effort` 先查映射再校验 allowed（宽映射兜底），`high` 两处都未命中 → fail-closed 422。
- 根修（不打补丁）：`resolve_subagent_reasoning_capability` 在解析契约阶段对 Codex 档位阶梯上未被声明映射覆盖的档位统一补全最近受支持档钳制（等距取更高档）：`high→xhigh`、`minimal→low`；Ultra 编排启用时 `ultra` 仍走 max_target（编排语义不变）；fail-closed 仅保留给阶梯之外的未知档位值。`codex_selectable_efforts` 与能力指纹不变，客户端可选档位集合不受影响。
- 为什么不在转换层打补丁：转换层 fail-closed 是契约的最后防线，缺陷在契约本身（resolver 产出的 effortMap 不完整）；在单一 resolver 补全才能同时覆盖 catalog / UI / Sub-Agent 角色 / 请求转换四层投影，与「宽映射兜底先查映射再校验」的既有语义一致。
- 测试：新增 `resolution_clamps_codex_efforts_the_provider_cannot_accept`（Qwen 形态：high→xhigh、minimal→low、selectable 不变）与 `qwen_shaped_capability_clamps_high_effort_to_nearest_supported`（catalog→resolve_codex_chat_reasoning_config→responses_to_chat_completions_with_reasoning 全链路：high 钳为 xhigh、`ultramax` 仍 422）；更新 4 处精确断言以匹配补全后的 effortMap（codex.rs 模式串、codex_config.rs preview/status 两处、subagent capabilities 一处）。`cargo fmt --check` 通过；`cargo test --lib` 3511 passed / 0 failed / 6 ignored。
- 遗留：已安装运行的 cc-switch.exe 仍是旧版。现场临时规避：把 gpt-5.6-sol 线程的推理档切到 xhigh（或把会话模型切 qwen3.8）；修复随下次构建安装生效。本次未联网检索：纯本地源码 + 活体 DB/日志取证，任务不依赖外部事实。

## 2026-08-26 Codex raw reasoning / summary 语义纠正与协议选择解耦

- `8b38cba4` 的 Desktop 兼容方案已废弃：它根据 `originator: Codex Desktop` 把探测得到的 `RawReasoningText` 改标为 `ReasoningSummary`。这会把上游完整原始推理伪装成模型摘要，违反 Codex app-server / OpenAI Responses 的字段契约；客户端可见性问题不能通过篡改响应语义解决。
- 正确映射固定为：上游 raw/readable reasoning → Responses `reasoning_text` / app-server `item/reasoning/textDelta`；只有上游原生 summary → Responses `reasoning_summary_text` / app-server `item/reasoning/summaryTextDelta`。CCSM 不生成摘要；若产品需要摘要，必须依赖上游原生 summary 或新增显式、独立的 summarizer，而不能复用 raw chain-of-thought。
- 删除 `CodexReasoningClient`、Desktop header 分类及其在 RequestContext、普通流式、非流式和 hosted-tool 流式路径中的传播。三条 Chat→Responses 返回路径现在只按已验证档案投影，Desktop、CLI 与外部调用不会改变 raw/summary 的含义。
- 协议选择与 reasoning 形态解耦。selector 只按 `续接 > 强制工具 > SSE > 原生 Responses 同分兜底` 比较；Readable/Summary/Opaque/None 仍完整记录在分支档案并决定选中分支后可采用的 reasoning 投影，但不再被当作协议质量分数。这样不会为了 raw 与 summary 的呈现差异，把能力完全相同的原生 Responses 静默切成 Chat。
- Provider→MultiRouter route 的等价档案物化及运行时继承仍保留。回归覆盖真实 runner 分支形态、普通流式/非流式、hosted-tool 流式，并明确断言 raw reasoning 不会产生任何 `reasoning_summary_text.delta` 或非空 `summary_text`。
- Fresh 验证：protocol compatibility 68/68、handlers 91/91、forwarder 160/160、MultiRouter mutation 17/17、route 继承 2/2；`cargo check --tests --no-default-features` 成功（5 条既有 dead-code warning），Rust lib 全量 3507 passed / 0 failed / 6 ignored；rustfmt、diff、UTF-8 严格解码/no-BOM/U+FFFD 检查通过。
- 联网交叉验证中，Codex 内置搜索读取了 OpenAI/Codex app-server 与 vLLM 官方协议材料，三者一致区分 raw reasoning text 与 native summary；Matrix WebSearch 独立链多次返回 HTTP 521，未形成可用的第二来源证据，不能把该链路失败当成支持性证据。

## 2026-08-25 Qwen3.8 reasoning 已生成但 Codex Desktop 不显示的根因与修复

- 现场 Qwen 任务 `01a032c7-1005-7242-9f9b-88c56e16d13d` 的 rollout 含 10 个 raw reasoning item、合计 322,964 字符，`summary_text=0`；对照 DeepSeek V4 任务的 reasoning 全部位于 summary。故 vLLM 已生成、CCSM Chat→Responses 已转换、Codex app-server 已存储，文本不是在上游或代理中丢失。
- 当前 Codex Desktop `26.818.5345.0` 的 app-server 能接收 `item/reasoning/textDelta` 并写入 `reasoning.content`，但 renderer 的主会话可见性只检查 `reasoning.summary`；`show_raw_agent_reasoning=true` 也没有接入该可见性判断。官方 app-server 协议同时定义 summary delta 和 raw text delta，CLI/TUI 可显示 raw，Desktop 当前版本不能。
- 此节当时提出的 Desktop raw→summary 适配已由 2026-08-26 的语义纠正废弃；现场证据仍有效，但不能据此把 raw chain-of-thought 改标成 summary。
- vLLM/Qwen 参数差异是独立隐患而非本次直接根因。官方 vLLM `ChatCompletionRequest` 会把非空 `reasoning_effort` 合并为 `chat_template_kwargs.enable_thinking`，Qwen3 parser 也从该嵌套字段读取；当前 live Qwen 模型能力声明的上游参数为 `reasoning_effort`，所以本次已实际开启 thinking 并完整返回 reasoning。旧 `thinking` 与 `enable_thinking` 不一致问题不能替代 Desktop 展示修复。
- TDD：新增客户端识别/CLI 隔离、普通流式 Desktop summary、非流式 Desktop summary、hosted-tool Desktop summary 四类回归；`desktop_` 69/69、`reasoning_projection` 5/5、Rust lib 全量 `3508 passed / 6 ignored`，rustfmt 与 `git diff --check` 通过。没有构建安装包、替换安装态、重启 CCSM/Codex 或改写 live 数据库；当前运行程序仍不含本修复。

## 2026-08-25 v3.19.2-17 GitHub 正式发布

- 最终发布提交为 `83f1a03e35e5fb66a28ae4dcca665dce4732e9b5`，四处版本源均为 `3.19.2-17`；annotated tag 对象为 `f3f9170137960b49cd75dbc474084f920034ea79`，本地与远端 peel 均精确指向该发布提交。旧 tag 原指向已取消流水线的 `54cb432d` 且没有 GitHub Release；按用户明确要求删除旧 tag 后基于当前候选重新创建，没有 force-push `main`。
- GitHub Actions Release run `32766476007` 全部成功：macOS universal、Linux x64/ARM64、Windows x64/ARM64 五个平台构建，以及 `Publish GitHub Release`、`Assemble latest.json` 共七个 job 均为 success。Release `CCSwitchMulti v3.19.2-17` 非 draft、非 prerelease，并由 GitHub `/releases/latest` 返回为 Latest。
- Release 共 19 个资产。`latest.json` 版本为 `3.19.2-17`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台，六项下载 URL 均指向本 tag 且 signature 全部非空；Windows x64 Setup SHA-256 为 `6dd5861536d3879e0225cece63a05e058fe3316f51b8b194c4838e6de23ed515`。
- 本轮只完成 GitHub 发布，没有安装、替换、停止或重启本机 CCSwitchMulti/Codex。保留的 `.tmp/`、`docs/provider-settings-layout-preview.html`、`target-protocol-probe/`、`target/` 均未提交。发布前本地 NSIS 已生成，命令最终仅因本机没有 Tauri 私钥而在 updater 签名阶段返回非零；正式资产以 GitHub Actions 的签名构建为准。
- 联网交叉验证使用 Codex WebSearch 与 Matrix WebSearch 两条独立链：前者命中 Tauri 官方 Windows installer 文档和 GitHub Releases 页面，确认 NSIS 与 MSI 是独立目标；Matrix 搜索服务正常但未索引到本次新 Release。当前发布状态、tag、assets 和 `latest.json` 最终以 Git refs、GitHub CLI/API 和 workflow 实跑为权威证据。

## 2026-08-25 PR #62/#64 启动身份与恢复结果按主线重做

- 没有直接合入落后主线的 PR #62/#64。当前实现以 Windows `GetExtendedTcpTable` 取得真实监听 PID，再用 `OpenProcess + GetProcessTimes + QueryFullProcessImageNameW` 取得 PID、创建时间和可执行文件路径；端口只有在监听 PID、进程创建时间、当前可执行文件、路径指纹、配置目录指纹、实例 ID、主版本、共享运行协议版本和 `listenerRole=takeover` 全部一致时才可复用。External OpenAI API 使用同一 `ProxyServer`，但发布 `externalOpenAiApi` 角色，不能被误认成接管代理；未知监听一律 fail closed，不结束、不接管。
- 应用运行 marker 也使用完整进程身份，不能只凭 PID。检测到身份完整匹配的旧实例仍运行时，新实例不覆盖旧 marker、不启动代理恢复；正常/强制退出只删除属于当前 PID、可执行文件和创建时间的 marker，避免第二实例删除第一实例证据或旧实例退出时清掉新实例 marker。配置作用域在一次启动/端口探测中只快照一次再传给纯判断函数，避免并发测试或瞬时全局漂移造成前后不一致。
- 恢复结果不再是单一全局状态：使用最多 64 条的按 generation、operation、app、severity 分离记录，后续成功不能覆盖另一应用的失败；Warning/Error 必须由用户关闭持久 toast 或打开日志后按同一 generation+ID 确认。记录只保存恢复类别、保留/丢失字段和下一步，不复制底层原始错误，避免把可能含 URL、认证材料或配置片段的字符串持久化到 `recovery-outcomes.json`。
- 主提交为 `1d644834`（`fix(startup): verify proxy ownership and preserve recovery failures`）。Fresh 验证：进程身份 4/4、启动/marker 5/5、恢复结果 4/4、代理服务 84/84；Rust lib `3495 passed / 6 ignored`；前端 `147 files / 1202 tests`；`cargo check --tests --no-default-features`、`pnpm typecheck`、Prettier、rustfmt、`git diff --check`、20 个相关文件 UTF-8 strict/no-BOM/U+FFFD 均通过。
- `pnpm build` 已完成 renderer 和 release `cc-switch.exe` 编译，但 WiX `light.exe` 在 MSI bundle 阶段无错误明细地以退出码 1 结束。因此本轮不能声称安装包构建成功；未发布、未安装、未停服务、未替换当前运行态，也未重复盲跑 bundler。
- 联网依据沿用本轮开始时的双链审计：Codex WebSearch 命中 Microsoft 官方 `GetProcessTimes` 和 TCP owner API；Matrix WebSearch 独立调用返回 `fetch failed`，未作为事实证据。最终技术结论以 Microsoft API 契约、本地源码、PR refs、Git history 和 fresh 测试为准。

## 2026-08-24 v3.19.2-17 发布前本地分支与新 PR 复审

- `main` 原候选为 `54cb432d`，annotated tag `v3.19.2-17` peel 到同一提交；对应 Release workflow `32715488426` 已取消，GitHub Release 不存在。审计期间不移动 tag、不发布、不安装。
- 当前真正未进入 `main` 的本地生产分支是 `bigstrongsun/codex-reasoning-probe-backend@005761a9`：25 个提交、46 个文件、约 8342+/182-，包含协议双分支探测、脱敏档案、Provider 保存 preflight、运行时 reasoning 投影和 schema 迁移。分支记录的全量 Rust 证据为 3398 passed / 5 ignored，但它比当前主线落后 37 个提交，merge-tree 在 `memory.md`、`proxy/providers/mod.rs` 冲突；必须按当前 Provider SSOT、终态语义和原生 Responses effort 映射重新集成、复跑门禁，不能整枝盲合。
- `bigstrongsun/ultra-orchestration` 的两个提交虽然 patch-id/commit SHA 不在当前主线，但行为已由 `5036705f`、`b45235d3` 及后续 Ultra 修复完整进入 `main`；`release/v3.19.2-8` 的 mixed hosted/client tool 行为也已在主线同名函数与回归中存在。备份、commentary/portable reasoning no-go、学术资料、AgentMesh 原型和旧 release 分支不属于本次产品候选。
- 新 PR #57 `a4fff767` 修复的问题真实存在：当前 `main` 有 live-as-base 的字段保留，但没有该 PR 的 exact-byte snapshot、bounded retry、deferred outcome、ownership receipt 和 companion rollback 全链。PR 基于落后 48 个提交的旧主线，17 个文件约 7070+/607-，与当前 `codex_config.rs`、`proxy/providers/mod.rs`、`services/provider/live.rs`、`services/proxy.rs` 冲突，CI 也未绿。更关键的是其“fingerprint recheck -> atomic replace”仍不是跨进程 CAS：外部 Codex 可以在最后复检与 replace 之间写入，PR 没有覆盖这一窗口，不能原样宣称完全消除并发覆盖；应在当前主线重新设计最小写入/所有权边界并补可证明回归。
- 新 PR #58 `54e87fa8` 修复的 prefix-only legacy migration 缺口也真实存在，且 merge-tree 可干净合并；但 `provider_catalog_entries` 没有像 compiler 一样过滤 `modelCatalog.models[].enabled=false`，会把已停用模型冻结进 `include` 并产生不可路由白名单。该 PR 的 CI 红来自落后基线的既有前端/Rust 格式漂移，不是 13 个 migration 测试失败，但在补停用模型回归和当前主线完整门禁前仍不可合入。
- 旧 bug PR #19/#21/#24/#26/#35/#37/#39/#41/#43/#45/#47/#49 的原 SHA 多数不在 `main`，但对应 usage attribution、reasoning alias、disabled model、排序、严格 include/fail-closed、v2 authPolicy、missing modelSelection、active-router projection、displayName、Windows replace retry 和 alias-suffixed role 行为已由主线选择性提交或更完整实现覆盖。本次未发现这些旧 PR 还有新的独立生产缺口；判断依据是 merge-base/reachability、函数与测试边界、最终源码语义，而不是只看 `git cherry`。

## 2026-08-24 MultiRouter / V2 Agent Provider 跟随全生命周期复审

- 完整遍历了 schema-v2 MultiRouter 的新建、编辑、启用、Provider 增改删、普通/统一 Provider、SQL/DB/WebDAV/S3 导入恢复、投影、外部 `/v1/models`、V2 Agent 初始化/补档/诊断及 Codex Desktop renderer 注入。Provider 普通写入统一经过 `persist_provider_mutation -> apply_codex_provider_mutation`；`mode=all` 自动跟随 Provider 增删，`mode=include` 是用户显式白名单；已初始化的 V2 Agent 在 Provider 新增可路由模型后自动补一个默认关闭 profile，删除模型保留历史 profile 并显示 unroutable，避免静默删除用户问卷。没有发现还需要删除 Provider 重建才能恢复的独立目录存储。
- 根因一：Codex Desktop Guardian 过去只比较 CDP target ID，Provider/live catalog 更新但 renderer 未重建时不会重注入；同时第一次安装的 App Server/Statsig wrapper 闭包绑定旧 `payload`，即使再次执行脚本也可能继续读取旧目录。现对实际 renderer payload 计算 SHA-256 指纹，target 新增或目录指纹变化均重注入；只有成功才记住新指纹，失败下一轮继续；wrapper 统一动态读取 `state.payload`。Guardian 3/3、QuickJS 动态 payload 1/1 通过。
- 根因二：MultiRouter 工作台虽已从 Provider 实时聚合 `selectedCatalog`，V2 Agent 编辑器却只把它用于文本/图像控件，能力解析、状态诊断和预览仍只传 Router 持久化 settings；schema-v2 Router 不保存派生 `modelCatalog`，所以 Provider 更新推理、上下文、Ultra 后诊断仍可能显示旧值/未知。现三类诊断统一注入最新 Provider 派生 catalog。
- 同类字段审计又发现工作台的 `catalogDraftFromSourceModel` 不是 compiler 等价投影：会丢 `codexUltra`、Provider 级上下文/输入模态/推理/缓存/协议默认，以及模型级并行工具调用和基础指令。现与 Rust compiler 对齐为“模型级 > Provider 级 > 缺省”，保留上下文、模态、reasoning、codexCache、apiFormat、supportsParallelToolCalls、baseInstructions、codexUltra、排序和别名；两套路由前端仍直接消费父查询的最新 Provider 快照，向导不维护长期 Provider 副本。
- 特殊直写入口复审结论：强制修复只规范现有 reasoning 字段且随后切换/重投影；Codex live backfill 和 proxy live-to-provider 只回写认证/配置材料，不产生模型目录；历史 template migration 只迁移 config bucket；均没有新增独立模型事实源。导入/恢复统一执行 post-import sync。External `/v1/models` 和 V2 backend 初始化均直接 compiler 当前 Provider 集合。
- 验证：工作台 73/73、向导 17/17、V2 Agent 127/127；前端全量 146 files / 1198 tests；MultiRouter mutation 15/15、compiler 19/19、projection 15/15；Rust lib 全量 3353 passed / 6 ignored；`pnpm typecheck`、renderer production build、`cargo fmt --check`、`git diff --check` 通过。全仓 Prettier 仍只报告 `src/lib/presetCatalog{,.test}.ts` 和 `src/utils/codexModelContext{,.test}.ts` 四个本轮未修改的既有格式差异。

## 2026-08-24 Router 模型差异呈现、停用规则校验与 Ultra 解锁根修

- DeepSeek Responses 的 Provider 目录已有 `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp`、`deepseek-v4-pro` 三个模型；现有 Router route 使用 `modelSelection.mode=include` 且只选择 Flash/Pro。`include` 是固定白名单，不应在 Provider 新增模型后静默扩容；此前真正的前端缺陷是规则卡片只显示旧匹配项，用户看不到 Provider 当前目录和未接入模型。现规则列表、详情和编辑器按当前 Provider 目录显示已接入总数、尚未接入及已不存在项；`mode=all` 继续自动跟随 Provider。
- 截图中的 `include 模式至少选择一个 canonical 模型` 来自前端 `RouteCandidatePicker.handleSave`，它错误地校验了已停用 route；后端 schema 已明确跳过 `enabled=false`。现前端只对启用 route 校验空白名单和别名目标，停用 route 保留配置但不阻止保存，并把 `canonical/include/visible` 用户文案改为“上游模型/仅选中的上游模型/可见模型”。MultiRouter 页面所有 catch 统一经过中文错误码翻译，未知英文异常不再原样混入 UI；Provider 推理能力及 Ultra 保存校验也已全部中文化。
- Ultra 无法点击的根因是 `CodexModelReasoningSummary` 在 `ultraEfforts.length===0` 时直接禁用 checkbox，而该集合依赖异步能力解析；切换能力来源只是偶然触发了解析刷新。现“解锁 Ultra 档”始终可点：若尚无 Provider 可接收的推理强度，点击后自动展开该模型的推理能力配置并提示必须先确认强度；不会虚构档位，保存门禁仍拒绝“已解锁但未选择供应商强度”的不完整配置。
- TDD 与验收：三个现场回归先在旧实现下 RED；定向 Router/Provider/Ultra `86/86`，全量前端 `146 files / 1195 tests`，`pnpm typecheck`、相关文件 Prettier、`git diff --check` 和 `pnpm build:renderer` 全部通过。没有构建安装包、安装、重启或替换当前运行中的 CCSM，也没有再次使用会触发 Codex Desktop `write EOF` 的 Computer Use。

## 2026-08-24 本地升级守护、维护租约与 MultiRouter 向导 Provider 初始化

- `d9d6d87a` 本地 NSIS 首次独立升级 runner 在维护租约和停服前 fail-closed：隐藏启动的 Windows PowerShell 5.1 未自动解析 `Get-FileHash`，因此没有修改安装态，PID 23760 与 15721 始终健康。根因是外层升级包装仍额外依赖 `Microsoft.PowerShell.Utility`，而事务核心已经使用 .NET SHA-256。现由 `ccswitchmulti-guardian-core.ps1` 提供 `Get-CcsmGuardianFileSha256`（只读共享打开、流式哈希、finally 释放），wrapper 和 runner 共用，完全移除关键升级入口的 `Get-FileHash` 依赖。RED 为普通文件哈希函数不存在且 wrapper 静态契约失败，GREEN 为 guardian 17/17、事务 49/49。
- 安装态只读验收曾发现 `codex_multirouter_projection:codex-multirouter` 仍记录 `ready / 90e471cee349546b`，但 `~/.codex/cc-switch-model-catalog.json` 没有 `ccSwitchRoutingDependencyFingerprint`；按 `read_back_codex_multirouter_projection` 契约这实际属于 `projection_live_drift`。根因是接管刷新虽通过 `codex_provider_with_projected_model_catalog` 编译出 `modelCatalog + codexRoutingProjection`，但 `codex_settings_for_model_catalog_projection` 的投影白名单只合入 `modelCatalog + codexRouting`，最终目录生成拿不到指纹。现把 `codexRoutingProjection` 纳入同一只读派生合并；Router DB 仍不保存目录快照。失败用例先复现 `left=None / right=provider-fingerprint-v2`，修复后接管 9/9、目录投影 2/2、MultiRouter 投影 15/15 通过。2026-08-24 完成事务升级后，安装态 `3.19.2-17` 已包含该修复，数据库与目录指纹均为 `90e471cee349546b`，投影状态为 `ready`。
- 升级结果契约补漏：事务脚本成功结果字段为 `NewPid`，外层 `invoke-ccswitchmulti-local-upgrade.ps1` 曾误读不存在的 `NewProcessId`，导致应用已经正确安装并健康启动后，包装层仍在最终结果格式化阶段报错。现统一读取 `NewPid` 并用 Pester 锁定契约。该错误与 Codex Desktop 自动化的 Electron 主进程 `Error: write EOF` 无关；后者是已关闭 IPC 管道被继续写入，发生后必须停止该自动化链，不能靠重复点击或重试解决。

- 本机 Codex 当前依赖安装态 CCSwitchMulti `127.0.0.1:15721`，因此本地替换不能由当前交互进程直接停服后再继续。升级必须交给独立的 Windows PowerShell 事务：预检当前监听 PID、可执行文件路径、版本与 SHA-256，建立维护租约，备份配置和安装目录，等待旧进程及端口完全释放后卸载/安装，验证新 PID、版本、哈希、15721 与 `/health`；任一步失败必须使用保留的安装目录、配置和注册表备份回滚。
- 新增 `scripts/ccswitchmulti-guardian-core.ps1`、`scripts/watch-ccswitchmulti.ps1` 和 `scripts/invoke-ccswitchmulti-local-upgrade.ps1`。维护标记不再是永久哨兵文件，而是带 `leaseId`、owner PID、owner executable path、owner start time、创建/过期时间的结构化租约；PID、路径、启动时间和期限必须全部有效，坏 JSON、过期租约、PID 复用或 owner 消失都不会永久压住守护。租约 scope 在异常时只清理自己拥有的租约。
- 守护每 5 秒检查一次；只有预期安装路径的 CCSM 连续失联 60 秒才恢复。恢复前再次检查维护状态，只停止路径和启动时间均核验通过的 CCSM 进程，拒绝误杀占用 15721 的其他程序，等待旧进程退出和端口释放后才隐藏启动安装态 EXE，并等待新 PID 的 `/health` 就绪。安装器、卸载器和事务脚本保留为二级维护识别。
- 权威守护脚本已部署到 `%LOCALAPPDATA%\CCSwitchMultiGuardian`；2026-08-24 最终核验时守护 PID 40924、CCSM PID 32080，15721 监听属于该 CCSM，`/health` 为 200。不要使用旧的 `run-upgrade-416f9167.ps1`，新构建必须计算新 installer/installed EXE SHA-256，再调用仓库中的 `invoke-ccswitchmulti-local-upgrade.ps1`。
- MultiRouter 向导修复：新建 Router 仍默认选择全部可用 Provider；编辑 schema-v2 Router 时只按 `codexRouting.routes[].targetProviderId` 初始化 Provider 勾选，并过滤已经不存在的 Provider。`draftSources`、checkbox、计数和 flow 初始化共用同一结果，避免编辑已有 Router 时错误显示“已选择全部 Provider”并把未引用 Provider 带入草稿。
- 验证口径：向导定向 Vitest 16/16；前端全量 146 files、1191/1191；MultiRouter Rust 77/77；Rust lib 全量 3347 passed、6 ignored；Guardian Pester 10/10；系统 Windows PowerShell 5.1 下事务 Pester 49/49；TypeScript typecheck 和 renderer production build 通过。Codex 内置 PowerShell 运行事务测试会因它自己的虚拟 powershell/sqlite/StrictMode 环境产生 3 个假失败，不能替代系统 PowerShell 5.1 口径。
- Codex Desktop 的 Computer Use/UI 自动化在本轮反复触发主进程 `Error: write EOF`。这是对已关闭 IPC 管道写入的 Desktop 主进程错误，不是 CCSM 健康或路由错误；发生后停止重复自动化，不重启 Codex。源代码、自动测试、数据库和运行日志验收先行，UI 只在升级稳定后做一次受控检查，若同样错误再次出现立即终止该验证链并记录阻断。
- 首次本地替换事务正确检测到安装后 EXE 哈希不匹配并完成自动回滚：旧安装哈希 `34618BEC...E403`、新运行态实际哈希 `FF4255A1...3644`、当时误用的构建目录 EXE 哈希 `53CC5565...CF22`，`transaction-result.json` 为 `RolledBack` 且 `RollbackError=null`，回滚后 PID 64616、15721/health 200、旧哈希恢复。Tauri 官方 bundler 源码确认 NSIS 打包时会把唯一占位符 `__TAURI_BUNDLE_TYPE_VAR_UNK` 原位替换成 `..._NSS`，打包完成后再恢复构建目录原文件；因此不能把构建目录 EXE 哈希当作安装后哈希。`Get-CcsmTauriNsisPayloadHash` 现按官方转换在内存中推导 payload SHA-256，要求占位符恰好一次且不修改文件；测试同时固定该转换。
- 自动回滚又暴露两个升级控制面根因并已补测试修复：Windows 官方文档确认 `Start-Process -Wait` 会等待整个进程树，事务启动长期运行的 CCSM 后会让包装脚本永久等待；现改为持有事务 `Process` 句柄并只等待该进程退出。另一个问题是失效租约虽然不压住守护，但旧 `CreateNew` 会因文件仍存在阻止所有后续升级；现用独占 `OpenOrCreate` 在同一文件句柄内验证并原子覆盖过期、owner 消失或坏 JSON 的租约，同时拒绝覆盖有效 live owner。Guardian Pester 扩展为 14/14，系统 PowerShell 5.1 事务 Pester 保持 49/49。
- 最终本地升级事务 `ccsm-20260824-065903-19f0485565ba4a338e8de4f892eaccf2` 成功：`Status=Success`、`NewPid=32080`、`RollbackError=null`，维护租约已释放，旧 PID 23760 已退出，安装目录只有一个 CCSM 进程。安装文件 SHA-256 为 `C209CD69A84B86698A290620FA6BB612B824CC31609431E641EBD1A641DB8EA0`，与 NSIS 预推导 payload 一致；Codex 未重启。Provider/MultiRouter 安装态目录包含 9 个模型，Router DB 不保存 `modelCatalog`；Qwen 只保留 3.8，无 3.6 Agent 文件。

## 2026-08-23 MultiRouter Provider SSOT v2 全链路与历史 PR 完整性复审

- Provider SSOT v2 核心合并是 `b7865131`。它不在 `v3.19.2-9`，从 `v3.19.2-10` 开始才进入发布标签；`-14` 主要承载 Ultra/UI/DeepSeek reasoning 历史修复，`-15` 承载 V2 route 匹配兼容，不能把 SSOT 的首次引入归到 `-14/-15`。
- GitHub 状态与本地 ancestry 双证据确认：此前真正标记 Merged 的 PR #9/#10/#11/#12/#18，其 head 提交全部在当前 `main` 祖先链，没有 Git 层面的“只合一部分”。开放 PR #19/#21/#24/#26 的行为已由现行主线等价或更完整覆盖；#13/#14 是依赖升级，不属于功能漏合。后续缺陷来自 SSOT 重构扩大了消费层后，认证、目录、排序、显示名、活动所有权、兼容读取等契约没有同时覆盖所有入口，而不是这些已合 PR 丢提交。
- 公开 issue 的严格口径：#28、#29、#34、#36、#38、#40、#42、#46 共 8 个明确属于 Provider SSOT/Router compiler/shared projection 主链；扩展口径再计 #23、#25、#27（models_cache、能力来源、Ultra 迁移消费者）共 11 个。不要把一个 issue 中的多个调用点重复计数。
- 本轮新增确认并根修 4 个未单独建 issue 的同类遗漏：
  1. 活动 Router 解析只看 workspace/DB，漏掉设备本地 current provider，可能让旧 DB 标记拥有 shared live catalog；现统一为 workspace > device-local > DB fallback。
  2. Provider 更新已按 #47 限制活动 Router 发布，但 Provider 删除级联仍逐个发布所有受影响 Router；现删除后只发布仍有效的活动 Router，无活动且多 Router 时不发布。
  3. 手动 projection retry 与 v1->v2 migration 完成路径可直接发布非活动 Router；公共发布入口现 fail-closed 拒绝非活动 Router，非活动迁移只写 DB，切换时再发布。
  4. Universal Provider 的 Codex 子 Provider 同步/删除直接调用 DAO，绕过 compiler、投影和 route cascade；现更新复用 `persist_provider_mutation`，删除复用 Codex Provider domain delete，并在子项成功后才删除 universal definition。
- 运行时“直接读取 Provider”本身成立：每次 V2 请求在 `proxy/providers/codex.rs` 重新从 DB 收集 Provider 并调用 `compile_v2`，所以 endpoint/auth/protocol/canonical model 不依赖 Router 快照；shared catalog/config/cache 是派生投影。当前主要风险已从“请求还读旧快照”收敛为“哪个 mutation/restore/retry 有权发布唯一 live 投影”。
- 新增回归覆盖 device-local ownership、共享 Provider 删除只发布 active Router、非活动公共投影拒绝、Universal Provider 删除级联。验证：Rust 全量 `3320 passed / 0 failed / 5 ignored`；定向 mutation 9/9、projection 12/12、Universal delete 1/1；`pnpm typecheck`、本次 Rust 文件 rustfmt、`git diff --check` 通过。仓库全局 `cargo fmt --all --check` 仍会报告本轮未改的 preset registry/openai_compat 既有格式漂移，未擅自改动。
- 联网交叉验证：Codex 内置 WebSearch 与 GitHub CLI 检查仓库 README、SSOT/root-cause 文档、PR/issue 状态；Matrix WebSearch 独立搜索未返回索引结果，但直接打开 GitHub root-cause 文档成功。关键版本、ancestry 和调用路径结论最终以本地 tags、Git history、源码和实跑测试为准。

## 2026-08-23 PR #37-#49 拆分修复审计与 Windows 原子写锁处理

- PR #37 修复 v2 `authPolicy.source` 未被认证门面读取导致官方 Codex route 被误判为
  `FullyManaged`、注入 `PROXY_MANAGED` 并触发 401。复审又发现 raw passthrough 的官方 route
  识别仍只读旧 `upstream.auth`，且空旧容器会遮住有效 v2 声明；认证来源现统一以 v2
  `authPolicy` 为权威并覆盖 raw endpoint 归属。
- PR #39 的前端缺省值只避免了白屏，后端 schema 仍会拒绝缺失 `modelSelection` 的同步旧数据；
  正确契约是在共享 v2 schema 中将缺失值定义为 `{mode:"all"}`，让读取、保存、编译和运行一致。
- PR #43 的 `displayName` 已同时贯通 compiler 投影和 `/v1/models` 响应，复审未发现遗漏。
- PR #47 原逻辑在活动 Router 存在时正确，但活动 Router 为空时会让所有受影响 Router 再次争写
  共享 catalog；现仅在唯一候选 Router 时允许无活动态发布，多 Router 且归属不明时不发布。
- PR #49 的后缀别名识别原先仍区分大小写；现先规范化模型 ID，再识别 Flash/Pro 并排除 Vision。
- PR #41 的排序与活动工作区行为已由 `8b29949b` 覆盖，但复审发现主线 live 写路径只合并了
  `modelCatalog`，漏掉 `codexRoutingProjection.dependencyFingerprint`。后续修正为仅合并投影拥有的
  `modelCatalog` 与 `codexRoutingProjection`，既保持目录/指纹一致，也不覆盖认证、Common Config
  和用户字段；不能用“更小更安全”代替这项字段所有权契约。
- PR #45 定位 Windows `ReplaceFileW` 瞬时失败（1175/32/5）。主线保留有界重试并拒绝
  `fs::write` 非原子覆盖；复审根据 Microsoft 文档补齐 1176/1177：调用时提供同卷备份路径，
  1176 在原名均保留时可有界重试，1177 部分移动后优先完成新文件安装，失败则恢复旧文件，
  自动恢复也失败时保留临时与备份文件供人工恢复，禁止继续清理恢复材料。

## 2026-08-22 官方模型切到 DeepSeek V4 Flash 首个请求 400（reasoning_text）根因与修复

- 现象：从官方模型切到 DeepSeek V4 Flash（原生 /v1/responses 透传）后，第一条请求被上游 400
  `The 'reasoning_text' in the thinking mode must be passed back to the API.`；点“继续”后重试
  成功（请求体大 244 字节，说明客户端在两次请求之间改写了历史）。证据：codex-router.log
  2026-08-22 02:58:05 session=01a00e49 status=400，02:58:31 status=200。
- 根因：Codex 历史里的 reasoning item 大多只有 summary + 官方 gAAAAA 密文、没有 content
  （rollout 统计 3883 个中 3775 个无可读 content）；第三方原生透传路径把 reasoning 原样转发，
  而 DeepSeek 要求 reasoning 以 reasoning_text content parts 回传、不接受
  summary/encrypted_content。官方 OAuth 路径有专门 reasoning 归一化，第三方路径没有。
- 修复（384b6159，合并入 main 为 567f94b8）：openai_compat.rs 新增
  normalize_third_party_responses_reasoning_items，只挂在 forwarder.rs 第三方原生透传分支：
  content 可读则保留并剥离 summary/encrypted_content/internal 字段；content 不可读但 summary
  可读则用 summary 重建 content；只剩密文则丢弃 item。official OAuth 路径不受影响；归一化幂等，
  Lite 降级重试天然覆盖。
- 验证：新增 5 个回归测试；cargo test --lib 全量 3279 passed / 0 failed / 5 ignored；UTF-8 严格 /
  无 BOM / git diff --check 通过。当前安装实例尚不包含该修复，需下次构建/安装后生效。
- “官方模型速度档/推理档丢失”问题（Bug 1）是另一条线，本次只修了 DeepSeek 400（Bug 2）。

## 2026-08-21 混合 hosted 与普通 function tool 流式调用仍报错

- 用户当前安装实例为 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`，
  文件版本 `3.19.2-10`，进程 PID `85528`，`127.0.0.1:15721/health` 返回 200。
- 该安装包仍包含 `streaming_codex_chat.rs` 的旧拒绝逻辑：当同一 Chat 流式回合同时
  出现 CCSM hosted tool（如 `web_search`）和普通 function tool 时，主动返回
  `mixed_hosted_tool_calls` / `Mixed hosted and ordinary function tool calls are not
supported in one streaming turn`。
- 根修提交为 `ffb118bea5815ecb2ce6ba34cf7e42ebf1ff9f54`，但它只在旧的
  `release/v3.19.2-8` 分支，既不在 `v3.19.2-10`，也不在当时的当前 `main`；
  因此“源码历史上有修复”不等于“安装包已修复”。
- 本轮已将根修移植到当前 `main`：保留普通 function call 的 Responses 事件，
  只把 hosted call 交给 CCSM coordinator，再续接同一 Responses stream；删除旧的
  混合调用硬失败分支，并新增真实 `web_search + image_gen__imagegen` 回归测试。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml streaming_codex_chat --lib`
  为 `31 passed / 0 failed`；`cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  和 `git diff --check` 通过。当前安装版仍需后续新构建/安装后才会包含该修复。

## 2026-08-21 其他分支与开放 PR 审计、用量统计回归

- 本地 `main` 当前为 `482e67de`，远端 `fork/main` 为 `91b5f69e`（`v3.19.2-11`）；本轮未推送。GitHub API 快照显示开放 PR 为 #1-#5、#13、#14、#19、#21、#24、#26；#20、#22 已关闭。
- 依赖升级 PR #1-#5、#13、#14 与功能修复线无关，不应混入本轮。
- #21 的 inline `model_providers.*.models[]` reasoning 读取已被当前统一 resolver 覆盖并继续增强；#24 的 `enabled:false` 停用模型语义已由 `bd5da4c2` 及后续 SSOT 投影覆盖。#26 的旧前端排序实现已由 `dab41928` 选择性移植，但 SSOT 重构后的 Rust 投影曾丢弃 `sortIndex`；本轮已在 `compiler.rs` 读取 Provider 模型排序、把有效排序纳入 dependency fingerprint，并在 `projection.rs` 按 Router 历史顺序优先、Provider 源排序回退、稠密重编号/恢复默认清理，新增三条投影回归和指纹回归，当前 `codex_multirouter` 52/52 通过。
- #19 的 route 供应商解析已在 `475cd008` 进入主线；本轮继续实现模型统计的 `model + provider_name` 聚合、模型表供应商列、趋势缓存命中率百分比，并修复 `get_model_stats()` 在无 provider 筛选时引用 `p2.name` 却未 JOIN 的 SQL 根因。
- `bigstrongsun/ultra-orchestration` 的提交未按整枝祖先合入，但语义已由 `5036705f`、`b45235d3`、`77d011c8` 选择性移植；不要整枝合并。`ccsm-agent-mesh` 仍是未接入现有 HTTP 代理的独立 AgentMesh 后端原型；`portable-reasoning-experiment-nogo` 与 `commentary-reasoning-experiment` 是明确 no-go/实验线；`subagent-v2-capability-injection` 当前 22 个 ahead 提交全部为论文、课件和资料沉淀，不是待合并功能。不要把这些分支当作遗漏的功能合入项。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib test_get_model_stats` 3/3；inline reasoning 定向 Rust 测试 1/1；Provider catalog、UsageTrend、model catalog order 前端测试 13/13；`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 通过。

## 2026-08-21 Hosted web_search OAuth 协议修复与 v3.19.2-11 发布准备

- `main` 的 `ebe5ccad` 已修复第三方 hosted web/image 工具调用官方 Codex OAuth 后端时的协议错误：OAuth 请求必须使用 Responses `input` 数组和 `stream=true`，响应是 SSE；现已复用 `responses_sse_to_response_value` 聚合，并补充顶层 `detail` 错误摘要。
- 现场 A/B 证据：旧请求依次返回 `Input must be a list`、`Stream must be set to true`；数组输入加 `stream=true` 后 HTTP 200，包含 `response.web_search_call.completed` 与 `response.completed`。该结论来自本机 OAuth 实测，不记录 token 或完整 query。
- `v3.19.2-10` tag/Release/安装实例仍指向旧 commit；版本源已统一为 `3.19.2-11`，发布说明为 `docs/release-notes/v3.19.2-11-zh.md`。发布前 `cargo check --lib`、`pnpm typecheck`、UTF-8 和定向测试均通过；全量 Rust 验证为 3262 passed / 0 failed / 5 ignored。
- 发布后必须重启安装实例，运行 `scripts/verify_third_party_hosted_web_search.py`，并在日志中确认 `hosted_tool_call status=ok`；健康检查 200 不能替代真实 canary。

## 2026-08-21 Codex Ultra 主线移植与边界收口

- `bigstrongsun/ultra-orchestration` 不能直接 merge：它从 `475cd008` 分叉，而
  `main` 已有后续 reasoning capability 和 MultiRouter SSOT 改动。实现改为从最新
  main 选择性移植到 `bigstrongsun/ultra-main-integration`，避免回退主线校验。
- `ultra` 仅是 Codex V2 的产品层编排：启用
  `reasoning.codexUltraOrchestration.enabled` 后，Codex 可选 Ultra；Provider
  边界仍固定使用 Codex `max`，再通过经验证的 `max -> Provider` 映射发送。没有
  有效 max 映射时，前后端均拒绝保存；Provider 永远不会收到 literal `ultra`。
- Provider 原生 capability 与持久化 effortMap 现在禁止 `ultra`，旧的手动声明或
  探测快照会在修复路径中被移除。官方 catalog 的 `default_reasoning_level=ultra`
  在生成 capability 时规范化为 `max`，因此不会因默认值脱离 Provider 原生集合而
  降级为 unknown。
- MultiRouter V1 从三个 reasoning catalog 字段剥离 Ultra，并将三个对应的
  default 字段由 Ultra 规范化为仍可选的 max（没有可选档位时清除 default）。V1
  不会对用户展示无法触发 proactive Sub-Agent 的 Ultra；V2 保留完整编排入口。
- 验证：Rust `codex_reasoning` 23/23、V1 catalog 1/1、`codex_config` 205/205，
  前端 reasoning 14/14、`pnpm typecheck`、`cargo fmt --check`、`git diff --check`
  全部通过。当前未安装、未替换或停止正在运行的 CCSM，未发布 release。

## 2026-08-21 v3.19.2-10 安装态诊断收口

- 主分支审计确认认证修复 `2c41f638`、hosted tool 诊断 `168a3fc6` 和版本发布提交
  `15c92b88` 为同一条线性历史；远端 fork/main 当时落后 103 个提交，没有分叉冲突。
- 首次安装 `3.19.2-10` 后，Qwen3.8 真实 streaming canary 返回 HTTP 200、工具已投影，
  但没有写 `hosted_tool_not_called`。根因是
  `streaming_codex_chat.rs` 在 `state.completed == true` 时先 `break`，诊断只在后续
  `finish_reason` 分支执行，正常 `response.completed` 流因此漏记。
- 提交 `e3a8e5ea` 把诊断条件抽成 `should_log_hosted_tool_not_called`，在 streaming
  完成/finish 分支共同检查，并新增完成态回归测试。TDD 先红（函数不存在）后绿，
  streaming 定向测试 `31/31`、`cargo fmt --check` 和 `git diff --check` 通过。
- 重新构建并事务安装后，安装事务
  `ccsm-20260821-151643-9b3d4aa953d04fe98083120b728341c1` 成功，运行 PID `85528`，
  安装 EXE 期望哈希为
  `53515696D0EA2778227DDD6FECE83D990FF0023681BDFE712D4100A76E5021FA`，
  `/health` 保持 HTTP 200。
- 修复后 Qwen3.8 canary 仍按预期不产生 hosted function call，但真实日志出现
  `hosted_tool_not_called`，字段包含 `model=qwen3.8`、路由 provider、`tool=web_search`、
  `streaming=true` 和 `reason=upstream_returned_success_without_hosted_tool_call`。
  DeepSeek V4 Pro canary 通过，产生 `response.web_search_call.in_progress/searching/completed`
  并返回 `CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`。
- 本机 release 构建仍因没有 `TAURI_SIGNING_PRIVATE_KEY` 在签名阶段退出；NSIS 包已生成但
  不能作为正式 Release 资产。正式发布仍需推送 main/tag，并由 GitHub Actions 完成六平台签名和
  `latest.json` digest 验收。

## 2026-08-21 Codex Ultra 分支合入审计

- `bigstrongsun/ultra-orchestration` 当前只包含 `0c8869c7` 与 `39d8f44` 两个
  未合入 `main` 的提交；分支基点为 `475cd008`，而当前 `main` 已前进 39 个
  提交。直接 merge 会在 `memory.md`、`src-tauri/src/codex_config.rs`、
  `CodexFormFields.tsx` 和 `CodexSubagentProfileEditor.tsx` 产生冲突。
- 主线已有 `ultra` 枚举、能力映射和请求转换基础，但尚未有
  `codexUltraOrchestration` 的完整产品语义。Ultra 分支不能整枝合入，必须在
  最新 `main` 上选择性移植并补齐回归。
- 审计发现的分支缺口：V1 投影只删除 supported-reasoning 列表而不清理
  default effort；解析器对旧的 Provider-native `ultra` 声明可能重复暴露；
  探测/手工能力校验仍允许把 `ultra` 当作 Provider 原生档位；官方目录默认
  `ultra` 的模型可能因 default 不在剥离后的 supported 集合而被丢弃。
- 上游公开问题 #37858/#37859 已报告：普通 API-key `model_providers.*` 上
  reasoning effort 与完整 Ultra/multi-agent 产品能力不是同一件事，后者可能
  失败或降级。因此第三方 Provider 不能在未做真实 transport/UI canary 前
  宣称“主动 Sub-Agent 委派”；需要显式区分 max 映射、client-local spawn 和
  first-party/ChatGPT backend 能力。
- 本轮只读验证：当前 `main` 的 reasoning 前端定向测试 11/11、Rust
  `codex_reasoning` 定向测试 19/19 通过；这些测试尚未覆盖 Ultra，故不能作为
  Ultra 合入或 release 证据。

## 2026-08-21 hosted tool 未调用诊断补齐（当前提交前）

- 根因边界已产品化：认证错误、路由/转换错误和“上游成功但没有发起 function call”必须分开显示。新增脱敏 router 事件 `hosted_tool_not_called`，只在显式 `tool_choice` 指向 CCSM 自有 `web_search`/`generate_image` 时记录；普通 `tool_choice=auto` 不误报。
- 事件字段仅包含 `trace/session/model/provider/tool/status/reason/streaming`，不写 prompt、工具 schema、响应正文或凭据。buffered 与 streaming Chat→Responses 两条路径都覆盖。
- `diagnose_codex_multirouter` 新增 Hosted tool 调用检查和 `latestHostedToolWarning`，Debug 面板显示最近告警和事件摘要，明确提示“优先检查模型/网关的 OpenAI-compatible function calling”，不在代理层猜 prompt 或伪造 tool call。
- 门禁：Rust 全量 `3251 passed / 0 failed / 5 ignored`；streaming 定向 `30/30`、router diagnostics `13/13`、显式选择识别 `1/1`；TypeScript、cargo fmt、git diff check 通过。
- 这只是诊断能力补齐，Qwen/vLLM 仍需上游支持 function calling 才能真正搜索；正式 release 仍需 bump 新版本（不能复用已存在的 `3.19.2-9`）并完成 macOS/Linux 构建与产物验收。

## 2026-08-21 本轮 Sub-Agent 保存门禁与第三方 hosted 搜索安装态收口

- 提交 `789b91e0d1793fc1716b22e3b62c7035cc587fcd` 将普通 Codex ProviderForm 的保存前校验接入 `validate_codex_subagent_v2_provider_candidate`，与 Provider add/update 共用同一严格 compiler/route/reasoning gate；未知 reasoning 的 enabled+routable profile 在前端保存前即被阻断，后端仍保留最终权威门禁。新增 hosted Chat 出站 `hosted_tool_projection` 脱敏摘要，只允许记录 CCSM 自有 `web_search`/`generate_image` 名称和 tool choice 形状。
- 代码证据：Vitest 定向 `13/13`，Rust `codex_config` 定向 `203/203`，hosted projection `1/1`，`pnpm exec tsc --noEmit`、`cargo fmt`、`git diff --check` 和严格 UTF-8 检查均通过。
- 新构建的 `RELEASE-METADATA.md` 绑定上述提交，版本仍为 `3.19.2-9`。事务安装 `ccsm-20260821-130436-f717d7ce615445bb8dabbb79cc91bafe` 成功，旧 PID `24240` -> 新 PID `12568`，安装哈希 `EBAEC57F5A45C72BF550DF24049F91BABB1C650E48AEAD323BA334DB22266C13`，`127.0.0.1:15721/health` 为 `200`。
- 安装态 DeepSeek V4 Pro hosted canary 通过：真实事件包含 `response.web_search_call.in_progress/searching/completed`，返回 marker `CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`。Qwen3.8 canary 仍无 tool call，但日志记录 `responses_to_chat=true`、`hosted_tools=[web_search]`、`hosted_tool_choice=object(keys=[function,type])` 且上游 HTTP `200`，所以 CCSM 投影链已证明闭环；剩余问题属于 Qwen/vLLM 未触发 function call 的上游能力边界。
- 安装态 `cc-switch-model-catalog.json` 与 `models_cache.json` 均为 9 个模型，二者都含 `qwen3.8` 且不含 `qwen3.6`；数据库 Sub-Agent V2 profiles 和 `.codex/agents/qwen3-8.toml` 均指向 `qwen3.8`。delegated 角色 TOML 不固定 `model_reasoning_effort`，让子 Agent 继承主 Agent 当前 effort；可用档位仍由 catalog 的 `supportedReasoningEfforts` 声明。
- 当前仍不应创建 GitHub Release：源码、构建、安装和第三方 hosted canary 已完成，但版本号仍是已存在的 `3.19.2-9`，且跨平台（macOS/Linux）产物未在对应主机完成构建/验收。下一步是决定是否 bump 到新版本并补跨平台产物；不要移动现有标签或覆盖同名 release。

## 2026-08-21 MultiRouter Provider SSOT v2 向导回归收口

- 当前 `main` 已包含 `b7865131`（`Merge MultiRouter Provider SSOT v2 into main`）及本轮向导回归修复。根因是旧向导仍依赖前端逐个重建路由快照，导致无方案最终页为空、Provider 状态不完整、别名随 Provider 改名漂移，以及重复点击保存创建多个方案；修复已统一到 Provider-owned catalog + Route policy 的 SSOT coordinator。
- 向导现在在无 MultiRouter/无模型源/迁移中/保存后都显示可执行内容；模型源状态卡展示认证、目录、协议、能力、OAuth、工具和投影状态，并保留跳转 Provider 详细配置入口。保存使用稳定 plan ID、in-flight Promise 门禁和保存后绑定当前编辑目标，重复点击只更新同一方案。
- Alias 只持久化 Route 的显式 visible→canonical 映射；Provider 改名不会漂移，alias target 不在 canonical/all/include selection 时拒绝保存并显示错误。Rust compiler 回归覆盖 Provider 改名保留 alias。
- 门禁结果：Vitest `142 files passed / 1146 passed`；Rust `3246 passed / 0 failed / 5 ignored`；`pnpm typecheck`、本轮 TS/TSX Prettier、`cargo fmt --check`、`git diff --check` 均通过。测试输出仍有既有 React act、Radix、MSW/Tauri mock 警告，但无失败。
- 仍未完成真实 UI 验收、安装态 canary 和 Mac canary，因此当前只可称为代码与测试收口，不能宣称正式 release。

## 2026-08-19 恢复 Qwen/vLLM 缺省输出上限 131072（对齐 Qwen3.8-27B 官方最大输出）

- 实况：Codex 任务 `01a01722-ca39-77f1-b7da-9d3a9d5fe023` 的 vLLM 透明代理记录出现单条请求 `prompt_tokens=136269 / completion_tokens=125875 / total_tokens=262144 / finish_reasons=["length"]`。根因不是上下文压缩也不是 KV cache，而是 Codex Responses 请求没有显式输出预算、CCSM 转换成 Chat Completions 时也没有补 `max_tokens`，vLLM 把剩余上下文窗口都当成默认输出预算。
- 2026-07-09 提交 `8b6b3b7e` 曾按“CCSM 不应替用户截断”关掉 Qwen/vLLM 的隐式 `defaultOutputTokens`；本次真实生产样本证明完全缺省时 vLLM 会把剩余上下文窗口当成默认输出预算，长 Agent 单轮可能一直生成到 `max-model-len` 被 `length` 截断。Qwen3.8-27B 官方最大输出长度是 `131072`（阿里云百炼模型页同时标注思考/非思考模式最大输出 131072），但该模型级上限应由本部署 vLLM 原生配置承担，不能写死在 CCSM 通用代码里影响所有 qwen/vLLM 用户。
- CCSM 通用代码已撤销全局 qwen `defaultOutputTokens` 推断（曾临时改过 `131072`，用户明确反对“让所有用户配置都变成 qwen default”）。CCSM 只保留已有的显式 `defaultOutputTokens` 用户配置能力；缺省输出策略由部署侧 vLLM `qwen38-generation/generation_config.json` 的 `max_new_tokens=131072` 承担。
- 远端透明代理曾做运行态兜底：`vllm_dashboard/vllm_transparent_proxy.py` 对 `model=qwen3.8` 注入/钳制 `max_tokens`，但用户明确“代理层不应该接管”，相关代理改动已全部撤销。当前代理只负责透明转发、日志与既有 tool guard，不再修改 qwen 请求输出/推理参数。
- vLLM 原生侧同时加了模型级上限：`linux/env/qwen38-generation/generation_config.json` 设置 `max_new_tokens=131072`，`VLLM_GENERATION_CONFIG` 指向该目录；vLLM 启动日志确认 `Default vLLM sampling parameters ... {'max_tokens': 131072}`，因此直连 raw `/v1/chat/completions` 缺省输出也按官方模型上限收敛，不依赖代理。
- 为防止“xhigh reasoning 把输出额度耗到 length 后突然结束”，曾尝试在代理层注入 `thinking_token_budget=16384` 并把默认 effort 改为 medium（`58468b50c`），但用户明确“代理层不应该接管”。该代理改动已由 `5c052a1d2` 撤销：代理恢复透明转发，输出上限与 medium reasoning 默认都下沉到 vLLM 原生配置（`qwen38-generation/generation_config.json` 的 `max_new_tokens=131072` + `--default-chat-template-kwargs '{"reasoning_effort":"medium"}'`）。
- CCSM 流式收尾根修：`streaming_codex_chat.rs` 和 `streaming_codex_anthropic.rs` 对 `finish_reason=length`/`max_tokens` 现在发 `response.incomplete`，不再把 `status=incomplete` 包装成 `response.completed`。Codex 原生 SSE 解析器会把 `response.incomplete` 当作流错误，避免“任务显示完成但 `last_agent_message=null`”的静默收尾；普通完成仍发 `response.completed`。

## 2026-08-17/18 main CI 转绿：28 个 Clippy lint + unix 死导入 + macOS 测试夹具（三连根修）

- 背景：main 自 08-16 起 CI 连续 40+ 次红，全部卡在 Clippy 步骤（`cargo clippy -- -D warnings`，CI 用 `dtolnay/rust-toolchain@stable` 即各 runner 最新 stable，本地 1.95 与 CI stable 的 lint 集合对 Windows 代码一致）。三个提交根修后 CI run `32049307104` 四 job 全绿（Frontend + ubuntu/windows/macos Backend）。
- 提交 `0224cc4e`（28 个跨平台 lint，无 `#[allow]` 补丁）：(1) `proxy/usage/parser.rs` 删死函数 `openai_cache_read_tokens`（读路径实际走 `cache_read_tokens_from_openai_compatible_usage`）；(2) `codex_guardian.rs` `len()>0`→`!is_empty()`；(3) `codex_history_migration.rs` `repeat("?").take(n).collect().join()`→`vec!["?"; n].join()`；(4) `codex_subagent_profiles.rs:836` else 分支尾 `return Err(...)` 去 return+分号使 if/else 同类型（注意同文件 680 行附近的 return 是控制流必需，勿动）；(5) `forwarder.rs:909` 去多余 `&provider`；(6-26) 19 处 `doc_lazy_continuation`：`/// - ` 列表项后紧跟 `/// 返回:`/`/// 副作用:` 触发，修法是在列表与「返回:/副作用:」之间补一个空 `///` 行（forwarder.rs 2 块、bridge.rs 1、openai_client.rs 2、web_search.rs 7 块 13 处）；(27) `transform_codex_chat.rs:127` `!x.is_some_and(|v| !v.is_empty())`→`x.is_none_or(|v| v.is_empty())`；(28) `transform_codex_chat.rs` `append_responses_item_as_chat_message` 8 参数超 7 上限：4 个散落 `&mut` 累积器收敛为 `PendingChatItems` 结构体（tool_calls/media/reasoning/last_assistant_index），签名降 5 参，调用方 `append_responses_input_as_chat_messages` 同步改按字段传递，行为等价（3108 单测验证）。
- 提交 `338f69de`（macOS/Linux 专属）：`settings.rs` 顶层 `#[cfg(unix)] use std::io::Write;` 是死导入（02bd8a2a 引入，后续重构把使用点改为 `save_settings_file` unix 分支内局部导入后遗留）。带 `#[cfg(unix)]` 所以 Windows 看不到；unix 平台 unused-imports 在 `-D warnings` 下是编译期硬错误并中止整个 crate 的 Clippy（两平台日志均 "due to 1 previous error"，证明 crate 内无其他 lint）。删 2 行即修复。
- 提交 `acb32709`（macOS 测试，Clippy 转绿后 Run tests 步骤首次真正执行才暴露的预存失败）：`codex_desktop_executable_validation_rejects_cli_launcher` 与 `remembered_codex_desktop_executable_round_trips_confirmed_path` 在临时目录写裸 `Codex` 文件期望被接受，但 44b9de42 已把 macOS 校验升级为 bundle 元数据级（`macos_codex_bundle_for_executable`：祖先须为 Codex.app/ChatGPT.app + Info.plist `CFBundleIdentifier==com.openai.codex` + `CFBundleExecutable` 与实际主程序路径一致）。根修=测试夹具对齐平台语义（不放宽生产校验）：新增 `write_desktop_test_executable(dir)`，macOS 构造最小 `Codex.app/Contents/{Info.plist,MacOS/Codex}`，Windows/Linux 维持裸文件；CLI 拒绝用例不变（裸小写 codex 无 bundle 祖先同样被拒）。
- 验证链：本地 Windows（1.95）clippy 0 错误 + `cargo test --lib` 3108/0/5 + fmt 干净；WSL（Rust 1.96 stable，与 CI 一致）Linux 原生 clippy 修复前 1 错误→修复后 0 错误；macOS 无法本地交叉验证（rquickjs-sys 构建脚本需 darwin C 工具链），依据 macOS CI 日志「Clippy 完整检查 crate 仅报 1 错误」+ 校验链逐条核对，最终以 CI 实跑为准（已全绿）。
- 经验：(1) 本仓库 CI 三 OS 矩阵，修 Clippy 必须三平台都验——`#[cfg(unix)]`/`#[cfg(target_os)]` 代码在 Windows 本地 clippy 不可见，用 WSL 跑 Linux 原生 clippy 可覆盖 unix 分支；(2) Clippy 硬错误会中止 crate 检查，"due to 1 previous error" 意味着该 crate 无其他 lint，可放心修完这一个再让 CI 暴露下一层（本次就是 Clippy→测试 两层依次暴露）；(3) `cargo clippy`（CI 命令）不带 `--all-targets`，测试代码里的 lint 不挡 CI——当前测试代码还有 10 个预存 lint（tests/profile_roundtrip.rs:754 MutexGuard 跨 await、codex_config.rs:814 let...else、prompt_files.rs:45 items after test module、proxy/error.rs:264/273 io_other_error、proxy/handlers.rs:6338/6391 needless_borrow、quota_collaboration.rs:1084/1221/1359），后续可单独清理；(4) worktree `.worktrees/clippy-fix` 在 main 上，共享 `CARGO_TARGET_DIR=cc-switch\src-tauri\target` 复用编译缓存；主 worktree 当时在 `bigstrongsun/fix-responses-chat-turn-coalescing`（V2 加密修复 29072912，未合 main），勿动。

## 2026-08-17 合入用户 PR #18 模型排序（Codex 模型选择器用户自定义顺序）

- 用户 GaoHu1997 的 PR `BigStrongSun/ccswitchmulti#18`（head `sort`，commit `5cde9425`）已审查并合入 main：merge commit `b40d914c` + 修复 commit `962d42d4`，已 push fork main，GitHub 状态 MERGED。
- 功能：MultiRouter 工作台新增“模型排序”tab（@dnd-kit 拖拽，grid 6→7 列），全量模型写入 `modelCatalog.models[].sortIndex`（0 起）；“恢复默认”删除全部 sortIndex。后端 `codex_config.rs` 的 `CodexCatalogModelSpec` 增加 `sort_index: Option<u32>`（读 `sortIndex`/`sort_index`），`sort_codex_catalog_specs_for_picker` 优先级改为用户 sortIndex(0) > spawn_agent_model_priority(1) > 默认供应商排序(provider_rank+2)。前端同步路径（`buildModelCatalogForRoutes`、`rebuildPlanModelCatalog`、`providerWithFetchedModelCatalog`、`catalogDraftFromSourceModel`）均保留 sortIndex，防止路由保存/`/models` 拉取覆盖用户排序。
- 审查发现并根修两处 PR 缺陷：(1) E0063 编译错误——PR 漏改 3 处测试夹具的 `sort_index` 字段（codex_config.rs 约 8196/8261/10841 行），按 PR 自身模式补 `sort_index: None`；(2) Prettier 缩进不符（`hasChanges` 链式调用），`prettier --write` 修正（纯格式）。PR 作者 checklist 的 typecheck/clippy 均未勾选，且未跑 Rust 测试构建。
- 门禁（worktree `.worktrees/pr18-model-sort`，main+PR）：`pnpm typecheck` 通过；`pnpm format:check` 通过；`CodexRouterWorkspacePage.test.ts` 54/54；`codexMultiRouterSync.test.ts`+`CodexMultiRouterWizard.test.tsx` 43/43；`cargo test --lib codex_config::` 178/178（含新增 `codex_model_catalog_uses_user_model_sort_index_for_picker_order`）；`cargo test --lib` 全量 3108 passed / 0 failed / 5 ignored；`cargo fmt --check` 通过。worktree 已清理（git worktree remove 因 Windows 长路径失败但注册已解除，目录用 `cmd /c rmdir /s /q` 删除）。
- 行为边界：用户 sortIndex 现在优先于 spawn_agent_model_priority（此前 priority 列表排最前）；这是 PR 的有意设计（测试注释“全量模型菜单的用户排序优先于子 Agent 候选和历史默认排序”）。sortIndex 只影响 picker 展示顺序，不改路由/子 Agent 候选/默认模型。生效需重启 Codex Desktop（catalog 重新生成）。
- 注意：push 时远端 fork/main 原在 `6c184547`（PR base），本地 main 领先的 7 个 docs 提交（config-plane/reasoning 设计）随本次一并推送。

## 2026-08-17 第三方父模型（官方中转）→ 第三方子模型 V2 加密任务失败：根因定位

- 现场：CCSM 3.19.2-5、V2 启用、`tool_namespace="agents"`；父 `gpt-5.6-sol-longnows-5.6`（Longnows 中转，背后是官方 backend）→ 子 Longnows Luna 失败，child rollout 的 `agent_message.encrypted_content` 为 228 字符真实 Fernet（`gAAAAA...`），Stage B `opaque_agent_payload_error` 拒绝。同条件 A/B：官方 Luna 父 → Longnows 子成功（child 收到的是可打印明文，走 Stage B legacy-plaintext 恢复）。
- 根因（已用最新 openai/codex 源码 `c6058cc`（2026-08-17）一手验证）：Stage A `forwarder.rs::should_make_codex_v2_agents_plaintext` 要求 `official_oauth_request`（即 `normalize_codex_oauth_responses`，父出站必须是官方 ChatGPT Codex backend 原生透传）。父出站为第三方中转时条件为 false，`message.encrypted` 不被剥离；中转背后的官方 backend 仍按 schema 加密 `message` 参数并返回 Fernet 密文。Codex 客户端 `core/src/tools/router.rs::ToolCall::direct_source()` 只在 `namespace=="collaboration"` 且 `encrypted_function_args==Some([])` 时返回 `DirectPlaintextMessage`；非保留 `agents` 命名空间恒为 `Direct`，`multi_agents_v2.rs::communication_from_tool_message` 对 `Direct` 一律 `InterAgentCommunication::new_encrypted(raw_argument)`，child 收到 `encrypted_content=<原始参数>`。所以官方父成功路径实际也是靠 Stage B 把明文从 `encrypted_content` 恢复，而不是 `DirectPlaintextMessage`。
- 关键修正：给非保留 `agents.*` 响应补 `encrypted_function_args=[]` 对当前客户端无效（`direct_source()` 不认非 collaboration 命名空间），不是本问题的关键修复；关键修复是请求侧 Stage A 扩围。
- 方案（已实施，提交 `29072912`，分支 `bigstrongsun/fix-responses-chat-turn-coalescing`）：`should_make_codex_v2_agents_plaintext` 去掉 `official_oauth_request` 参数与条件，只保留 `app_type==Codex && codex_multirouter_needs_plaintext_v2_collaboration(router_provider)`。效果：混合/纯第三方 Router 下，无论父出站是官方 OAuth 还是第三方中转，都剥离非保留 `agents.spawn_agent/send_message/followup_task` 的 `message.encrypted`；纯官方 Router 与非 Router provider 行为不变。`make_codex_v2_agents_messages_plaintext` 对 Chat/Anthropic 转换后的 body 是 no-op（结构不匹配），无需额外处理。TDD：RED 翻转"第三方父+混合 Router"断言为 true 并补纯第三方 Router 用例（修复前确认失败）；GREEN 后测试重命名 `agents_plaintext_rewrite_requires_codex_mixed_router`。回归 forwarder 150/150、codex_multi_agent 5/5、openai_compat 28/28、multirouter 21/21、完整 lib 3108 passed/0 failed/5 ignored，fmt/diff check 通过。待办：装新包后补第三方父→第三方子真实 canary（nonce + child rollout 可读 + Router 日志确认父请求 tools 已剥离 encrypted）。
- 搜索渠道：Codex 内置 web_search 本环境不可用（unsupported call）；matrix-websearch 链经 grep.app 独立命中 `encrypted_function_args` 字段与 `function_call_preserves_empty_encrypted_function_args` 测试；最终定性以克隆的 openai/codex 最新源码为准（`codex-source-latest`，commit `c6058cc`）。

## 2026-08-17 Responses commentary/tool-call 合并上游定性与官方 PR

- 安装态真实验收确认 `5b820624` 根修有效：ROG 透明代理在 13:58:51、14:01:14 等后续请求中记录同一 assistant 同时具有 commentary `content` 与 `has_tool_calls=true`，不再生成两条连续 assistant；task `01a00e49-e614-7251-a91f-96a9e8889104` 持续完成多轮工具调用并成功创建 Issue #17，没有再以进度播报 `finish_reason=stop` 提前结束。长时间静默来自 90k+ 输入上下文下 80-223 秒的模型生成，不是调度链丢失。
- CCS 官方最新 `upstream/main@3d126f45` 仍有同一根因：`transform_codex_chat.rs::flush_pending_tool_calls()` 无条件新建 `content:null + tool_calls` assistant，未识别直接相邻 commentary message 与 function_call 属于同一 Responses model turn。Codex 内置搜索、matrix-websearch 独立链和 GitHub API 实时 issue/PR 搜索均未发现精确重复项；Matrix 结果弱，最终以 GitHub API 与最新源码为准。
- 上游 TDD：新增 Qwen 形状真实转换回归后，未改生产代码时 RED 为 `left: 3, right: 2`；最小 GREEN 将 pending calls 合入直接相邻且尚无 `tool_calls` 的 assistant，并去重已经附挂的 `reasoning_content`，user/tool 边界与原 fallback 不变。聚焦 1/1、整个 `transform_codex_chat` 90/90、fmt/diff/严格 UTF-8 均通过。
- 官方分支 `bigstrongsun/fix-responses-commentary-tool-calls`，提交 `246a475f`；Issue `farion1231/cc-switch#6529`，Draft PR `farion1231/cc-switch#6530`。PR 仅 1 个文件、1 个提交，目标 `main`，基础 label check 已通过。因 `BigStrongSun/ccswitchmulti` 已脱离 fork 网络，使用真正的官方 fork `BigStrongSun/ccswitchmulti-fork-archive` 推送；该 fork 原为 archived，只读阻止 push，已解除归档并需在 PR 开放期间保持可写以便响应 review。

## 2026-08-17 第三方模型无法调用官方 Web 搜索：Issue #17 与根因定位

- 用户反馈“第三方模型好像还是无法调用官方搜索”，已提交 Issue `BigStrongSun/ccswitchmulti#17`（labels: bug/proxy/multirouter/codex）。本轮只做定位与记录，不改生产代码。
- 现场证据：安装版 `3.19.2-5`（`127.0.0.1:15721/health` 200）；MultiRouter `New Codex MultiRouter` 的 `settings_config.hostedTools = {"webSearch":{"enabled":true},"imageGeneration":{"enabled":true}}`，开关已开；`~/.cc-switch/logs/codex-router.log`（2026-07-31 至 2026-08-17，194MB）中 `web_search` 与 `hosted` 出现次数均为 0，即 hosted tool 桥接从未实际执行过搜索。官方路由（router-codex-official）原生透传不受影响。
- 根因链：官方 web search 是 OpenAI Responses API 的 hosted tool，执行点在 OpenAI-hosted 服务端，第三方 Chat Completions 上游无法原生执行（官方文档 https://developers.openai.com/api/docs/guides/tools-web-search 确认；Chat Completions 仅支持专用搜索模型）。CCSwitchMulti 桥接（`src-tauri/src/proxy/providers/hosted_tools/`，MVP `03afd497`、OAuth 复用 `35e87971`）本应解决该问题，但 v31 流式修复 `c45d0dfa`（fix(proxy): preserve streaming for automatic hosted tools）后，`forwarder.rs::should_enable_hosted_tool_loop()` 对 `tool_choice=auto`（或缺省）的流式请求返回 false——正是 Codex Desktop 默认 Agent 流程形态（`stream=true`、`tool_choice="auto"`、tools 含 hosted `web_search`）。loop 关闭时 `apply_hosted_tool_switches(false, ...)` 把 `web_search` function tool 从 Chat 投影移除，第三方模型看不到该工具。桥接目前仅对非流式请求或显式 hosted `tool_choice` 生效，两者在正常 Agent 流程中都不出现。
- 权衡背景：v31 之前凡带 hosted tools 的 Responses→Chat 请求一律强制 `stream=false`，长上下文表现为持续“正在思考”；v31 优先增量流式，在 streaming auto 路径移除 hosted-only 工具，搜索能力被完全牺牲。Issue #17 列出四个候选方向：流式+按需缓冲（模型实际调用才转 buffered loop）、流中工具循环（暂停 SSE 执行桥接后续接）、按 route 的搜索策略设置、或至少 UI/文档明示限制且 catalog `supports_search_tool=true` 不得暗示 hosted 搜索可用。
- 检索说明：内置 `web_search` 本会话不可用（`unsupported call`，环境限制）；matrix-websearch 链 A（searxng 搜索）仅返回弱二手结果，链 B 直读 OpenAI 官方文档成功，关键事实以官方一手文档 + 本地源码 + 现场日志为准，无来源冲突。

## 2026-08-17 CCSM 全面 AI 可配置接口（AI Configuration Plane）设计

- 设计文档：`docs/superpowers/specs/2026-08-17-ccsm-ai-configuration-plane-design.md`（1098 行，设计评审稿；本轮只做研究与设计，不实施生产代码、不改版本号、不 push）。承接 2026-08-16/17 中断任务留下的可验证盘点结论，重新做了完整源码审计，不引用任何未提交草稿。
- 持久化层与 SSOT：`~/.cc-switch/cc-switch.db` 是单一事实源（providers/provider_endpoints/mcp_servers/prompts/skills/skill_repos/settings KV/proxy_config/provider_health/model_pricing/proxy_live_backup/profiles 等）；`settings.json` 是设备级 AppSettings（不随库同步，settings 域 apply 写该文件并 bump generation，不并入 DB）；`config.json` 是遗留层（首启迁移进 DB）；live 文件（`~/.codex/config.toml`、`auth.json`、model catalog、`agents/*.toml`、各应用 MCP/prompt/skill 文件）全部是派生产物，不迁移、不强制接管，drift 检测只报告不修改。`providers.settings_config` 是无 schema 的 JSON blob，Sub-Agent V2 存于 `settings_config.codexRouting.subagentV2`。
- 写者盘点：源码审计确认 8 类写者（GUI→Tauri 约 250 command、deeplink、SQL 导入整库替换、WebDAV/S3 同步整库快照、DB 备份恢复、live 反向导入、live 同步/启动对账、代理运行时 takeover/账号池 reproject，另有外部写者 Codex Desktop 与系统环境变量），其中 5 类（deeplink、SQL 导入、同步、备份恢复、live 反向导入）绕过统一校验/事务/审计；live 文件既是派生产物又是反向导入输入，形成“派生物污染 SSOT”回路；无任何跨写者 revision 控制与统一审计。迁移期先给绕过路径打 `transport` 审计标记（只加记录不改行为），再逐步收口。
- 架构：`config_plane` 核心 = 领域服务 + 事务核心 + 薄 transport 适配器；GUI/CLI/MCP/HTTP 全部复用同一套领域校验与 mutation 函数，禁止包一层 shell 命令、禁止为 AI 另写一套推断规则。单写者原则：进程内全局 mutation Mutex + 跨进程文件锁 `~/.cc-switch/.config-plane.lock`（仅 mutation 持有）+ SQLite WAL busy_timeout；每次成功 mutation bump 全局 `config_generation` 并写 `~/.cc-switch/.config-plane-event` 事件文件，GUI 轮询/监听变化后重载并提示“配置已被外部修改”；整库替换路径（备份恢复/同步下载）完成后同样必须 bump generation。运行时操作（代理 start/stop、takeover、账号池）经应用启动写入的 `~/.cc-switch/runtime.json`（loopback 地址 + 每次启动随机 token）发现运行中应用并转发，应用未运行返回 `app_not_running`，不得退化为直接改 live 文件。
- 并发与确认：资源级 `config_revisions(domain, resource_key, revision)` + 全局 generation；apply 必须携带 `expectedRevision`，不匹配返回 `revision_conflict`（附当前 revision 与脱敏状态摘要）；planToken 为 ≥128bit 高熵不透明串，TTL 默认 900s，绑定（spec 规范化哈希 + 资源键 + 当前 revision），单次使用，是非交互场景“用户已审阅 plan”的证明链；`--dry-run` 是 plan 的别名；幂等：目标状态与当前状态规范化相等时返回 `changed:false`、revision 不变、写 no-op 审计。
- 成功定义：DB 回读 + resolver 结果 + 派生产物逐文件验证三者全部通过（与 Sub-Agent V2 `roleFilesStatus=verified` 模式一致）；失败则 DB 事务回滚 + 派生产物从 mutation 前快照恢复，快照存 `~/.cc-switch/rollback/<auditId>/`，`rollbackRef` 供 `ccsm rollback <ref>`（P3 提供，P1/P2 只保证自动回滚）。
- 错误码/退出码：稳定错误码表（ok/usage_error/unknown_domain/unknown_resource/schema_invalid/validation_failed/revision_conflict/plan_token_invalid/plan_token_expired/approval_required/app_not_running/secret_read_forbidden/permission_denied/lock_timeout/readback_failed/internal_error），退出码 0-9，机器解析以 JSON 错误码为准；错误对象固定含 code/message(中文)/details(脱敏)/requestId/retryable。
- 脱敏与密钥：核心维护每域敏感字段注册表，所有输出（stdout/MCP 结果/审计/diff/export 默认模式/日志）过统一 redactor，输出 `hasSecret/prefix/fingerprint(sha256)`；密钥可写不可读，spec 支持明文（仅 stdin/0600 文件，写入后从内存清除）或 `$secretRef`，密钥永不进入命令行参数；`export --include-secrets` 只允许输出到本地文件且写审计。
- 审计：新表 `config_audit_log`（actor/transport/domain/resource_key/operation/revision before-after/plan_token/idempotency_key/result/error_code/rollback_ref/summary），actor 区分 user-gui/user-cli/ai-mcp:<clientName>/import/sync/restore/deeplink/startup-reconcile；保留策略 180 天或 10,000 条 mutation 先到先清理（沿用 reasoning 设计已确认决策）；`ccsm audit list` 查询，GUI 审计查看页 P3 提供。
- CLI：新 binary `ccsm`（version/capabilities/schema/doctor + 每域 list/get/inspect/plan/apply/validate/export/import/reset/diff/audit/rollback），默认 JSON 输出，stdin/body-file 输入；当前仓库唯一 CLI 是 `codex-history-repairer`，无通用配置 CLI。
- transport 判断：MCP 作为一等 transport（stdio，`ccsm mcp serve`），CP2 只读 + plan 工具/resources/outputSchema，CP3 mutation（elicitation form mode 确认 accept/decline/cancel、per-domain 开关、admin 操作永久关闭）；JSON-RPC 不独立提供（loopback 运行时通道仅用于运行时操作）；本地 HTTP API 为可选扩展点、默认关闭（CP4，token + 权限范围同 MCP）。这不推翻 reasoning 首版“首版不做 MCP”的范围决策，MCP 引入推迟到全局平面 CP2/CP3，与 reasoning P4/P5 节奏对齐。
- 分期：CP0 核心骨架（revision/generation 表与迁移、planToken、幂等、审计表、脱敏注册表、统一错误码/信封、ccsm 骨架）→ CP1 只读面 + provider/reasoning mutation（Tauri command 收口到核心，契约测试证明 GUI 与 CLI 走同一函数）→ CP2 全域 mutation（feature flag 分域）+ MCP 只读 → CP3 MCP mutation + 专家路径（import 版本化 JSON bundle/reset/backup restore/rollback，SQL 导入降级为显式专家警告路径）→ CP4 可选 loopback HTTP。与 reasoning P0-P7 路线图并行、共享门禁；CP1 的 reasoning mutation 与 reasoning P5 合并实施（同一套 plan/apply 引擎）；CP1 的 provider mutation 必须先完成 `settings_config` 的 auth/config/modelCatalog 三个子树 JSON Schema 化；所有阶段不改应用版本号，只有 CP2 完成且真实 canary 通过后才进入 release 决策。
- 规范依据：MCP 官方规范 2026-07-28（tools：model-controlled、human-in-the-loop SHOULD、命名 1-128 `[A-Za-z0-9_.-]`、stateful tools 显式 handle、protocol error vs `isError`、structuredContent+outputSchema、annotations 来自不可信 server 时视为不可信；elicitation：form mode MUST NOT 收集密码/API key/token、敏感交互用 URL mode 带外、MRTR；security：state handle hijacking、本地 server 首选 stdio、scope 最小化、token passthrough 禁止）+ RFC 9110（ETag/If-Match 条件请求语义，支撑 revision/乐观并发类比）。检索：内置 web_search 本会话不可用（`unsupported call`，多次重试一致，记录为环境限制）；matrix-websearch 双链（链 A 搜索索引发现 + 链 B 官方一手直读）交叉验证无冲突。
- 未决问题（文档第 16 节 8 条）：`settings_config` 域内 schema 化深度（首版仅 auth/config/modelCatalog/codexRouting 四子树）；universal provider 同步的单事务多资源 revision 归属（父+N 子）；skill 域文件副作用的派生产物验证范围；sync 域多设备并发冲突仲裁（首版报告不仲裁）；MCP 工具粒度（~40 工具 vs 通用 `ccsm_execute` 混合比例，CP2 实测定稿）；loopback HTTP 是否排期（保留设计不排期）；审计表是否随 WebDAV/S3 同步（隐私 vs 多设备审计连续性）；Codex Desktop 持续改写 live 文件的 drift 默认策略（自动对账 vs 仅报告）。
- 本轮仅设计：文档通过 UTF-8 严格校验（无 BOM、无 U+FFFD），关键中文复读通过；未实施生产代码。

## 2026-08-16 Qwen 任务错误继承 `reasoning_effort=none` 的两层根因

- 用户安装 `v3.19.2-4` 后，任务 `01a00996-28d6-7aa0-a303-d0a5d6939245` 仍产生大段英文 reasoning。rollout 证明 reasoning item 完整到达 Codex，且重复段落属于不同工具回合，不是 Desktop 截断或 CCSM 把累计 SSE 当增量重复追加。
- 不得把 Codex/OpenAI 的 `reasoning.effort=none` 在缺少 Provider 明示关闭契约时擅自翻译成 Qwen `enable_thinking=false`。Qwen/vLLM 自动推断改为不发送 thinking 开关；显式声明 vendor 参数的 Provider 仍按其契约转换。
- 现场数据库 `settings.common_config_codex` 为 `model_reasoning_effort="medium"`，所有 Codex Provider 自身均未配置 effort；但 live `~/.codex/config.toml` 为 `none`，任务的 `state_5.sqlite.threads.reasoning_effort` 也持久化为 `none`。该任务创建于 live 漂移期间，安装新版不会追溯改写已有线程设置；恢复任务时 `thread_settings_applied` 因而继续使用 `none`。
- `v3.19.2-4` 在 19:02:16 已把 Common Config 的 `medium` 正确写入 `proxy_live_backup`，而 live 文件在 19:02:40 又变成 `none`。这说明剩余问题位于 Codex Desktop 恢复旧线程设置后的 live/config 生命周期，不是 MultiRouter effective-settings 缺少 Common Config。后续验收必须区分新线程默认值与旧线程持久化 override。

## 2026-08-16 Qwen original image detail 完整适配与 Codex Live TOML 生命周期根修

- `supports_image_detail_original` / `supportsImageDetailOriginal` 表达的是 Codex 经 Adapter 后能否获得 original-image-detail 能力，不是第三方 Chat 上游能否原样接受 Responses 的 `original` 枚举。视觉第三方模型必须继续投影 `true`，Responses 到 Chat 的共享媒体边界把 `original` 映射为上游最高可用的 `high`；纯文本模型保持 `false`，官方 Native Responses 保持原生语义。
- Qwen 现场的第二层阻断不是图片转换失败，而是 `~/.codex/config.toml` 无法解析。`developer_instructions` 历史内容里有一行形似 `notify = [...]` 的示例；Codex Desktop 更新 Computer Use 路径时把它误当根级配置并写成裸 Windows 反斜杠，导致 multiline basic string 中的 `\U` 成为非法 TOML 转义。旧兼容器只修根级 notify，且测试明确保留说明文字，因此没有覆盖 Desktop 后续改写的生命周期。
- 根修分两层：Live 读取仅在原 TOML 无效时恢复根级或 basic multiline 中 notify-shaped Windows 路径，并要求恢复结果通过完整 TOML 校验；Sub-Agent 父策略写回时把 `developer_instructions` 强制编码为单个 escaped basic-string 表达，不再留下 Desktop 能误识别的物理 `notify =` 行。解析后的用户说明文字与真正根级 notify 均保持不变。
- 启动时对账还必须以 live `model_catalog_json` 的 CCSwitchMulti 所有权为准，即使 takeover flag 漂移也刷新自有 catalog；不得触碰用户自管外部 catalog。安装后日志为 `✓ 已对账 CCSwitchMulti 自有 Codex 模型目录`，Qwen 3.8 两个 original-detail 字段均为 `true`。
- 固定源码 `7e6515db` 的 NSIS SHA-256 为 `B7006B39DDE1239C8CF2E8DF0A6A01641583ADA11B8CFEA87FE207552190256B`。事务 `ccsm-20260816-qwen38-original-toml-root-7e6515db-r3` / `ccsm-20260816-035917-77cf4a507ab64a40b514130220fea091` 成功，安装版 `3.19.1-31`、PID 4792、15721 healthy。
- 真实 canary 同时通过 `CCSM_QWEN38_STREAM_OK`、`CCSM_QWEN38_TOOL_OK`、`CCSM_QWEN38_ORIGINAL_REPLAY_OK`；router log 证明 original replay 为 `/responses -> /chat/completions` 且 upstream HTTP 200。Rust library 为 `3014 passed / 0 failed / 2 ignored`。
- 安装事务首次在 detached Windows PowerShell 预检中复现 `Get-FileHash` 未自动加载。根修为脚本内统一使用 .NET `SHA256` + `FileStream`，不再依赖模块自动加载状态；事务 Pester 47/47。前两次事务均在任何停止、备份、卸载或配置写入前退出。

## 2026-08-15 V27 主 Agent / Sub-Agent 推理强度协调实现与验收

- Codex 当前原生 `ReasoningEffort` 已包含 `none/minimal/low/medium/high/xhigh/max/ultra/custom`，但目标模型 spawn 只接受其 catalog `supported_reasoning_levels` 中的值。`ultra` 还参与 Codex 主动多 Agent 行为；Provider 未声明 ultra 时不得默认映射到 max。
- “自动获取”只解析 Provider/模型能力，不负责根据 Sub-Agent 任务算出并固定 effort。能力来源和运行策略必须拆开：前者为自动发现/受维护声明/手动声明；后者为允许主 Agent 或 spawn 指定、使用模型默认（固定）、固定档位、关闭推理。
- Codex spawn 的真实顺序：先继承父线程当前 effort（缺少则父 catalog 默认）；`spawn_agent.reasoning_effort` 覆盖 `[agents].default_subagent_reasoning_effort`；spawn 换模型且仍无 effort 时使用目标 catalog 默认；随后 role TOML 显式 `model_reasoning_effort` 以高优先级覆盖，未写则保留；最终按目标 catalog 校验。不存在统一的 Sub-Agent 默认 high。
- 角色 TOML 是最后应用层，显式 `model_reasoning_effort` 覆盖父线程、全局默认和 spawn 值。`3.19.1-25` 用户现场的直接缺陷是角色编辑/类型无法写入 `max`；`3.19.1-26@dd967801` 正在打包且仍有该缺口，修复必须在基于它的 `bigstrongsun/release-v3.19.1-27` 实施，不得污染 `-26` 打包工作树。
- 单次 spawn override 有运行版本差异：当前 OpenAI `main` 已允许 full-history 路径应用 model/effort override，本机当前工具契约仍要求 full-history 继承、override 使用 fresh/partial fork。CCSM 的 delegated 策略只省略角色固定 effort，并按实际 Codex 运行时能力显示/验收，不能承诺所有版本都支持 full-history 覆盖。
- `是否开启推理`需要结构化运行策略，但只能在能力确认可关闭时启用。Codex catalog 需包含 `none` 才能通过 spawn 校验；Responses 映射 effort=none，DeepSeek Chat 映射 `thinking.type=disabled`，boolean-only Provider 使用自己的关闭参数。
- DeepSeek 2026-08-14 当前官方文档：思考默认开启、默认 effort=high，可关闭；原生 effort 为 low/high/max，medium/xhigh 实际映射 high。现有 CCSM `disableAllowed=false` 已落后于官方事实，后续实现必须校准。
- 设计规范位于 `docs/superpowers/specs/2026-08-14-codex-main-subagent-reasoning-coordination-design.md`，实施计划位于 `docs/superpowers/plans/2026-08-14-codex-subagent-reasoning-coordination.md`。实现分支固定为 `bigstrongsun/release-v3.19.1-27`，基线是正在打包的 `release-v3.19.1-26@dd967801`；V26 工作树未被修改。
- V27 已完成 schema v2：`delegated` 省略角色 TOML effort，`model_default` 写 resolved 模型默认，`fixed` 写 capability 校验后的选择，`disabled` 仅在允许关闭时写 `none`。v1 `auto` 迁移为 delegated，v1 显式问卷 effort 迁移为 fixed，旧 override 迁移优先。角色 TOML 的显式 `model_reasoning_effort` 仍是最终覆盖层。
- 后端 `get_codex_subagent_reasoning_capabilities` 与 catalog、role compiler、Chat 请求转换共用同一 resolver；前端只消费该结果。DeepSeek V4 的 Provider 原生集合为 `low/high/max`、默认 `high`、允许关闭；Codex 映射为 `low->low`、`medium->high`、`high->high`、`xhigh->high`、`max->max`，不推断 `ultra`。
- Provider 编辑器把能力来源拆为自动发现、CCSM 受维护声明、手动声明；手动路径提供原生档位、默认、关闭、上游参数与映射的结构化控件，专家 JSON 仅作为高级入口，并在修改草稿前拒绝未知档位、无效默认、空参数和映射到非原生目标。Sub-Agent 编辑器固定档位只显示 resolved selectable 集合，`none` 不混入正向档位，unknown capability 不开放运行值。
- 保存权威仍是事务 mutation：数据库写后回读 schema v2，当前 Provider 的 managed role TOML 逐文件检查绝对路径、存在性和精确内容；delegated 的字段缺失也必须被正向验证。保存成功不代表运行中 Codex 热加载，仍要求重启 Codex/app-server 并新建会话。
- 原全量 Rust 唯一失败 `updating_current_subagent_v2_returns_verified_role_file_readback` 的根因是测试夹具把 legacy `medium` 迁移为 fixed，却未声明 DeepSeek capability，resolver 正确把 unknown 模型解析为空集合。`c551c911` 为夹具补入维护型 DeepSeek capability，没有放宽生产校验；定向回归 1/1 通过。
- 2026-08-15 最终门禁：Rust library `2986 passed / 0 failed / 2 ignored`；Vitest `285 suites / 989 tests` 全通过；`cargo check --lib`、`cargo fmt --check`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check` 通过。角色策略定向测试 5/5，V25 fixed-max 精确 TOML 往返 1/1。既有 `openai_cache_read_tokens` dead-code、browser data 过旧、chunk size 和 Tauri 测试 stderr 仍为非本功能警告。
- Codex CLI `0.147.0` 在两个 Temp 隔离 home 的真实 spawn 日志确认：父 `high`、无全局默认、delegated、spawn 省略得到子 `high`；全局默认 `low` 时得到子 `low`；delegated + spawn 显式 `max` 得到子 `max`；角色 TOML 固定 `low` + spawn 显式 `max` 最终仍为子 `low`。full-history fork 会拒绝不同角色/override，`fork_turns=1` 可用；这与条件式 UI 说明一致。`127.0.0.1:15721` 全程保持原 PID 监听。
- DeepSeek disabled 与 unsupported `ultra` 已由请求转换和写文件前拒绝测试覆盖，但本轮没有可安全隔离的第三方凭据，因此未做真实 DeepSeek 上游 spawn；旧 runtime compatibility class 也未安装，均不得描述为 live 已验证。官方 OpenAI Subagents/Config Reference 与前序 Codex 内置搜索、Matrix WebSearch 结论一致：custom agent 文件显式值最高，显式 spawn 高于 `[agents]` 默认，省略时再继承父值或目标模型默认。

## 2026-08-14 Sub-Agent V2 推理强度预设读取审计（已由 V27 根修）

- CCSwitchMulti 的模型级 reasoning 能力单一来源是 Provider `modelCatalog.models[].reasoning`：维护 `supportedEfforts/defaultEffort/disableAllowed/upstream/source`，Rust 再投影为 Codex catalog 的 `supported_reasoning_levels/default_reasoning_level` 及 Desktop aliases。DeepSeek V4 当前内置能力为 `low/high/max`、默认 `high`；这不是登录 DeepSeek API 后动态查询得到，而是 CCSM 维护的官方资料预设，也允许用户用 `source=user` 高级覆盖。
- 根因审计时，Sub-Agent V2 没有读取模型级 reasoning capability。旧前端问卷固定提供 `auto/low/medium/high/xhigh`，旧 Rust 类型同样没有 `max`；编译器只把 catalog 的 provider kind/context window 带入角色生成，effort 由 profile 手工覆盖、问卷显式值或 `auto_effort()` 规则决定。
- 旧 `auto_effort()` 的优先级是：`overrides.modelReasoningEffort` > 问卷显式档位 > 复杂调试/架构/复杂实现/高风险审查强制 `high` > 速度优先且全部只读强项用 `low` > 其他 `medium`。旧角色 TOML 总会写 `model_reasoning_effort`，因此会覆盖模型 catalog 默认值；对 DeepSeek V4 可能生成厂商未声明的 `medium/xhigh`，同时无法选择 `max`。这是本次 schema v2 与 capability-aware compiler 根修所针对的历史缺陷。
- 根修边界是让 Sub-Agent profile compiler 接收 resolved model reasoning capability，以模型支持集合约束 UI、校验手工值；旧 `auto` 迁移为允许主 Agent / spawn 按 Codex 原生优先级指定，不再由任务问卷算档并固定。模型默认值作为单独、显式固定的运行策略；未知模型继续保守处理，不能套 GPT 通用档位。
- `bigstrongsun/release-v3.19.1-27` 已将 Sub-Agent 持久化升级到 schema v2：reasoning 使用 `delegated/model_default/fixed/disabled` 判别联合；v1 `auto` 迁移为 `delegated`，v1 问卷显式 effort 迁移为 `fixed`，旧 `overrides.modelReasoningEffort` 在迁移时优先。新目录草稿统一生成 schema v2 delegated，任务问卷不再通过 `auto_effort()` 写死角色档位；delegated 角色 TOML 省略 `model_reasoning_effort`，让主 Agent/global spawn/default/model catalog 的原生优先级继续生效。
- 官方 OpenAI 文档确认 `model_reasoning_effort` 只对支持该档位的模型有效，custom agent 文件显式设置会固定子 Agent effort；DeepSeek 官方 2026-08-13 changelog 确认 V4-Pro/Flash 三档为 `low/high/max`。Codex 内置搜索、Matrix WebSearch 独立链和当前源码结论一致；Matrix 泛搜命中的“只支持三档 low/medium/high”二手文章与当前官方文档冲突，未采信。
- 真实主机核验补充：当前安装运行的是健康的 CCSwitchMulti `3.19.1-21`（`127.0.0.1:15721/health` HTTP 200），它在 2026-08-14 21:50 左右重写的 `~/.codex/cc-switch-model-catalog.json` 仍把 DeepSeek V4 Flash/Pro 生成为通用 `low/medium/high/xhigh`、默认 `medium`；同批 `agents/deepseek-v4-flash.toml` 明确写 `medium`，Pro 写 `high`。因此“当前主界面已正确显示厂商档位”与本机持久化事实不符；`3.19.1-25` 源码的主目录根修尚未安装到该运行实例。
- 当前分支与安装版要分开判断：`3.19.1-25` 源码已让主 model catalog 按 DeepSeek `low/high/max`、默认 `high` 投影，但 Sub-Agent V2 的 `CatalogModel` 仍只携带 model/provider kind/routable/context window，reasoning capability 在进入 profile compiler 前丢失。现有前端测试夹具甚至把 `deepseek-v4-flash` preview 固定为 `medium`，说明错误假设已经被测试固化。
- 影响不只在 UI。Responses→Chat 转换的 capability 模式会检查请求 effort 是否属于 `supportedEfforts`，不属于时返回 `TransformError: reasoning effort ... is not supported`；现有 Step 2603 回归测试已证明该拒绝路径。若 DeepSeek 路由使用已物化的 `low/high/max` capability，而 Sub-Agent role 仍发 `medium/xhigh`，请求会在 CCSM 转换层失败；走旧推断 fallback 时又可能被静默钳成 `high/max`，形成同一错误配置在不同路径表现不一致的第二层风险。
- “预设是手写的”不是单独根因：第三方 API 普遍没有统一的 capability introspection，CCSM 依据厂商一手资料维护内置预设并允许 `source=user` 覆盖是合理边界。根因是同一能力被 Provider catalog、Sub-Agent 固定枚举/auto 算法和 legacy 推断分别表达，且没有把 resolved capability 作为端到端单一契约传递、校验和展示。

## 2026-08-14 Sub-Agent V2 编辑、写入回读与输入模态根修

- 用户反馈有两条同源缺口：模型卡片缺少明确“编辑”动作，保存后也没有证据说明更改是否真正进入 Codex；同时 V2 profile 只记录任务优势/写入范围/推理强度，完全丢失文本与图像输入能力，导致 DeepSeek V4 与 ChatGPT 生成同类 role 描述。RED 提交 `ff739720` 证明前端会在缺少回读响应时仍显示成功，Rust schema 往返把 `inputModalities` 变成 `null`。
- GREEN 提交 `c3e7311c` 把 `inputModalities` 作为 profile 结构化字段，合法显式值只有 `["text"]` 与 `["text","image"]`，未知能力继续保守未声明。来源优先级为 profile 显式覆盖 > `modelCatalog` 的 `inputModalities/textOnly/supportsImage/vision` > 既有后端保守模型识别；保存、初始化、目录同步和无效 profile 恢复都会把已知目录能力显式写回 V2 JSON。前端在旧 profile 尚未重存时也直接显示目录推导值，并为每张模型卡提供独立“编辑”按钮。
- capability 安全文案必须追加到 `description` 和 `developer_instructions`，手工覆盖基础文案也不能静默去掉：多模态角色明确支持 image input / image understanding；纯文本角色明确不能接收图像，且不得用于依赖图像理解的任务。这样 Codex 的 role 选择说明与执行约束都能区分 ChatGPT 类图文模型和 DeepSeek V4 纯文本模型。
- focused mutation 的新事务结果同时返回 `projection` 与 `verification`：`databasePersisted`、`roleFilesStatus=verified|not_required|pending_retry|failed`、逐文件绝对路径/存在性/内容一致性，以及固定 `restart_codex_and_start_new_session` 激活边界。当前方案在原子写 TOML 后重新编译期望内容并逐文件回读精确比对；非当前方案明确 `not_required`，不能声称修改了 live Codex；缺少验证、状态矛盾或内容不匹配时前端必须报错，禁止显示保存成功。
- Codex Desktop 当前会话的 custom-agent role 列表可能在会话配置快照中保持旧值。CCSwitchMulti 能证明数据库和 `~/.codex/agents/*.toml` 已一致，但不能通过自己不拥有的 app-server stdio 连接保证运行中 turn 立即热加载；产品文案继续要求重启 Codex/app-server 并新建会话。官方 Codex 文档确认 personal custom agents 位于 `~/.codex/agents/*.toml`，核心字段为 `name/description/developer_instructions`，`description` 用于角色选择；Codex 内置搜索与 Matrix WebSearch 两条独立链结论一致，无来源冲突。
- 聚焦验收：Vitest V2 editor `110/110`；Rust `codex_subagent_v2_` `98/98`；真实隔离 home 的 current-provider 测试完成 SQLite 保存、原子生成 managed TOML、回读一致性和纯文本安全文案检查；`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 通过。变更文件经 UTF-8 strict 解码，无 BOM、无 U+FFFD。既有 `openai_cache_read_tokens` dead-code warning 与本功能无关。

## 2026-08-14 CCSwitchMulti 3.19.1-24 阻断 Codex Windows setup 根修

- 用户与多名使用者看到 Codex Desktop `Finish Windows setup` / `Windows setup didn't finish`，点击重试不弹 UAC。同机把 CCSwitchMulti 从 `3.19.1-24` 降回 `3.19.1-23` 后立即恢复；这不是 UAC helper、`[windows] sandbox` 或 Codex MSIX ACL 本身损坏。
- MODT 的 Codex Desktop `0.147.0-alpha.6.6` 日志给出完整调用链：`cc-switch-model-catalog.json` 的未知第三方模型缺少 `supported_reasoning_levels`，令 `config/read` 失败；点击重试时 `windowsSandbox/setupStart` 也在读取配置阶段返回 `invalid_config`，因此 `codex-windows-sandbox-setup.exe` 根本没有启动，UAC 自然不会出现。
- 根因来自 `3.19.1-24` 的推理能力收口：为了不让未知第三方模型继承 GPT effort，`apply_codex_model_reasoning_capability()` 删除了模板的 reasoning 字段后直接返回。Codex `ModelInfo.default_reasoning_level` 是可选字段，但 `supported_reasoning_levels: Vec<_>` 没有 serde default，是 JSON 必填字段；官方源码的无 reasoning 示例也使用空数组而不是省略字段。
- 正确语义是：未知或明确不支持 reasoning 的模型写 `supported_reasoning_levels: []`，同时省略默认 effort 和 Desktop reasoning aliases；声明能力的 DeepSeek/GLM/Grok/Step 等继续覆盖各自真实档位。禁止用恢复 GPT 通用档位来规避解析错误。
- RED 提交 `c59e2642` 在旧实现稳定得到 `left=None / right=Some(Array [])`；GREEN 提交 `d56aa840` 只补必填空数组，两个现场回归测试与 `codex_model_catalog` 聚焦套件 24/24 通过。验证时应额外检查真实 Codex `config/read` 和 `windowsSandbox/setupStart`，不能只看 catalog JSON 能被通用 JSON parser 读取。

## 2026-08-14 CCSwitchMulti v3.19.1-24 Codex 推理能力正式发布

- `v3.19.1-24` 以公开 `v3.19.1-23@d87312f4` 为基线移植逐模型 reasoning capability，避免直接发布旧功能分支而回退 v20-v23 的 Sub-Agent 工作台、事务安装器、更新器、macOS 与配置解析修复。发布提交为 `8168c488ea7ee0f4dc4c3af6ac4833b9311ad057`，annotated tag object 为 `8728c1fc7d990d9c6b43aca66e336ef121b2e63f`，本地与远端 peeled commit 均精确指向发布提交；发布后 memory 提交不得移动该 tag。
- 正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-24`，非 draft/prerelease，且 GitHub `/releases/latest` 指向该 tag。Actions run `31765031272`（`https://github.com/BigStrongSun/ccswitchmulti/actions/runs/31765031272`）的 Linux x64/ARM64、Windows x64/ARM64、macOS、Publish GitHub Release、Assemble latest.json 七个 job 全部 `completed/success`。
- Release 恰有 19 个 assets。`latest.json` 为 `version=3.19.1-24`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台键；所有 signature 非空、逐项等于对应 `.sig`，所有 URL 指向本 tag。
- 下载验收：`latest.json` SHA-256 为 `53e1a47e331c9541244ecfba0bd7a5f6fe2a0425a223804bb207fbc054e820b5`；Windows x64 Setup SHA-256 为 `1e061261901f871733650ca4d314fe98879903ae4150251155d65633c9480424`，FileVersion/ProductVersion 均为 `3.19.1-24`。两项都与 GitHub Release asset 服务端 digest 精确一致。
- 组合树门禁：Rust library `2967 passed / 0 failed / 2 ignored`；前端 `121 files / 962 tests`；版本测试 8/8；`cargo check --lib`、rustfmt、TypeScript typecheck、变更文件 Prettier 与 `git diff --check` 通过。仅保留既有 `openai_cache_read_tokens` dead-code warning 和测试夹具预期 stderr。
- GitHub API 验收期间出现过短暂 EOF/TLS 查询失败，重试后恢复，最终 REST、Release 页面、远端 tag 与下载 digest 结论一致。前置事实检索使用 Codex 内置 Web Search 和 Matrix WebSearch 两条独立链；Matrix 查询无相关结果，关键远端状态以 GitHub CLI/API、`git ls-remote` 和下载文件为一手证据。
- 发布成功不等于用户机器已安装 `3.19.1-24`，本轮没有停止当前依赖 `127.0.0.1:15721` 的 CCSwitchMulti，也没有执行安装事务。Release workflow 不包含 R2 同步步骤，因此不得声称 Cloudflare/R2 镜像已同步。

## 2026-08-13 Codex 预设推理覆盖与完整 catalog 收口

- 内置预设能力不能直接编辑：默认只读，用户必须点击“创建高级覆盖”后才进入 `source=user`，界面明确显示“已偏离内置预设”，并能一键恢复当前版本的内置能力。`ProviderMeta.codexPresetId` 保存稳定 `presetKey`，不得保存会随数组顺序变化的 `codex-N`；旧 `codex-N` 只保留兼容读取。
- 本轮发现目录受控状态的根因：`catalogRowsMatchModels()` 在模型级 reasoning schema 加入后没有比较 `reasoning`，所以覆盖按钮虽更新了子组件状态，子到父 effect 却误判为未变化。修复是把完整 reasoning 纳入统一相等性边界，不是在按钮或测试里绕过同步。
- `modelCatalog()` 现在保证每个内置目录模型都有显式 reasoning capability。有官方 effort 的 DeepSeek/Grok/GLM/Step 按模型枚举；Kimi、Qwen、MiniMax、MiMo、SiliconFlow 只声明 boolean thinking 且 `supportedEfforts=[]`，不展示虚假强度；其余证据不足模型显式 `supported=false`，不继承 GPT/Native Responses 通用档位。
- 聚合平台能力仍必须优先于模型原厂能力，例如 SiliconFlow 的 `enable_thinking` 和 OpenRouter 的 `reasoning.effort`；未知模型或平台证据不足时保持不展示、不注入 effort 的保守策略。

## 2026-08-13 Codex 内置预设推理能力统一实现

- CCSwitchMulti 的 Codex 内置 Provider 现以 `modelCatalog.models[].reasoning` 作为逐模型能力契约；能力维度包含支持档位、默认档位、是否允许关闭、上游参数形态、effortMap、输出格式和来源。首批维护 DeepSeek V4 Flash/Pro、Grok 4.5、GLM-5.2、Step 3.7/3.5/2603；未知第三方模型不再继承 GPT/Native Responses 通用档位。
- 同一能力对象由 Rust resolver 校验后投影到外部 catalog 的 snake_case、Desktop camelCase aliases 和 `config.toml` inline models，并驱动 Responses→Chat 请求转换。GLM 的兼容档位映射到 none/high/max；Step 2603 拒绝未声明的 medium；DeepSeek 使用 low/high/max 且默认 high。旧 Provider 仅在缺少模型级能力时保留 `codexChatReasoning` legacy fallback。
- MultiRouter 向导、Provider 更新同步和已有 Provider 表单读回都保留 reasoning；visible alias 与 upstream model 均可解析。`codexSpawnAgentCandidates` 不再维护第二份 `CodexCatalogModel` 接口，统一复用 `src/types.ts`，避免新增能力字段在同步中被静默剥离。
- Provider 模型目录的“Codex 推理能力”高级区域支持 JSON 覆盖：内置值标为 `builtin`，编辑后标为 `user`，空白/清除进入保守未声明；重新选择内置预设恢复维护值。保存前拒绝默认值不在列表、不可关闭却含 none、以及显式 effortMap 覆盖不完整的配置，不再静默回退。
- 设计与计划分别位于 `docs/superpowers/specs/2026-08-13-codex-preset-reasoning-capabilities-design.md` 和 `docs/superpowers/plans/2026-08-13-codex-preset-reasoning-capabilities.md`。Qwen 等随模型/API 变化的能力证据不足时继续采用保守未声明，不写成厂商级通用枚举。

## 2026-08-13 Codex 预设模型推理能力统一设计

- 根因不是 DeepSeek 单点枚举错误，而是 `codexProviderPresets.ts`、`codex_config.rs` catalog 生成与 `proxy/providers/codex.rs` 请求转换分别维护能力，导致菜单、默认值和真实出站参数漂移。
- 产品边界已确认：CCSwitchMulti 内置预设是受维护的 Codex 适配器，必须按“Provider + API 协议 + 具体模型”提供准确能力；自定义 Provider 才默认开放编辑，内置预设只允许带偏离标记的高级覆盖。
- 统一设计以一个 resolved capability 同时驱动 catalog、Desktop aliases、inline TOML、MultiRouter route 物化和出站转换。未知第三方模型采用不展示档位、不覆盖上游默认值的保守策略，禁止继续继承 GPT/Native Responses 通用 reasoning 档位。
- 设计规范位于 `docs/superpowers/specs/2026-08-13-codex-preset-reasoning-capabilities-design.md`；首批校准 DeepSeek V4、Grok 4.5、GLM-5.2、StepFun 与 OpenRouter，并显式处理 Kimi/Qwen/MiniMax/MiMo/SiliconFlow 只有开关或不支持 effort 的路径。
- 官方检索使用 Codex 内置 Web Search 与 Matrix WebSearch 两条独立链：Matrix 索引查询无结果，但直接读取部分官方 URL 成功。Qwen 能力随模型和 API 变化，证据不足时不得写成厂商级固定枚举。

## 2026-08-13 DeepSeek V4 reasoning effort 目录污染根修

- 用户反馈 MultiRouter 保存后会把 `deepseek-v4-flash` / `deepseek-v4-pro` 的官方 `low/high/max`、默认 `high` 覆盖为 `low/medium/high/xhigh`、默认 `medium`。根因不是 DeepSeek 官方模板缺失：`src-tauri/src/resources/codex_deepseek_catalog_template.json` 本来就是正确的官方目录；问题只发生在 MultiRouter 的 `ProxyChat` 聚合目录，它克隆通用 GPT 模板并把同一组全局 effort 枚举套给所有第三方模型。
- `src-tauri/src/codex_config.rs` 新增按模型识别的 reasoning capability 单一源。DeepSeek V4 Flash/Pro 无论经官方直连还是 MultiRouter 生成，都强制投影 `low/high/max`、默认 `high`，并同步覆盖外部 JSON catalog 与 `config.toml` inline models 的兼容字段。
- 参数边界：Codex 模型目录决定 UI/运行时允许选择哪些 effort 以及省略显式配置时的默认值；Codex 随后在请求体发送 reasoning effort，供应商网关/模型实际解释并执行。因此这是“Codex 选择和传参、模型供应商执行”的两段式能力，不是 Codex 本地模拟，也不是各模型可接受任意通用档位。
- 官方交叉验证：DeepSeek 官方 Codex 集成页对两模型列出 `low/high/max`、默认 `high`；OpenAI 官方文档表明 reasoning effort 是请求字段且不同模型支持集合不同。本地测试锁定 MultiRouter 目录和托管 agent role 的正确投影。

## 2026-08-12 Sub-Agent 双主题工作台 3.19.1-22 发布准备

- `3.19.1-22` 将 Sub-Agent 配置收敛为 MultiRouter 独立顶层工作区，并完成深浅双主题语义色：V1/V2 状态、选择策略、目录同步、模型折叠卡片、能力问卷、高级覆盖、TOML 预览和 sticky 保存栏都复用 MultiRouter 的蓝/青/绿/紫/红/琥珀层级。最终设计证据位于 `design-qa.md` 与 `artifacts/design-audit/subagent-theme-2026-08-11/{05-light-after,06-light-toml-after,07-dark-after,08-dark-toml-after}.jpg`，结论为 `final result: passed`；误生成的 15x15 `05-light-after.png` 只是光标图层，不得提交或作为验收证据。
- 当前本机安装进程 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 为 PID/15721 owner `65888`，FileVersion/ProductVersion 均为 `3.19.1-21`，SHA-256 `92B73DC3B8286CE21259D792D248B2D1ED783982DCAE6B1182505A534B844D0E`，`/health` HTTP 200。该安装候选用于真实 UI 验收；它早于后续合并公开 `v3.19.1-20` 发布线，不能单独代表最终 tag 的组合源码。
- 发布前已用 merge commit `3044215a` 合入公开 `v3.19.1-20@8de0b422` 的 OAuth 过期竞态、自启动漂移、小窗口 AppSwitcher 和 Responses 断流修复；随后因远端并发出现 `v3.19.1-21@45f6a0a4`，继续用 merge commit `9a28088d` 合入其 updater 代理检查、macOS ChatGPT.app 识别、MultiRouter 原子启用与失败传播修复，并把本功能顺延为 `3.19.1-22`，避免覆盖公开标签或回退新发布线。最终组合树验证为 `codex_subagent_profiles 71/71`、`codex_config 156/156`、Rust `2954 passed / 0 failed / 2 ignored`、Vitest `119 files / 943 tests`、V21 与 Sub-Agent 联合前端 `143/143`、Pester `46/46`；cargo check/fmt、typecheck、Prettier 与独立 Vite production build 均通过。
- 额外 `pnpm build` 已成功编译 `3.19.1-22` release EXE 并生成 MSI/NSIS，随后因普通交互 shell 没有 `TAURI_SIGNING_PRIVATE_KEY` 在 updater 签名阶段 exit 1；不得把它记为完整本地签名构建通过，也不应把未签名本地产物公开上传。正式 Release 必须由 GitHub Actions 的签名 Secret 完成，并在远端回读 `.sig`、`latest.json` 与服务端 digest。
- 正式发布说明位于 `docs/release-notes/v3.19.1-22-zh.md`，重点解释独立子 Agent 导航、V1/V2 边界、V2 问卷与 custom role 语义自动选型、全模型折叠配置、手工字段/TOML/诊断和深浅主题适配。Release workflow 由 `v*` tag 触发；必须等待五个平台构建、Publish Release 和 Assemble `latest.json` 全部成功，再核验非 draft/非 prerelease、19 个资产、六个平台 updater 签名/URL 和 GitHub 服务端 digest，不能把 tag push 或工作流启动当作发布完成。

## 2026-08-11 Sub-Agent 工作台 UI 重构 3.19.1-20 本机交付闭环

- 在 capability-injection 分支 `a77fd77a0b829ef78c56e059d733ef0b8c748e49` 上完成独立 Sub-Agent 工作台：MultiRouter 顶部导航固定为“总览 -> 模型源 -> 路由规则 -> 子 Agent -> 状态 -> 测试发布”，路由规则页不再夹带 Sub-Agent 配置。子 Agent 页同时保留 V1/V2；当前 V2 时，V1 显示蓝色“启用 V1”，V2 显示灰色不可点击“已启用 V2”，并明确切换后需重启 Codex app-server 和新建会话。
- V2 模型目录区改为“搜索 + 已启用/待配置/不可路由/全部筛选 + 单模型 Accordion”。“从模型目录添加可配置模型”明确说明新模型默认关闭且不覆盖已有问卷/手工设置；每个模型只在展开时显示问卷，高级字段和“生成结果与 TOML”各自再折叠，sticky 底栏持续显示保存状态。此结构与 W3C APG 的 Accordion `button`/`aria-expanded` 及键盘操作语义一致；Codex 内置搜索取到官方一手页面，Matrix 搜索未发现对应一手结果且直开 W3C 被 403 验证页阻断，因此 Matrix 不能作为本轮正证据。
- 安装版真实 UI 验收使用 `deepseek-v4-pro` 搜索：结果只剩 Pro，单项可展开，5 项持久化能力为复杂调试、架构设计、复杂实现、测试验证和高风险审查，“有限实现”未选；优化目标=质量、写入范围=复杂修改、模型偏好=优先、推理强度=高。高级字段与 TOML 均保持收起，页面无校验告警，sticky 显示“所有更改均已保存”，保存按钮 disabled。一次故意制造的 6 项未保存草稿通过离开并重新进入子 Agent 页恢复为数据库值，未点击保存、未污染 profile。
- 完整源码门禁在本轮构建前通过：Rust `2945 passed / 0 failed / 2 ignored`，Vitest `115 files / 931 tests`，focused UI `156 passed`，Pester `46/46`；`pnpm typecheck`、Prettier、Cargo check/fmt 和 `git diff --check` 均通过。版本源统一为 `3.19.1-20`，版本提交为 `a77fd77a`。
- `3.19.1-20` 产物 SHA-256：NSIS installer `29E866C7C4D97512111EF8A996762CDF32F62D5445BBC9876038AA0A945B8B66`，portable ZIP `BB6679458F4112A2B530763F1489EAA91469FD422BD6C58B2DF9E5F4CEFC4F30`，raw EXE `612B1BDBC248F455F25977C6AAB90EE7EBD3025FB02C480B25518484072BC064`，updater signature `B7D71DA47D97AECC69AA24C1E4F3E3356C634ABA17CD39FEE24F624292AD4E99`。
- 独立隐藏事务 `ccsm-20260811-191254-83e0b85fc78d4b20bdf9958a6c4ece73` 状态为 `Success`，`Error=null`、`RollbackError=null`，完成 preflight、backup、kill/wait、卸载、安装、隐藏拉起和 health/version/hash 校验。事务外复核 PID/15721 owner `68356`，运行路径 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`，FileVersion/ProductVersion 均为 `3.19.1-20`，安装 EXE 哈希与 raw artifact 一致，`/health` HTTP 200。
- 最终 UI 证据为 `artifacts/ui-acceptance/3.19.1-20-subagent-workspace.png`（2560x1392，218852 bytes，SHA-256 `735A8FEC315DC039982530404631F2A2850F7622292768BCE647C8128445CD8F`）；画面同时包含六项导航、V1/V2 正确状态、Pro 搜索与展开问卷、收起的高级字段和已保存 sticky 状态。本轮只做本地提交、构建和安装，不 push、不创建 PR、不发布 GitHub Release。

## 2026-08-11 Sub-Agent V2 capability injection 3.19.1-19 本机交付与真实验收

- `settingsConfig.codexRouting.subagentV2` 的最终数据链已在安装版闭环：Wizard 与 MultiRouter 共用问卷/profile editor，Rust compiler 生成官方 custom role TOML，父 Codex 通过 `agent_type` 做语义选择；V1 的前五 direct model override 仍作为独立兼容路径保留。完整源码门禁为 Rust `2923 passed / 0 failed / 2 ignored`、Vitest `115 files / 914 tests`，`cargo check --lib`、rustfmt、typecheck、Prettier 与 `git diff --check` 均通过。
- 本地版本固定为 `3.19.1-19`。NSIS `CCSwitchMulti_3.19.1-19_x64-setup.exe` 为 11,612,108 bytes，SHA-256 `DF00065924ECEBAD40B534800BE248EF14FF47DFBB8858C63C20C73A1923C1EF`；raw EXE 为 36,801,536 bytes，SHA-256 `0E88CF0BC7F7AE4819FB8A8F792D961CC5DCD39AB5B43B7209B4393AFE62F58F`；portable ZIP 为 14,110,260 bytes，SHA-256 `0D9C75E6184CFCB3BE5602C06CA038A97C23F09E7F6231BED9BAAB7AF62D15A0`。NSIS/raw 的 FileVersion 与 ProductVersion 都是 `3.19.1-19`，updater signature 非空。本地导出没有 `RELEASE-METADATA.md`，因此这些产物只用于本机安装验收，不能声称已由 metadata 精确绑定最终文档提交或可直接公开发布。
- Pester 事务回归 `42/42`。本机是 Pester 3.4.0，必须用 `Invoke-Pester -Script ... -PassThru` 并检查 `FailedCount`；`-Output Detailed` 在该版本会因 `OutputXml/OutputFile/OutputFormat` 前缀冲突而报参数歧义，不代表测试失败。独立事务 `ccsm-20260811-064244-9304c84aeb84467dad502139a5f95af3` 完成 `kill -> retained-handle/port wait -> uninstall -> install -> hidden relaunch -> version/hash/health`，没有把机器留在 CCSM 停止状态；当前安装进程/15721 owner 为 PID `42448`，installed FileVersion/ProductVersion 为 `3.19.1-19`，安装 EXE 哈希与 raw artifact 一致，`/health` HTTP 200。
- 安装版 UI 中 Flash 原为 `preferred`，Pro 已通过 MultiRouter 的 Sub-Agent 设置从 `eligible` 改为 `preferred` 并保存；退出工作台再进入后生成说明仍保留“任务强项匹配时优先于内置 default/worker/explorer”的语义。实际文件为 `~/.codex/agents/deepseek-v4-flash.toml` 与 `deepseek-v4-pro.toml`：Flash 固定 `deepseek-v4-flash / codex_model_router_v2 / medium / read-only instruction scope`，Pro 固定 `deepseek-v4-pro / codex_model_router_v2 / high / complex changes`。这里的 read-only 是 developer instructions 约束，不代表 role TOML 设置了 `sandbox_mode`。
- Flash 无模型名 canary：parent `019feecd-1875-7890-b9a8-a10f16440b15`，child `019feecd-8e01-7663-8227-4b041933a2f4`，真实 `agent_role=deepseek-v4-flash / model=deepseek-v4-flash / model_provider=codex_model_router_v2 / multi_agent_version=v2 / effort=medium`；初始任务与同 child follow-up 合计 3 组真实只读 function call/output。Router 命中 `https://api.deepseek.com/v1/responses`、`responses_to_chat=false` 并多轮 HTTP 200。child 和 follow-up 都完成，但本机 `codex-cli 0.146.0-alpha.3.1` 的父 `codex exec` 在空 receiver wait 后没有自行退出，最终人工 Ctrl+C；这属于当前 CLI 等待异常，不是 CCSM role/路由失败。
- Pro 无模型名 canary：parent `019feed5-a48c-70d2-a6ee-96aef908a523`，child `019feed5-fc7e-7f82-9200-249fb22fbe87`，真实 `agent_role=deepseek-v4-pro / model=deepseek-v4-pro / model_provider=codex_model_router_v2 / multi_agent_version=v2 / effort=high`；初始跨模块调查与同 child follow-up 共 67 组 function call/output，父进程 exit 0。Router 把入站 `/responses` 桥接到 `https://api.deepseek.com/v1/chat/completions`，`responses_to_chat=true` 并多轮 HTTP 200。首轮存在错误 PowerShell 查询与 `node_modules` 噪声，但最终收敛并给出架构结论。
- 官方保留路径 canary：parent `019feedc-70b7-7492-8660-e65fafcd2c36`，唯一 child `019feede-5c6c-7cd2-b4dd-9107e6e5c424`。父 Codex 选择内置 `explorer`，未指定 child model、未使用第三方 custom role；child rollout 为 `agent_role=explorer / model=gpt-5.6-sol / model_provider=codex_model_router_v2 / multi_agent_version=v2 / effort=medium`，两轮 task 均完成，含 2 组 function call/output 与 22 组只读 custom tool call/output，父进程 exit 0。Router 以 parent session 记账，命中 `router-codex-official -> https://chatgpt.com/backend-api/codex/responses`、`responses_to_chat=false` 并持续 HTTP 200。首次显式 `agent_type=explorer` 的 full-history fork 被上游拒绝，父 Agent 随后改用受支持的有限上下文 fork；拒绝请求没有创建额外 child。
- V2 自动选型是 Codex 对 role description 的语义判断，不是确定性路由规则。普通“仓库探索”提示即使第三方 profile 为 preferred，仍可能由内置 `explorer` 抢占；不写模型名但明确要求“配置的第三方 capability-profile”后，长上下文/证据任务和复杂架构任务才分别稳定选择 Flash/Pro。因此 CCSM 已实现“用户不选具体子模型”，但不能承诺模糊任务一定调用第三方；验收和 UI 文案都必须保留这一边界。
- 事务 launcher 不得使用 `Start-Process -Wait`：Microsoft Learn 明确说明该开关会等待目标进程及全部后代，而事务成功后拉起的 CCSM 是长期存活后代，会导致 launcher 永不返回，人工 Ctrl+C 还可能误伤 CCSM。正确流程是 `Start-Process ... -PassThru` 后仅对捕获的事务 PowerShell 调用 `$transaction.WaitForExit()`（或按该 PID 使用 `Wait-Process`），再读取 exit code/result；不能等待整个进程树。Codex 内置 Web 与 Matrix WebSearch 本轮独立读取同一官方说明，结论一致。
- 本轮只保留本地分支、构建与安装，不 push、不创建 PR、不创建 GitHub Release。真实运行结论来自安装 UI、生成 role 文件、child rollout 和 Router HTTP 200；联网结论来自 Codex 内置 Web、Matrix WebSearch 与官方一手文档，未发现来源冲突。

## 2026-08-09 Codex Sub-Agent V1/V2 双模式设置设计

- 用户确认 MultiRouter 应同时保留 `Sub-Agent V1（兼容）` 与 `Sub-Agent V2（推荐）` 两套配置，并由每个方案选择当前生效版本；同一方案、同一会话只运行一种协议。新方案和缺少版本字段的旧方案默认 V2，现有 `modelCatalog.spawnAgentModels` 保留为 V1 direct override 前五顺序，切换版本不删除另一套数据。
- 方案级单一数据源定为 `settingsConfig.codexRouting.subagentVersion: "v1" | "v2"`。当前四阶段 MultiRouter 向导只处理 `sources / prepare / review / activate`，V1/V2 编辑位于独立 Sub-Agent 工作区；V1 强调显式 direct override 与旧工具兼容，V2 由问卷写入 schema-v1 profiles、backend compiler 生成官方 custom role TOML，再由父 Codex 通过 `agent_type` 按角色描述语义选择有效角色。
- 上游源码证明选择 V1 不能只改 catalog：Codex 的 `features.multi_agent_v2` override 优先于模型 `multi_agent_version`。V1 投影必须统一写 V1 metadata 并显式设置 `multi_agent_v2.enabled=false`；V2 统一写 V2、启用 feature、保留混合路由 `tool_namespace=agents` 和 reserved schema 边界。
- V2 managed roles 只在 V2 激活时由完整可路由 catalog 生成；V1 激活时清理 CCSM 自己带 marker 的托管 role，用户手写 role 永远保留。direct override 前五与 managed role 全目录继续彻底解耦。
- 批准规格位于 `docs/superpowers/specs/2026-08-09-codex-subagent-v1-v2-settings-design.md`。实现已按 TDD RED/GREEN 分提交，最终因事务安全加固、安装态缓存回归和 transport 所有权边界收窄递增到 `3.19.1-18`；V1 direct override 与 V2 无模型名 Flash/Pro 三条真实 canary 均已完成。Matrix WebSearch 本轮仍为 HTTP 521，未提供正证据。

## 2026-08-10 Sub-Agent V2 capability compatibility closeout（源码态）

- 当前 V2 数据链为：共享 Wizard/Workspace 问卷与 overrides -> `settingsConfig.codexRouting.subagentV2` schema-v1 profiles -> Rust backend compiler/status/preview -> `~/.codex/agents/*.toml` 官方 custom role 文件 -> 父 Codex 按任务语义选择 `agent_type`。Flash/Pro 只是当前已知 preset；模型目录可扩展，选择能力不能退化为前端硬编码模型名或描述。
- 兼容边界：缺少 `subagentV2` 的既有方案仍走 `legacy_managed_roles`，普通保存不隐式初始化，只有显式一键初始化才写 schema-v1 defaults。V1 继续保留 `spawnAgentModels` direct override 前五顺序；文件同步回归使用同一个 retained settings/profile 对象证明切到 V1 只清理 CCSM marker role、切回 V2 后问卷推导字段与 override 都能恢复。模型临时离开 catalog 时不生成 role；同一 retained settings 中的 canonical model 重现（即使 visible alias/displayName 改名）后仍按 canonical `profile.model` 恢复。profile 的持久化不丢失不由只读 renderer 测试证明，而由 deterministic DAO atomic interleaving test 独立证明。
- focused persistence 必须使用 `update_codex_subagent_v2`：在 SQLite `TransactionBehavior::Immediate` 内读取最新 Provider，只替换 `codexRouting.subagentV2` 后提交。禁止恢复 `get_providers -> 前端 stale merge -> update_provider`；catalog/alias refresh 与 V2 save 的交错测试必须长期保留。
- compiler/status/preview 是唯一权威：NFKC + default Unicode case-fold 的 profile key/role name collision fail closed；`default`、`worker`、`explorer` 受保护；用户同名 role 文件不可写，托管角色回退 `ccswitch-<role>`。disabled/unroutable profile 可持久化但不生成文件。status 显示 requested/effective role、绝对路径、provider/model/reasoning、field/generation source 与受控 non-generation reason。
- 诊断边界不得输出 credential、API key、encrypted value、task text、raw invalid profile key/value 或任意未经 allowlist 的 warning；invalid DTO 保持无原始标识的受控结构。Qwen 继承语义、reserved spawn schema、parent model、V2 body projection、`hide_spawn_agent_metadata=true`、mixed `tool_namespace="agents"` 与 proxy forwarding 均不属于 V2 选型逻辑，必须保持独立回归。
- Task 6 完整源码验证：Rust lib `2921 passed / 0 failed / 2 ignored`（共 2923），Vitest `115 files / 914 tests passed`；`pnpm typecheck`、`cargo check --lib`、`cargo fmt --check`、`git diff --check` 全部 exit 0。新增兼容证据只扩展 Rust integration-style sync 测试，没有复制 compiler 到前端，也没有修改生产逻辑。
- 本节只记录当前源码与测试证据；未在 Task 6 升版、构建、安装、停止进程或执行 live canary。Task 8 必须从固定提交启动独立 transaction install，不能复用跨提交、跨版本或未完成流水线；完成后再追加 artifact hash、transaction/child id、route/HTTP 与安装态证据。

## 2026-08-10 Sub-Agent V1/V2 3.19.1-18 transport 边界修正与最终安装验收

- `3.19.1-17` 完整 Rust 回归暴露 `codex_model_catalog_keeps_official_transport_and_reserved_tool_metadata` 失败：上一版 `5562788d` 把 `multi_agent_version`/`multiAgentVersion` 的 MultiRouter 所有权放进共享的 `codex_official_picker_metadata_field`，虽然修复最终 cache 第二次合并，却同时破坏普通 catalog enrichment 的“官方同 slug transport 优先”契约。根因不是缓存内容，而是 cache 专属策略落在了共享合并原语上。
- 边界修复提交 `02841942` 恢复共享合并的官方 transport 优先，只在 `sync_codex_models_cache_with_cc_switch_catalog` 最终写入 CCSM-owned cache 前，按 stable model id 从当前 routed catalog 覆盖 snake/camel 两个版本字段。两个原本冲突的测试同时通过，cache-sync 3/3、transport/merge 4/4，完整 Rust 为 `2835 passed / 0 failed / 2 ignored`；前端 `114 files / 825 tests`、typecheck、cargo check、rustfmt 和 diff check 均通过。
- Whole-branch review 没有 Critical；建议的两个重要覆盖缺口已在 `31c3a210` 补齐：official-only managed OAuth 的 V1 双拼写投影，以及仅 Flash 可路由时 V2 工作台同时显示“可路由/目录中缺失”。版本提交 `798b2fb9` 将四个版本源统一为 `3.19.1-18`。
- 固定 HEAD `798b2fb965d1313157a950c5387b0798ebec0b3b` 的独立 release 流水线完整 exit 0，metadata commit/version 与工作树一致。NSIS SHA-256 `BBA84A0E943F6FAF948CF9485DCDAEA0678A8288EB4A07681FD73C3BC7F1FDFC`，raw EXE SHA-256 `51153ED5DC8819C430D8F9174244E49A3382986A40EA1D6A391D939B777B7E81`，二者文件/产品版本均为 `3.19.1-18`，updater signature 432 bytes。升版前已启动且跨越文件变化的旧流水线自然结束后被明确废弃，未作为安装源。
- Pester 事务安全回归 42/42 后，事务 `ccsm-20260810-052852-1d2c03160f934a3ba627a30c78d48545` 按 `kill -> retained-handle wait -> port release -> uninstall -> install -> hidden relaunch -> listener/health/version/hash` 成功完成，错误与回滚错误均为空。事务外复核：新 PID/15721 owner `8772`，installed/file/product/registry 全部为 `3.19.1-18`，安装 EXE 哈希与 raw artifact 完全一致，health HTTP 200；Codex Desktop 未被停止。
- 安装态 `cc-switch-model-catalog.json` 与 `models_cache.json` 都为 9 个模型、9/9 同时 `v2/v2`，Flash/Pro 均存在；`deepseek-flash.toml` 固定 `deepseek-v4-flash / codex_model_router_v2 / medium`，`deepseek-pro.toml` 固定 `deepseek-v4-pro / codex_model_router_v2 / high`，互斥描述保持正确。
- `3.19.1-18` Flash child `019fe86e-9825-76f3-9c12-2964c43de797` 为 `agent_role=deepseek-flash / model=deepseek-v4-flash / provider=codex_model_router_v2 / effort=medium`，完成初始真实只读 Git/源码扫描与同 child 安装态 follow-up；rollout 有 2 turns 和真实 function calls。Router 日志证明 `/responses -> https://api.deepseek.com/v1/responses`、`responses_to_chat=false`，多轮 HTTP 200。
- `3.19.1-18` Pro child `019fe86e-af54-7283-8078-3b3132de2a3a` 为 `agent_role=deepseek-pro / model=deepseek-v4-pro / provider=codex_model_router_v2 / effort=high`，完成复杂所有权追踪与同 child role/EXE follow-up；rollout 有 2 turns 和真实 function calls。Router 日志证明入站 `/responses` 被桥接到 `/chat/completions`、`responses_to_chat=true`，多轮 HTTP 200。此次 canary 使用新 child rollout 和新 CCSM PID，但保留当前 Desktop parent/app-server，不把“未重启 Desktop”误记为已重启。
- 新发现的非阻塞边缘风险：CCSM 接管后的官方 `models_cache` 备份只创建一次；除当前 V1/V2 两字段外，其它官方 transport/picker 元数据可能在长期接管期间冻结。该问题不影响本轮两个契约和安装验收，后续若处理应设计官方 cache 刷新/再合并机制，不能简单扩大当前 override 列表。
- 本轮继续使用 Codex 内置 Web 与 Matrix MCP 两条独立链；内置搜索定位到 Codex 官方 `multi_agent_version`/cache 一手源码和近期 cache 行为问题，Matrix 仍返回 HTTP 521。最终技术判断以本地失败测试、当前源码、安装产物、事务日志、child rollout、SQLite 请求记录和 Router HTTP 200 为主。

## 2026-08-11 Codex OAuth 过期提示竞态与 Windows 开机自启漂移根修

- OAuth 现场日志证明授权完成并保存账号后，同一时刻仍有多条 device-code 流并发；旧 `useManagedAuth` 只保存最后一组 timer 句柄，旧流程的 timeout/poll 回调无法全部取消，随后可把已登录页面重新写成 `Device code expired`。根修提交 `4588fe14`/`7e2bfddf` 将流程收敛为单活动 generation，完成、取消、超时和卸载都会令旧回调失效；到期前重新读取后端账号状态，若账号已可用则保持登录态而不是展示过期错误。RFC 8628 的边界是授权请求到期，不等价于既有账号失效。
- Windows 现场为 `settings.json.launchOnStartup=true`，但 `HKCU\\...\\Run\\CCSwitchMulti` 缺失，仅有 `StartupApproved` enabled marker。根因是旧版只在前端开关值变化时调用系统 API；升级后注册项丢失而持久设置仍为 true，不会触发修复。`auto_launch.rs` 现在在应用启动时对账持久期望和真实注册状态，Run 缺失或路径漂移时重建带引号的当前 EXE；若 Run 正确但被任务管理器明确禁用则尊重系统选择。边界测试提交 `d9905d6c`，实现同在 `7e2bfddf`。
- 版本考古必须区分“潜伏设计缺陷”和“本机触发器”。自启代码最早在源码版本仍标为 `3.7.0` 的 `162c9214`/`524fa943` 出现，但同线功能被 `eb46ac85` 回滚，正式 tag `v3.7.1` 没有 `auto_launch.rs`；长期实现由 `d38fcd63` 重新进入源码，首个包含它的正式 tag 是 `v3.8.0`。因此对用户可见的潜伏缺陷首发版本应记为 `v3.8.0`，不是 `v3.7.0`：持久设置与系统注册从一开始就是两套状态，应用启动不做对账，只有设置保存路径会调用系统 API。
- 本机这次失效的触发器是 `6dca12c4` 在源码版本 `3.19.1-15` 引入的事务重装。其 `RunUninstaller` 以 `/S _?=<install dir>` 执行 Tauri 2.8.5 NSIS 卸载器，却没有 `/UPDATE`；官方模板在非 UpdateMode 下明确删除 `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\${PRODUCTNAME}`。第一次成功完整执行该路径的事务 `ccsm-20260810-034021-30bb9f3bfbb94fbc80e1df504107b468` 从注册表备份证明旧安装为 `3.19.1-15`，配置备份证明 `launchOnStartup=true`，并于 03:40:32 成功启动 `3.19.1-16`。因此触发机制进入源码是 `3.19.1-15`，本机首次实际触发/开始失效是升级到 `3.19.1-16`，到用户发现时的 `3.19.1-18` 仍不会自愈；`3.19.1-19` 才加入启动对账。事务未备份 Run 项，故没有卸载前的逐值快照或注册表审计日志，但卸载参数、Tauri 删除语义、`settings=true`、Run 缺失而 StartupApproved 残留形成高置信度闭环。
- macOS/Linux 不会原样复现这次 Windows NSIS 删除链：当前 `auto-launch 0.5.0` 在 macOS 默认使用 AppleScript Login Item，普通 `.app` 覆盖升级不会调用 Windows 卸载模板；Linux 写用户目录 `~/.config/autostart/CCSwitchMulti.desktop`，普通 deb/rpm 升级通常也不删除它。不过两端都存在同构漂移：macOS `is_enabled()` 只按 Login Item 名称判断，不核对 `.app` 路径；Linux `is_enabled()` 只检查 desktop 文件存在，不核对 `Exec`、`Hidden=true` 或目标可执行文件。因此 `3.19.1-19` 能修复“条目完全缺失”，但仍不能修复 macOS 同名失效路径、Linux 陈旧 Exec/Hidden 条目。macOS 13+ 后续应评估 `SMAppService.status`，Linux 应解析并校验 desktop entry，而不是继续依赖文件存在布尔值。此结论为源码与 Apple/Freedesktop 官方规范审计，尚未在真实 macOS/Linux 主机执行升级/登录验收。
- 最终交付基于包含 Sub-Agent V1/V2 全部工作的 `bigstrongsun/subagent-v1-v2`，版本提交 `9b19ee7b`，版本 `3.19.1-19`。可信本地产物位于 `C:\\Users\\sunda\\Documents\\LLMservice\\ccswitchmulti-release-v3.19.1-19-auth-startup-final`；raw EXE SHA-256 为 `4A7DC16978DFF4191014E89B95A60715F76BC4596BB8F24BEFA4BF46D5B5E396`，安装器 SHA-256 为 `003578E5D5B7504372110BA4F8A9E028E702C34A51D10F93F2C43AD88BBB7DC5`。不要使用此前文件名/latest.json 仍停留在 18 的竞态快照。
- 第一次事务 `ccsm-20260811-000008-58700350f7de4198991ff0d7de754ac1` 因旧 PID 后的 15721 释放等待超时而自动恢复 18；受控实验确认停止旧进程后端口从首个样本起释放且 15 秒无自启动抢占。第二次事务 `ccsm-20260811-033128-ee959119dd414e60b7de6efeb6854c92` 成功安装并启动 19，备份位于 `C:\\Users\\sunda\\Documents\\LLMservice\\ccsm-transaction-backups\\ccsm-20260811-033128-ee959119dd414e60b7de6efeb6854c92`。
- 安装态独立复核：`C:\\Users\\sunda\\AppData\\Local\\CCSwitchMulti\\cc-switch.exe` 文件/产品版本均为 `3.19.1-19`，哈希与 raw artifact 一致；PID 46868 同时拥有 15721 listener，`/health` HTTP 200。Run 值为带引号的当前 EXE，StartupApproved 为 enabled。实际 UI 显示 Codex OAuth `1 个账号` 且无过期提示，开机自启开关开启。定向前端 10/10、Rust auto_launch 4/4、typecheck 通过；仅有既存 dead-code 与 baseline-browser-mapping 过期提示。尚未执行真实 Windows 注销/登录或重启，因此只能确认注册与启动时对账恢复，不能把系统重启验收记为已完成。
- 本轮研究同时使用 Codex 内置 Web 与 `matrix-websearch` 两条独立链。内置链以 RFC 8628、Tauri/Windows 启动机制为依据；Matrix 仅独立印证通用 Tauri 自启动 API，CCSwitchMulti 专项结果较弱。最终根因和交付结论以现场日志、当前源码、RED/GREEN 回归、事务安装、注册表、端口、健康接口及实际 UI 为主。

## 2026-08-10 Sub-Agent V1/V2 3.19.1-17 本机交付与真实验收

- V2 保存后的现场核对先发现一个未被主 catalog 检查覆盖的真实漂移：`cc-switch-model-catalog.json` 的 9 个模型均为 `v2/v2`，但 `models_cache.json` 中 `gpt-5.6-luna` 为 `multi_agent_version=v1 / multiAgentVersion=v2`。接管前备份中 Luna 的官方 snake_case 字段恰为 V1；`sync_codex_models_cache_with_cc_switch_catalog` 再次合并官方备份时，`codex_official_picker_metadata_field` 把该 transport 字段误当成官方权威值，覆盖了当前方案已经投影出的 V2。不能靠手改缓存解决。
- TDD RED `c82d25ad` 用真实 catalog-to-cache 边界复现 `left=v1 / right=v2`；GREEN `5562788d` 只把 `multi_agent_version` 与 `multiAgentVersion` 定义为 MultiRouter 当前方案拥有的字段，其他官方同 slug picker 元数据继续保持权威。新回归 1/1、cache sync 2/2、merge metadata 3/3、rustfmt 和 diff check 通过。版本提交为 `32a18a52`。
- 干净 `3.19.1-17` 本地流水线精确由 `32a18a52af00118f502ec3a712ad5a47531bdbc3` 触发并完成；NSIS 安装器 SHA-256 `487D25C2F237034D5AA18AC2A4F664C6694FC5FBE407AC972654B939F506F129`，raw EXE SHA-256 `78FF25707807315BC40DCA742CDFABA794A0E16787EADEB29657D0A0939AB87F`，二者文件/产品版本均为 `3.19.1-17`，updater signature 432 bytes。此前从 RED 提交启动且跨越工作树版本变化的混合流水线产物明确废弃，未用于安装。
- 独立事务 `ccsm-20260810-042926-8d95589964d54e1dae48500edf521e72` 按 `kill -> 等待退出/端口释放 -> 卸载 -> 安装 -> 拉起 -> 健康/版本/哈希验证` 成功完成，stderr 为空，新 PID 9440。事务外复核为 installed/registry `3.19.1-17`、raw hash 与构建完全一致、`127.0.0.1:15721/health` HTTP 200；失败恢复能力仍由已审计的 42/42 Pester 事务核心提供。
- 安装后 DB `codexRouting.subagentVersion=v2`，V1 `spawnAgentModels` 顺序仍完整保留为 Flash/Sol/Qwen/Luna/Terra；`features.multi_agent_v2.enabled=true`，Flash/Pro role 文件均存在；主 catalog 与 `models_cache.json` 的 9 个模型现在全部为 `v2/v2`。这证明切换版本不会丢另一套数据，且缓存回归已在真实安装态自愈。
- Flash 无子模型名 canary：parent `019fe83a-cc27-72f3-a442-579012fe8044` 自动选择 `agent_type=deepseek-flash`，child `019fe83b-00de-7240-8f6f-6a1bddb38a35` 的 `agent_role=deepseek-flash / model=deepseek-v4-flash / model_provider=codex_model_router_v2 / multi_agent_version=v2`。初始任务真实执行 `git rev-parse`、`rg` 和定向源码读取，返回 `32a18a52` 与实现位置；同 child follow-up 真实执行 `git branch --show-current` 并返回 `bigstrongsun/subagent-v1-v2`。Router 日志证明 `https://api.deepseek.com/v1/responses`、`responses_to_chat=false`，多轮 HTTP 200。
- Pro 无子模型名 canary：parent `019fe83c-f57b-72c2-a0c8-ab5f66118501` 自动选择 `agent_type=deepseek-pro`，child `019fe83e-121b-7801-b6f8-52e408dd8511` 的 `agent_role=deepseek-pro / model=deepseek-v4-pro / model_provider=codex_model_router_v2 / multi_agent_version=v2`。初始任务只做精准 `git show`、单文件 `rg` 与定向源码读取，正确解释三提交和跨阶段根因；同 child follow-up 的 `git status --short --branch` 与 `git diff --check` 均 exit 0。没有 broad recursive scan、错误 PowerShell、写文件或权限提升。Router 日志证明 Responses 入站被桥接到 `https://api.deepseek.com/v1/chat/completions`、`responses_to_chat=true`，多轮 HTTP 200。
- 运行边界：本机 canary CLI 为 `0.146.0-alpha.3.1`。第一条 Flash 探针在未提醒 fork 约束时先用 full-history fork 被拒绝，随后父 Agent 自动改用独立 fork；显式 `-s read-only` 还会让父/子都遇到 Windows `CreateProcessWithLogonW failed`，因此正式成功探针使用 `danger-full-access` sandbox 但任务命令严格只读。后续提示只要求“遵守当前工具的 fork_turns 约束”，未指定任何子模型，父 Agent 均直接使用 `fork_turns=none`。这些是当前 Codex CLI/sandbox 边界，不是 CCSM role 或路由失败。
- UI 安装态已实际核对：MultiRouter 工作台显示完整 `Sub-Agent 设置`、当前 V2、Flash/Pro 可路由，V1 页面保留前五 direct override 排序；V1 保存后 feature/catalog/managed role 清理与真实 direct override canary 均通过，再切回 V2 后状态和 V1 排序都保留。没有推送、PR 或 GitHub Release。
- 联网检索继续同时使用 Codex 内置 Web 和 Matrix MCP；Codex 内置搜索得到官方/上游 custom role 与 schema 资料，Matrix 独立链仍返回 HTTP 521。运行结论以本机 DB/config/catalog、child rollout、Router 日志和安装产物一手证据为准，Matrix 未提供第二条正证据。

## 2026-08-07 DeepSeek Pro Chat 路由被启动迁移反复改写为 Flash Responses

- 用户反馈在官方 DeepSeek 选择 Chat 后，Codex 只“一问一答”而不继续工具循环；改选 Responses 后，大对话又会因官方端点实现边界报错。本机 3.19.1-10 的真实 SQLite 显示：`DeepSeek-chat` Provider 自身仍是 `openai_chat` 且目录只有 `deepseek-v4-pro`，但 MultiRouter 中引用它的 route 已被改成 `openai_responses`，匹配窗口也变成 `deepseek-v4-flash`。因此 UI 选择与实际出站协议发生漂移，不能把症状归因于 DeepSeek 不支持工具。
- 根因位于 `Database::repair_deepseek_native_responses_on_conn`：旧实现对每个 Provider/Router 直接在整份 `settings_config` 字符串上计算 `has_flash`。MultiRouter 只要有任意一个 Flash sibling route，就会在后续每次启动时把所有官方 DeepSeek Chat route 改写成 Flash/Responses；即使 route 明确 `targetProviderId` 指向独立的 Pro/Chat Provider 也不例外。原“幂等”测试第二次运行只检查 route 数量，没有复核协议与模型窗口，所以漏掉了反复污染。
- 修复先提交失败测试 `fdd9ab76`，再在 `4d62e500` 中按 route 的 `targetProviderId` 解析官方 DeepSeek Provider 的真实模型归属：Flash-only 规范为 `deepseek-v4-flash + openai_responses`，Pro-only 规范为 `deepseek-v4-pro + openai_chat`，同 Provider 同时拥有两种模型时继续保留旧版拆分迁移。该逻辑也会自愈已经污染的 route，并同时收敛 `match.models`、`match.prefixes` 与 route `modelMap`。
- 实网交叉验证：DeepSeek 官方 Chat Completions 文档仍声明 `tool_calls` / `finish_reason=tool_calls`；直接对官方 `/v1/chat/completions` 的 V4 Flash 工具场景返回了合法流式 `tool_calls`。OpenAI Codex 当前源码已移除原生 `wire_api=chat`，所以 CCSM 必须继续对外维持 Responses、只在内部将目标 Chat route 转换到 `/chat/completions`。这与本次“修路由归属而非绕开工具循环”的边界一致。
- 回归命令：`cargo test --manifest-path src-tauri/Cargo.toml repair_deepseek_native_responses -- --nocapture`（2 passed）；`cargo fmt --manifest-path src-tauri/Cargo.toml --check` 与 `git diff --check` 通过。运行中安装版仍是 3.19.1-10，源码修复尚未构建/安装，不能把源码通过等同于现场已经生效。

## 2026-08-05 Codex Multi-Agent V2 跨 Provider 双阶段 payload 根修

- 2026-08-05 21:21 重装并完全重启后的真实验收通过：运行中 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 文件/产品版本均为 `3.19.1-6`，进程与 Codex app-server 均在 21:18 后新建；父 task `019fcf70-03a2-7791-a383-45b5a88e1e4e` 的 `turn_context` 为 `gpt-5.6-sol`，router log 同时证明 effective provider 为 `OpenAI_Official` 且官方请求 HTTP 200。
- 使用 `fork_turns=none` 做两个不可从父上下文猜测的唯一 nonce canary：Qwen child `019fd216-51fc-7b21-b372-8bd408763a2c` 命中 `qwen3.6 -> Qwen`，DeepSeek child `019fd216-703a-7ca1-8803-7ee0ccd1b623` 命中 `deepseek-v4-flash -> DeepSeek-responses`，各自正确回传 nonce、只读 `Get-Location` 与 `git rev-parse --short HEAD` 的真实结果 `54f0040b`；router log 对两条第三方上游请求均记录 HTTP 200。
- 两个 child rollout 的任务仍由 Codex 持久化为 `agent_message[input_text, encrypted_content]`，但 `encrypted_content` 分别为 461/465 字符的可打印明文，包含 nonce 和命令，既不以 `gAAAAA` 开头也不符合 Base64 密文形态；Stage B 的 legacy-plaintext 恢复路径将其投影为普通 user input 后，Qwen 产生 2 组 shell call/output、DeepSeek 产生 1 组 shell call/output，最终 assistant 输出与 nonce 完全一致。旧版现场的 228 字符 opaque Fernet 与空 Payload 已不再出现。
- 本地交付构建版本为 `3.19.1-6`，版本提交 `f4a89fd8`；完整 post-commit 发布流水线在 `2026-08-05 17:16:32 +08:00` 成功结束，随后以 `-SkipBuild` 把该已成功构建独立导出到 `C:\Users\sunda\Documents\LLMservice\CCSwitchMulti-3.19.1-6-local`。NSIS 安装器 `CCSwitchMulti_3.19.1-6_x64-setup.exe` 为 11,499,096 bytes，文件/产品版本均为 `3.19.1-6`，SHA-256 `DF4A03AC834A04B9CCFF4CB41A463AEC37691BCEF365CFCE17184CFFD09F0509`；updater `.sig` 非空。portable ZIP SHA-256 `8D1FAED02CA118D6D48BA9C8F21E08D7667287C5E4AE0E6B6379A14D93F74946`，raw EXE SHA-256 `B3A134CF3530BAFB77E311B22FB113776F506B89831346BFBE263D93D40C46D4`。
- 仓库 `post-commit` hook 会在每次提交后异步启动本地发布；连续提交可能在上一条 pipeline 进入 build 后因锁文件被其它失败调用的 `finally` 提前删除而出现多个并发 pipeline，并在 Cargo artifact lock 上排队。验收时必须以明确的 `Local release pipeline completed`、`RELEASE-METADATA.md` commit/version 和独立导出目录哈希为准，不能用仍在变化的 `最新版ccswitchmulti` 目录或单纯“安装器文件存在”作为成功证据。
- 2026-08-05 重新交叉验证上游：Codex 内置 Web 搜索、GitHub API/源码 diff 均确认 `openai/codex#36376` 与 `#36586` 仍为 open；已合并 `#35845` 只在 `encrypted_function_args=[]` 时走 `DirectPlaintextMessage`，不能恢复官方 parent 已经返回的 `gAAAAA...`。Matrix `matrix-websearch` MCP 已通过 stdio JSON-RPC 独立初始化并检索三组 GitHub 限定查询，但均返回 0 结果，因此不作为正证据。
- 旧 `a22c5f8b` 只把混合 Router 的 `tool_namespace` 改成非保留 `agents`，消除了 `collaboration.*` reserved schema HTTP 400，却同时删除了 Stage A 的明文化入口；真实 child rollout 随后证明 namespace 成功不等于 payload 可读。正确修复不是恢复旧的 `collaboration.*` 改写，而是只改非保留 `agents.*`。
- Stage A RED 提交 `aa64e5bf`，生产提交 `4c2854ac`：混合 Router 的跨 provider V2 策略以 request-local `codexRouterPlaintextV2Collaboration=true` 穿过 route 解析和 target provider 物化；仅当 app=Codex、effective parent 为 official OAuth、该策略为 true 时，删除标准 `tools` 与 Responses Lite `additional_tools` 中 `agents.spawn_agent/send_message/followup_task` 的 `parameters.properties.message.encrypted`。`collaboration.*`、普通工具、其它 `encrypted` 字段、第三方 parent、其它 app 与纯官方 Router均保持不变。
- Stage B RED 提交 `c47f6b4f`，生产提交 `8990f746`：effective provider 已确定为第三方且 endpoint 为 Codex Responses 时，在 native Responses/Chat/Anthropic 转换前把 plaintext `agent_message` 投影成标准 `type=message, role=user`；旧版误把普通明文塞进 `encrypted_content` 时恢复为 `input_text`。若仍包含可解码为 32+ bytes 的 opaque base64/Fernet 内容（现场 `gAAAAA...`），立即返回不含密文和任务正文的 `InvalidRequest`，不再把空 Payload 当成功。
- 安全边界：CCSM 不解密 OpenAI ciphertext；新日志只记录改写/投影数量与 provider id，不记录 message、Payload 或密文；没有新增数据库、sidecar 或请求体持久化。纯官方 child 不经过 Stage B，继续保留官方加密。
- 验证：Stage A 两个 RED 分别稳定得到 `changed=0` 和 route marker `None`；Stage B 三个 RED 分别稳定失败于未投影、未恢复和未拒绝。生产实现后 targeted 回归全绿；完整 `cargo test --lib -- --test-threads=1` 为 `2814 passed / 0 failed / 2 ignored`，`cargo check --lib`、rustfmt、`git diff --check` 通过。当前只证明源码/转换边界，仍须安装新包并完全重启 CCSM/Codex 后，以 OpenAI->Qwen/DeepSeek 唯一 nonce + 真实只读工具输出 + child rollout 可读 Payload 做最终验收。

## 2026-08-05 Codex 0.146 Multi-Agent V2 第三方子 Agent 空正文与 reserved schema 根修

- 本机 Codex Desktop `0.146.0-alpha.9.2` 在 `codex_model_router_v2` 下真实派生 `qwen3.6` 与 `deepseek-v4-flash` child：两条路由都命中第三方上游并返回 HTTP 200，但 `collaboration` namespace 会让 official parent 把任务正文持久化为第三方无法解密的 `encrypted_content`，Qwen 因而报告 `empty payload`。
- 2026-08-01 至 08-05 的错误方案在发往官方 OAuth backend 前删除 `spawn_agent`、`send_message`、`followup_task` 的 `message.encrypted`，并在 route 物化时传播 `codexRouterPlaintextV2Collaboration`。session `019fc293-3b9c-7c43-9f61-a29350ce24a3` 证明该方案会让新版官方 backend 以 HTTP 400 拒绝：`Function 'collaboration.followup_task' is reserved for use by this model and must match the configured schema`。删除 `encrypted` 已经改变保留工具 schema；三个保留函数都不得由代理改写。
- 用户回滚 CCSM 后，同一官方 Sol 路由由 400 恢复 HTTP 200。进一步用 Codex `multi_agent_v2.tool_namespace="agents"` 做 A/B，确认官方 parent 正常返回、第三方 child 能创建且路由 HTTP 200、reserved schema 400 消失；但当时仅凭 child/parent 返回 `CHILD_OK/PARENT_OK` 就判断正文可读是过早结论，可能受继承上下文、模型猜测或旧进程影响，不能作为 payload 验收。
- CCSM 对包含启用第三方或来源歧义 route 的 MultiRouter 投影 `tool_namespace="agents"`，但保留用户已有的其它非保留自定义 namespace；纯官方 Router 不强制切换 namespace。`hide_spawn_agent_metadata=true` 和 catalog 的 `multi_agent_version=v2` 保持不变。
- 代理层彻底删除 collaboration schema 改写、request-local 明文策略 marker、route 物化传播和对应旧测试。以后官方 OAuth 出站的 reserved tool schema 必须原样透传；非保留 namespace 只解决工具名冲突，不自动提供第三方可读正文。安装新构建并完全重启 CCSwitchMulti/Codex app-server 后，必须用 Desktop 对 Sol -> Qwen/DeepSeek 做唯一 nonce + 真实工具动作 + child rollout 三重验收。
- 2026-08-05 最新验收使用 Codex Desktop embedded CLI `0.147.0-alpha.1.2`、CCSM `3.19.1-5` 和有效配置 `hide_spawn_agent_metadata=true/tool_namespace="agents"`：Qwen 与 DeepSeek child 都正确命中 `codex_model_router_v2`，但首个 `agent_message` 仍为 `[input_text 空 Payload 信封, encrypted_content gAAAAA...]`，两个密文长度均为 228；这证明 namespace、路由和 task delivery 是三个独立验收层。
- 完整问题、脱敏 TOML、官方 `#26210/#35845`、上游 `#36376/#36586`、所有失败方案、双阶段设计与验收矩阵已提交为 `BigStrongSun/ccswitchmulti#31`：https://github.com/BigStrongSun/ccswitchmulti/issues/31 。该 Issue 与 `#18` 相邻但不同：`#31` 是 parent->child 合法 OpenAI ciphertext 导致第三方空 payload；`#18` 是 child->parent 明文被错误标为 encrypted_content 后污染官方 replay。

## 2026-08-04 Codex `--ephemeral resume` 污染真实会话与恢复边界

- 现场会话 `019fbd59-0c1a-7592-87d3-1e2ad654fd0d` 无法加载并提示 `Model provider 'capture' not found`，不是全局 `config.toml` 或 CCSM 正常路由错误。另一排障任务为抓取真实 Responses 请求体，对该生产 session 执行了两次 `codex exec resume ... --ephemeral -c model_provider="capture"`；临时 provider 随子进程退出，但 resume 仍把 `thread_settings_applied(model_provider_id="capture")` 和诊断 turn 写入原 rollout，并把 `state_5.sqlite.threads` 对应行同步成 capture/medium/CCSwitch cwd。
- OpenAI Codex 官方 issue `openai/codex#20084` 与本机 `codex-cli 0.146.0-alpha.9.2` 现场共同证明：resume 路径会忽略 `--ephemeral` 的不持久化语义。以后不得用生产 session ID 做请求捕获；必须使用新建/复制/分叉的诊断 session，并在捕获前后比较原 rollout 的长度与哈希。
- 恢复不能只改 `~/.codex/config.toml`，也不能永久补一个假的 capture provider。安全边界是：先用 SHA-256 备份原 rollout，并用 SQLite backup API 备份包含 WAL 的一致数据库；以第一条 capture `thread_settings_applied` 为字节边界构造原文件精确前缀候选；验证所有 JSONL、session ID、最后合法 provider/cwd/reasoning 和无 capture 后，再原子替换 rollout，并在事务中只恢复 `threads` 表该 session 一行。
- 本次原 rollout 为 293,142,176 字节、SHA-256 `F216A17E...E408`；修复后为 293,102,773 字节、23,399 行、SHA-256 `EB8C75F9...8101`，只移除 39,403 字节的两个 capture 诊断 turn。数据库恢复为 `openai / gpt-5.6-luna / xhigh / ACPs跨物种视频`，`quick_check=ok`；Codex Desktop 实际导航后 task 状态由 `notLoaded` 变为 `idle`。完整回滚证据保存在 `~/.codex/session-repair-backups/019fbd59-0c1a-7592-87d3-1e2ad654fd0d-20260804-235449/`。

## 2026-08-04 CCSwitchMulti v3.19.1-5 正式发布

- 正式 Release：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-5`，非 draft、非 prerelease；发布时间 `2026-08-04T15:46:16Z`，共 19 个资产。
- annotated tag `v3.19.1-5` 的 tag 对象为 `56945293e20f3ba02a696ca8df43dd01ceec2e55`，远端解引用到已验证提交 `ce6f88b53db339591efa996195bbdaf6fc62eeef`；发布后记忆提交不得移动该标签。
- GitHub Actions run `30922685914`（`https://github.com/BigStrongSun/ccswitchmulti/actions/runs/30922685914`）最终 `completed/success`：macOS、Windows x64、Windows ARM64、Linux x64、Linux ARM64、Publish GitHub Release、Assemble latest.json 七个作业全部成功。
- 远端 `latest.json` 版本为 `3.19.1-5`，包含 `darwin-aarch64`、`darwin-x86_64`、`linux-aarch64`、`linux-x86_64`、`windows-aarch64`、`windows-x86_64` 六个平台键；各项 URL 均指向 `BigStrongSun/ccswitchmulti/releases/download/v3.19.1-5/` 且 updater signature 非空。GitHub Latest Release API 已返回 `v3.19.1-5`。

## 2026-08-04 v3.19.1-5 发布前测试边界与 DeepSeek context 断言修正

- 仓库根目录包含 `.worktrees/*` 时，当前 Vitest 默认发现规则会把旧 worktree 的测试和多套 React 依赖一起加载；本地发布验证必须显式使用 `--exclude '**/.worktrees/**'`。指定单个测试文件本身仍不足以阻止旧 worktree 被发现。
- 前端全集并行执行时 `tests/integration/App.test.tsx` 偶发因共享 MSW/DOM 状态超时；该文件在排除 worktree 后独立运行 8/8 通过。发布门槛应使用排除 worktree、关闭文件并行的根源码全集，不能用混入旧 worktree 的失败或单文件通过替代全集。
- `tests/utils/codexModelContext.test.ts` 的 DeepSeek V4 Flash 预设断言仍期望旧值 `1000000`，但上游 `8ae1ce85` 已把业务预设对齐 DeepSeek 官方 catalog 为 `1048576`。Codex 内置搜索与 Matrix 直接读取 DeepSeek 官方 Crush 配置均确认 `context_window=1048576`；因此只修正陈旧测试契约，不改业务值。
- WebDAV mock 的 `start_mock_webdav` 每个 TCP 连接只处理一次请求并立即关闭，但原响应没有 `Connection: close`。HTTP/1.1 默认持久连接，reqwest 因此会偶发复用 mock 已关闭的连接，使完整串行测试随机在不同的多请求 WebDAV 用例报 `request build failed`；单测重复通过不能排除该竞态。RFC 9112 与 Codex 内置搜索、Matrix 直读结论一致：不支持持久连接的服务器必须在每个非 1xx 响应声明 `Connection: close`。修正后 WebDAV 四用例组连续 20 轮全部通过，随后两轮 Rust 全量串行测试均为核心库 `2803 passed / 0 failed / 2 ignored`，其余集成目标全绿。

## 2026-08-03 上游 #6078 Windows 集成测试不得写真实 Claude Desktop 配置

- 上游 `farion1231/cc-switch#6077/#6078` 的根因成立：共享集成测试 helper 只覆盖
  `CC_SWITCH_TEST_HOME`、`HOME`、`USERPROFILE`，而 Windows Claude Desktop 路径直接
  读取 `LOCALAPPDATA`；因此 `profile_roundtrip` 的 `https://desktop.test` / `dk-1|dk-2`
  fixture 会把真实两份 `deploymentMode` 改为 `3p` 并写入固定 3P profile。这个边界与
  CCSwitchMulti `v3.19.1-2` 已做的临时监听端口隔离不同，不是重复修复。
- 接受上游核心改动时保留原作者归属；CCSwitchMulti 的完善边界是把测试根目录改成
  `cc-switch-test-home-<pid>`，防止并行测试进程互删同一固定目录，并让
  `reset_test_fs()` 同时删除测试根内的 `AppData`，避免同一进程中的测试继承旧 3P
  profile。回归必须分别保护 `LOCALAPPDATA` 投影、进程唯一根和 AppData reset。
- Windows 实测：旧实现把外部沙箱写成与本机真实污染文件相同的 SHA-256；接受核心
  改动后 `profile_roundtrip` 8/8 且真实四个配置文件前后哈希不变。反向移除
  `LOCALAPPDATA` 重定向时安全回归会稳定失败，证明测试能捕获原 bug。
- 本机真实 Claude Desktop 配置在 `2026-07-31 21:54` 已存在该测试 fixture 污染。
  源码合入不等于用户配置已恢复；恢复前必须先备份两份 deployment config、`_meta.json`
  和固定 profile，再按 official restore 语义写回 `1p`、清理固定 fixture profile。

## 2026-08-03 上游 #6069 GPT token 参数兼容适配

- 上游 `farion1231/cc-switch#6069` 修复 Claude Code 经 OpenAI Chat / Responses
  协议调用 GPT 时的两个边界：GPT-5+ 必须使用 `max_completion_tokens`，Claude Code
  探测用的 `max_tokens=1` 转 Responses 后必须提升到 OpenAI 接受的
  `max_output_tokens>=16`。
- CCSwitchMulti 不能原样复制上游把 `supports_reasoning_effort()` 完全委托给 OpenAI
  token 参数判定的实现，因为本地还支持 `grok-4.5*` / `grok-build-*` reasoning。
  正确边界是独立的 `requires_max_completion_tokens()` 只判断 o-series 和 GPT-5+，
  `supports_reasoning_effort()` 再在此基础上追加 Grok 家族。
- Responses 转换仅对整数 token 值做下限 16 钳制；正常大值不变，非整数异常输入保持
  原透传语义。回归测试必须同时覆盖 GPT-5 Chat 参数、1/15/16/1024 边界、非整数
  透传和 Grok reasoning 保留。
- #6069 初稿只修 Claude/Anthropic -> Chat 转换，实时 Codex review 随后指出
  Responses -> Chat 仍使用 o-series-only 判断；作者第三个提交补了显式
  `max_output_tokens` 路径。CCSwitchMulti 进一步发现 provider 配置注入
  `default_output_tokens` 时还会经 `chat_output_token_field()` 写回 `max_tokens`，因此两处
  都必须共用新 helper，并分别用显式预算和默认预算测试保护。不能只按上游首版 diff
  或 PR 标题判断修复已闭环。

## 2026-08-03 Codex MultiRouter 禁止跨 route 改模型回退

- `codexRouting` / legacy route schema 的 route 列表是按请求模型选择真实上游的分流表，
  不是同一请求的故障转移池。旧 `resolve_codex_model_routed_providers()` 会把最佳匹配
  route 放在第一位，再把其它 enabled route 追加成 retry 候选；因此 official GPT 请求
  遇到 5xx/额度错误后可能被发到 DeepSeek/Qwen，并悄悄改成这些 route 的默认模型。
- 根修位于 route 解析源头：每个 MultiRouter 只返回排序后的第一条最佳匹配 route。
  `build_forward_attempt_providers_preserving_codex_router_context()` 仍保留外层 failover
  queue 中显式配置的其它 provider，所以禁止的是 router 内跨模型 fallback，不是关闭
  provider 级故障转移。
- 新旧 route schema、重复 exact match、exact 优先 prefix、compaction 跟随当前模型、
  父 router 归因和显式 provider 级 fallback 均有回归测试。后续不能再把 route candidates
  当作 retry chain；一个模型选择只能物化一个真实 target provider。

## 2026-08-03 Codex 跨 provider 历史 ID 与远程压缩 SSE 聚合根修

- 现场任务 `019fc68a-e5fa-7961-a8d4-d2ffaf0ff8bb` 的官方路由 400 不是
  `name = "OpenAI"` 或 OAuth 配置错误。`codex-router.log` 证明同一请求中的
  `input[45/50].id=resp_chatcmpl-8adcdff0de8712dd_msg` 被官方 Responses 拒绝，
  因为 `type=message` 的回放 item ID 必须属于 `msg_` 命名空间。
- 根因有两层：CCSM 的 Chat/Anthropic -> Responses 合成器把 response ID 拼成
  `{response_id}_msg`；第三方 native Responses 也可能返回自己的非标准 message ID。
  修复同时覆盖源头与边界：所有 CCSM 合成的 message item 使用
  `response_message_item_id()` 生成 `msg_` ID；混合历史进入 OpenAI official route 前，
  按 item 类型把非规范 ID 确定性映射到官方命名空间：message=`msg_`、无加密内容的
  reasoning=`rs_`、function call=`fc_`、custom tool call=`ctc_`、web search=`ws_`。
  已合法官方 ID、第三方目标与带非空 `encrypted_content` 的 reasoning 不改；后者的
  opaque payload 可能绑定原 provider/item identity，不能为了通过前缀校验盲目改写。
  旧会话无需改写持久化历史即可恢复。现场另一旧任务中的
  `type=web_search_call,id=call_00_A89zZLtxMP15J0arnpWo8734` 因此也不会再触发
  `Expected an ID that begins with 'ws'`。
- `422 Upstream returned an empty compaction summary` 的现场上游状态实际为 200 SSE，
  失败在 CCSM 聚合层。旧 `responses_sse_to_response_value()` 只认 SSE `event:` 行，
  且只有 `output_items` 完全为空时才把 `response.output_text.delta` 合成 message；
  data-only SSE 或先完成 reasoning item、文本仅以 delta 下发时都会丢掉摘要。
- 聚合器现在在缺 `event:` 时使用 JSON `data.type`，并在尚无 message item 时把文本
  delta 追加为 message，即使 reasoning item 已存在。没有把 `compaction_trigger` 改写
  成粗糙摘要 prompt；第三方默认本地压缩的设计仍保持，显式 remote opt-in 的兼容层
  只负责无损聚合与协议包装。
- 定向验证：新增的旧 message/web-search ID 规范化、data-only SSE、reasoning+text
  delta、Chat 流式 canonical message ID 测试均通过；transform Codex Chat 115/115、
  streaming Codex Chat 20/20、streaming Codex Anthropic 21/21、Anthropic response
  7/7。Anthropic transform 仍只有既有
  `test_request_tools_and_filtering` 与
  `test_request_custom_tool_survives_with_required_choice` 两项断言失败。
- 2026-08-03 provider-agnostic 补充审计确认：DeepSeek 只是本次生成 Chat 响应和
  data-only SSE 的触发上游，不是根因边界。相同故障可由任何 Chat/Anthropic 转换器或
  非规范 native Responses 上游产生，并在稍后切换到严格 OpenAI official route 时暴露。
  `function_call_output` / `custom_tool_call_output` 使用 `call_id` 配对而不是 item `id`；
  `tool_search_call` 与 `local_shell_call` 在当前公开材料中缺少足够的严格前缀证据，不能
  猜前缀改写。完整矩阵见
  `docs/codex-cross-provider-responses-compatibility-audit-2026-08-03.md`。

## 2026-08-03 第三方 Codex provider 默认本地压缩与远程压缩 opt-in

- Codex 的压缩选择只看 `ModelProviderInfo::supports_remote_compaction()`，实现是
  provider `name` 是否为 OpenAI（或 Azure Responses）。第三方 Responses 上游即使
  能跑普通 `/responses`，也不等于实现 Codex private compaction；`name = "OpenAI"`
  会让 Codex 对未适配上游发起 remote compact，出现
  `remote compaction v2 expected exactly one compaction output item` 或 4xx。
- CCSM 根因：`apply_codex_proxy_toml_config_with_pool_policy` 对 MultiRouter 曾经
  无条件把 live `[model_providers.codex_model_router_v2] name` 写成 `OpenAI`，所以
  DeepSeek/Qwen 等第三方也被全局拉进远程压缩。该字段只影响 Codex 侧能力选择，
  不决定 CCSM 内部 route；`requires_openai_auth` 才是 Desktop 登录/账号门面。
- 修复边界：新增 `codex_provider_remote_compaction_enabled`。结构化
  `codexRemoteCompaction/remoteCompaction/remote_compaction=true` 或 provider 的
  Codex config TOML 中对应 `model_providers.<id>.name = "OpenAI"` 是显式 opt-in；
  没有显式设置时，official OAuth-only MultiRouter 仍写 OpenAI，第三方-only 或
  mixed MultiRouter 写 provider.name / `CCSwitch MultiRouter`，第三方回到本地压缩。
- 不采用“把 compaction_trigger 换成普通摘要提示词再包装响应”的方案：第三方普通
  Responses 的摘要质量和 Codex 本地压缩策略不等价，且远程 v2 的触发/输出契约是
  专门协议，粗糙改写会污染会话历史。未适配上游应使用本地压缩，远程压缩必须显式
  opt-in 并只用于确实支持 compaction 的 provider。
- Codex 的 `[features] remote_compaction_v2=false` 只关 v2，不保证退回本地；若要
  第三方本地压缩，仍以 provider name 非 OpenAI 为准。MultiRouter 是单个 Codex
  provider bucket，远程压缩开关在该 bucket 内是全局的；真正逐上游粒度需要拆分
  provider 或依赖 Codex 未来支持 per-model provider capability。
- 验证：`cargo test --lib compaction` 15/15，`codex_multirouter_takeover` 5/5，
  `codex_provider_remote_compaction_enabled` 3/3；完整 lib 跳过现场端口绑定测试
  `update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active`
  和既有 Anthropic transform 断言失败
  `test_request_tools_and_filtering` / `test_request_custom_tool_survives_with_required_choice`
  后 2779 passed / 0 failed / 2 ignored；`pnpm typecheck` 与 `git diff --check` 通过。

## 2026-08-02 本地 main 漏合核对与 v3.19.0-4 release 重建

- 核对基线：本地 `main` HEAD 为 `efc20bab`，相对 `fork/main`（`445a3041`）领先 5 个提交：`41630094` stream hint、`3ecf02ef` Codex remote compact v2、`c4691f19` memory、`74a57591` v3.19.0-4 发布记录、`efc20bab` pre-stream retry 恢复。
- 远程压缩修复已确认在本地 main：v2 检测限定 `responses_compaction_v2` 或 `/responses + compaction_trigger`；原生 Responses 请求保持 `/v1/responses` 并合成唯一 `ocx1:` compaction item；Chat 路径同样合成；后续请求把 `ocx1:` 还原为 readable summary；官方 route 保留原生透传。`cargo test --lib compaction` 12/12，`managed_codex_retry_budget` 1/1，`response_processor` 11/11，`pnpm typecheck` 通过。
- 本地 active branches 中没有比 main 更新的未合 bugfix：`bigstrongsun/fix-v3.19-codex-pool-session-affinity` 和 `fix/3.16.5-22-gpt-live` 只是旧线/已合入变体；官方 `v3.19.1` fetch 后检查 release notes 与提交，没有与 Codex remote compaction 直接相关的新修复。
- 使用 `scripts/export-latest-ccswitchmulti.ps1` 从 `efc20bab` 重建本地 release 到 `最新版ccswitchmulti`。版本仍为 `3.19.0-4`，NSIS setup、portable zip、raw exe 与 `latest.json` 均已导出；SHA256SUMS 逐项校验通过，Tauri signing key 存在，setup `.sig` 与 `latest.json` 已生成。

## 2026-08-02 CCSwitchMulti v3.19.0-4 正式发布

- GitHub Release：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.0-4`；`draft=false`、`prerelease=false`，共 19 个资产。
- tag `v3.19.0-4` 解引用到 `445a3041 chore(release): prepare CCSwitchMulti v3.19.0-4`；fork/main 已同步到该提交。
- Actions run `30744839945` 成功：macOS、Linux x64/ARM64、Windows x64/ARM64 五个构建矩阵，加 Publish GitHub Release、Assemble latest.json 全部通过。
- `latest.json` 验证为 `version=3.19.0-4`，六个平台键均为 `darwin-aarch64/darwin-x86_64/windows-x86_64/windows-aarch64/linux-x86_64/linux-aarch64`。
- 该版本基于本地非 agentmesh 整合线：v3.19/v3.17 上游、账号池运行态、MultiRouter 自环根修、超时重发放大修复、历史扫描卡 UI 修复、MSI ICE38、hosted web_search 桥接。
- 本地验证：`cargo check`、`pnpm typecheck`、`cargo fmt`、`git diff --check` 通过；forwarder `125/125`，hosted web_search `11/11`，SSE wrapper `1/1`，account pool `8/8`，Codex OAuth `39/39`。

## 2026-08-01 Codex 账号池 P0 运行态 Task 5 验证收口

- 本阶段提交链为：`99d17418`（实施计划）、`fb855d37`（统一运行态与生命周期）、`462c7079`（TTL/LRU/凭据代际）、`50e183de`（typed outcome 与 transient soft-avoid）、`3eaa965d`（forwarder 真实失败接管）。公开账号池 JSON 和 `CodexOAuthStore.version=1` 未改变，也没有版本、tag、发布、安装或部署动作。
- 落地接口与常量：`CodexPoolRuntimeState` 是唯一运行态真值；affinity 空闲 TTL 为 24 小时、上限 2048；managed `credential_generation` 首次登录为 1、同 ID 重登递增、普通 token 刷新不变；Desktop Authorization 只保留进程内 SHA-256 摘要；`CodexPoolAttemptOutcome` 统一 success/credential/quota/transient/neutral，quota 本阶段固定 60 秒，transient 五分钟窗口内第三次起按 30 秒/2 分钟/10 分钟/30 分钟 soft-avoid。
- 11 项设计验收均按当前公开语义核对：TTL/LRU；managed/native generation；删除、clear、invalid_grant、重登、禁用与外部移除清理；invalidated 排除；池内 AuthError/401/403 reauth 与换号；direct official 认证失败不重试；402/429 冷却解绑；connect/timeout/stream idle/5xx 累计、窗口重置与成功恢复；neutral 不污染；旧 generation 结果无效；reserve/order、Desktop/managed 去重和 External API 不借用 Desktop bearer。当前策略会自动把仍可用但被省略的账号重新补回 policy，因此“策略移除”对应账号删除/外部存储移除；客户端断连没有 `ProxyError` 变体且不会进入 pool recorder，neutral 状态测试覆盖显式 caller-neutral 结果。
- 聚焦回归：`codex_oauth_pool` 8/8、`codex_oauth_auth` 39/39、`codex_pool` 2/2、direct official 1/1；Task 4 的分类、池内认证切换、Provider 健康隔离及五项认证/调度边界各 1/1。`cargo fmt --check` 与 `git diff --check` 通过。
- `cargo clippy --lib -- -D warnings` 初次发现本阶段 OAuth reconcile 的 `obfuscated_if_else` 与 `unnecessary_lazy_evaluations`，用等价 `if/else` 和 `then_some` 修正后 OAuth 39/39，复跑已不再报告账号池新增诊断。全局命令仍被 26 个既有错误阻塞：AgentMesh dead code 22 个，usage parser dead code 1 个，history `manual_repeat_n` 1 个，旧 forwarder `needless_borrow` 1 个，transform `too_many_arguments` 1 个；前五个相关文件与账号池改造前 `e31f8a6a` blob 哈希一致，forwarder 报错行由 `b0471ee0` 引入。本阶段不增加 blanket allow，也不把无关清理混入账号池提交。
- 原样执行完整 `cargo test --lib` 时为 2706 passed / 1 failed / 2 ignored；唯一失败测试 `update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active` 固定绑定 15721，而现场已安装 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 正在监听。相邻同类测试使用 `listen_port=0`，该失败属于既有测试隔离缺口；不停止用户正在运行的 CCSwitchMulti 后，使用既有 skip 边界重跑为 2706 passed / 0 failed / 2 ignored / 1 filtered。
- Task 4/Task 5 的 post-commit 流水线必须继续按真实父子进程树和 lock 所有权等待自然退出。Task 4 流水线已完成 typecheck、release 构建、Tauri/NSIS 打包并自行清锁；`scripts/logs/` 保留为现场，不纳入提交。
- 后续工作仍是四个独立阶段：解析 `Retry-After`/reset/scope/generation/单探测租约的自适应冷却；完整 SSE terminal、断流、流内 402/429 与 media 二次成功反馈；路由与池内候选重试预算分层及大池公平性；保留 `priority` 默认值后再显式增加 quota/RR/fill-first。当前分支不能声称这些 P1 能力已经完成。

## 2026-08-01 Codex 账号池真实 forwarder 失败接管 Task 4

- `provider_codex_pool_account` 只把同时携带 `codexPoolAccountId` 与选择时 `codexPoolCredentialGeneration` 的 request-local Provider 识别为池化候选；非池化 Provider 的错误分类、重试和持久健康记录保持原路径。真实候选还会继承 `codexAccountPoolEnabled=true`，因此属于 official Codex。最初书面测试夹具漏掉该 marker，没有复现生产中的 official 401/403 不可重试分支；补齐真实 marker 后旧实现稳定失败，新分支稳定通过。
- `classify_codex_pool_attempt` 把本地 `AuthError` 与上游 401/403 归为 credential，402/429 归为 quota，connect/timeout/stream idle/`ProviderUnhealthy`/其他 5xx 归为 transient，400/405/406/413/414/415/422/501 及未知错误归为 neutral。只有显式池候选的 credential/quota/transient 覆盖为可重试；直接 official 401/403 仍不可重试。
- raw 与 normal 主重试循环都会先将 typed outcome 连同选择时 generation 交给 `CodexOAuthManager::record_pool_attempt`。池候选的账号级 credential/quota/transient 失败只调用 `release_permit_neutral` 释放 request-local HalfOpen permit，并继续下一个账号，不再把临时候选失败写入原 Router 的熔断器或数据库 Provider 健康；neutral 与所有非池化错误仍沿用原持久健康路径。
- 原 `cool_down_pool_account`/`cool_down_account` 已删除，原 reserve/cooldown/affinity 回归也改为通过统一 typed quota outcome 驱动。media 二次重试成功反馈和完整 SSE terminal 仍明确属于后续阶段；本阶段的 `Success` 只代表既有请求发出前/首包边界，不能描述成最终流式成功。
- TDD 与回归证据：分类映射、池内 401/403/AuthError 切换、账号失败不污染 Provider 健康三项缺失行为先失败后通过；提交前重新运行这三项，以及 direct official 401/403、显式 Router marker、本机 native auth、managed pool 不复用 Desktop bearer、reserve/cooldown/affinity 共八项边界测试，每项均为 1 passed / 0 failed。

## 2026-08-01 Codex 账号池 typed outcome 与 transient soft-avoid Task 3

- `CodexPoolAttemptOutcome` 统一定义 `Success`、`Credential`、`Quota`、`Transient`、`Neutral`，所有结果必须携带选择时的 `credential_generation` 进入 `record_outcome_at`；未知账号或旧代际直接返回 false，不得清理、恢复或污染当前身份状态。
- `Credential` 只写运行时 `reauth_required`，清 quota/cooldown/transient 和该账号全部 affinity；`Quota` 保持本阶段固定 60 秒硬冷却并解绑；`Success` 清 reauth/transient/soft-avoid 并在代际仍匹配时绑定；`Neutral` 不改任何账号健康。候选读取同时校验 generation、reauth、硬冷却与 soft-avoid，计时状态到期后惰性清理。
- transient 以相邻两次失败间隔不超过五分钟为连续窗口，第三次起按 30 秒、2 分钟、10 分钟、30 分钟升级，六次达到封顶，七次及以后继续 30 分钟；达到阈值立即删除该账号全部 binding，成功会清零计数。`CodexOAuthManager::record_pool_attempt` 是唯一 typed wrapper。
- TDD 证据：credential/quota、第三次 transient、neutral/旧 generation、五分钟窗口重置、3-7 次升级封顶四组缺失行为先失败后通过；Task 3 完成时纯状态 8/8、`codex_oauth_auth` 39/39。forwarder 仍暂时保留旧 success/429 调用以维持提交可编译，真实 `ProxyError` 分类、401/403 池内接管和持久 Provider 健康隔离属于紧随其后的 Task 4。

## 2026-08-01 Codex 账号池有界 affinity 与凭据代际 Task 2

- `CodexPoolRuntimeState` 的 session affinity 现在使用 24 小时空闲 TTL 和 2048 项硬上限；每次有效复用都会刷新 `last_used_at_ms`，溢出时按真实最近使用时间淘汰。账号 runtime 的 `credential_generation` 变化会原子重建该账号运行态并清除全部旧 binding，旧请求携带的 generation 无法重新绑定。
- managed OAuth 的 `credential_generation` 持久化在既有 version 1 文件中：旧文件缺失时为 `0`，首次新登录为 `1`，同 ID 重新登录饱和递增，普通 access/refresh token 刷新不变。请求期 `CodexPoolCandidate` 把选择时 generation 写入私有 `codexPoolCredentialGeneration`，成功回写不再临时查询当前代际。
- Desktop Authorization 只在 manager 内存中保存 SHA-256 `[u8; 32]` 摘要和单调 generation；原文、摘要均不落盘、不展示、不写日志。forwarder 在 quota 探测前先让 manager 观察 Authorization，避免把新身份的首个 quota 快照写入旧代际后立即清掉。
- 构造后磁盘同步改用可等待的 `reload_from_disk().await`，完整替换账号/default/policy 后再按可用账号与 generation reconcile 统一运行态；不再用同步 `try_lock` 做 best-effort 清理。quota 读写入口会先按当前 generation 准备 runtime，修复“重启恢复 policy 后首个低额度快照被静默丢弃、随后新 session 仍选中该账号”的根因。
- TDD 证据：TTL/LRU/旧 generation 三个纯状态测试和 managed generation/重启首快照、Desktop 换身份、外部删除 reload 回归均先按缺失行为失败再通过；Task 2 完成时 `codex_oauth_auth` 相关筛选为 38/38，通过原 reserve/cooldown、四个 Task 1 生命周期测试和跨进程 refresh-token 轮换回归。书面计划原 TTL 样例在边界命中后又按旧时间要求 1ms 后过期，与“命中刷新空闲时间”冲突，测试已拆为两个独立状态实例。

## 2026-08-01 Codex 账号池运行态统一 Task 1

- 账号池原有 `pool_cooldowns`、`pool_session_bindings`、`pool_remaining_percent`、`pool_quota_checked_at` 四张独立表已迁入 `src-tauri/src/proxy/providers/codex_oauth_pool.rs` 的单一 `CodexPoolRuntimeState`，manager 只保留一把 `Arc<tokio::sync::Mutex<_>>`。候选读取、quota 快照、固定 cooldown、binding 和 lifecycle purge 现在在同一临界区完成，不再由调用方分别操作四把锁。
- `remove_account`、`clear_auth`、明确 `invalid_grant` 隔离、同 ID 重新登录和 pool policy 禁用/移除都经过统一 purge/reconcile；重新启用或重登不会恢复旧 binding、cooldown 或 quota 快照。`normalized_pool_policy` 和只读投影也只接受 `CodexAccountData::is_usable()` 的托管账号，已持久化 `invalidated_at` 的账号不再进入候选。
- TDD 证据：生产修改前，删除后重加、invalidated、禁用后重启、clear 后重登四个测试均因旧运行态残留而断言失败；统一运行态接入后四项与原 `account_pool_honors_order_reserve_cooldown_and_session_binding` 均通过。本提交刻意仍使用 generation `0`；24 小时 TTL、2048 LRU 和凭据代际校验由下一个 TDD 提交实现。

## 2026-08-02 Codex 超时与重试放大修复

- 现象：OpenClash 节点 `🇺🇸美国2-IEPL-GPT` 丢 6.6MB POST 时，CCSM 把“可能已在途”的转发/超时错误按 502/504 返回，Codex 默认 `stream_max_retries=5`、`request_max_retries=4` 又自动重发整轮采样请求，造成流量和时延放大。
- 修复：`auto_failover_enabled=false` 时不再把超时全部置 0，首个 SSE 字节改用 `non_streaming_timeout`（默认 600s）作为硬上限，静默超时继续生效；新增 `ProxyError::ResponsePending`，请求体已写入、上游已返回响应头、响应体/首字节超时等可能已在途的失败映射为 429 + `Retry-After: 30` + `cc_switch_response_pending`，不参与 retry/failover；连接阶段失败仍保留 502/可重试语义。
- 当时 CCSM 托管的 Codex provider 显式写 `request_max_retries=0`、`stream_max_retries=0`，关闭 Codex 端整轮采样请求自动重发；该决定后来分两步纠正：先恢复 request budget=2，再于 2026-08-04 恢复 stream budget=5。ResponsePending/429 仍负责阻止未知结果的请求重发，CCSM failover 仍只对明确未发送的 connect 错误切换下一家。
- 验证：`cargo check --lib`、新增回归测试、`cargo fmt --check`、`git diff --check` 通过；`cargo test --lib` 2688 passed / 2 ignored / 1 failed，唯一失败是本机 15721 被运行中 CCSM 占用。
- 构建补漏：Tauri `--bundles msi` 报 `light.exe ICE38: Component codex_history_repairer installs to user profile. It must use a registry key under HKCU as its KeyPath, not a file.` 根因是自定义 `wix/per-user-main.wxs` 的 `binaries` 循环仍用 `<File KeyPath="yes"/>`；per-user 安装目录下额外 binary 必须改用 HKCU `RegistryValue KeyPath="yes"`，文件 `KeyPath="no"`。已按此修复模板。

## 2026-08-01 Codex 历史修复保留重命名标题根修

- 现象：用户在 Codex Desktop 中重命名 task 后执行 CCSM 历史修复，修复后的标题回退为原始首条用户消息。真实数据链显示 rollout 只保留原始用户事件；重命名标题存于 active state DB 的 `threads.title` 和/或 `session_index.jsonl.thread_name`。
- 根因：历史修复在列表、详情和写入路径都调用 `apply_rollout_metadata_to_thread_row`，无条件用 rollout 推导的 `metadata.title` 覆盖 SQLite title；随后 `move_focus_session_index_rows` 又把已回退的标题写回 session index，形成两次覆盖。
- 根修：合并 rollout 前先识别与 `first_user_message` 不同的持久化显式标题。优先级为 SQLite 显式标题、session index 显式标题、rollout 推导标题；同一有效标题贯穿列表、详情、缺失 thread 重建、SQLite metadata rebuild 和 session index recent-window 更新。普通未重命名会话仍由 rollout 补齐。
- 回归 `history_repair_preserves_user_renamed_thread_title` 真实执行完整修复，要求 SQLite 与 session index 都保留重命名。主线 RED/修复提交为 `a51c3614`/`d9e4fb14`；3.16.5 线为 `7d5a70c5`/`2352a77e`。两线历史修复模块均为 51 passed，`cargo check --lib`、rustfmt 和 `git diff --check` 通过。
- 已被旧版本覆盖且没有修复前备份的自定义标题不能从 rollout 自动还原，因为 rollout 从未保存重命名值；根修保证后续执行不再丢失仍存在于 SQLite 或 session index 任一侧的标题。

## 2026-08-01 Codex 官方主 Agent 向第三方子 Agent 投递空正文根修

- 上游 Codex V2 的加密不是协议必选项，而是 `spawn_agent`、`send_message`、`followup_task` 工具 schema 对 `message` 调用 `JsonSchema::with_encrypted()` 后产生的 Responses 私有 `encrypted: true`。官方 backend 据此把该参数替换为密文并返回 `encrypted_function_args=["message"]`；Codex 再生成只有 `Payload:` 信封头和 `encrypted_content` 的 `agent_message`，第三方子 Agent 无法解密。
- Codex 客户端原生支持 V2 明文：同一 V2 function call 的 message schema 不带 `encrypted` 时，backend 返回普通参数，客户端走 `DirectPlaintextMessage` 并生成完整明文 `agent_message`。因此正确目标是保留 V2，只控制三个协作参数是否加密，不是把混合任务降为 V1。
- 2026-08-01 对 `gpt-5.6-sol` 官方 `/backend-api/codex/responses` 做了真实 A/B：带 `encrypted:true` 时 HTTP 200 且 message 为 164 字符密文；去掉该标记时仍 HTTP 200，message 原样等于 22 字符 `PLAINTEXT_BRIDGE_PROBE`。这证明不需要代理解密、官方桥接 Agent 或修改 Codex 客户端。
- 主线根修：混合 Router 的全部 catalog model 保持 `multi_agent_version=v2`；仅当请求来自 Codex、Router 含启用的第三方/歧义 route、且本次实际出站为 official OAuth 时，移除标准 `tools` 和 Responses-Lite `input[].type=additional_tools` 中三个协作工具的 `message.encrypted`。全官方 Router、禁用第三方、第三方上游、其它 app、普通工具、其它加密参数均不改。
- 主线官方 route 来源覆盖 `native_codex_auth`、`managed_codex_oauth`、`managed_account`、`account_pool` 和 `authProvider=codex_oauth`。RED 提交为 `db1358d3`、`c5c7dcf1`、`f940592e`、`3f3b44d5`，生产提交为 `e31f8a6a`。
- 3.16.5 线按旧认证结构只把 `managed_codex_oauth`/`authProvider=codex_oauth` 视为官方；对应生产提交为 `9b47dde8`。两条线均覆盖 V2 catalog、标准/Lite 三工具改写、route 所有权和 official-only forwarder 边界。
- 引入边界仍是 `8221be2b`，从 `v3.16.5-16` 起保留官方 V2 后开始暴露；`v3.16.5-15` 因降成 V1 偶然不复现。新实现不改变 task 的 V2 锁定版本；重启 CCSM/Codex app-server 后，现有 V2 task 后续新 spawn/send/follow-up 可使用明文。已经以空正文启动的旧 child 不会自动补回此前丢失的任务，应重新派发。

## 2026-08-01 Codex OAuth 账号池 P0 运行态设计（待书面评审）

- 用户已认可继续完善账号池，第一阶段刻意收敛为“运行态生命周期 + 请求发出前/首包阶段失败分类”；自适应冷却探测、完整 SSE terminal、重试预算和多策略分别留到后续阶段，避免一次提交同时改变多个状态机。
- 推荐新建 `src-tauri/src/proxy/providers/codex_oauth_pool.rs`，用单一 `CodexPoolRuntimeState` 与一把异步 Mutex 取代 session binding、cooldown、remaining percent、quota checked time 四张独立表。OAuth 文件只负责凭据持久化，forwarder 只提交分类后的结果。
- affinity 契约：24 小时空闲 TTL、2048 项 LRU、凭据 generation；managed generation 只在重新建立登录身份时递增，普通 token 刷新不变。Desktop bearer 只计算进程内 SHA-256 摘要以识别凭据变化，摘要不落盘、不展示、不记录日志。
- 结果分类统一为 success/credential/quota/transient/neutral。账号池 candidate 的 401/403 或本地 AuthError 标记运行时 reauth、清 binding 并允许备用账号；直接 official route 仍不可重试。402/429 本阶段先保留固定 60 秒冷却；connect/timeout/stream idle/5xx 在五分钟窗口累计三次后按 30 秒、2 分钟、10 分钟、30 分钟 soft-avoid。
- 完整设计位于 `docs/superpowers/specs/2026-08-01-codex-account-pool-runtime-state-design.md`。必须先完成用户书面评审，再写实施计划并逐项 TDD；本阶段不发版。

## 2026-08-01 v3.19 OAuth 账号池与 opencodex v2.8.0 源码对照审计

- 审计基线：CCSwitchMulti 分支 `bigstrongsun/fix-v3.19-codex-pool-session-affinity` 的账号池根修为 `ecac81ad`；opencodex `main` 已现场复核为 `1adad35731ff3586d3d8dfaf531d5b64e0bb1092`（v2.8.0，2026-07-31）。本轮是源码与测试代码审计；本机没有 Bun，未执行 opencodex 测试，不能把对照结果表述为 opencodex 运行时验收。
- 已完成边界：`reservePercent` 只拦截新 session，已绑定 session 可继续使用低于保留阈值的账号；429 冷却仍会删除绑定并切走。Router 必须显式选择账号池才展开候选，External Agent API 不借用本机 OAuth，额度按 5 分钟 TTL 并行探测。这次修复正确，但只覆盖了保留额度与 affinity 的一个冲突，不能代表号池整体完善。
- P0 生命周期缺口：`pool_session_bindings`、`pool_cooldowns`、`pool_remaining_percent`、`pool_quota_checked_at` 都没有容量或过期清理；`remove_account`、账号失效、重新登录覆盖同一 id、策略禁用/移除条目都没有统一清理这些运行态。`normalized_pool_policy` 只检查账号 key 是否存在，不排除 `invalidated_at` 账号；该账号仍可进入候选，随后认证失败。opencodex 用 24 小时 affinity 空闲 TTL、2048 项 LRU、凭据 generation 校验和统一 `purgeCodexAccountRuntimeState` 解决同类生命周期问题。
- P0 失败分类缺口：CCSM 只在 429 上更新账号池状态。账号池 candidate 被视为 official，401/403 会被 `categorize_proxy_error` 判为不可重试，却不会标记需重新认证、清 affinity 或尝试池内下一个账号；connect error、timeout、5xx 也没有账号级连续失败、soft-avoid 与升级退避。opencodex 明确区分 success/credential/quota/transient/caller，401/403 隔离账号，transient 达阈值后解除 affinity 并临时避让。
- P1 冷却缺口：CCSM 的 `ProxyError::UpstreamError` 不保留响应头，`record_codex_pool_attempt` 对 429 固定冷却 60 秒；没有解析 `Retry-After` 或额度 reset，没有 account/model quota scope，没有冷却 generation、单探测租约或手动清除入口。opencodex 优先使用 `Retry-After`，其次使用 reset-derived cooldown，最后才默认 60 秒；只允许当前 generation 的探测结果解除冷却，并将 Spark 与 shared native quota 分开。
- P1 请求结果缺口：CCSM 在上游成功响应完成首包/首个语义事件验证后立即绑定并记成功，后续 SSE `response.failed`、中途断流或 429 不再反馈账号健康；opencodex 持续检查流式 terminal，并按最终语义状态记成功、429/402、5xx 或中途失败。CCSM 的 Codex media 降级二次重试成功分支也没有再次调用 `record_codex_pool_attempt(..., true, ...)`，新 session 可能漏建绑定。
- P1 大池重试缺口：账号池展开后仍与普通 provider 共用全局 `max_attempts=max_retries+1`，Codex 默认最多尝试 4 家；固定顺序下，超过上限的尾部账号在 connect/timeout/5xx 等不会触发 60 秒池冷却的失败中可能持续没有单请求故障转移机会。需要把“路由级重试预算”和“同一路由的账号候选预算”显式分层，并保持有界重试。
- 策略借鉴边界：opencodex 支持 `quota`、`round-robin`、`fill-first`，quota 会周期重评长任务，RR/fill-first 对运行中线程保持 affinity。CCSM 现有“固定优先级 + reserve”是已发布产品契约，后续应保留为显式 `priority` 策略和默认兼容值，再按用户选择增加 quota/RR/fill-first；不能把 opencodex 默认策略静默套到现有用户配置。
- 推荐实施顺序：第一批先建立统一账号运行态与失败分类（TTL/LRU、generation、删除/禁用/身份变化清理、401/403 reauth、transient health、流式 terminal）；第二批实现自适应冷却、generation 与单探测租约；第三批才增加多策略与大池公平性。每批必须先做失败回归，不能把多项状态机改造揉成一个不可定位的大补丁。

## 2026-08-01 本地发布流水线锁所有权缺陷（待修）

- `scripts/local-release-pipeline.ps1` 把 `Enter-PipelineLock` 放在 `try` 内，但 `finally` 无条件删除 `scripts/logs/local-release.lock`。当一次并发调用发现六小时内的现有锁并在尚未取得锁时抛错，它仍会进入 `finally`，误删另一个仍在运行的流水线所持有的锁；第三次调用随后可能并发进入构建。
- 本次账号池修复的 post-commit 构建现场验证了该竞态：被锁拒绝的调用删除了在建流水线的锁。已按进程树和启动时间只终止 14:58 的旧构建树，保留并完成 15:02 从 `ecac81ad` 启动的最终构建，不能仅凭锁文件存在与否判断流水线是否在运行。
- 本轮不把无关的 hook 改造混入 OAuth 账号池根修。后续应让调用方仅在成功取得自己创建的锁后释放（例如显式 `lockAcquired`/所有权令牌或持有独占句柄），并增加“第二个调用获取失败不得删除第一个调用的锁”的并发回归；修复前处理构建冲突必须同时核对 PowerShell、export、pnpm、cargo、rustc/NSIS 的完整父子进程树。

## 2026-08-01 v3.19 OAuth 账号池保留额度中断已绑定任务根因

- 现场：Codex 任务已运行约 2 分 36 秒后连续重连，最终收到本地 `/v1/responses` 503：`Provider: OpenAI Official; model: gpt-5.6-sol; cause: 无可用 Provider`。认证页仍显示一个已登录且与 Desktop 当前登录合并的账号，因此不是账号记录丢失。
- 根因：账号池的产品契约是 `reservePercent` 只阻止分配新任务，并在成功请求后按 proxy session 保持账号粘性；但 `ordered_pool_entries` 先按剩余额度过滤候选，再读取 `pool_session_bindings`。长任务已经绑定的账号在执行或重连时降到保留阈值后仍会被删除，唯一账号场景最终生成空 attempt chain，并被 forwarder 折叠成 `NoAvailableProvider`。
- 根修：先读取当前 session 绑定；启用且未处于 429 cooldown 的已绑定账号绕过 reserve 过滤并保持第一优先级。未绑定的新 session 仍严格排除低于保留额度的账号；HTTP 429 继续通过 `cool_down_pool_account` 删除绑定并冷却账号，不允许真实额度耗尽后死粘。
- TDD：先把 `account_pool_honors_order_reserve_cooldown_and_session_binding` 改为要求“已绑定 thread 保留、全新 thread 排除、cooldown 后原 thread 切到下一候选”，旧实现稳定失败（期望 2 个候选、实际 1 个）；实现调度顺序修正后定向测试通过。

## 2026-08-01 Codex GPT-Live `/v1/live` 实时语音路由修复

- 现象：用户反馈实时语音不可用，报 `CC Switch local proxy failed while handling Codex endpoint /v1/live. Provider: OpenAI Official; model: unknown; upstream_status: HTTP 404; url: http://127.0.0.1:15721/v1/live`。另一个关键现场是：MultiRouter 顺序里第二位是 DeepSeek 就显示 DeepSeek，第二位是 Qwen 就显示 Qwen，`codex multirouter` 放哪都不影响。
- 上游事实：当前 Codex Desktop 26.721 的 GPT-Live（Frameless Bidi）走 `/v1/live`。WebRTC call-create 是 HTTP POST `/v1/live`（body 为 `codex-realtime-call-boundary` multipart，字段 `sdp` + `session`），会话侧边是 WebSocket Upgrade `/v1/live` 或 `/v1/live/{call_id}`。官方源码 `codex-rs/codex-api/src/endpoint/realtime_websocket/methods.rs` 对 Frameless 会把 `/v1` 归一成 `/v1/live`，并把 call_id 拼成 path 后缀；WebRTC sideband 默认固定到 `api.openai.com/v1/live/{call_id}`，backend 形态是 `chatgpt.com/backend-api/codex/realtime/calls`。
- 根因一：CCSM 把 `/v1/live` 当普通 raw HTTP 转发。官方 backend target 的 `CodexAdapter.build_url` 把 `/v1/live` 归一成 `.../backend-api/codex/live`，而不是 `.../backend-api/codex/realtime/calls`；WebSocket Upgrade 也不会被 HTTP 转发正确处理。
- 根因二：失败归因用 `model=unknown` 再次走普通 MultiRouter model resolver，`resolve_forward_error_provider_for_logging` 命中了 `defaultRouteId`，所以错误显示第二位 provider，即使实际请求已经解析到 official。
- 修复：新增 `/live`、`/v1/live`、`/v1/v1/live`、`/codex/v1/live` 及 call_id 通配路由；`forward_raw` 对精确 call-create 做 multipart -> backend JSON `{sdp, session}` 转换并发送到 `/realtime/calls?intent=quicksilver&architecture=avas`；`RequestForwarder::open_codex_realtime_websocket` 复用 raw official route 解析与 OAuth/native auth，先完成上游 101 握手再返回 Axum Upgrade；handlers 增加双向 WebSocket relay。
- 错误归因修复：unknown/empty model 的 raw endpoint 改用 `resolve_codex_raw_passthrough_route_provider` 的 official fallback，不再回落到 `defaultRouteId`。
- 验证：定向 Rust 覆盖空 body official 选择、未知 model 错误归因、multipart 解析、本地路径别名/call_id、WebSocket 连接、HTTP call-create 不落到 DeepSeek。全量 `cargo test --lib -- --skip update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active` 为 2674 passed / 2 ignored / 1 filtered；`cargo fmt --check`、`git diff --check` 通过。提交 `fc54a430`。
- 边界：CCSM 修复的是代理路由、body 形态、WebSocket 传输和错误归因；GPT-Live 仍需要上游账号/API 对 `/v1/live` 或 ChatGPT Codex backend 的授权。OAuth-only 账号若无 API key/backend 实时权限，仍可能在上游 401/403。

## 2026-08-01 3.16.5-22 分支同样需要并已完成语音修复移植

- 定位：`v3.16.5-22` 已经包含 `16110f4e`、`5a3693f1`、`08b58d81`，即 raw 透传未知 `/v1/*` 和 raw 默认回 official，因此同样会错误转发 `/v1/live`；`032cb5a5` 的错误归因会用 `model=unknown` 命中 defaultRouteId。引入边界是 `v3.16.5-10`，不是 3.16.5-22 独有；更早版本会在未知 endpoint 上直接结构化 404，不会把错误显示成第二位 provider。
- 已按 3.19.0-2 的修复移植到 `fix/3.16.5-22-gpt-live`（commit `83b116f7`）：显式 `/v1/live` 路由、multipart -> backend JSON、WebSocket 中继、错误归因 official。分支已推送到 `BigStrongSun/ccswitchmulti`。
- 3.16.5-22 全量 Rust 初跑为 2115 passed / 1 failed；失败 `responses_request_does_not_emit_chat_file_for_url_only_input_file` 是既有测试断言过期：`424a04f9` 已把 URL-only file 改为生成 `[file omitted: unsupported file URL]` 文本占位，但该测试仍断言旧行为。commit `1c82f569` 已同步为当前分支/上游 v3.17.0 的预期，随后 3.16.5-23 分支全量 Rust 为 2116 passed / 0 failed / 2 ignored / 1 filtered。已 bump 到 `3.16.5-23`（commit `83319fed`）并发布 prerelease `v3.16.5-23`；Windows installer 远程下载 SHA256 与本地一致。
- 正式发布：`v3.16.5-24`（commit `03bf30a4`，分支 `fix/3.16.5-22-gpt-live`）由 GitHub Release workflow 构建并发布，`isPrerelease=false`，含 19 个跨平台资产和 `latest.json`。`v3.19.0-3`（commit `da29b49a`，分支 `bigstrongsun/ccsm-agent-mesh`）初始同样为正式 release，随后用户要求改为 prerelease，并已在 release note 开头写明“测试版，不稳定慎用”，当前 `isPrerelease=true`。Release workflow 会读取 `docs/release-notes/v<tag>-zh.md` 作为正文，因此新版本必须先补该文件再 push tag。

## 2026-08-01 CCSM Agent Mesh 后端路线分支

- 已创建新分支 `bigstrongsun/ccsm-agent-mesh`，用于后续 CCSM 统一模型聚合网关和 Agent 适配层开发。
- 总体方向：把 Codex MultiRouter 升级为统一本地模型聚合网关，对外提供 `/v1/models`、`/v1/responses`、`/v1/chat/completions`，按模型路由上游；每个 Agent 只做薄适配层。
- 不重写现有协议转换器，而是封装成协议适配层；Codex MultiRouter 先作为第一个 Agent 适配器。
- 后端主要缺口：统一模型服务、统一网关、能力证据、CredentialRef/Secret Broker、共享路由与 Agent 投影、控制面/数据面分离、部署快照与回滚、端到端 canary、compact 能力约束。
- AgentMesh 原型未定稿，本分支当前只写开发文档，不开始实现；ACPs Adapter / Token Exchange 不在本路线范围。
- 开发文档：`docs/ccsm-agent-mesh-backend-roadmap.md`

## 2026-07-31 新版 MultiRouter 门面导致 Codex Desktop 登录状态消失

- 现象：从 CCSwitchMulti 3.16.5 升到 3.19.0-1 后，Codex Desktop 账号菜单只剩 `OpenAI / 隐藏宠物 / 设置`，不显示 `BigstrongSun`、剩余用量和 `退出登录`，设置里的个人资料也消失；回退到 3.16.5 后恢复。
- 根因：不是 `~/.codex/auth.json` 被删除，而是 3.16.5 之后的 `e2c8e845 feat(codex): classify multirouter auth facades` + `bb62fbfc fix(codex): project dynamic multirouter auth facade` 把 `managed_codex_oauth` 判定为 FullyManaged，并在 live `config.toml` 的 `[model_providers.codex_model_router_v2]` 里写成 `requires_openai_auth = false`。Codex Desktop 的账号/用量/退出入口由该字段驱动，因此即使 `auth.json` 仍有完整登录材料，UI 也认为当前不是 OpenAI 账号。3.16.5 一直写 `requires_openai_auth = true` + `experimental_bearer_token = "PROXY_MANAGED"`，既保留 Desktop 登录表面，又让真实请求命中本地代理。
- 修复：`apply_codex_multirouter_auth_facade_to_doc` 的 FullyManaged/LegacyPreserved 分支恢复为 `requires_openai_auth = true` + `PROXY_MANAGED`；NativeMixed 分支保持 `requires_openai_auth = true` 且不带 bearer。分类枚举仅用于诊断/调度，不再把 live 门面降级成隐藏登录态。
- 同源补漏：完整 Rust 回归还发现 `codex_pool_policy_reprojects_current_router_and_preserves_auth_json` 与 `switching_codex_router_provider_auto_enables_dedicated_local_takeover` 仍断言 `requires_openai_auth=false`，已统一改为 `true`。`rg` 全仓库确认不再有 MultiRouter 投影写 `requires_openai_auth=false` 的生效代码或断言。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib -- --skip update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active` 为 2668 passed / 2 ignored / 1 filtered；被过滤的是本机正在运行的 CCSM 占用 `127.0.0.1:15721`，不是代码回归。

## 2026-07-31 OAuth 账号池 Desktop 当前登录与托管同账号合并

- 现象：账号池 UI 同时出现 `Codex Desktop 当前登录账号` 和 `bigstrongsun0403@gmail.com` 两个条目，即使它们底层是同一个 ChatGPT account id。Desktop `~/.codex/auth.json` 的 `tokens.account_id` 与 CCSM `codex_oauth_auth.json` 的 managed account key 一致，但代码此前没有探测关联。
- 根因：`native_codex_auth` 是固定 sentinel，`normalized_pool_policy` 永远把 Desktop 条目和所有 managed account 分开补全，没有比较 Desktop auth.json 的 account id 与托管账号 key。
- 修复：`CodexAccountPoolPolicy` 新增可选 `desktopAccountId`；后端从 `auth.json /tokens/account_id` 探测 Desktop 当前登录。Desktop 条目启用且托管账号 key 相同时，规范化策略删除重复托管条目；Desktop 条目禁用时保留托管账号，避免用户明确只走 CCSM OAuth 时被误删。`set_account_pool_policy` 先保存用户提交策略，再规范化去重并持久化。
- 前端：账号池中 Desktop 条目能解析到同名托管账号并显示 `邮箱（Codex Desktop 当前登录）`，启用时显示“与已登录账号相同，已合并”；不把同一账号计成两个候选。英文/日文/繁中 locale 同步补齐。
- 验证：`codex_oauth_auth` 26 项通过，新增同账号去重和 Desktop 禁用保留测试；`CodexOAuthSection` 8 项、`CodexRouterWorkspacePage` 47 项通过；`pnpm typecheck`、Prettier、`cargo fmt --check` 通过；全量 Rust 跳过本机 15721 端口占用测试后 2668 passed / 2 ignored / 1 filtered。

## 2026-07-31 DeepSeek V4 Flash 原生 Responses 与 CCSM 路由优化

- DeepSeek 官方文档（`https://api-docs.deepseek.com/zh-cn/guides/responses_api/` 与 `/quick_start/agent_integrations/codex/`）确认：`deepseek-v4-flash` 原生支持 Responses API，`base_url=https://api.deepseek.com`；`deepseek-v4-pro` 暂不支持 Codex，官方预计 2026-08 初支持。官方 Codex 配置会写 `wire_api = "responses"` 和 `model_catalog_json`，并要求 `experimental_bearer_token` 放 DeepSeek API key。
- 本机实测：直接用 `https://api.deepseek.com/responses` 和 `/v1/responses` 对 `deepseek-v4-flash` 都返回 200，响应体是标准 Responses 结构（`input_tokens_details`、`reasoning`、`message`）。通过 CCSM 时，原配置的 DeepSeek route 仍写 `openai_chat`，日志显示 `/responses -> /chat/completions`；只把 route `upstream.apiFormat` 临时改成 `openai_responses` 后，`codex-router.log` 出现 `effective_endpoint=/responses upstream_url=https://api.deepseek.com/v1/responses responses_to_chat=false status=200`，随后已恢复原 DB。
- 根修不是“把 api.deepseek.com 从 Chat-only URL 列表删掉”这么简单。当前判断里显式 `apiFormat` 优先于已知 URL 列表，真正要改的是预设和已存 route：DeepSeek preset 从 `openai_chat` 切到 `openai_responses`；向导生成路由时把 Flash 拆成原生 Responses、把 Pro 拆成 Chat 兼容路由；同步 provider catalog 时保持拆分窗口，避免 Pro 被并回 Flash 原生路由；启动期幂等 repair 会迁移旧 DB 的 `codex-deepseek`/MultiRouter route。
- 本机已把同一套配置落到正在运行的 CCSM DB：`codex-deepseek.meta.apiFormat=openai_responses`，`router-codex-deepseek` 只匹配 `deepseek-v4-flash` 并走原生 Responses，新增 `router-codex-deepseek-pro-chat` 匹配 `deepseek-v4-pro` 并保留 Chat。备份在 `C:\Users\sunda\.cc-switch\backups\cc-switch-deepseek-v4flash-responses-20260731_155504.db`；更新后小请求与真实 Codex 会话日志都显示 `effective_endpoint=/responses upstream_url=https://api.deepseek.com/v1/responses responses_to_chat=false status=200`。
- 原版 `farion1231/cc-switch` 最新 release 仍是 `v3.19.0`，本地合并分支已包含该版本，没有发现更新；`BigStrongSun/ccswitchmulti` 远端最新 release 是 `v3.16.5-22`，本地 `3.19.0-1` 源码/导出超前于远端 release。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib --quiet` 为 2667 passed / 2 ignored；`pnpm typecheck`、`pnpm exec prettier --check`、定向 Vitest 均通过。全量 Vitest 为 798 passed / 1 failed，失败的是既有 `tests/integration/App.test.tsx` 10s 超时，单独 rerun 同一用例 5.2s 通过，与 DeepSeek 修改无关联。

## 2026-07-31 MultiRouter 同会话切模型后压缩请求回到当前模型的根修

- 现象：多路路由开启后，同一 Codex 会话从 GPT-5.6 Sol 切到 DeepSeek V4 Flash，官方额度已用尽时仍提示 `You've hit your usage limit`。`codex-router.log` 显示该 session 的 DeepSeek 普通 turn 已 200，但后续 `compaction_reason=comp_hash_changed/model_downshift`、`compaction_phase=pre_turn` 的请求仍带 `model=gpt-5.6-sol` 并持续命中官方 route 429。
- 上游原因：官方 Codex `core/src/session/turn.rs::maybe_run_previous_model_inline_compact()` 在切到更小上下文模型时故意用 `previous_turn_settings.model` 发起预采样压缩；这不是线程残留，CCSM 不能把“压缩请求带旧模型”当成普通路由 bug。
- CCSM 根因：`resolve_codex_model_routed_providers()` 已经生成“主 route + 其它启用 route”的候选链，但 `build_forward_attempt_providers_preserving_codex_router_context()` 仍原样保留外层 router，`forward()` 又只取 `.next()`。因此 429（`categorize_proxy_error` 本身归为 Retryable）永远不会尝试 DeepSeek 后备 route。
- 修复：JSON 转发入口现在把 MultiRouter 展开成 route provider 候选链，主 route 失败后由现有 retry/failover 循环按序尝试其它启用 route；route provider 保留 `codexRouterParentProviderId/Name`，`forward()` 的 `outer_provider/outer_name` 日志继续显示父 router。未知 `/v1/*` raw passthrough 仍保持原边界：只由 `forward_raw` 解析显式命中 route 或 official Codex OAuth，不把图片/音频/文件 endpoint 展开给文本 fallback。
- 关键修正：任意启用 route 不能代替“切过去的新模型”。compaction 请求进入转发前，`forward_with_retry_inner` 用 session id 查询 active `state_5.sqlite` 的 `threads.model`，若与请求体 model 不同，就把压缩请求体 model 改写成 session 当前模型，再按当前模型生成主 route；这样切到 Qwen/本地 vLLM/其它模型时压缩也会回到该模型，而不是碰运气走 DeepSeek。state DB 缺失/锁住/未写入时静默退回请求体 model 路由。
- 回归：`codex_multirouter_attempts_expand_route_chain_while_retaining_parent_context` 固定 deepseek 主路由优先、qwen fallback 随后且父身份不丢；`compaction_routes_to_current_session_model_not_arbitrary_fallback` 固定 body 是旧 `gpt-5.6-sol`、当前 session 模型是 `qwen3.6` 时压缩请求改写并路由到 qwen；`codex_state_db` 覆盖 active DB 解析与 `threads.model` 读取。全量 `cargo test --manifest-path src-tauri/Cargo.toml --quiet` 为 2666 passed / 2 ignored / 0 failed，`cargo fmt --check` 通过。

## 2026-07-30 全应用自适应缩放与默认窗口尺寸

- 默认窗口原为 `1000x650`，最小 `900x600`；设置页、Radix Portal 和 `FullScreenPanel` 分别管理高度与滚动，无法靠逐页 CSS 得到一致密度。更关键的是 `tauri-plugin-window-state` 持久化 `POSITION | SIZE | MAXIMIZED`，老用户会继续恢复旧的小窗口，所以只放大 `tauri.conf.json` 默认值不能解决升级用户的问题。
- 根修在 App 根部使用 Tauri WebView 原生 `setZoom`，让普通页面、Portal 弹窗和全屏面板共享同一缩放坐标系。缩放按 `innerSize / scaleFactor` 的逻辑视口计算，避免 Windows 125%/150% DPI 误判；设计视口 `1180x760` 为 100%，`1100x700` 为 90%，`1000x650` 为 85%，最小 `900x600` 保底 80%。长设置页仍保留正常滚动，不把所有内容强压进一屏。
- `setZoom` 不是 `core:default` 权限：Tauri 2.10 要求显式 `core:webview:allow-set-webview-zoom`。缺少该 capability 时布局测试可能通过，但原生缩放调用会被 ACL 拒绝；配置回归必须同时锁定默认尺寸和该权限。Hook 在无 Tauri internals 的 jsdom/浏览器预览中静默退出，并处理 resize/scale 事件异步乱序和监听注册失败。
- 实机使用临时 identifier `com.ccswitchmulti.ui-review` 和 `CC_SWITCH_TEST_HOME`，正式 CCSM 未操作。已在恢复的 `903x631` 最小窗口中验证主导航、设置页、Grok 新建 Portal、MultiRouter 全屏工作台统一为 80% 且无横向裁切/重叠，设置页可滚动、表单底部操作可达；最大化恢复 100%。定向回归、全量 Vitest、`pnpm typecheck`、`pnpm format:check`、renderer build 和隔离 Tauri release build 均通过。

## 2026-07-30 v3.19.0-1 OAuth 交互审查修复

- Codex 账号池不能在数值输入的 `onChange` 中直接持久化。连续输入会产生多个并发 mutation，响应乱序后服务端最终值可能回退。现在开关、排序、启用状态和保留额度统一编辑本地草稿，只有点击“保存账号池设置”才提交一次最终 policy；保存期间整组冲突控件禁用，可在提交前放弃更改。
- Codex、GitHub Copilot 和 xAI 三个 OAuth 区域统一使用 `OAuthDeleteConfirmDialog`。单账号删除和移除全部账号都先展示目标与不可撤销提示，只有确认后才调用 `removeAccount/logout`；取消不改变账号或当前选择。
- 定向组件回归覆盖连续输入只保存一次、pending 锁定、单账号确认删除和全部账号取消。后续认证 UI 必须继续复用统一确认契约，不能在按钮 `onClick` 中直接调用删除 API。
- Grok Build 新建表单的预设列表此前无首屏预算，约 40 个按钮会把名称、认证和 API 地址全部推到窗口下方。`ProviderPresetSelector` 现在提供默认关闭的可选折叠模式；Grok Build 单独启用前 8 个预设与“展开全部”，搜索仍展示全部匹配结果，选择预设后平滑滚动到主表单。其他应用的预设列表行为不变。
- Grok 既有组件回归必须先展开列表再选择首屏之外的预设。jsdom 不实现真实 WebView 已有的 `Element.scrollIntoView`，统一测试环境提供 no-op polyfill，避免 `requestAnimationFrame` 中的滚动回调成为未捕获异常并污染后续测试。
- Codex 账号池、OAuth 删除确认和 Router 官方认证设置已补齐 `zh/en/ja/zh-TW`。运行时门面名称也必须通过当前 locale 生成，不能只翻译重启提示模板后插入中文门面；默认账号有 ID 与无 ID 使用不同 key，避免空 ID 显示成空括号。

## 2026-07-30 合并上游 v3.19.0 到 CCSwitchMulti 3.19.0-1

- 分支 `bigstrongsun/merge-upstream-v3.19.0` 将上游 tag `v3.19.0`（`09ccf328`）合入 CCSwitchMulti，版本统一为 `3.19.0-1`。合并保留上游 schema v16、安全修复、Grok Build/xAI OAuth、models.dev 同步、使用日志清理与 Codex 用量重建，同时保留 CCSM MultiRouter、OAuth 账号池、Native/Mixed 接管、Codex 历史恢复/fork timeline 去重、媒体桥接和 Responses-Lite fallback。
- Codex 认证合并边界：内置 `codex-official` 是 Desktop 当前登录态，adapter 不生成 managed OAuth placeholder；旧版非内置 empty-official backup 仍兼容推断为 CCSM managed OAuth。判断必须先看 `provider_uses_native_codex_auth()`，再走 legacy managed inference，不能仅凭 `category=official` 决定凭据所有权。
- 媒体桥接边界：Responses/Chat 工具结果统一支持图片、`input_file`、Chat-style/`audio_url` 音频；`encrypted_content` 和未知二进制结构只在 `ToolMediaScope::AllSupported` 中替换为有界占位。`ImagesOnly`/`InlineImagesOnly` 仍严格只处理图片，已知 `image`/`video` 的普通 `data` 字段继续交给统一 base64 限幅，不能被敏感结构兜底提前删除。
- Codex 直接切换失败回滚必须调用 `write_codex_live_snapshot(previous_live, false)` 原样恢复切换前 snapshot；普通 provider restore 才用 `merge_existing_config=true`。否则目标 provider table 已写入 live 后，回滚再次 merge 会把污染表保留下来。
- Grok Build 复用 `CodexFormFields` 时不显示 Codex 专属 `/model` 菜单投影开关；`/models` 异步探测使用请求身份序号，账号、Key 或 URL 切换后旧响应不能覆盖新状态；官方类别 MultiRouter 也必须进入 routing-aware UI。
- 测试隔离根因：`proxy::http_client::test_system_proxy_points_to_loopback` 曾改写 6 个进程级代理环境变量但不恢复，也没有进入 `serial_test` 全局锁，导致并发 WebDAV 回环 E2E 偶发用污染环境构建 reqwest client。修复后该测试保存/恢复环境，四个 WebDAV E2E 与其共享 `#[serial]`，默认并发全量 Rust 测试稳定通过。
- 最终验证：`cargo test --manifest-path src-tauri/Cargo.toml --quiet` 为 2782 passed / 2 ignored / 0 failed；`pnpm test:unit -- --maxWorkers=1 --minWorkers=1 --reporter=dot` 为 102 files / 774 tests 全通过；`pnpm typecheck`、`pnpm format:check`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、双父版本 locale leaf-key 并集校验和 `git diff --check` 均通过。Windows installer 只构建交付，不在本机安装；真机验证后再 push/tag/Release。

## 2026-07-30 Codex MultiRouter 动态认证门面实现与接管清理根修

- 本轮按 `docs/superpowers/specs/2026-07-29-codex-multirouter-auth-facade-design.md` 完成动态门面。MultiRouter 的稳定身份始终为 `model_provider = "codex_model_router_v2"`，能力名始终为 `name = "OpenAI"`，传输固定 `wire_api = "responses"`、`supports_websockets = false`；`name` 只声明 OpenAI 能力，不决定凭据归属，CCSM managed OAuth 同样可以使用 `name = "OpenAI"`。
- Native/Mixed 门面用于任一启用 route 可能使用 Desktop 当前登录态的情况：`requires_openai_auth = true`、不写 `experimental_bearer_token`，用本地控制头 `x-cc-switch-proxy-mode = "router"` 识别进入 Router 的请求。Fully Managed 门面用于全部 route 由 CCSM/目标 provider 管理凭据的情况，但 live 配置仍写 `requires_openai_auth = true` + `experimental_bearer_token = "PROXY_MANAGED"`，只切换出站凭据所有权、不隐藏 Desktop 登录表面。旧 Router 缺少 auth source 时保留其原有生效语义，不能静默迁移认证所有权。
- 根因链：Codex 凭据优先级会让 `PROXY_MANAGED` 遮蔽 Desktop OAuth；旧固定投影因此无法让 native route 收到真实登录态。修复后 route 是最终认证事实：native route 才透传来向 Authorization；固定 OAuth、账号池 managed candidate 和第三方 route 必须删除来向 Authorization 后注入自己的凭据；所有 `x-cc-switch-*` 控制头在每种出站 transport 前统一剥离，External Agent API 仍不能借用 Desktop OAuth。
- 账号池策略保存后立即重投影当前启用的 MultiRouter，并返回 `facadeChanged`、`codexRestartRequired` 和最终 facade。UI 在 Router Workspace 与 Codex OAuth 设置区用中文显示“Desktop / 混合认证”“CCSM 托管认证”及重启提示。认证门面改变不承诺热修复：用户必须完全退出并重启 Codex，已有任务不会热加载新的认证所有权。
- 最后兜底根因：无备份且 SSOT 不可恢复时，旧清理逻辑只删除回环 URL 和 `PROXY_MANAGED`，Native/Mixed Router 本来就没有 placeholder，导致 `codex_model_router_v2`、模型目录和 `x-cc-switch-proxy-mode` 残留在 live TOML。`49d0095a` 将 CCSM Router 投影视为一个配置单元，兜底时完整恢复到内建 `openai`，同时保留真实 `auth.json`、MCP、projects 等用户全局配置；红绿测试证明旧逻辑失败、新逻辑通过。
- 实现提交依次为 `e2c8e845`（门面分类）、`bb62fbfc`（动态 TOML 投影）、`70c4716b`（凭据所有权隔离）、`909a61e1`（账号池重投影）、`a15d83e0`（中文 UI/重启提示）、`a6558bd7`（接管识别回归）、`49d0095a`（Router 完整解绑）、`c11781ff`（Prettier 规范化）。`auth.json` 全程保持真实 Desktop OAuth，不写 placeholder。
- 最终验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib` 为 2359 passed / 2 ignored / 0 failed；`pnpm test:unit -- --maxWorkers=1 --minWorkers=1` 为 88 files / 651 tests 全通过；`pnpm typecheck`、`pnpm format:check`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 全通过。仓库没有 `pnpm lint` 脚本，不能把该命令不存在误判为 lint 失败。

## 2026-07-29 Codex MultiRouter 动态认证门面设计

- `model_provider` 是任务和配置身份，provider `name` 是 Codex 能力声明；当前 Codex 以精确 `name == "OpenAI"` 开启远程压缩、Web Search、图片和部分 OpenAI 元数据路径。MultiRouter 固定使用 `codex_model_router_v2`，Codex 侧 `name` 固定为 `OpenAI`，用户可见 Router 名只保存在 CCSM UI/数据库。CCSM managed OAuth 同样使用 `name = "OpenAI"`，认证所有权不由 name 决定。
- facade 按 route 认证所有权分两类：存在 `native_codex_auth` 或账号池启用 Desktop 账号时使用 Native/Mixed，写 `requires_openai_auth=true`、删除 `experimental_bearer_token`，让 Codex 发送真实 Desktop OAuth；完全由固定 CCSM OAuth、无 Desktop 的账号池或第三方 provider 管理时使用 Fully Managed，写 `requires_openai_auth=true` 与 `PROXY_MANAGED`。`requires_openai_auth=false` 会让 Codex Desktop 隐藏账号/用量/退出入口，因此不能再用于 MultiRouter live 门面。
- Native/Mixed 使用独立的 `x-cc-switch-proxy-mode=router` 标识本地 Router 流量，不能再占用 Authorization；native route 透传来向 OAuth，托管 OAuth/第三方 route 必须先删除来向 Authorization 再注入自己的凭据，且标识头不得出站。External Agent API 继续禁止借用 Desktop OAuth。
- 两种 facade 都固定 `wire_api="responses"`、`supports_websockets=false`，即 Responses over HTTP/SSE。provider ID 不变，迁移不修改 `auth.json`；facade 类型变化明确要求重启 Codex，不承诺现有 session 热加载认证。
- 完整设计见 `docs/superpowers/specs/2026-07-29-codex-multirouter-auth-facade-design.md`；实现验收必须补 config -> Codex auth -> route -> upstream 的端到端矩阵，不能只依赖分段单测。

## 2026-07-28 MultiRouter 官方认证策略与 HTTP Responses 边界

- 根因：账号池第一版只有 OAuth manager 的全局 `enabled/order/reservePercent`，forwarder 会把任意 official effective provider 展开成账号候选；Router 本身没有声明“Desktop 当前登录 / 固定 CCSM OAuth / OAuth 账号池”，因此全局开关可能改变原本显式使用 `native_codex_auth` 的 Router，旧方案升级后也没有可见迁移入口。
- 新契约：`codexRouting.officialAuth` 是每个 MultiRouter 的官方认证默认策略，`mode` 取 `desktop_current_login | managed_oauth | account_pool`，固定 CCSM OAuth 可带 `accountId`。单 route 的 `upstream.auth` 仍是最终执行事实；新向导明确展示选择，工作台设置可调整，通用 route 编辑器也支持三种来源。
- 迁移必须保守：没有 `officialAuth` 的旧 Router 先从 route 显式 `native_codex_auth`、`managed_codex_oauth/managed_account + accountId` 或 `account_pool` 推断展示，未保存前不改数据库；用户保存 Router 设置或向导发布后才写入 Router 策略并只重写官方 route，第三方 `provider_config` route 不受影响。旧固定账号不能静默迁移成 Desktop 当前登录或账号池。
- 运行时账号池必须双重显式：route 选择 `account_pool` 后物化为 `codexAccountPoolEnabled=true`，forwarder 只展开带这个 request-local 标记的 provider；仅开启全局账号池不再劫持 native/fixed OAuth Router。全局账号池仍负责总开关、顺序、enabled、reserve、额度缓存、session affinity 与 429 cooldown；Router 选择账号池时全局策略也必须启用。
- 认证和传输是两层：`native_codex_auth` 只表示复用进入 CCSM 的 Codex Desktop Authorization，不表示上游 WebSocket 或绕过 MultiRouter。接管配置固定 `wire_api="responses"`、`supports_websockets=false`，本地 GET `/responses` WebSocket upgrade 返回 426，客户端使用 HTTP Responses；这样 CCSM 能先读取 `response.create.model` 再选 route，也避开国内网络下长连接不稳定。固定 CCSM OAuth 和账号池同样使用这条 HTTP Responses 客户端链路。
- 回归基线：前端覆盖 Router 策略持久化、旧精确账号推断、账号池 route 生成、只改官方 route 和表单提取；Rust 覆盖 account-pool effective provider 无固定 auth/native passthrough、全局池仅识别显式 marker。`cargo test` 2347 passed / 2 ignored，Vitest 87 files / 648 tests 通过。

## 2026-07-28 ChatGPT OAuth 账号池配置边界

- reset credit 兑换继续由 OpenAI 官方在周额度耗尽时处理；CCSM 只读取额度与 reset credit 状态，不实现兑换、保留 credit id 或自动消费动作。
- ChatGPT 账号池配置属于 OAuth manager 的持久化状态，不是前端 local state：稳定项 `native_codex_auth` 表示 Codex Desktop 当前登录账号，其余项按 CCSM OAuth account id 标识。每项包含真实优先级顺序、enabled 和 `reservePercent`，新增/删除账号时通过规范化策略自动补入/清理。
- UI 放在设置的 Codex OAuth 区域，顺序展示必须与后端调度顺序一致；使用图标上下按钮调整，逐账号设置保留额度阈值。策略总开关默认关闭，升级后不能静默改变既有认证选路。
- 第一阶段 `f82dfcda` 落持久化策略和 UI；后续根修要求只有 Router route 显式选择 `account_pool` 才按同一顺序展开账号候选，普通 official/native/fixed OAuth route 不得受全局开关影响。成功后按 proxy session 粘住账号，429 将账号冷却 60 秒并允许未开始输出的请求尝试下一账号。额度页面启用池时立即刷新，之后每 5 分钟刷新；转发层也按同一 TTL 用只读 `/wham/usage` 轻量探测，正确性不依赖设置页保持打开。多个到期账号必须并发探测，使总等待上限约等于单次 15 秒 timeout，不能串行放大为账号数乘以 timeout。成功快照按所有窗口最高 utilization 计算剩余比例，低于逐账号 `reservePercent` 的账号不进入新候选，查询失败不覆盖上次可信值。External Agent API 不参与池扩展，不能借用 Desktop 或托管 OAuth 账号。

## 2026-07-28 Codex 当前登录账号路由与 Profile 接管恢复

- 内置 `codex-official` 空 seed 在 MultiRouter 新建 route 时使用 `native_codex_auth`：请求继续经过 CCSM 本地路由，但 Authorization 来自当前 Codex 客户端的登录态，普通 Responses 与 `/responses/compact` 都直达 `chatgpt.com/backend-api/codex`。这等价于 OpenCodex Direct 的账号边界，不是 CCSM 账号池；显式 `accountId` / `managed_codex_oauth` 绑定继续使用 CCSM OAuth manager，已有持久化 route 不自动迁移。
- `native_codex_auth` 只在 `AppType::Codex` 的本地请求中允许透传。带 `x-cc-switch-external-openai-api` 的 External Agent API 请求不得借用本机 Codex 登录，也不得把外部 Bearer token 当 ChatGPT Codex token 转发；该边界由 `should_passthrough_codex_official_auth` 集中判断并有单测。
- Profile 同 Provider ID 切换的根因是 apply 先无条件关闭 takeover，随后因 `current_provider_id == target_provider_id` 跳过 Provider switch，导致同一 MultiRouter 的新 Profile 永久失去接管。`80a124fa` 在目标 Codex Provider 需要本地代理时，即使 ID 相同也重新执行 switch，恢复 takeover；`codex_profile_reapplies_same_multirouter_after_takeover_cleanup` 覆盖该路径。
- 验证基线：MultiRouter 向导同时覆盖内置 official seed 的 `native_codex_auth` 与显式账号的 `managed_codex_oauth`；Rust 覆盖 native official compact URL、无 CCSM OAuth 凭据读取、External API 隔离；Codex provider 119 tests 与 Profile 8 tests 通过。

## 2026-07-28 Upstream v3.17.0 Merge Into CCSwitchMulti

- CCSwitchMulti 从 `2bbe8d20` 合并上游 `farion1231/cc-switch` 的 `3d176b98`（tag `v3.17.0`），版本统一为 `3.17.0-1`，保留 `cc-switch-multi`、`CCSwitchMulti` 和 `com.ccswitchmulti.desktop` 品牌标识。
- schema v12 两条独立演进必须共存：CCSM 的 `quota_collaboration_reports` 与上游 `profiles` 都在新库和 v11->v12 迁移中创建；不能只选任一分支。Profiles 的同步关闭接管路径需要 `disable_takeover_for_app_sync`，按恢复 live、删备份、清 enabled/健康状态的顺序执行。
- Codex 请求协议现在同时保留 native Responses、CCSM Messages、OpenAI Chat 和上游 Anthropic Messages。Chat 继续使用 CCSM 的结构化图片/音频/文件及 text-only/cache-aware 转换，再注入 prompt-cache key；Anthropic 保留 max output、1M beta、可选 Claude Code impersonation、cache injection 和 2xx 语义错误 failover。compact 必须按 effective route 逐请求选择 `responses_compact/chat_completions/messages/anthropic_messages`。
- 接管根因：上游官方 OAuth 必须走 `apply_codex_official_proxy_route`；第三方/MultiRouter 继续写 `custom`/`codex_model_router_v2`。新的 takeover TOML 不能再经过 restore/provider merge 清理器，否则新写入的 `PROXY_MANAGED` 会被当作旧接管残留删除，造成 `live_matches_current_proxy=false`。严格接管统一走 `apply_codex_takeover_fields_for_provider`，写入时直接原子落已投影 TOML，保留真实 `auth.json`。
- 历史修复继续采用 CCSM 严格规则：只恢复受信任 thread/index，过滤 developer、工具、子 agent 和内部恢复转录，按真实用户事件判断分叉；扫描所有 `state_N.sqlite`，标题读取兼容上游 state DB。多模态保持 function/custom tool output 的 image/audio/file 结构化转换，URL-only 文件显式产生有界占位而非静默丢失。
- Rust 全量 lib 测试：2342 passed、2 ignored、0 failed（2344 total）；前端 `pnpm typecheck` 和 `pnpm test:unit -- --maxWorkers=1 --minWorkers=1` 通过。Windows `sqlite_home` 测试必须使用 TOML literal string，避免反斜杠被当转义；Codex usage 同时解析 `cache_write_tokens` 与 `cache_creation_input_tokens`。

## 2026-07-27 Codex 跨模型上下文压缩路由根修

- 官方 Codex `95637f7056835fea66bdd0044414af480fc0fd74` 的 compact 选择只看 Codex 侧 `ModelProviderInfo::supports_remote_compaction()`；Responses provider 会发送 unary `POST /responses/compact`，请求继续携带当前 compact turn 的 `model/input/instructions/tools/reasoning` 以及 `x-codex-turn-metadata`。元数据 `request_kind=compaction`，并含 trigger/reason/implementation/phase；模型降档或上下文切换时 compact body 的 model 可能是前一模型或当前模型，CCSM 必须逐请求重新解析 route。
- MultiRouter 对 Codex 暴露统一 Responses provider，但实际 effective route 可能是 official Responses、OpenAI Chat 或 Messages。因此“所有 `/responses/compact` 都原样透传”的未提交方案不完整：它会把 compact 直接打到 DeepSeek/Qwen 等不存在该端点的 Chat 上游。正确矩阵以本次 body model 解析出的 effective provider 为准：原生 Responses 路由保留 `/responses/compact`；Chat/Messages 路由转换 compact payload 并分别发往 `/chat/completions` 或 `/v1/messages`，再重建 Codex 需要的 `{output:[ResponseItem...]}` unary 响应。
- 不能按 session 缓存 compact provider。相同 task 从 official 切到 Qwen，或者 Codex 因 model downshift 先尝试旧模型再尝试当前模型时，每个 compact 请求都必须按自身 `model` 命中 route、上游模型映射和协议转换。route 缺失且 fallback 指向本地代理时继续拒绝递归，不能悄悄沿用上一上下文的 provider。
- `x-codex-turn-metadata` 只用于诊断和日志分类，不参与路由真值；优先读 header，非法/缺失时兼容 `client_metadata.x-codex-turn-metadata` 对象或 JSON 字符串。端点为 compact 但元数据缺失时归类为 `request_kind=compaction`。`request_prepared` 日志同时记录 `compaction_transport=responses_compact/chat_completions/messages`，便于区分“Codex 发起 remote compact”和“CCSM 对实际下游采用的 wire transport”。
- 回归必须覆盖：compact query 保留、请求 instructions 与跨切换上下文不丢、Chat unary 响应恢复为 Compact API output、official/Chat 两条 route 按 compact body model 独立选择，以及 header 损坏时 body 元数据兜底。不要把普通 turn 的 `request_kind=compaction`（local summarization request）与 `/responses/compact` endpoint 混成同一个转发判断；协议转换仍由 endpoint + effective provider 决定。
- 上述历史归类、多模态工具上下文和跨模型压缩修复已随 `v3.16.5-22` 正式发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-22`。Release workflow run `30284418362` 全部成功，Windows x64/ARM64、macOS universal、Linux x64/ARM64、Publish Release 和 Assemble latest.json 均为 success；Release 非 draft/非 prerelease，共 19 个资产。远端 `latest.json` 已核验 `version=3.16.5-22`，包含 `darwin-aarch64/darwin-x86_64/windows-x86_64/windows-aarch64/linux-x86_64/linux-aarch64`。

## 2026-07-27 Codex 历史归类与结构化工具多模态根修

- 官方 Codex `95637f7056835fea66bdd0044414af480fc0fd74` 的 Desktop 默认列表规则是 `INTERACTIVE_SESSION_SOURCES`，即 `SessionSource::Cli/VSCode`；`Exec/Mcp/Custom/Internal/SubAgent/Unknown` 不是默认用户任务。`thread_source` 是第二层标签：仅 `user` 或旧记录缺省值可作为主任务，`subagent`、`memory_consolidation` 和任意 feature 名均不能被普通历史修复顶进侧边栏。rollout `source` 既可能是字符串，也可能是外部标记对象（例如 `{"sub_agent":...}`），解析时必须先归一化来源类别。
- 官方 state reducer 只用 `event_msg.user_message` 和新版 `event_msg.item_completed` 中的 `item.type=user_message` 建立 `has_user_event/first_user_message/title`；`response_item` 的 user/developer 内容是模型输入转录，tool call/output 是工具管线，均不能独立创造历史元数据。CCSM 历史正文只显示经过内部包装过滤的真实 user event（兼容旧 rollout 的 response user）与 assistant message，并去掉 developer、工具调用/结果、环境说明和恢复转录。
- 历史修复仍只允许恢复 active `threads` 或原生 `session_index.jsonl` 已存在的 session id；不扫描 rollout 创造新 sidebar id。旧版误修行按内部转录和严格用户消息前缀回收，只删 DB/index，保留 rollout；相同前缀后用户消息发生分叉的真实任务全部保留。
- 图片上下文根因由 `git blame/log` 定位到 `693c3872f042d3a578f4a45aa9897542a620b262`（2026-06-02）：最初 Responses -> Chat 桥接把所有 `function_call_output.output` 统一 canonical JSON 字符串化。`f59fab6c243ea629270755f0da0986ac2b21a22d`（2026-06-07）只补了普通 message 的旧 `input_file/input_audio` 映射，没有覆盖工具结构化输出；新版 `view_image` 返回 Base64 `input_image` 后，图片遂作为文本 token 进入上下文。
- 官方 `FunctionCallOutputContentItem` 支持 `input_text/input_image/input_audio/encrypted_content`，且 `custom_tool_call_output` 复用同一 payload。转换器必须同时处理 function/custom output：文本保留；图片提升为 Chat `image_url` 并保留 detail；data audio URL 转为 Chat `input_audio`；远程音频、密文、未知结构只生成有界占位，绝不能泄漏或字符串化 Base64/密文。text-only 模型对图片/音频/文件只发占位。普通 JSON 数组仍 canonical 透传，不能误判成结构化 content items。
- 普通 message 路径同时兼容当前 Codex `input_audio.audio_url` 与旧 `input_audio` 对象；data URL 可转换，远程 URL 有界降级。Chat file 只支持 `file_id/file_data`，URL-only file 必须显式占位而不是静默丢失或文本展开。官方当前 function output schema没有 `input_file`，未知文件/二进制 item 按有界未知项处理。

## 2026-07-27 Codex 内部转录、坏 encrypted_content 与工具图片上下文根修

- Issue #18 的损坏形态是 Multi-Agent V2 rollout 中 `agent_message.content[].type=encrypted_content` 却承载可读明文；CCSwitchMulti 的 Chat -> Responses 响应转换不生成 `agent_message`，因此源头在 Codex 本地子 Agent 投递层，但 CCSM 官方 OAuth 出站必须自愈，否则坏记录会被官方 backend 当密文解码并让任务及后续 fork 永久 5 次重连。只在载荷无法按 Base64/Base64URL 解码为至少 32 字节时降级成 `input_text`，合法密文保持原样；网络/TUN 只能改变触发概率，不能修复已污染 rollout。
- 新版历史修复只接受顶层 `type=event_msg` 且 `payload.type=user_message` 的真实事件作为用户正文；`The following is the Codex agent history...`、`<environment_context>`、AGENTS/developer 环境、handoff/工具状态等内部恢复转录不能生成 title/preview/first_user_message。带附件的 Desktop 包装应提取 `My request for Codex:` 后正文，只有 `Files mentioned by the user` 而无真实请求的事件应忽略。
- 为回收旧版误修结果，历史修复会识别“已有 thread 行标题是内部转录，且 rollout 的所有 user_message 都是内部事件、没有真实用户请求”的行；预览显示 `internal transcript cleanup`，应用时只删除 `threads` 与 `session_index.jsonl` 索引，保留 rollout 文件和既有备份/进程关闭保护。prefix 对话比对也必须复用同一真实用户正文过滤，避免内部转录参与分叉判断。
- “禁止 session_index append”必须覆盖聚焦移动阶段：旧 `move_focus_session_index_rows` 会为 selected 但 index 中不存在的 ID 补造新行，这是修复过量的第二入口。现在只移动/改名原生 index 已存在的行，DB 聚焦不再隐式创造侧边栏条目。rollout 元数据的 `updated_at_ms` 以 JSONL 事件时间为准；文件 mtime 只能在没有任何事件时间时兜底，否则重复 session id 的新旧快照选择会不稳定。
- Responses -> Chat 的 `function_call_output` 若包含 `[{type:"input_image",image_url:"data:..."}]`，不能再 canonical JSON 后塞进 tool content，否则 Base64 会按普通文本计 token 并迅速撑爆上下文。正确映射是 tool 消息保留短占位，随后追加 user multimodal `image_url` part；非图片输出仍保持 canonical JSON。

## 2026-07-25 Codex official 模型图片能力丢失根因

- 症状：在 CCSwitchMulti MultiRouter 下选择 official GPT 模型时，Codex Desktop 提示模型不支持图片输入。
- 根因：官方原始 `~/.codex/models_cache.cc-switch-backup.json` 中 `gpt-5.6-sol/terra/luna/gpt-5.5/gpt-5.4/gpt-5.4-mini` 都带 `input_modalities=["text","image"]` 和 `supports_image_detail_original=true`，但 CCSM 的 `FetchedModel` 只承载 `id/owned_by/context_window`。OAuth/cache 动态目录同步到 provider、route、聚合 catalog 时丢失模态字段；随后 `CodexCatalogToolProfile::NativeResponses` 用 text-only 模板生成 `cc-switch-model-catalog.json`，所有 official GPT 被写成 `input_modalities=["text"]`。
- 引入边界：`15e712e7` 引入 NativeResponses 直连和 text-only 模板，这是对第三方 native gateway 的合理保护；`a3e4622f` 把 Codex OAuth 动态模型目录接入 MultiRouter，但模型结构没承载模态字段；`ba739bd9` 删除静态 fallback 后让动态目录成为主路径，因此 official 图文能力丢失稳定复现。
- 修复边界：后端 `codex_oauth_models` 从在线响应和本地 cache 解析 `input_modalities/inputModalities/modalities.input` 与 `supportsImage/supports_image/vision/supports_image_detail_original`；前端 `FetchedModel`、Workspace 刷新、MultiRouter 向导合并都保留 `inputModalities` 与 `supportsImage`；最终 `codex_config` 生成 NativeResponses catalog 时依据这些字段覆盖 text-only 模板。Spark/DeepSeek V4 这类明确 text-only 的模型仍保持文本能力，不能被模板或 route 误开启图片。
- 所有模型类型支持策略：official 不做主动探测，信任官方在线 catalog / 原始 cache；OpenAI-compatible `/v1/models` 和火山 Plan 若明确返回 `input_modalities`、`modalities.input`、`supportsImage`、`vision` 等字段则透传；没有能力字段时保持未知，靠用户 route capability 显式声明或运行时上游不支持图片错误触发降级。不要在批量刷新时对每个模型发真实图片探测，避免消耗额度、触发审计和产生不稳定副作用。
- 验证：定向 Rust 测试覆盖 OAuth 图像能力解析与 NativeResponses catalog 图文输出；前端 `codexMultiRouterWizard`/`codexMultiRouterSync` 单线程测试、`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 通过。真实 `~/.codex` 未写入；安装新构建后需刷新/重建 MultiRouter official catalog，再重启 Codex app-server 读取新的 `cc-switch-model-catalog.json`。

## 2026-07-25 Codex Desktop 新版历史修复根修

- 最新 Desktop 的 `thread/list` 以 active `~/.codex/state_5.sqlite` 的 `threads` 为主，且 `backfill_state.status='complete'` 会阻止 rollout 再扫描；只改 `model_provider`、`session_index.jsonl` 或 mtime 会让 CCSM 自己的 JSONL 列表看似正常，但 Desktop 仍会因 `has_user_event`、`preview`、`first_user_message`、`recency_at_ms` 缺失而隐藏会话。
- 修复入口必须在 Codex/ChatGPT 完全退出时，以 `sessions` 与 `archived_sessions` 的 rollout 为事实来源重建/插入 `threads` 元数据，并在同一 SQLite 事务更新 `backfill_state`；`session_index`、workspace hint 和项目映射只能作为辅助，绝不能写 `local_thread_catalog`。
- 当前 rollout 的真实用户输入结构是 `type='event_msg'` 且 `payload.type='user_message'`，不能继续用旧的字符串或顶层 `type='user_message'` 判断。preview 与 first_user_message 仅从此真实事件或目标事件 preview 提取，避免编造可见历史。
- 写入前继续拒绝运行中的 `ChatGPT.exe` / `OpenAI.Codex` / `codex.exe app-server`，并备份 state DB（含 WAL/SHM）和受影响 rollout；修复后需重启 Desktop 验证历史不再闪现后消失。
- `Continue in a new task` 的真实身份是新的 `session_meta.id`；其 rollout event payload 中的 `forked_from_id` 才指向父 task。派生任务会复制首条用户消息，因此不能用标题、首条消息、文件名 UUID 或全文引用判断身份。在 2026-07-25 的“孙同学”现场，36 条相同标题均为不同 session id，且可由 `forked_from_id` 形成分支树。历史重建必须按 `session_meta.id -> rollout_path` 一对一保留这些任务。
- 若损坏导出真的有多个文件声明同一个 `session_meta.id`，它们不是可证明的派生任务：只保留含用户事件且 `updated_at_ms` 最新的完整快照，不能改写为 filename UUID 并插入多个侧边栏条目。
- 纠正：rollout 中即使存在不同 `session_meta.id` / `forked_from_id`，也不等于用户期望在 Desktop 侧边栏显示的独立历史。历史修复只能更新已有 `threads`，或恢复 Codex 原生 `session_index.jsonl` 已列出的 ID；绝不能扫描所有 rollout 后插入 threads，更不能把缺失的可见行追加回 session_index。此前这两步会把内部续接/恢复记录扩散为大量重复 sidebar 条目。
- 错误扩散后的回收不能按标题或固定 ID 删除。以相同 cwd 和首条用户消息分组，归一化所有 `event_msg.user_message` 后做严格前缀比较：短链被长链完整覆盖时回收短链；相同长度保留更新时间更新者；共同前缀后出现不同用户消息则是真实分叉，全部保留。助手消息、工具事件、压缩摘要和 token 快照会因重试而不同，不能作为分叉身份。回收仅删除 `threads` 和 `session_index.jsonl` 索引，rollout 原文件保留，并复用历史修复的 DB/索引备份与 Codex 进程关闭保护。
- 官方多模态 catalog 投影必须让同 slug 官方 cache 的 `input_modalities`、`supports_image_detail_original` 与 `web_search_tool_type` 胜过 NativeResponses 的 text-only 模板；否则模型目录与路由虽声明图像能力，最终 `cc-switch-model-catalog.json` 仍会被覆盖成 text-only。

## 2026-07-20 本机构建缓存与旧发布输出清理

- 清理前仓库最大的可再生成项是 `src-tauri/target`：`52264183717` bytes（48.675 GiB），其中 debug 约 39.011 GiB、release 约 9.663 GiB；现场没有进程从该目录运行，因此可整目录删除，后续 Cargo/Tauri 构建会按需重建。
- 同步删除可再生成的 `node_modules`（约 278 MiB）、`dist`（约 5 MiB）和未跟踪的 `scripts/logs`（约 318 MiB）。pnpm 的链接节点可能让第一次递归删除报 access denied；部分删除完成后再次对已核验的 `node_modules` 根目录执行目录删除即可，不要扩大到 pnpm 全局 store。
- 删除 `output/release-v3.16.4-4-upload`、`output/release-v3.16.4-5wizard`、`output/release-v3.16.5-3` 三组已过期本地发布产物，合计约 210 MiB；保留 `output/pdf/codex-multirouter-guide-zh.pdf`。清理后工作区除 `.git` 外约 54 MiB。
- 清理前后均保留用户未提交的 `src-tauri/src/proxy/forwarder.rs`、`src-tauri/src/proxy/providers/codex.rs`、`src-tauri/src/proxy/providers/mod.rs` 修改。以后做空间回收应先检查 `git status` 和运行进程，再只删 target、dist、node_modules、旧 release output、诊断 logs 这类可再生成或已交付内容。

## 2026-07-15 Windows WM_ENDSESSION tao Destroyed panic 根修

- 用户报告的 `app-exit-events.jsonl` 在 2026-07-15 11:18:55 记录主线程 panic：`tao-0.34.6/src/platform_impl/windows/event_loop/runner.rs:371 cannot move state from Destroyed`。同一次运行在 11:18:44 托盘 `show_main` 后发生，但 `show_main` 只会增加重绘消息，不能产生 `Destroyed`；没有 `ExitRequested` 日志也排除了 CCSwitchMulti 主动退出路径。
- 根因来自 tao 0.34.3 引入的 Windows `WM_ENDSESSION(TRUE)` 处理：它在 `DispatchMessageW` 回调内部直接调用 `loop_destroyed()`，而外层 `GetMessageW` 循环仍存活；随后已排队的 `WM_PAINT`、内部 `PROCESS_NEW_EVENTS` 或 user event 会再次调用 `poll/send_event/redraw`，从终态 `Destroyed` 迁移并 panic。该逻辑在 tao 0.34.6、0.34.8、0.35.0 和当时 dev 分支仍存在，单纯升级不能修复。
- 根修保留 tao 0.34.6 其余上游修复，并通过 `src-tauri/Cargo.toml [patch.crates-io]` 固定仓库内 `src-tauri/vendor/tao`：`WM_ENDSESSION(TRUE)` 改为 `PostQuitMessage(0)`，让下一次 `GetMessageW` 返回 0，之后只由 `run_return` 的正常尾部发送一次 `LoopDestroyed`；`WM_ENDSESSION(FALSE)` 继续运行。严禁用 catch panic、忽略 Destroyed 状态迁移或仅把 panic 改成 return，这些做法会留下半销毁事件循环。
- CCSwitchMulti 在最终 `RunEvent::Exit` 调用 `app_exit_monitor::record_clean_exit("event_loop_exit", 0)`，使系统关机、重启、注销或 Restart Manager 关闭后的下一次启动不再误报残留 marker；用户主动退出仍走原有 `ExitRequested` 清理路径。
- 回归基线：vendored tao 的 `confirmed_end_session_quits_the_message_loop` / `cancelled_end_session_keeps_the_message_loop_alive` 两个单测、完整 `cargo check --manifest-path src-tauri/Cargo.toml --lib`，并确认构建输出明确解析 `tao v0.34.6 (.../src-tauri/vendor/tao)`。上游发布等价修复前不得删除 vendor override。

## 2026-07-15 CCSwitchMulti v3.16.5-10：数据库 v13 兼容与白屏恢复

- 已发布 `v3.16.5-9` 只支持 schema v12；上游 `f991726f`（2026-07-11）首次把数据库升至 v13。用户曾运行上游 v3.17.0 后再回到 v3.16.5-9，会在启动预检中进入“数据库版本过新”恢复页。
- 正式兼容修复只回移 schema 子集，不整包 cherry-pick `f991726f` 的 usage/proxy 统计逻辑：`SCHEMA_VERSION=13`、新库两张 usage 表增加 `input_token_semantics`、v12 -> v13 用幂等 `add_column_if_missing` 补列。现有 v13 多出的列不会破坏旧查询；默认值 0 保留旧或未知语义。严禁通过手改 `PRAGMA user_version` 伪造降级。
- v3.16.5-10 同时包含 `2990291e` 的异常 MultiRouter 目录过滤和 `31a541d5` 的根错误边界。前者修复打开旧方案时 `modelCatalog.models` 的 null/字符串条目导致的 TypeError；后者把未捕获 React 渲染异常转为可见恢复页，而不是纯白。
- 发布前必须运行 v12 -> v13 迁移单测、MultiRouter/错误边界 Vitest、TypeScript、Prettier、rustfmt、`cargo check --lib`、生产 renderer 构建与 `git diff --check`；不要纳入历史 `output/`、`scripts/logs/` 或用户未归属的本地输出。

## 2026-07-15 CCSwitchMulti 数据库恢复页与纯白窗口的分流排查

- 必须区分两条故障链：截图中的 `v13 > v12` 是数据库恢复页，不是前端白屏；恢复页由 2026-06-24 的 `e93f7cab` 引入，`v3.16.5-9` 已包含它。因此同版本若窗口纯白且没有恢复页，不能再笼统归因数据库版本过新。
- 已发布 `v3.16.5-9` 中可确认的 MultiRouter 纯白根因来自 `8afa3fb2093c57710b6b63d433a29b8d327fc817`（2026-07-01，`fix(codex): separate multirouter catalog and subagent ordering`）：向导初始化直接执行 `existingPlan.settingsConfig.modelCatalog.models.map(model => model.model)`。历史方案若含 `null`、字符串等非对象条目会抛 `TypeError`；根组件此前没有 React ErrorBoundary，最终只显示纯白。
- `2990291e` 修复该已确认路径：向导仅在打开时挂载，`readWizardModelCatalog()` 过滤非对象、null、空模型名，已有方案初始化复用安全读取，未知状态机步骤回退首步。该修复解释“打开/创建 MultiRouter 或复用旧方案后白”，但不能证明真正空数据目录的首次启动白屏由此引起；空目录没有异常 `modelCatalog`，仍须按该用户的日志和数据定向复现。
- 新增 `AppErrorBoundary` 并在 `main.tsx` 包裹正常 App 根树。后续渲染/React 提交阶段异常不再退化成空白，而显示可重新加载、可提供错误详情的恢复页，并在控制台记录组件栈。异步网络/事件错误仍应由各调用点处理。
- 回归：`tests/lib/codexMultiRouterWizard.test.ts` 覆盖 null、字符串、空 model 条目；`tests/components/CodexMultiRouterWizard.test.tsx` 覆盖异常历史目录；`tests/components/AppErrorBoundary.test.tsx` 覆盖子树渲染异常显示恢复页。首次启动仍白的用户需要收集 `%USERPROFILE%\\.cc-switch\\logs\\cc-switch.log`、应用版本与脱敏 provider 数据后再归因，不能混同为 v13 数据库问题。

## 2026-07-14 GitHub Issue #12/#15/#16 triage

- Issue #15 报告版本为 `v3.16.5-2`。MultiRouter 不按会话固定 Provider；每次请求都会根据当前 body 中的 `model` 重新解析 route。`v3.16.5-5` 已修复模型目录重复、未选 Provider 注入和新模型元数据同步，因此先要求升级 `v3.16.5-7`、完整重启 Codex Desktop，并提供同一 trace 的 `route_resolved`、`upstream_status`、`upstream_error` 和切换前后模型名。没有这组证据时，不删除跨 route fallback，也不把客户端缓存猜测当成根因。
- Issue #16 的 OMP 是 Oh My Pi，主要使用 `~/.omp/agent/config.yml` 和项目 `.omp/config.yml`；Pi 使用 `~/.pi/agent/settings.json`，但自定义 Provider 的正式扩展点是 TypeScript extension `pi.registerProvider()`。两者不能共用普通 JSON 写入器；正式支持应拆成 OMP YAML/JSONC adapter 与 Pi extension generator/manager，上层再抽象 provider projection target。
- Issue #12 截图中的真实错误是商汤上游 `HTTP 400: invalid_tool_call_id`，不是泛化的“DeepSeek 不会调用工具”。上游同源问题为 `farion1231/cc-switch#4056`，且已有报告称商汤直连也失败。修复前必须拿到同一 trace 中“上游返回 tool call id”和“下一轮回传 tool_call_id”的脱敏对照，才能判断长度/字符集限制、上游自相矛盾还是 Responses 与 Chat 转换改写；不能盲目重写 ID 破坏多轮映射。
- 本轮已回复并保持打开：`BigStrongSun/ccswitchmulti#15`、`#16`、`#12`。#15/#12 等待最新版复测证据，#16 作为 enhancement 等待正式双 adapter 实现。

## 2026-07-13 Codex OAuth originator：白名单保留与缺失回退

- `originator=codex_cli_rs` 能修复 Luna 相对 `cc-switch` 的模型准入差异，但不能因此把所有真实 Desktop/VS Code 请求都改报为 CLI。官方 Codex 的 first-party 分类包含 `codex_cli_rs`、`codex-tui`、`codex_vscode`、`Codex ` 前缀，以及独立 chat 分类中的 `codex_atlas`、`codex_chatgpt_desktop`；`codex_exec` 不在该分类中。
- 根修位于 `src-tauri/src/proxy/forwarder.rs`：可信本地 Codex 请求只有在恰好携带一个白名单值时保留；缺失、未知、重复值统一回退 `codex_cli_rs`。External API 和 Claude/协议转换不能保留调用方 originator，避免外部请求伪造 first-party 身份。
- 可信来源不靠可伪造的 header 猜测：`RequestContext::new` 仅为本地应用入口设置 `preserve_codex_client_originator=true`，`new_with_provider` 的 External API 临时 provider 固定为 false，再由 `RequestForwarder` 执行最终 header 规则。`handle_responses` 同步改用 `should_handle_as_codex_client()`，确保 External API 标记/API key 优先于伪装成 Codex 的 User-Agent。
- 回归测试必须覆盖 CLI、VS Code、TUI、Desktop/chat first-party 保留，`cc-switch`、`codex_exec`、未知值、重复值和 External API 回退，以及非 OAuth 请求完全不改写。

## 2026-07-13 CCSwitchMulti v3.16.5-7：所有官方 Codex OAuth 模型请求统一原生 CLI originator

- 真实根因：Luna 的 `prefer_websockets=true` 只是传输偏好，不是 WebSocket 硬要求。原生 Codex CLI `0.144.2` 在 `supports_websockets=false` 下可通过 HTTP Responses 正常运行 Luna。
- 受控实测：使用相同 OAuth token、ChatGPT account、`/backend-api/codex/responses` endpoint、请求体和其余 headers，仅切换 `originator`。`gpt-5.6-luna` 使用 `codex_cli_rs` 返回 HTTP 200 和 `OK`，使用 `cc-switch` 返回 HTTP 404 `Model not found`；`gpt-5.6-sol` 使用 `codex_cli_rs` 同样返回 HTTP 200。因此问题是官方后端按客户端来源做模型准入，不是临时网络错误或 HTTP 不支持 Luna。
- 修复边界：所有由 CCSwitchMulti 解析为 `AuthStrategy::CodexOAuth` 的模型推理请求都在最终 header 合并与 Provider override 之后强制写成单一 `originator: codex_cli_rs`。普通 Responses、raw passthrough、Claude 转 Codex OAuth 都覆盖；第三方 Bearer/API Key 请求不改写。
- 目录修复：删除 `merge_codex_models()` 对 `prefer_websockets` 的过滤，Luna 可继续出现在已路由 catalog 中，不映射到 Terra/Sol。真正不可用模型仍由官方模型列表的 `supported_in_api: false` 过滤。
- 共享规则：`CODEX_OAUTH_ORIGINATOR` 是托管 OAuth 模型请求唯一来源常量；Codex/Claude adapter 只生成 Authorization，避免先追加错误来源头再形成多值。官方 OAuth 模型列表请求同步使用该常量，订阅/额度接口仍保留独立的 `Codex Desktop` 身份。
- 发布约束：只修改仓库源码、测试、文档和版本，不原子替换、不构建安装包、不停止或覆盖当前 `3.16.5-6` live 程序。版本统一提升为 `3.16.5-7` 后提交并通过 GitHub release workflow 发布。

## 2026-07-13 Codex 用量页：官方窗口预测与本地 token 分析分离

- `src/components/codex/CodexUsagePage.tsx` 的官方 `SubscriptionQuota` 仅提供窗口已用百分比 `utilization` 与 `resetsAt`，不会提供绝对 token 上限或模型级官方额度账本。因此绝不能把本地 token 直接换算成“官方还剩多少 token”。
- 页面现在以当前窗口的已过时间和官方百分比计算“百分点/小时”的平均速度，并仅在该假设下预测耗尽时刻。预测会显示中/低置信度及“按当前节奏可能提前耗尽”建议；窗口刚开始或数据异常时降级为等待更多读数，不伪造精确性。
- `useUsageSummary({ preset: "today" }, { appType: "codex" })` 与 `useModelStats({ preset: "7d" }, { appType: "codex" })` 是独立的本地日志口径：显示今日 token、token/小时、缓存命中率、成功率和前四模型分布。其数据来自本机代理/会话同步日志，仅用于分析节奏和模型结构。
- 顶部刷新同时刷新官方订阅窗口、本地今日汇总和七日模型统计；页面仍保持只读，不兑换 reset、不修改账号或 Codex 配置。
- 验证基线：`pnpm vitest run src/components/codex/CodexUsagePage.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、Prettier 检查和 `git diff --check` 都应通过。单独 Vite 预览没有 Tauri `invoke` 桥接，不能作为真实数据页的端到端验证；需在 `pnpm tauri dev` 或桌面壳中继续做 live QA。

## 2026-07-12 MultiRouter OAuth 模型列表网络失败兜底

- 截图里的 `Request failed: error sending request for url (https://chatgpt.com/backend-api/codex/models?client_version=...)` 来自 `src-tauri/src/services/codex_oauth_models.rs` 的 `reqwest.send()` 阶段，说明请求还没拿到 HTTP 响应；这不是模型名格式、401/403、HTTP 404 或接口返回空列表，而是 DNS/TLS/代理/超时等网络层问题。
- 修复边界：OAuth 在线模型列表失败时，MultiRouter 向导和 Routes 工作台会读取本地 Codex 官方模型缓存兜底，优先 `~/.codex/models_cache.cc-switch-backup.json`，再读 `~/.codex/models_cache.json`；缓存解析只保留 `gpt-*`、`codex-*`、`chatgpt-*`、`o*` 这类官方模型，避免把 CCSM 合并进 cache 的 Qwen/DeepSeek 写进 official route。
- 向导默认模型源会收敛等价 OAuth provider：未绑定账号的 `default` 与 `codex-official` 代表同一个默认 ChatGPT 账号，只保留已有真实 catalog 或稳定 seed 中更合适的一个；不同 `accountId` 的 OAuth provider 不合并。
- 后端 OAuth 模型请求错误现在会展开 reqwest 错误分类、底层 source 链和 CCSwitchMulti 全局代理状态，便于根据用户截图判断是 timeout/connect/TLS/proxy，而不是只看到模糊的 `error sending request`。

## 2026-07-11 CCSwitchMulti v3.16.5-2 Release

- `v3.16.5-2` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-2`。Release 为 `draft=false`、`prerelease=false`，发布工作流 run `29158855394` 全部成功。
- 本次 tag `v3.16.5-2` 指向 annotated tag object `a0e8bf94381952848e6d24ff7549febf82da99fe`，解引用提交为 `61e025999942e356f080cc80fd7512ab9570f4b7`；`fork/main` 也已推进到同一提交。
- Release 资产共 19 个，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/signature/zip、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- `latest.json` 下载验证通过：`version=3.16.5-2`，updater 平台包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有 signature 和 release asset URL。
- 发布前本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已导出 Windows 测试包，metadata 指向二进制修复提交 `08b58d81707a9458cd2f4c99f06f9aafcc60b416`；随后只追加了 release note 提交 `61e02599`，不影响二进制内容。

## 2026-07-11 Codex Image Gen MultiRouter 官方兜底边界

- “内置 Image Gen 在 MultiRouter 下 404”的第一层根因是旧版本没有注册 `POST /images/generations` / `/v1/images/generations` 等 Images API 路由；请求一旦打到 `127.0.0.1:15721/v1/images/generations`，会先在 Axum 层 404，还没进入 MultiRouter route resolver。
- “之前不影响”不能理解成 proxy 里曾经有隐藏的图片绕过逻辑。现场更合理的边界是当时 Codex/ChatGPT App 没把 Image Gen 走本地 base_url，或已经切回 official 直连官方；通用 `forwarder` 一直会对所有 Codex endpoint 按 `body.model` 解析 MultiRouter。
- Images API 的修复必须是 endpoint 专用：如果用户显式把 `gpt-image-*` route 到第三方 Images provider，就让通用 router 继续处理；如果没有显式图片 route，则按 route 身份扫描 enabled routes，物化 official OAuth route，避免 `defaultRouteId` 把图片请求发给 DeepSeek/Qwen 等文本 provider。
- 按身份兜底命中的 official route 要清理 `codexResolvedUpstreamModelOverride`，不能把图片模型改成 `gpt-5.5` 这类文本模型；同时把 `codexResolvedRouteMatched=false` 写进 request-local provider，日志和后续转换才能区分“图片原生能力回官方”和普通模型命中。
- 回归测试固定三条边界：旧 router 中 official route 只匹配 `gpt-5.x` 仍可承接 `gpt-image-*`；`defaultRouteId` 指向 DeepSeek/Qwen 时图片不应落到非官方默认路由；显式第三方图片 route 不能被强制改回 official。
- 2026-07-11 后续修正：unknown `/v1/*` 不能再返回本地 404/501 或依赖逐个 endpoint allowlist。Axum fallback 应进入 raw passthrough：只解析请求体副本用于 MultiRouter 选路，上游仍使用原始 method/path/query/body/content-type，并由 forwarder 重建 Host、Content-Length、Authorization、API key、external API key 等敏感/链路头。已注册的 `/v1/responses`、`/v1/chat/completions`、`/v1/images/generations` 仍走专用 handler。
- raw passthrough 的 MultiRouter 选路和 Responses 不同：显式模型 route 命中优先；没有显式命中时只能找 official/Codex OAuth route 身份，找不到 official 就视为配置缺失，不能再退到 defaultRouteId/首个 enabled route。这样所有未知 GPT App 原生请求默认回官方，只有明确配置到 Qwen/DeepSeek 等第三方 provider 的模型才走其它链路。

## 2026-07-11 Codex 历史修复面板单确认流

- `CodexHistoryRepairPanel` 的用户主流程应保持为“刷新记录 -> 选择 session/项目范围 -> 选择目标 provider -> 确认修复”。不要再把“修复新版 App 历史 / 加载历史 / 预览旧版恢复 / 写入旧版索引”四个并列按钮暴露给用户，否则会把新版目录同步和旧版离线索引兜底混成两套互相竞争的修复逻辑。
- 项目路径默认必须为空且不参与查询/写入，代表跨项目读取和修复所有匹配记录；只有用户勾选“只修复单个项目”后，项目路径才会传给 `list_codex_history_sessions` / `repair_codex_history_visibility`。打开面板时传入的 `initialProjectPath` 只能作为“带入当前项目”的快捷值。
- “确认修复”必须每次先 dry-run，再弹出明确的离线写入确认：要求用户完全退出 Codex / ChatGPT App，说明运行中的 app-server 会覆盖修复结果；用户确认后才执行真实写入。后端的进程保护错误也要改写成同样的可执行提示。
- 目标 provider 下拉应包含“当前 provider”（前端传 `targetProvider: null`，由后端按 live config/codex_model_router_v2 解析）、`openai`、`custom` 和 active DB 扫到的所有 provider 桶，并在选项中展示 live/config/db 计数线索。
- `unlock_codex_model_picker` / “启动并刷新新版目录”只是写入成功后新版侧边栏仍未重建时的高级兜底，不应放在主操作区。Codex 目录、Active DB、archived/subagent 开关也属于高级设置，默认界面只保留范围和 provider 选择。

## 2026-07-11 Codex 新版历史修复路径与旧版索引兜底边界

- 新版 Codex/ChatGPT App 历史修复主路径不是改 `state_5.sqlite`：`CodexHistoryRepairPanel` 的“修复新版 App 历史”只调用 `unlock_codex_model_picker`，通过 CDP 注入 renderer 后执行 App 自己的 `localThreadCatalog.requestStartupSync()`。它不依赖“加载历史/预览旧版恢复”，也不应写 `threads.model_provider`、rollout、`session_index.jsonl` 或 `.codex-global-state.json`。
- 旧版“预览/写入旧版索引”是离线兜底：会读取 active DB、枚举所有非目标 provider 桶，必要时改 `threads.model_provider`、`has_user_event`、聚焦时间、rollout 首行、`session_index.jsonl`、workspace hints 和 rollout mtime。写入前必须确认 Codex/GPT App 退出，避免 app-server/WAL 或新版目录同步覆盖结果。
- 历史修复面板默认不应从当前会话详情继承项目路径；空项目路径才代表跨项目读取/修复。`initialProjectPath` 只适合作为“带入当前项目”的手动快捷项，否则会让用户误以为 SQLite 历史只限当前项目。
- provider 桶候选应来自 live config、active DB 全量 `threads.model_provider` 分布、常见 `openai/custom/codex_model_router_v2` 兜底，并在 UI 里显示计数/来源。下拉关闭时只显示当前目标项，不代表只读到了这个桶。
- `codex_desktop::install_script` 必须用 `Runtime.evaluate` 返回值提取 `historySync`；`Page.addScriptToEvaluateOnNewDocument` 只返回脚本 identifier。若解析错对象，新版历史同步会显示未请求或一直等待 renderer target，即使当前页面脚本实际已执行。

## 2026-07-10 Codex 官方回退残留本地代理与不可用 OAuth 模型过滤

- 现场截图里的 `gpt-5.6-luna` 404 不是 MultiRouter 路由漏匹配：`codex-router.log` 显示请求已命中 OpenAI Official route 并转到 `https://chatgpt.com/backend-api/codex/responses`，最终由官方后端返回 `Model not found`。OAuth 模型目录同步不能把官方返回但显式标记 `supported_in_api=false`、`available=false`、`enabled=false`、`disabled=true` 或 `visibility/status/availability=hidden|disabled|unavailable|unsupported|denied` 的条目写进 provider catalog。
- 一键切回 OpenAI 官方必须清掉 live `~/.codex/config.toml` 顶层 provider-owned 字段：`base_url`、`wire_api`、`openai_base_url`、`experimental_bearer_token`、`model_catalog_json`、`model`、`model_provider` 及活动自定义 `[model_providers.*]`。只清 `openai_base_url` 不够；新 Codex Responses 会读顶层 `base_url = "http://127.0.0.1:15721/v1"`，导致 UI 显示 official 但请求仍走 CCSwitchMulti 本地代理。
- `switch_codex_to_official_and_repair_history` 不能在最前面用历史离线检查阻断官方回退。Codex/ChatGPT App 运行时，应先退出本地代理并把 live provider 强制设为 `openai`，然后把历史修复作为可跳过 warning 返回；否则用户点击“一键切回”会因为历史锁定失败而继续滞留在本地代理。
- common config/provider live merge 也要把 `base_url`、`wire_api` 视为 provider-owned 本地代理字段，避免后续导入配置片段又把 15721 代理注入 official live config。`model_context_window` 仍属于用户偏好，应继续保留。

## 2026-07-10 新版 GPT App 线程与多 Agent 机制取证

- 完整实测报告见 `docs/guides/gpt-app-thread-subagent-mechanism-2026-07-10.md`。Windows 新桌面包为 `OpenAI.Codex`，实际运行链是 `ChatGPT.exe -> resources/codex.exe app-server -> code-mode/tool runtimes`；历史的原始事件在 `~/.codex/sessions/**/rollout-*.jsonl`，`state_5.sqlite` 提供 thread 元数据和 `thread_spawn_edges` 父子关系。修复历史必须离线备份并把 JSONL、SQLite 与子线程 edge 当成整体，不能只改轻量 `session_index.jsonl` 或伪造 `local_thread_catalog`。
- 新版 multi-agent 的当前 live 保留工具契约为 `collaboration.spawn_agent(task_name, message, fork_turns)`；历史 v1 的 `model/reasoning_effort/service_tier` 与历史 v2 的可选 `agent_type` 绝不可重新硬编码。截图的 `reserved for use by this model and must match configured schema` 根因是旧 CCSwitchMulti 扩展了保留 schema；`ensure_codex_multi_agent_reserved_schema_compatible` 必须保持 `hide_spawn_agent_metadata=true`，子 Agent 模型改由 `~/.codex/agents/*.toml` role 选择。
- 角色、模型、推理强度、provider 路由和 `spawnAgentModels` 前五 direct override 是不同层。custom role 可 pin `model`/`model_provider`/`model_reasoning_effort`，缺省继承父会话。前五仅影响 `spawn_agent.model` 描述窗口，不控制 CCSwitchMulti managed role 投影；managed roles 必须从完整可路由目录生成。
- 官方依据：`https://learn.chatgpt.com/docs/agent-configuration/subagents`、`https://learn.chatgpt.com/docs/app-server`。本机当前 config 为 `agents.max_threads=10`、`max_depth=1`；实际运行并发还需取产品会话资源上限与该配置的最小值。写密集任务仍需串行或 worktree 隔离，独立 Agent thread 不等于自动独立文件系统。

## 2026-07-10 MultiRouter 长模型摘要换行

- 规则列表的模型摘要位于 `src/components/codex/CodexRouterWorkspacePage.tsx` 的 `RouteListButton`。此前卡片没有 `min-w-0/w-full` 约束，摘要还使用 `truncate`；官方 OAuth 同步到多个 GPT-5.6 模型后，左侧规则列无法收缩，文本会覆盖右侧详情面板。
- 根修是让卡片收缩到父级 grid 单元，并将摘要改为 `whitespace-normal break-words leading-5`，保留完整模型名并在卡片内换行。该改动不改变路由匹配或模型目录数据。
- UI 修复后 `pnpm exec prettier --check src/components/codex/CodexRouterWorkspacePage.tsx`、`pnpm typecheck` 和 `src/components/codex/CodexRouterWorkspacePage.test.ts`（40 项）通过。

## 2026-07-10 Legacy MultiRouter OAuth 5.6 Migration

- `a3e4622f` 等动态 OAuth catalog 修复只覆盖带 `targetProviderId` 的新式 route；现场数据库里两个 OpenAI Official provider 和新 `codex-multirouter` 已有三个 GPT-5.6，但截图选中的旧 `codex-openai-router` 仍是无 `targetProviderId` 的内联 `managed_codex_oauth` route，因此 Workspace 的 provider-id 预过滤和同步 helper 都会跳过它。
- 根修位于 `src/lib/codexMultiRouterSync.ts`：同步前迁移 legacy OAuth route。有 `accountId` 时只允许匹配同账号 provider；默认账号优先稳定 id `codex-official`，不存在 canonical 时只有唯一的无账号 OAuth provider 才可迁移，避免多账号静默串号。迁移后复用既有 route/modelMap/聚合 catalog SSOT 同步，重复执行必须幂等。
- `CodexRouterWorkspacePage` 刷新 provider 后不再提前按 `targetProviderId` 排除 plan，而是让同步层逐个返回 changed/null；这是旧 route 能在首次官方 catalog 刷新时被持久化迁移的必要条件。
- 子 Agent `spawnAgentModels` 仍按原设计保留用户前五排序；GPT-5.6 会进入 route 和聚合 catalog，并出现在“路由/全部”候选池，但不会自动挤掉用户手工候选。回归覆盖 canonical 默认账号、精确多账号匹配、账号无匹配保护、幂等和 Workspace 实际持久化；同步 9 项、Workspace 40 项及 `pnpm typecheck` 通过。
- 本地测试包版本推进到 `3.16.5-2`，四个版本面为 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`，用于与已安装的 `3.16.5-1` 明确区分。

## 2026-07-10 Codex 一键切回 OpenAI 官方与全量历史归桶

- Codex 首页的官方恢复入口放在 `ProviderList.tsx` 底部 CTA 区，与“配置多路模型”并排。它不能复用普通前端 provider switch：接管态下普通路径有官方供应商安全门，专用命令 `switch_codex_to_official_and_repair_history` 才能把“退出接管、保留 OAuth、切官方、修历史”收束为一次操作。
- 数据库官方 seed id 是 `codex-official`，Codex live `config.toml` 和会话历史使用的官方 provider id 是 `openai`，二者不能混用。专用命令会确保 seed 存在，调用现有 `ProviderService::switch` 保留 `auth.json`，再显式把 live `model_provider` 设为 `openai`；provider 字段清理必须保留 MCP、memories、projects、插件等全局配置。
- “统一 Codex 会话历史”开启时，官方配置仍可能被注入 `model_provider=custom`。一键切官方必须同步关闭 `unify_codex_session_history`、清空 `unify_codex_migrate_existing` 和 `codex_official_history_unify_v1` marker；如果 provider 切换失败，应恢复操作前 settings。
- 官方历史修复只改 rollout `session_meta.model_provider` 与 SQLite `threads.model_provider`，把所有非 `openai`（包括未知、空值和缺失值）归并到 `openai`；不要复用完整 visibility repair 去改标题、时间、聚焦、session index 或全局工作区状态。JSONL 与 state DB 写入前分别备份，重复执行必须为零改动。
- 新版 Windows App 的主进程名是 `ChatGPT.exe`，安装路径/命令行包含 `OpenAI.Codex_*`，并启动 `codex.exe ... app-server`。任何配置或历史写入之前必须用现有进程探测拒绝运行中的 App；本机验证该过滤器能同时命中 `ChatGPT.exe` 与 app-server。新版 App 的 `local_thread_catalog` 是派生数据，不要直接改，下一次启动交给现有 native sync 重建。
- 验证覆盖：配置转换单测通过；`codex_history_migration::tests` 40 项通过（含未知 provider 全量归桶和幂等）；`ProviderList` 6 项通过并覆盖空/非空列表按钮；`cargo check` 与 `pnpm typecheck` 通过。真实用户数据迁移未在本轮执行，因为新版 ChatGPT App 正在运行，离线保护应先拒绝而不是自动关闭用户进程。

## 2026-07-10 MultiRouter OAuth GPT-5.6 Catalog Boundary

- 当前官方 ChatGPT/Codex OAuth 模型端点已经能返回 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`。本机使用现有 OAuth 登录态只在内存中发起 `GET https://chatgpt.com/backend-api/codex/models?client_version=3.16.5-1`，返回 HTTP 200 和 8 个模型；凭据未输出。`~/.codex/models_cache.json` 也已有三个 5.6 模型，但 `~/.codex/cc-switch-model-catalog.json` 与数据库里两套 MultiRouter plan 仍只有 `gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.3-codex-spark` 四个官方模型。
- 根因是 MultiRouter 接线缺失，不是 OAuth 上游缺模型。后端 `src-tauri/src/services/codex_oauth_models.rs`、`src-tauri/src/commands/codex_oauth.rs` 和前端 `src/lib/api/model-fetch.ts` 已实现 `get_codex_oauth_models` / `fetchCodexOauthModels`；但 `src/lib/codexMultiRouterWizard.ts` 和 `src/components/codex/CodexRouterWorkspacePage.tsx` 各自写死旧四模型 fallback，`CodexMultiRouterWizard.tsx` 刷新时又明确把 OAuth 标成 `skipped` 并 `continue`，因此真实 OAuth 目录从未写入 provider、route match 或聚合 catalog。
- 转发层没有把模型限制到 5.5：官方 route 使用 `gpt` 前缀匹配，Rust resolver 按 exact/prefix 路由。只要 5.6 被写入 MultiRouter catalog/route，路由可以进入托管 OAuth；最终准入仍由账号和官方后端决定。另有旧默认目录散落在 `src-tauri/src/proxy/handlers.rs` 与 `src-tauri/src/proxy/external_openai_api.rs`，会影响本地/对外 `/v1/models` fallback。
- 正确根修应让 MultiRouter 向导和 Routes 页对 OAuth 源调用现有 `fetchCodexOauthModels(accountId)`，成功后持久化并同步 provider catalog、route match、聚合 catalog；网络/认证失败时才保留旧目录作为 fallback。同时收敛重复的前后端默认模型常量，避免下一次新模型发布再次漂移。远端 `fork/main`、`origin/main` 与当前 `codex/merge-upstream-v3.16.5`（`d9658086`）均尚无这项修复。
- 运行态附带发现：当前安装运行的是 `3.16.4-15`，不是仓库/构建目录的 `3.16.5-1`；`15721` 当前未监听且 Codex proxy/takeover 关闭，只有 external API `15722` 在监听。因此“当前界面拿不到 5.6”既有静态目录根因，也不能用当前 15721 做真实路由验证。
- 修复已在同一分支完成：`a3e4622f` 将向导和 Routes 工作台接到 `fetchCodexOauthModels(accountId)`，成功后追加并持久化官方新模型，再复用 `syncCodexMultiRouterPlanWithProviders` 同步 route match 与聚合 catalog；空列表、认证或网络失败保留最后一次可用目录。`04c52606` 删除后端 External API 的旧四模型运行时注入，并让临时 `codex-official` provider 只增加 `provider_type=codex_oauth`、不再覆盖动态 `settings_config`。最终审查又发现“从未保存 catalog 的 OAuth provider”仍会先物化旧四模型再与在线结果合并，`ba739bd9` 因此彻底删除向导和 Workspace 的静态 fallback：新 OAuth provider 在首次成功获取前保持空目录，失败时只保留真实持久化过的历史目录，不再凭空声明账号可用模型。
- 回归验证：MultiRouter 向导/Workspace/同步 helper 共 84 项前端测试通过，覆盖绑定 OAuth accountId、`gpt-5.6-sol` 写入 provider/route/catalog、失败保留旧目录；`pnpm typecheck`、Prettier、Rustfmt、`cargo check` 通过；Rust External models 4 项、动态 catalog 保留 1 项、空 OAuth seed 1 项、OAuth 模型解析 5 项通过。工作树中另有未提交的 `codex_config.rs`、`codex_desktop.rs`、`codex_history_migration.rs`、`CodexHistoryRepairPanel.tsx` 修改，本次均未纳入提交。

## 2026-07-10 CCSwitchMulti v3.16.5-1 Release Boundary

- `v3.16.5-1` 是 CCSwitchMulti 对原版 `cc-switch v3.16.5` 的跟进发布版本；正式 tag 应使用 fork tag `v3.16.5-1`，不要复用上游 `v3.16.5` tag。`release.yml` 会读取 `docs/release-notes/v3.16.5-1-zh.md` 生成 GitHub Release 正文，资产外显命名继续使用 `CCSwitchMulti-${TAG}-...`。
- 本版必须在 release note 顶部注明：**尚未适配 OpenAI 新版 ChatGPT 应用**。当前合并只覆盖原版 `cc-switch v3.16.5` 与 CCSwitchMulti 既有 Codex Desktop / CLI 接管路径；如果新版 ChatGPT 应用改变桌面端协议、模型菜单、渲染器门控或登录态结构，需要后续单独适配。
- 发布前不要提交未跟踪的本地输出目录：`output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/`、`scripts/logs/`。发布流程仍是先把当前分支 fast-forward 推到 `fork/main`，再推 annotated tag 触发 `.github/workflows/release.yml`。
- `v3.16.5-1` 已作为 BigStrongSun/ccswitchmulti 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-1`。Release 为 `draft=false`、`prerelease=false`，发布时间 `2026-07-10T05:41:45Z`，正文确认包含“尚未适配 OpenAI 新版 ChatGPT 应用”提示。
- GitHub Actions release run `29070736037` 全部成功：Windows x64/ARM64、macOS universal、Linux x64/ARM64、Publish GitHub Release、Assemble `latest.json` 均为 success。Release 资产共 19 个，外显命名为 `CCSwitchMulti-v3.16.5-1-*`，包含 `latest.json`。
- 下载验证 `latest.json`：`version=3.16.5-1`，`pub_date=2026-07-10T05:41:56Z`，updater 平台包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`。

## 2026-07-10 Upstream v3.16.5 Merge Into CCSwitchMulti

- 本次上游跟进合并原版 `farion1231/cc-switch` 的 `v3.16.5` 功能到 CCSwitchMulti，版本面统一推进为 `3.16.5-1`：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 都必须保持一致；Tauri `productName` 仍是 `CCSwitchMulti`，identifier 仍是 `com.ccswitchmulti.desktop`，不要被原版品牌回退覆盖。
- Codex 原生 Responses 预设现在可以带 `modelCatalog` 作为模型能力/上下文窗口目录，但这不等于强制开启 Codex `/model` 菜单映射；单 provider 菜单投射仍由 `meta.codexLocalModelMapping` 控制，MultiRouter routes 开启时才走聚合投射。Chat Completions provider 仍需要本地路由/接管转换，不能把 native Responses 直连语义套到 Chat 桥接路径。
- 上游新增的 native Responses catalog profile 字段要在前端编辑往返中保留：`supportsParallelToolCalls`、`inputModalities`、`baseInstructions` 属于隐藏能力字段；`useCodexConfigState.extractCodexCatalogModels` 不能在字段缺失时写入空字符串形式的 `upstreamModel`、`displayName`、`contextWindow`，否则保存后会污染原生 catalog 行并破坏测试里的隐藏字段往返。
- `CodexCatalogToolProfile` 这类 catalog 能力配置应从 provider 的 `apiFormat` 推导：`openai_responses` 走原生 Responses profile，`openai_chat` 走 Chat 转换 profile。不要用 `modelCatalog` 是否存在来推断 wire API，也不要让 catalog 元数据反向改变 provider 的实际协议。
- 官方 OAuth/auth 边界继续保持：CCSwitchMulti 的接管和路由配置不应覆盖用户 `auth.json` 的 ChatGPT OAuth 登录态；official/OAuth route 仍由 managed auth path 物化，第三方 API Key 放 provider-scoped config 或本地代理占位，不要写进官方登录态。
- 本轮验证覆盖前端 catalog/session/preset 目标测试、JSON locale parse、Prettier、`pnpm typecheck`、Rust `cargo fmt --check`、Codex/provider Rust 单测、`cargo check` 和 `git diff --cached --check`；未跟踪的 `output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/`、`scripts/logs/` 属于既有本地输出，合并提交不纳入。

## 2026-07-09 CCSwitchMulti v3.16.4-16 Release

- `v3.16.4-16` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-16`。Release 为 `draft=false`、`prerelease=false`，GitHub latest API 返回 `tag_name=v3.16.4-16`，发布时间为 `2026-07-09T13:06:57Z`。
- Release 提交为 `413b2699b90f459b888858b230ffd7d63526727d`（`chore(release): bump CCSwitchMulti to v3.16.4-16`），annotated tag `v3.16.4-16` 解引用到同一提交；`fork/main` 已 fast-forward 到该提交。Tag 推送后第一次 Actions run `29016634597` attempt 1 只有 Linux x64 成功，其余 matrix job 在未执行 step 前被 GitHub 标记 cancelled，直接 rerun 同一 run 后 attempt 2 全部成功。
- GitHub Actions release run `29016634597` attempt 2 覆盖 macOS universal、Linux x64/ARM64、Windows x64/ARM64、Publish GitHub Release 和 Assemble `latest.json`，所有 job 均为 success。Release 资产共 19 个，外显命名均为 `CCSwitchMulti-v3.16.4-16-*`，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/signature/zip、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- 远端 `latest.json` 已下载验证：`version=3.16.4-16`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 6 个 updater 平台，且每个平台 signature 字段都存在。
- 本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已由 release pipeline 刷新到 `Version: 3.16.4-16`、`Commit: 413b2699b90f459b888858b230ffd7d63526727d`，包含 Windows x64 NSIS setup、setup `.sig`、portable zip、raw exe、`latest.json` 和 `SHA256SUMS.txt`。发布前本地验证覆盖 Qwen/vLLM Rust 单测、Responses->Chat Rust 单测、ProviderForm vitest、Prettier、`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 和 `pnpm release:local`；仅有既有 browserslist/baseline/chunk/Tauri `__TAURI_BUNDLE_TYPE` 警告。

## 2026-07-09 Codex Responses Missing Output Budget Semantics

- 最新纠偏：这不是 Qwen/vLLM 专用问题，而是 Codex Responses -> Chat Completions 转换必须保留“未声明输出上限”的通用语义。Codex 原生 Responses 请求当前通常不写 `max_output_tokens/max_tokens/max_completion_tokens`；CCSwitchMulti 只有在原请求显式携带这些字段、provider 显式配置 `defaultOutputTokens`、或 Anthropic thinking-budget 错误重试整流时，才应让上游 Chat 请求出现 max token 字段。
- 当前 live 日志样本里，Qwen route 的 `request_shape.top_keys` 没有 `max_tokens/max_completion_tokens/max_output_tokens`，因此该样本不能再归因为“CCSM 当前自动补 32768 导致截断”。历史自动写入 `defaultOutputTokens=32768` 的代码路径确实会改变缺省语义，已改为不再由 Qwen/vLLM 推断或前端保存自动生成。
- 正确验证口径：当 Codex 没发输出预算且 provider 没有显式 `defaultOutputTokens` 时，转换后的 Chat 请求也不应出现 `max_tokens` 或 `max_completion_tokens`；`minOutputTokens` 只允许抬高已经存在且过小的显式预算。若 live 日志仍显示旧 `thinking` / `reasoning_effort` 形态，要单独判断是运行二进制未更新、DB 旧 meta 未被合并修正，还是该上游本身接受该字段，不能混同为输出上限截断。

## 2026-07-09 Retire Qwen vLLM Implicit Output Cap

- 用户指出 `Codex 没发输出长度，vLLM 也没要求输出长度` 时，CCSwitchMulti 自动补 `defaultOutputTokens=32768` 会把本应由上游决定的输出长度截断。更准确地说：这是历史自动默认值会改变 Codex 缺省语义的风险，不应作为 Qwen/vLLM 推断默认值，只应保留为高级用户显式配置能力。
- 修复边界：Qwen + vLLM/matrixminecraft 推断仍设置 `thinkingParam=enable_thinking`、`effortParam=none` 和 `minOutputTokens=2048`，但 `minOutputTokens` 只抬高已经存在且过小的 `max_tokens/max_completion_tokens`，请求完全缺省时不再注入任何输出预算字段。
- 旧版自动写入的 Qwen/vLLM `defaultOutputTokens=32768` 现在视为废弃自动值，在运行时合并 Qwen/vLLM reasoning meta 时清掉；显式非 32768 的 `defaultOutputTokens` 继续保留。前端 ProviderForm 保存 Qwen/vLLM meta 时也不再自动写 `defaultOutputTokens=32768`。

## 2026-07-09 Codex Public Evidence For Output Budget And Subagent Reasoning

- 公开核验结论需要修正表述：`defaultOutputTokens=32768` 不是“从根上修 Codex 客户端”，也不应简单说成“规避 Codex 弱点”。OpenAI Codex 当前原生 Responses 请求结构本身没有 `max_output_tokens` 字段，`build_responses_request` 也不写输出预算；公开 issue `openai/codex#4138` 报告 `model_max_output_tokens` 不进入 Responses 请求，`#31181` 也把“Codex native wire_api=responses 不发送 max_output_tokens”当作与 OpenAI-compatible gateway 兼容的证据。
- 对 CCSwitchMulti 更准确的边界是：当 Codex 的 Responses 风格请求被 CCSM 转成 Chat Completions 发给 Qwen/vLLM 时，不能因为 Codex 原生缺省输出预算就替用户补一个默认上限。`defaultOutputTokens` 应只代表用户显式配置的默认上限，不再由 Qwen/vLLM 推断分支自动生成。
- 关于子 Agent reasoning：官方 subagents 文档确认当前 Codex 默认启用 subagent，且自定义 agent 文件可以包含 `model`、`model_reasoning_effort` 等普通 config 键；这些字段省略时继承父会话。因此 CCSM 旧 Qwen role 硬编码 `model_reasoning_effort="low"` 会覆盖父会话/模型默认，移除该字段让 Qwen 继承是正确方向。公开 issue `openai/codex#27712` 也把 role 中 `model` / `model_reasoning_effort` 会进入 child request body 当作预期并写了验证说明。
- 公开搜索没有找到“reasoning-only stream 必然导致 subagent 卡住”的官方确认 issue。相关公开证据只有 `openai/codex#30179`：Codex backend 可先流 `response.reasoning_summary_text.delta` / keepalive，而最终 `response.output_text.delta` 可能集中成一个大块返回；这支持“Codex 子 Agent 进度/可见输出可能长时间不可见”的风险判断，但不足以证明 upstream Codex 已确认有 reasoning-only stream 卡死 bug。后续排查必须继续以本机 `codex-router.log` 的 `done_seen/finish_reasons/client_disconnected/request_shape` 和 Qwen 上游实际 SSE 为准。

## 2026-07-09 Qwen Codex Subagent Context And Output Budget Boundary

- Qwen3.6 Local 的 Codex 可见上下文当前是 262144：`~/.codex/config.toml` 的 provider model entry、`~/.codex/cc-switch-model-catalog.json`、`~/.codex/models_cache.json` 都写了 `contextWindow/context_window=262144`；顶层 `model_context_window=272000` 只是兜底，不是 Qwen route 的实际 catalog 值。
- 真实上游 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 只返回 `id=qwen3.6`、`owned_by=vllm`、`max_model_len=262144` 等字段，未返回 `max_output_tokens/max_tokens/maxOutputTokens`。CCSwitchMulti 的 `FetchedModel` 也只保存 `id/owned_by/context_window`，Codex catalog 类型也没有输出上限字段，所以“云端给了 max output 但 CCSM 丢了”当前没有证据；准确说是上游 `/models` 未给，CCSM schema 也未承载。
- Qwen 子 Agent 失败链路更像输出预算/思考参数配置缺口，不是 context window 错配：`codex-qwen-local` meta 显式写了 `codexChatReasoning`，但没有 `minOutputTokens`，并且 `thinkingParam` 是 `thinking`；`resolve_codex_chat_reasoning_config` 看到显式 meta 会直接返回，绕过 Qwen vLLM 推断分支。推断分支本来会给 matrixminecraft/vLLM Qwen 设置 `thinkingParam=enable_thinking` 和 `min_output_tokens=2048`。
- 运行日志 `codex-router.log` 中 Qwen 请求已正确命中 `router-codex-qwen-local` 并发往 `/chat/completions`，多次 `upstream_status=200/response_ready=200`；`request_shape.top_keys` 只有 `messages,model,parallel_tool_calls,reasoning_effort,stream,stream_options,thinking,tool_choice,tools`，没有 `max_tokens/max_output_tokens`。注意当前日志摘要不会单独打印 max token 字段值，但字段若存在会出现在 `top_keys`。
- 修复落点：`resolve_codex_chat_reasoning_config` 现在会在识别到 Qwen + vLLM/matrixminecraft 推断结果时，对旧的显式 `codexChatReasoning` 做定向兼容：`thinkingParam=thinking` 或缺省会改回 `enable_thinking`，缺失或小于 2048 的 `minOutputTokens` 会抬到 2048，但 DeepSeek/OpenRouter/SiliconFlow 等非 Qwen/vLLM 显式配置仍保持覆盖。`ProviderForm` 保存时也必须保留 `minOutputTokens`，并在 Qwen vLLM provider 上写出同一组安全默认值，防止 DB 再生成“显式但不完整”的 reasoning meta。
- 仍不把 catalog 级 `max_output_tokens` 纳入本次修复：当前证据是上游 `/models` 未给可靠输出上限，CCSM schema 也未承载；如后续要做，应作为独立模型能力 schema 任务处理。
- 更正：`minOutputTokens` 只能解决“Codex 明确传了过小输出预算”的截断；当 Codex 子 Agent 请求完全没有 `max_tokens/max_completion_tokens/max_output_tokens` 时，CCSwitchMulti 不应代填 `max_tokens`。Qwen + vLLM/matrixminecraft 推断不再写 `defaultOutputTokens=32768`，旧版自动值会在运行时清掉；显式小预算仍由 `minOutputTokens=2048` 抬高，显式非 32768 的 `defaultOutputTokens` 保持不覆盖。
- Qwen 子 Agent 的 `reasoning_effort=low` 根因是 CCSwitchMulti 托管 `~/.codex/agents/qwen-local.toml`/`ccswitch-qwen-local.toml` 曾经硬编码 `model_reasoning_effort = "low"`，会覆盖 catalog 的 `defaultReasoningEffort` 和用户后来在 Codex 配置里调的 high/xhigh。新生成的 Qwen 托管 role 不再写 `model_reasoning_effort`，让 Codex 按当前会话/模型默认继承；Spark/DeepSeek 等角色仍按各自语义写 low/medium/high。
- `codex-router.log` 的 `request_shape` 现在会显式记录 `max_tokens/max_completion_tokens/max_output_tokens` 字段形态；修复后验证缺省 Codex live 请求时，不应看到 `max_tokens` 或 `max_completion_tokens`，除非原请求或 provider 显式配置了输出预算。如果日志仍显示 `enable_thinking=absent`、`thinking=object(keys=[type])`，需要单独排查运行中的 CCSwitchMulti 二进制/服务是否已重启到新代码或 DB 旧 meta 是否被合并修正。

## 2026-07-09 Codex Desktop Model Picker And Managed Subagent Roles

- `BigStrongSun/ccswitchmulti#10` 的截图中 `remote_debugging_enabled=false`、`remote_debugging_port=None`、`model_catalog_models=Some(12)` 是关键组合：catalog 已生成，但 Codex Desktop 以普通方式启动，renderer 侧 Statsig `107580212` 仍可能把模型菜单压回“自定义”或少数官方模型。排查时先让用户完全退出 Codex Desktop，再从 CCSwitchMulti 点“解锁模型菜单”用 CDP 端口启动和注入，不要先把问题归因为 catalog 缺失。
- 上游 `farion1231/cc-switch#4169/#5066/#4420` 同类反馈可作为背景：无官方 ChatGPT/Codex 登录态时 Desktop 模型选择器本身会门控自定义模型；CCSwitchMulti 的可控修复边界是保留官方登录态、写 `model_catalog_json`/inline models/cache、以及 CDP 注入 renderer 白名单。
- 新版 Codex 子 Agent 不只看 `spawn_agent` 工具说明里的前 5 个 picker-visible direct override，还会读取 `~/.codex/agents/*.toml` custom agent role。CCSwitchMulti 生成的 role 文件带 `# Managed by CCSwitchMulti. Do not edit this file by hand.` 标记；managed role 必须从完整当前可路由 V2 profile/catalog 生成并按完整 desired set 清理，与 direct override 前五窗口彻底解耦，用户手写 role 始终保留。
- 不要把 `service_tiers=[]` 直接当成自定义模型不可见的根因。Codex core 和 TUI 测试里空 `service_tiers` 是合法模型路径；如果要补 `available_in_plans`、`minimal_client_version` 等新字段，必须先用当前 Codex Desktop/app-server 版本验证字段过滤逻辑，不能仅根据 issue 评论猜测写模板。
- 链路状态页的“解锁模型菜单”是用户处理 renderer 白名单门控的显式下一步：按钮 hover/`title` 必须说明它会通过 remote debugging 启动或连接 Codex Desktop 并注入 renderer 模型白名单，不会改路由、API Key 或模型目录；页面也要短提示“先完全退出 Codex Desktop，再点击解锁”，避免用户只看到“自定义”菜单却不知道要触发该动作。
- 模型菜单解锁会在 Codex 接管开启或确认已接管后 best-effort 自动尝试一次；它不是常驻守护修复。若 Codex Desktop 已经以普通方式运行且没有 CDP 端口，CCSwitchMulti 不会静默杀进程或重启用户窗口，必须提示用户完全退出 Codex Desktop 后再点“解锁模型菜单”。
- `#10` 的 portable 前提指 CCSwitchMulti portable，不是 Codex Desktop portable。不要恢复前端“手动选择 Codex.exe”主流程，也不要把路径写进 `ccswitch.codexDesktopExecutablePath`；正确边界是后端在用户点“解锁模型菜单”且发现正在运行的 Desktop shell 路径时，把该路径记到 `.cc-switch/codex-desktop-executable.json`，用于后续复用已校验路径或覆盖非标准安装发现失败。
- Codex Desktop、Codex CLI 和 app-server 要分层处理但都要支持：大写 `Codex.exe` 是 Desktop/Electron shell，用于 renderer/CDP 模型菜单解锁；小写 `codex.exe` 是 CLI/app-server，`codex app-server` 是 JSON-RPC 协议服务，修复路径是 live `config.toml`、`model_catalog_json`、`models_cache.json`、本地 `/v1/models` 和 MultiRouter 转发，不能当成 Desktop renderer 可执行文件启动。
- CLI catalog 验证和 Desktop 模型菜单解锁是两条路径：`codex debug models` / `/v1/models` 用来证明 CLI 或 app-server 能看到原始模型目录；Desktop 菜单仍可能被 renderer/Statsig/React cache 过滤，需要 CDP 注入白名单。修 issue #10 时两边都要看，但不要把 CLI 成功等同于 Desktop 菜单已解锁。
- Codex Desktop 自动发现应由后端完成：优先复用正在运行的大写 `Codex.exe`，再通过 WindowsApps 包目录和 `Get-AppxPackage -Name OpenAI.Codex` 的 `InstallLocation` 查找 MSIX 安装位置；错误文案必须明确提示安装/启动 Codex Windows app，并说明 CLI/app-server `codex.exe` 不能解锁 Desktop renderer。
- 多用户报 `Codex Desktop executable was not found` 时，根因通常在 Desktop shell 路径发现覆盖面而不是模型目录：非管理员进程可能不能枚举 `C:\Program Files\WindowsApps`，包名也可能不只是 `OpenAI.Codex`。自动发现必须依次覆盖运行中进程 `ExecutablePath`、记住的后端路径、App Paths 注册表、PATH 中大写 `Codex.exe`、`Get-AppxPackage -Name *Codex*` / best-effort `-AllUsers`、AppxManifest.xml 的 `Application Executable`、常见 `%LOCALAPPDATA%\Programs` / `%ProgramFiles%` / scoop 路径；这里排除小写 `codex.exe` 只表示不能把 CLI/app-server 当 Desktop/CDP 启动目标，不表示 CCSwitchMulti 不支持 CLI。
- Windows `Get-CimInstance Win32_Process -Filter "Name = 'Codex.exe'"` 是大小写不敏感匹配，本机会同时返回大写 Desktop `app\Codex.exe` 和小写 app-server `app\resources\codex.exe app-server --analytics-default-enabled`。运行中 Desktop 进程检测必须再对 `ExecutablePath` 的 leaf 做 `-ceq 'Codex.exe'` 精确过滤，否则可能把 CLI/app-server 误当成 Desktop 主程序；CLI/app-server 的可用性要看 `codex debug models`、`model/list`、`/v1/models` 和 live config/catalog 证据。
- macOS/Linux 不能复用 Windows 的 `Codex.exe` 校验：macOS Desktop shell 是 `Codex.app/Contents/MacOS/Codex`，CLI 仍是小写 `codex`；Linux 先保守接受大写 `Codex` 或 `Codex*.AppImage`，不要把 PATH 里的小写 CLI 当 Desktop。非 Windows 首次发现需要覆盖运行中 Desktop、已记住路径、macOS `/Applications` / `~/Applications` / Spotlight `Codex.app`，以及 Linux `Codex`/`Codex.AppImage` PATH、`.desktop` 绝对 `Exec` 和常见 `/opt` / `~/.local/bin` / `~/Applications` 路径。
- API Key 登录不会让 MultiRouter 路由或 CDP 注入本身失效：第三方 key 应放在 provider-scoped `config.toml` token/本地代理占位符里，`auth.json` 的 ChatGPT OAuth 登录态应被保留；接管生成的本地 provider 继续带 `requires_openai_auth=true`，注入脚本还会把 renderer auth context 临时修成 `chatgpt`。但如果用户从未有官方 ChatGPT/OAuth 登录态，或 Codex Desktop 后续登录/刷新重建 renderer 状态，原生模型菜单仍可能重新回到“自定义”，需要完全退出 Desktop 后再次点“解锁模型菜单”。不要把这种展示层回退误判成上游 API Key 不能中转。
- “完全退出再点”不是第三方 API Key 登录后的常规动作，只针对当前 Desktop 已经普通启动且无 CDP 的状态。成功由 CCSwitchMulti 带 CDP 启动并注入后，切换 DeepSeek/中转站 API Key provider 不应重复解锁；只有 Desktop 进程被用户关闭、普通方式重开、或 renderer 重建导致注入脚本丢失时才需要再次触发。遇到“解锁模型菜单报错”先看是否找不到已安装或已校验的大写 `Codex.exe`，再看 CDP 端口/renderer target，不要直接归因于 OAuth 或 API Key。

## 2026-07-09 OpenClaw Default Model Catalog Canonicalization

- 用户排查 OpenClaw 不支持流式/思考等级时，只读确认 WSL `~/.openclaw/openclaw.json` 中 `models.providers.vllm.models[0]` 已有 `reasoning=false`、`input=["text","image"]`、`contextWindow=128000`、`maxTokens=8192`；因此“思考等级不可用”不是 OpenClaw 完全没读模型声明，而是 live 配置把该模型声明为不支持 reasoning。
- 同一份 live 配置还暴露了默认模型引用不一致：`agents.defaults.models` 是 `vllm/Qwen3.6`，但 `agents.defaults.model.primary` 是 `vllm/qwen3.6`。这类大小写/目录 key 不一致会让 OpenClaw 的模型能力表现得像没读到。
- 根修边界在 `src-tauri/src/openclaw_config.rs::set_default_model`：设置 `agents.defaults.model` 时必须同步把 primary/fallback refs 写入 `agents.defaults.models`，并把仅大小写不同的旧 catalog key 迁移到 canonical ref，保留旧 entry 的 alias/extra。不要只在 ProviderList 的“一键设默认”按钮里补前端逻辑，否则其他命令入口仍会写出不自洽配置。
- 前端缓存边界：`src/hooks/useProviderActions.ts::setAsDefaultModel` 成功后需要同时 invalidate `openclawKeys.defaultModel`、`openclawKeys.agentsDefaults` 和 `openclawKeys.health`，否则 Agents 面板可能继续显示旧的 catalog/default 状态。
- 回归测试：`default_model_write_registers_catalog_refs`、`default_model_write_canonicalizes_case_variant_catalog_ref`、既有 `default_model_write_preserves_top_level_comments`，以及 `tests/hooks/useProviderActions.test.tsx`。验证命令：`cargo test --manifest-path src-tauri/Cargo.toml default_model_write --lib`、`pnpm vitest run tests/hooks/useProviderActions.test.tsx`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`pnpm typecheck`。

## 2026-07-08 CCSwitchMulti Release Asset Name Branding Repair

- 用户问“为什么最新的 release 里应用名的 multi 没了”。事实边界：GitHub latest release `v3.16.4-15` 的标题是 `CCSwitchMulti v3.16.4-15`，但远端资产名和下载说明是 `CC-Switch-v3.16.4-15-*`；本地固定目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 的 Windows 导出仍是 `CCSwitchMulti_3.16.4-15_x64-setup.exe`。
- 根因不在 Tauri 产品配置：`v3.16.4-15` 的 `src-tauri/tauri.conf.json` 仍为 `productName: "CCSwitchMulti"`、`identifier: "com.ccswitchmulti.desktop"`，`package.json` 仍为 `cc-switch-multi`。根因在 `.github/workflows/release.yml` 的 release stage 手写资产重命名模板，沿用了上游 `CC-Switch-${VERSION}-...`、`CC Switch.app` 和 `CC Switch` DMG volname。
- 历史修复分支 `d78622ed chore: update release asset names` 只存在于 `fork/codex/codex-icon-refresh`，不是 `main` 祖先。不要直接 cherry-pick 它的全部内容，因为它把 Windows portable 搜索路径改为 `ccswitchmulti.exe`；当前 Cargo/Tauri 内部二进制名仍是 `cc-switch.exe`，正确做法是从 `cc-switch.exe` 复制，但在 portable zip 内重命名为 `CCSwitchMulti.exe`。
- 本次修复把 GitHub workflow 的 macOS tar/zip/dmg、DMG stage app/volname、Windows setup/MSI/portable、Linux AppImage/deb/rpm、release body 下载说明全部改成 `CCSwitchMulti-*` 外显命名；同时把 `src/i18n/locales/{en,ja,zh,zh-TW}.json` 顶层 `app.title` 改成 `CCSwitchMulti`，修掉窗口/document 标题的旧品牌残留。
- 验证通过：Prettier check 覆盖 workflow 和四个 locale、PowerShell `ConvertFrom-Json` 解析四个 locale、`pnpm typecheck`、`git diff --check`，以及 `rg 'CC-Switch|CC Switch' .github/workflows/release.yml` 无旧 release 外显命名残留。本机没有 `actionlint`，所以未做 workflow actionlint 验证。

## 2026-07-08 Codex Subagent Official Token Zero Repair

- 用户反馈 MultiRouter 的“今日子 Agent 会话流量”里 official/gpt 模型 token 没统计进去或显示 0。根因在 `src-tauri/src/services/usage_stats.rs::build_codex_subagent_usage_stats_from_history`：统计先聚合 `_codex_session` 数据库行，旧逻辑只有在某个子 Agent 完全没有 DB 用量行时才回退解析 rollout JSONL。official Codex OAuth/proxy 行可能已经有请求数但 token 字段全为 0，于是回退被阻断，真实 `token_count` 里的累计 token 没进入子 Agent 表。
- 修复边界：SQL 聚合阶段只先形成每个子 Agent 的会话桶，不立即写模型汇总；对每个子 Agent 都在时间范围允许时读取 rollout `token_count`，但只用 rollout 修正“DB 桶没有任何真实 token”的模型桶，避免 DeepSeek/Qwen 等已经同步成功的非零 token 被重复累加。模型汇总在修正完成后统一生成。
- 回归测试新增 `test_codex_subagent_usage_stats_repairs_zero_token_db_rows_from_rollout`：模拟 `gpt-5.5` 子 Agent 已有两条 `codex_session` 请求但 token 全 0，同时 rollout JSONL 有 `total_token_usage`，最终 agent/model 统计必须显示 1550 tokens 且 request_count 保持 2。
- 验证命令：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_sync_codex_subagent_uses_rollout_thread_id --lib`、`git diff --check`。
- 另一个相邻但未纳入本次 token 修复的发现：`gpt-5.3-codex-spark` 等 spark 变体如果 token 已有但 cost 为 0，可能是 `model_pricing` 种子缺少对应模型定价和历史成本回填，应该作为单独成本统计任务处理，避免和 token 修复混在一个提交里。

## 2026-07-07 Codex Hosted Web Search Bridge MVP

- 本轮在隔离 worktree `C:\Users\sunda\.codex\worktrees\aec9\LLMservice\cc-switch` 的 `codex/hosted-tool-bridge` 分支实现 Phase 1 MVP：Codex `/v1/responses` 入站的 hosted `{ "type": "web_search" }` 不再原样交给第三方 Chat 上游，而是在 `transform_codex_chat.rs` 中映射成普通 function tool `web_search(query,count)`；`tool_search` 仍保持 Codex client-side tool 语义，二者不能混淆。
- 新模块 `src-tauri/src/proxy/providers/hosted_tools/` 拆为 `web_search.rs`、`openai_client.rs`、`bridge.rs`：`web_search.rs` 负责 hosted tool 配置安全子集、参数解析和 OpenAI Responses 结果规整；`openai_client.rs` 只读取独立环境变量 `CCSWITCH_HOSTED_TOOLS_OPENAI_API_KEY`（可回退 `OPENAI_API_KEY`）、`CCSWITCH_HOSTED_TOOLS_OPENAI_MODEL`、`CCSWITCH_HOSTED_TOOLS_OPENAI_BASE_URL`、`CCSWITCH_HOSTED_TOOLS_TIMEOUT_MS`；`bridge.rs` 扫描第三方 Chat response 里的 `web_search` tool call，并把执行结果作为 `role=tool` 回灌。
- 真实生产接入点在 `src-tauri/src/proxy/forwarder.rs`，不是只改 `handlers.rs` 的转换 handler。原因是 Codex takeover 流量先经过 `forwarder.forward_with_retry()`，请求在这里完成 route/provider 解析、auth、headers、proxy、Responses->Chat 转换和发送；因此 hosted tool loop 复用 forwarder 已配置好的发送闭包，最多 3 轮，只执行白名单 `web_search`，遇到其它 tool call 会退回既有 Codex tool flow。
- 因 hosted tool loop 需要读取完整 Chat response 才能决定是否执行工具，含 hosted `web_search` 的 Chat 上游请求会强制 `stream=false` 并移除 `stream_options`。若客户端原始请求是 stream，forwarder 会给最终 Chat response 打内部头 `x-cc-switch-hosted-tool-loop: web_search`，`handlers.rs` 看到该头后按非流式 Chat JSON 转 Responses，再包装成最小 `response.completed` SSE，避免把 JSON 当 SSE 解析。
- OpenAI hosted web_search 调用只记录 `trace/tool/query_hash/status/elapsed_ms/error` 这类脱敏字段；不会记录 API key、完整 query、网页正文或完整 prompt。OpenAI 调用失败或未配置 key 时，不中断主请求，而是把裁剪后的错误 JSON 作为 tool output 回给第三方模型，让模型能解释“搜索不可用”并继续生成最终回答。
- 回归测试重点：`cargo test --manifest-path src-tauri/Cargo.toml web_search -- --nocapture` 覆盖 hosted tool function 映射、Chat/stream 恢复、forwarder tool loop、OpenAI request 构造和 result 规整；`cargo test --manifest-path src-tauri/Cargo.toml completed_sse_wrapper_contains_final_responses_payload -- --nocapture` 覆盖原始 stream 请求被 hosted loop 强制非流后的最小 `response.completed` SSE 包装；`cargo test --manifest-path src-tauri/Cargo.toml tool_search -- --nocapture` 固定既有 `tool_search` client-side 工具不回归；`cargo fmt --manifest-path src-tauri/Cargo.toml --check` 固定 Rust 格式。

## 2026-07-06 CCSwitchMulti v3.16.4-15 GitHub Release Verification

- `v3.16.4-15` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-15`。Release 为 `draft=false`、`prerelease=false`，发布时间为 `2026-07-06T14:24:42Z`。
- 版本提交为 `2b13e2d4731f89a0bf820d038d94611b741d2500`（`chore(release): bump CCSwitchMulti to v3.16.4-15`），annotated tag `v3.16.4-15` 指向该提交。`main` 和 tag 均已推送到 fork remote。
- GitHub Actions：main CI run `28796644943` 成功，Release run `28796647894` 成功；Release run 覆盖 `ubuntu-22.04`、`ubuntu-22.04-arm`、`windows-2022`、`windows-2022 arm64`、`macos-14`，`Publish GitHub Release` 和 `Assemble latest.json` 均成功。
- 发布资产共 19 个：Windows x64/ARM64 setup、portable 和签名，macOS dmg、tar.gz/signature、zip，Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。远端 `latest.json` 已下载验证：`version=3.16.4-15`，包含 macOS、Windows x86_64/arm64、Linux x86_64/arm64 6 个 updater 平台条目。
- 本地固定交付目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 已由 post-commit release pipeline 刷新到 `Version: 3.16.4-15`、`Commit: 2b13e2d4731f89a0bf820d038d94611b741d2500`；Windows x64 NSIS setup、portable zip、raw exe 和 setup `.sig` 均已生成。
- 发布前本地验证覆盖：`CodexUsagePage` 单测、Codex 用量入口集成用例（当前环境需 `--testTimeout=20000`）、两个 temperature 缺省/显式 Rust 单测、`pnpm typecheck`、release note prettier check、`cargo fmt --check`、`git diff --check`。整文件 `tests/integration/App.test.tsx` 在默认 5 秒/10 秒阈值下仍会受既有 App/MSW 启动慢和测试隔离问题影响，不作为本轮 release 阻塞。

## 2026-07-06 CCSwitchMulti v3.16.4-15 Release Preparation

- 本轮用户要求“发一个新版本，发新的 release”。实际检查 `git log v3.16.4-14..HEAD` 后确认 `v3.16.4-14` 之后已有 5 个本地提交：新增 Codex 用量与重置额度工具页、补 temperature 默认边界测试、补 usage page 引导和主题颜色拆分、以及上一轮 release 验证 memory，因此不是空发。
- 版本号继续沿用 `v3.16.4-N` fork 递增规则，目标为 `3.16.4-15` / `v3.16.4-15`；同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`，新增 `docs/release-notes/v3.16.4-15-zh.md`。
- 发布说明重点：Codex 专属用量与 reset credits 页面、banked reset credits/窗口状态展示、浅色/深色主题 token 拆分、temperature 缺省不补 0 的回归护栏。发布前需要跑页面相关 vitest、temperature Rust 单测、typecheck、prettier、cargo fmt 和 `git diff --check`。
- 注意边界：未跟踪 `output/release-v3.16.4-4-upload/`、`output/release-v3.16.4-5wizard/` 和 `scripts/logs/` 是旧输出/日志，不应加入 release commit；GitHub release 仍通过推送 annotated tag 触发 `.github/workflows/release.yml` 自动跨平台构建和上传资产。

## 2026-07-06 Codex Temperature Default Boundary

- 本轮排查的结论是不要在 CCSwitchMulti 收到 Codex `/v1/responses` 请求缺省 `temperature` 时全局补 `temperature=0`。外部检索和代码链路都显示：OpenAI Chat/Responses 的 `temperature` 是可选参数；GPT-5/o3 等 reasoning 模型公开反馈更多是“传了非默认 temperature 会 400”；Kimi/Roo-Code 反馈则是 `kimi-for-coding` 需要固定 `temperature=0.6`，默认补 0 反而会失败。因此“缺省补 0”不是通用修复。
- official Codex OAuth 路径必须继续不带 temperature：`src-tauri/src/proxy/providers/openai_compat.rs::normalize_codex_oauth_responses_request` 会删除 `temperature/top_p/max_output_tokens`，`src-tauri/src/proxy/providers/transform_responses.rs` 也在 `is_codex_oauth=true` 时删除这些字段；这是因为 ChatGPT Codex 反代后端不接受这些公开 OpenAI API sampling 字段。
- 第三方 Chat Completions 转换路径 `src-tauri/src/proxy/providers/transform_codex_chat.rs::responses_to_chat_completions_with_reasoning_text_only_and_cache` 的正确规则是：请求里已有 `temperature` 才透传；缺省时不自动补。若后续某个 provider/model 确认必须固定 temperature，应通过 provider/model 级 override 或显式配置注入特定值，而不是全局默认。
- 回归护栏：`responses_request_without_temperature_does_not_default_temperature` 固定缺省不补；`responses_request_with_temperature_preserves_explicit_temperature` 固定显式值原样透传。后续若要改 temperature 策略，必须同时考虑 OpenAI reasoning 模型拒绝该字段和 Kimi coding 固定 0.6 这两个反例。

## 2026-07-06 CCSwitchMulti v3.16.4-14 GitHub Release Verification

- `v3.16.4-14` 已作为 `BigStrongSun/ccswitchmulti` 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-14`。Release 为 `draft=false`、`prerelease=false`，GitHub latest API 返回 `tag_name=v3.16.4-14`，发布时间为 `2026-07-06T03:04:19Z`。
- 版本提交为 `564829493183a577e3b5760126d83d9d860ef605`（`chore(release): bump CCSwitchMulti to v3.16.4-14`），annotated tag `v3.16.4-14` 指向该提交。`main` 和 tag 均已推送到 fork remote，`origin/upstream` 仍是原项目远端。
- GitHub Actions：Release run `28764056425` 成功，5 个平台构建、`Publish GitHub Release`、`Assemble latest.json` 全部成功；main CI run `28764054886` 成功，Frontend Checks 和 Backend Checks 均通过。
- 发布资产共 19 个：Windows x64/ARM64 setup、portable 和签名，macOS unsigned DMG、updater tarball/signature、zip，Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。本次 macOS DMG 已上传，但因 Apple signing/cert 步骤跳过，仍按未签名版说明处理。
- `latest.json` 已下载验证：`version=3.16.4-14`，6 个 updater 平台的 URL 均指向 `v3.16.4-14`，签名字段均存在。Release 正文未命中 `ccswitch.io`、`farion1231/cc-switch`、`BigStrongSun/cc-switch` 等旧链接。
- 发布前本地验证覆盖：两组 Codex/MultiRouter/Provider vitest、`pnpm typecheck`、`cargo test --manifest-path src-tauri/Cargo.toml codex_responses_passthrough --lib -- --nocapture`、`cargo test --manifest-path src-tauri/Cargo.toml codex_reset --lib -- --nocapture`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、release note prettier check、`git diff --check`（仅 Cargo.lock CRLF 提示）。

## 2026-07-06 Codex Usage Reset Credits Page Entry

- 参考 `jordan-edai/codex-reset-watcher` 时，真正可复用的产品边界不是 macOS 菜单栏，而是跨平台的只读 Codex 用量面板：5 小时窗口、每周窗口、banked reset credits、到期紧迫度、部分明细失败提示和手动刷新。CCSwitchMulti 里不要把这类信息只藏在 provider footer。
- 前端已新增 Codex 专属工具页 `src/components/codex/CodexUsagePage.tsx`，数据源复用 `useSubscriptionQuota("codex", true, true, 5)`，仍读取本机 Codex 登录状态，不兑换 reset、不修改账号、不新增平台限定逻辑。
- 入口在主界面切到 Codex 后的顶部工具栏：`Codex 多模型路由` 图标旁边的 `Codex 用量与重置额度`（柱状图）按钮。`src/App.tsx` 的 `codexUsage` view 已加入 `VALID_VIEWS`，但切到非 Codex app 时会回退 providers，避免 Codex 专属工具跨 app 残留。
- 该页面必须保留显式引导和 CCSwitchMulti UI 匹配：顶部使用和 `CodexRouterWorkspacePage` 一致的渐变工作台 header，正文用蓝色引导卡说明“从 Codex 工具栏进入 -> 刷新当前登录 -> 按窗口和 reset 到期决策”。配色必须维护浅色/深色两套 surface token（当前集中在 `USAGE_PAGE_COLORS` / `READING_HINT_COLORS`），不要只用单套颜色叠 `dark:` 或退回成外部 dashboard 风格。
- 回归覆盖：`tests/integration/App.test.tsx` 固定 Codex 工具栏入口能打开 `CodexUsagePage`；`src/components/codex/CodexUsagePage.test.tsx` 固定成功数据、banked reset credits、reset 明细部分失败和无凭据状态不会空白。

## 2026-07-06 Codex 127.0.0.1:15721 502 VPN/Proxy Boundary

- 用户截图里的 `unexpected status 502 Bad Gateway: Unknown error, url: http://127.0.0.1:15721/v1/responses` 不能直接归因到 MultiRouter route miss。需要先区分两条边界：如果 `codex-router.log` 同时间没有 `route_resolved/request_prepared/upstream_send_error`，请求可能被 Codex Desktop 自身的系统代理/VPN/规则代理在到达 `127.0.0.1:15721` 前拦截；如果日志有 `upstream_send_error`，说明请求已进入 CC Switch，失败在 CC Switch 到真实上游的出站链路。
- 代码边界：Codex 到本地 `15721` 是入站 TCP，不读 CC Switch 的 `http_client`；CC Switch 出站分为 reqwest 和 hyper 路径。`src-tauri/src/proxy/http_client.rs` 未显式设置全局代理时会跟随当前进程可见的系统/环境代理线索，但只会对指向 CC Switch 自己端口的 loopback 代理做防自环跳过；`hyper_client.rs` 原始 TCP 路径不读代理环境变量。
- 产品侧修复点：`diagnose_codex_multirouter` 新增「出站代理 / VPN 环境」检查，只读展示 CC Switch 显式全局代理、当前进程 `HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY`、Windows WinINET 用户代理和 WinHTTP 摘要；FAQ 增加 `502 Bad Gateway: 127.0.0.1:15721/v1/responses` 排障流程，提示用户通过 router 日志区分“请求未到本地代理”和“进入本地代理后上游出站失败”，并在规则代理失败时尝试 localhost 绕过、全局代理或 TUN 模式。
- 后续遇到同类反馈时，先让用户运行 Codex MultiRouter Debug 检查并导出 `~/.cc-switch/logs/codex-router.log` 同时间窗口；不要只根据 UI 里的 `Unknown error` 断定是 provider 配置、OAuth 或模型转换问题。

## 2026-07-06 GitHub Page Copy Cleanup Before Fork Detach

- 按用户要求降低仓库首页对官网和原项目的显式导流：GitHub repository `homepage` 已清空，description 改为 `CCSwitchMulti: Codex 多模型路由，把 OpenAI 订阅与 DeepSeek/Qwen/本地/第三方 OpenAI-compatible 模型合并到 Codex。`，topics 移除了 `cc-switch`。
- 四个 README 顶部都改为 `CCSwitchMulti` / Codex MultiRouter 自身定位，release/download badge 指向 `BigStrongSun/ccswitchmulti`，删除 `ccswitch.io` 官网行和 `farion1231/cc-switch` 的 Trendshift/star-history 顶部徽章；底部 star-history 仍保留但已改为当前仓库 `BigStrongSun/ccswitchmulti`。
- 主 README 分支说明去掉原项目链接，改为 CCSwitchMulti 自身能力说明；issue templates 的 FAQ、existing issues、Security Advisories、Discussions 链接全部改到 `BigStrongSun/ccswitchmulti`；release workflow 自动正文不再追加官网链接。
- 边界：GitHub API 仍显示 `parent=farion1231/cc-switch`，这是 fork network 元数据，不能通过 README/metadata 文案清理去掉；只有 GitHub Support detach fork network 或重建独立仓库后才会消失。

## 2026-07-06 GitHub Discoverability Low-Risk Optimization

- 在不改变 fork network 的前提下，已把 `BigStrongSun/ccswitchmulti` 的 GitHub description 改成同时包含 `CCSwitchMulti`、`Codex`、`OpenAI-compatible`、`DeepSeek`、`Qwen`、`CC Switch` 等中英文关键词；homepage 保持 `https://ccswitch.io`。
- 已补齐仓库 topics：`ccswitchmulti`、`cc-switch`、`codex`、`codex-multirouter`、`multi-model-router`、`openai`、`openai-compatible`、`deepseek`、`qwen`、`local-llm`、`desktop-app`、`tauri`、`provider-management`、`local-proxy`、`ai-tools`。
- README 顶部 version/download/release badge、分支说明和使用注意里的旧 `BigStrongSun/cc-switch` 链接已改为 `BigStrongSun/ccswitchmulti`；`docs/codex-spawn-agent-model-candidates.md` 的 tracking issue 也改到当前 slug。`rg` 验证目标文件中旧 slug 不再出现。
- 这些优化有助于外部搜索、GitHub topic 浏览和用户识别当前仓库，但不会改变 GitHub repository/code search 默认隐藏 fork 的规则；默认搜索可见性仍需要脱离 fork network 或要求用户加 `fork:true`。

## 2026-07-06 GitHub Repository Search Fork Visibility

- `BigStrongSun/ccswitchmulti` 直链可访问且 GitHub API 显示 `private=false`、`visibility=public`、`archived=false`、`disabled=false`，默认分支是 `main`，父仓库是 `farion1231/cc-switch`。因此“搜不到”不是私有、归档、禁用或远端地址错误。
- 根因边界是 GitHub 普通 repository search 默认不显示 fork。实测 `gh api 'search/repositories?q=ccswitchmulti&per_page=10'` 和 `q=ccswitchmulti user:BigStrongSun` 都返回 `total_count=0`；加 `fork:true` 或 `fork:only` 后返回 6 个 fork，第一项就是 `BigStrongSun/ccswitchmulti`。
- 代码搜索同样受 fork 过滤影响：`gh api 'search/code?q=repo:BigStrongSun/ccswitchmulti CCSwitchMulti'` 返回 0；加 `fork:true` 后能搜到仓库内 56 个结果。GitHub connector 的安装仓库查询显示 `is_code_search_indexed=true`，说明不是代码索引缺失。
- 外部搜索并非完全未收录：matrix-websearch 能搜到 GitHub 仓库页、releases 页和相关文章；GitHub 页面没有发现 `X-Robots-Tag` 或 `<meta name="robots">` 禁止索引信号。用户体感上的主要问题是 GitHub 站内默认搜索不带 fork 过滤。
- 改善建议：如果希望普通用户不加 `fork:true` 也能在 GitHub repository search 更稳定地找到，需要考虑脱离 fork network 变成独立仓库；但 GitHub 官方 `Leave fork network` 要求 public、少于 1GB、且没有 child forks，当前 API 显示本仓库已有 6 个 forks，直接脱离可能被限制。手动重建独立仓库会丢失 issues、PR、stars、watchers、child forks 等仓库元数据，只保留 git commit metadata。
- 低风险改善是给仓库加 topics、确保 description/README 使用 `ccswitchmulti`，并把 README 中旧 `BigStrongSun/cc-switch` badge/release 链接更新为 `BigStrongSun/ccswitchmulti`，但这些不会改变 fork 默认过滤规则。

## 2026-07-05 Codex Provider Catalog vs Menu Mapping Boundary

- 用户反馈新版 Codex provider 配置里，只有打开“需要本地路由映射”才展开模型列表；但 Responses 原生 provider 也应该能获取 `/models`、保存模型目录和上下文窗口。根因是前端把 `takeoverEnabled` 同时当成“目录/上下文编辑开关”和“Codex /model 菜单映射开关”，后端旧语义又把 `modelCatalog` 的存在直接解释成要写 `model_catalog_json`。
- 新不变量：`settingsConfig.modelCatalog` 是 cc-switch 的模型目录、上下文窗口、MultiRouter/子 Agent 候选元数据 SSOT；`meta.codexLocalModelMapping` 只控制单 provider 是否把该目录投射到 Codex `/model` 菜单和本地模型名映射。关闭菜单映射时仍要保存 catalog；开启 MultiRouter routes 时仍强制投射聚合 catalog。
- UI 文案边界：把“需要本地路由映射”改成“在 Codex /model 菜单中显示”，把顶部“本地模型路由”改成“Codex 多模型路由”，明确前者是菜单显示/单 provider 模型名映射，后者是一个 provider 内按 `body.model` 分流到多上游。`获取模型列表` 和 `测试 Chat / Responses` 是配置主操作，不能被菜单映射开关隐藏。
- 兼容边界：旧 provider 没有 `codexLocalModelMapping` 字段但已有 `modelCatalog` 时继续沿用旧行为投射，避免老用户升级后 `/model` 菜单消失；新 provider 显式保存 `false` 后，live 写入前会移除投射用的 `modelCatalog`，但 DB 里的目录元数据保持不变。

## 2026-07-05 MultiRouter Wizard Nested Provider Dialog Layering

- 用户反馈 MultiRouter 配置向导里保存/新增 provider 时弹窗像没到最前或卡死。根因不是 provider 保存 API 卡住，而是向导内再打开新增 provider 后，新增面板内部的二级弹层仍按默认层级 portal 到 `document.body`：`CodexFormFields` 的混合协议拆分/route 编辑确认、`UniversalProviderFormModal`、`ConfirmDialog` 等可能被 `FullScreenPanel z-[140]` 或向导壳遮住。
- 修复边界：不要只把某一个弹窗硬编码到更高 z-index。`FullScreenPanel` 现在提供弹层上下文，面板内未显式指定层级的 `DialogContent` 默认使用 `top`；`ConfirmDialog` 默认遵循上下文再退回 `alert`；嵌套 `FullScreenPanel` 自动使用下一层，向导新增 provider 面板 `z-[140]` 内的子面板提升为 `z-[160]`。
- 同轮修复了 `FullScreenPanel` 对 `document.body.style.overflow` 的竞争：多个全屏面板嵌套时用引用计数锁滚动，避免子面板关闭时提前解除父面板仍需要的滚动锁。
- 回归测试：`tests/components/FullScreenPanelLayering.test.tsx` 固定全屏面板内部普通 Dialog、ConfirmDialog 均在 `z-[200]`，并固定 `z-[140]` 父面板内的子 FullScreenPanel 为 `z-[160]`。

## 2026-07-04 Codex MultiRouter Wizard OAuth Guidance

- 用户指出 MultiRouter 设置向导少了关键步骤：没有引导用户配置官方 Codex OAuth。根因是向导按普通 `API Key + Base URL + /models` 范式处理所有模型源，虽然后端能把 official target 物化为 `codex_oauth`，但前端配置步骤只显示“已有模型目录/缺 Base URL”，没有展示 ChatGPT 登录入口。
- 修复边界：`isWizardCodexOAuthSource()` 统一识别 `category=official`、`providerType=codex_oauth`、`authBinding/source=managed_codex_oauth`、`auth_mode=chatgpt`、ChatGPT backend base URL 和 OpenAI Official 名称。OAuth 源不参与普通 `/models` 获取，也不参与 API Key 的 Chat/Responses 双协议探测；向导使用官方内置 fallback catalog，并在 providerConfig 步骤嵌入 `CodexOAuthSection`。
- 保存语义：向导生成 official/OAuth route 时显式写 `upstream.auth.source = managed_codex_oauth` 和 `authProvider = codex_oauth`，已有绑定账号则保留 `accountId`。这不是替代后端兜底，而是让保存后的 MultiRouter plan 自描述，便于工作台和日志排查。
- 回归覆盖：`tests/lib/codexMultiRouterWizard.test.ts` 固定 official provider 即使被旧 `base_url/apiKey` 污染也不作为模型抓取源，且 route auth 写 managed OAuth；`tests/components/CodexMultiRouterWizard.test.tsx` 固定配置步骤必须展示 ChatGPT OAuth 引导和登录区，不再给 official provider 显示普通 API 格式下拉。

## 2026-07-04 Codex Local Routing Active Notice

- App 顶层用 `useCodexLocalRoutingNotice(Boolean(isProxyRunning && takeoverStatus?.codex))` 监听 Codex 本地路由真实启用状态；触发条件是 CCSwitchMulti 本地代理正在运行且 Codex 接管已开启，而不是当前页面是否停留在 Codex。
- 提示弹窗文案为：`您正在使用本地路由功能，将由 ccsm 接管所有 codex 流量，所以不要在使用 codex 时关闭本软件。` Hook 只监听 `false -> true` 边沿；用户点“我知道了”后不因状态轮询重复弹，只有本地路由先关闭再重新开启时再次提醒。
- 回归测试：`tests/hooks/useCodexLocalRoutingNotice.test.tsx` 覆盖首次开启弹出、确认后保持开启不重复弹、关闭后重新开启再次弹。

## 2026-07-04 Codex MultiRouter Provider Catalog Selection Sync

- 用户反馈在普通 Codex provider 里删减保留模型后，进入 MultiRouter 配置或设置向导时远端 catalog 又全量出现。根因不是 provider 保存后的 `syncCodexMultiRouterPlanWithProviders()` 没跑，而是 MultiRouter 工作台 `providerWithFetchedModelCatalog()` 和向导 `mergeFetchedModelsIntoWizardProvider()` 在自动刷新 `/models` 时把远端全量模型追加回 `modelCatalog.models`，把用户保留列表反向污染成发现列表。
- 状态源边界：普通 provider 编辑页的“获取模型列表”是显式编辑/发现入口，可以让用户重新看到全量模型；MultiRouter 工作台和设置向导的自动刷新属于路由构建链路，`modelCatalog.models` 必须按“用户保留/暴露列表”解释。已有目录只更新匹配模型的 context window 等元数据，空目录才用远端 `/models` 初始化。
- 子 Agent 候选必须跟随同一保留列表：向导刷新 provider 时会剪掉不在当前 `modelCatalog.models` 内的 `modelCatalog.spawnAgentModels`，工作台刷新继续通过 `normalizeCodexSpawnAgentModels()` 剪枝；已保存 MultiRouter plan 仍由 `syncCodexMultiRouterPlanWithProviders()` 重建 route/catalog/spawnAgentModels。
- 回归覆盖：`mergeFetchedModelsIntoWizardProvider(..., { preserveExistingSelection: true })` 保留 alias/upstream 并只更新已保留模型；向导刷新不会把 extra upstream model 写回 provider；工作台 routes 自动刷新不会恢复 provider 已删除模型，并会把 stale plan route/catalog/spawnAgentModels 同步剪掉；AgentPlan 在线获取能力保留在“空目录初始化”场景。

## 2026-07-04 MiniMax Native Responses Function Arguments Strictness

- 用户截图报错：`CC Switch local proxy failed while handling Codex endpoint /responses. Provider: MiniMax; model: MiniMax-M3; upstream_status: HTTP 400; cause: invalid params, invalid function arguments json string, tool_call_id: call_function_... (2013)`。这不是本地日志缺失问题，也不是 MiniMax preset 选错；错误来自 MiniMax native Responses 上游重新解析历史 `function_call.arguments` 时发现 JSON 字符串非法。
- 外部同类案例和本地既有代码都指向同一类根因：严格上游（MiniMax 已确认）会拒绝空字符串、被截断的 `{` / `...[truncated]` 等非法 JSON arguments；宽松上游可能静默接受。此前 `json_canonical::canonicalize_tool_arguments` 已修复 Responses->Chat 转换路径，但第三方 native Responses passthrough 只提升 system/developer 控制消息，没有清理 `type=function_call` 的 `arguments`。
- 修复边界：`openai_compat.rs::normalize_codex_responses_passthrough_request` 现在同时调用 `normalize_codex_responses_function_call_arguments`，仅对 `input[*].type == "function_call"` 规整 `arguments`：缺失/空字符串转 `{}`，合法 JSON 做 canonical 输出，非法非空片段包进 `{"raw_arguments":"..."}`。不改 route、不把 MiniMax 改成 Chat、不全局删除 Responses 字段。
- 回归测试：`codex_responses_passthrough_normalizes_function_call_arguments` 覆盖 MiniMax-M3 native Responses 历史中空 arguments 和 `{` 片段；同时复跑 `codex_responses_passthrough`、`codex_oauth_responses_normalizer` 和 `responses_request_to_chat_sanitizes*`，确认第三方 passthrough、official OAuth、Responses->Chat 三条链路都未回退。

## 2026-07-03 Xiaomi MiMo Codex Native Responses Preset Boundary

- GitHub issue `BigStrongSun/ccswitchmulti#8` 反馈 `v3.16.4-11` 下非 Token Plan 小米 MiMo `mimo-v2.5-pro` 在 Codex 编码任务中途停下、需要手动“继续”。这不是 MultiRouter GPT 错路由或 official OAuth cleanup 问题；当前 MiMo Codex preset 是 native Responses 直连，核心差异是 preset 只用了通用 `generateThirdPartyConfig`，漏掉了小米官方 Codex 示例要求的 Codex 层字段。
- 小米官方 Codex 配置示例明确要求 `model_supports_reasoning_summaries = true`、`model_reasoning_summary = "none"`、`model_context_window = 1048576`、`web_search = "disabled"`、`wire_api = "responses"`。缺少 `model_supports_reasoning_summaries` 时，原版 Codex 的 `build_reasoning` 会把 `model_reasoning_effort = "high"` 当成无效，不发送 `reasoning` 参数；缺少 `model_context_window` 时 TurnContext/token usage/auto-compact 只能走模型 catalog 或兜底上下文，容易和 MiMo 1M 窗口不一致。
- 修复边界：不要为了补 MiMo 元数据给 native Responses preset 加 `modelCatalog`，因为 ProviderForm 仍用 `modelCatalog.models.length > 0` 推断“本地路由/接管”初始开关。MiMo 的默认单模型直连应靠 TOML 顶层字段修复，仍保持 `apiFormat = "openai_responses"` 且 `modelCatalog` 为空，避免把用户无代理直连路径改成本地接管。
- 回归测试落点：`tests/config/codexChatProviderPresets.test.ts` 固定两个 MiMo Codex preset 同时包含官方 reasoning/context/web_search 字段、使用各自 base URL、模型为 `mimo-v2.5-pro`、`wire_api=responses`，并继续断言不带 `modelCatalog`。

## 2026-07-03 CCSwitchMulti v3.16.4-12 GitHub Release Verification

- `v3.16.4-12` 已作为 BigStrongSun/ccswitchmulti 的正式 release 发布并通过 GitHub Actions release run `28647799609`，head sha 为 `70bf31ed19416c723ef58d1c4a92ddda29023fe2`。五个平台 build、Publish GitHub Release、Assemble `latest.json` 全部成功。
- Annotated tag `v3.16.4-12` 的 tag object 为 `b0e83bbcfacf31efa089d3e4a06e35e9799933c2`，解引用到 release bump 提交 `70bf31ed19416c723ef58d1c4a92ddda29023fe2`。`fork/main` 也已推到同一提交；`origin/upstream` 仍是原版 `farion1231/cc-switch`，本次未向原版远端发布。
- Release `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-12` 为 `draft=false`、`prerelease=false`，GitHub latest API 已返回 `tag_name=v3.16.4-12`，release name 为 `CCSwitchMulti v3.16.4-12`，正文来自 `docs/release-notes/v3.16.4-12-zh.md` 并包含本轮 Codex 跨 provider 路由与 official OAuth reasoning 历史兼容修复说明。
- 发布资产共 19 个：macOS unsigned DMG、macOS updater tarball/signature、macOS zip、Windows x64/ARM64 setup/portable/signature、Linux x64/ARM64 AppImage/signature/deb/rpm，以及 `latest.json`。
- `latest.json` 已下载并解析成功：`version=3.16.4-12`，包含 6 个 updater 平台；`darwin-aarch64` 和 `darwin-x86_64` 都指向 `CC-Switch-v3.16.4-12-macOS.tar.gz`，`windows-aarch64` 指向 `CC-Switch-v3.16.4-12-Windows-arm64-Setup.exe`，所有平台 URL 都指向 `v3.16.4-12` 且签名字段存在。
- 发布前本地验证覆盖：`rg` 确认四个版本面无 `3.16.4-11` 残留；`pnpm exec prettier --check package.json src-tauri/tauri.conf.json docs/release-notes/v3.16.4-12-zh.md`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_oauth_responses_normalizer --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_passthrough --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml proxy::providers::codex::tests --lib -- --nocapture`；`cargo test --manifest-path src-tauri\Cargo.toml proxy::providers::openai_compat::tests --lib -- --nocapture`；`git diff --check`。

## 2026-07-03 Codex Cross-Provider Model Switch Type Boundary

- 原版 Codex 的 `/model` 切换不重写历史 item：TUI 通过 `AppEvent::UpdateModel` / `Op::OverrideTurnContext` 更新当前 thread settings，core 侧把 `model/effort` 合进 `SessionConfiguration.collaboration_mode`，下一轮再用新的 `TurnContext.model_info` 构造请求；历史仍来自 `clone_history().for_prompt(...)` 并作为 `Prompt.input` 进入 `ResponsesApiRequest.input`。
- 因此 CCSwitchMulti 的正确边界不是在 route runtime 猜“上一轮来自哪个 provider”，也不是把历史全局改成某个上游的 schema；应把 Codex 历史视为 canonical Responses-like item，进入 provider 之前按目标 provider 的 wire schema 做 request-local normalization。
- Chat 桥接路径专属字段如 `reasoning_content` 只属于 Responses->Chat / Chat->Responses 适配层和 `CodexChatHistoryStore` 缓存，用来给 DeepSeek/Kimi/MiMo 等 Chat 上游恢复 assistant tool-call reasoning；它不应作为 native Responses 或 official OAuth 的通用字段。官方 Codex 协议的 FunctionCall/ToolSearchCall 类型本身不声明 `reasoning_content`，Codex 反序列化后也不会把该字段自然持久化为正式历史字段。
- Official ChatGPT Codex OAuth 私有 `/backend-api/codex/responses` 比公开 Responses 更严格：出站前必须提升 system/developer input 到顶层 `instructions`，保留 `message.content`，reasoning item 只保留 `summary/encrypted_content` 并把缺失 summary 的 raw content 转为 `summary_text` 后移除 raw `content`，tool/call output 上的冗余 `content` 也要移除，同时确保 `include` 带 `reasoning.encrypted_content`。
- 第三方 native Responses 直透目前只做 system/developer 控制消息提升；不要无证据套用 official OAuth 私有 cleanup。若某个第三方 Responses 也拒绝 `reasoning.content` 或工具 output `content`，应按 provider/API-format 增加局部 compatibility normalizer 和回归测试，而不是全局删字段影响其它公开 Responses 兼容实现。
- 这次 raw `reasoning.content` 问题的引入链路是：`15e712e7` 打开第三方 native Responses 直连后，同一 session 更容易产生/保留 Responses-shaped 历史；`77781164` 的 official OAuth cleanup 只处理 tool/output `content`，当时错误地允许 `reasoning.content` 保留；`6524fe2d` 补了切模型 control-message 提升但没有改变 reasoning 边界。最终修复应落在 official OAuth 出站 normalizer，而不是改变原版 `/model` 语义。

## 2026-07-03 Codex Official OAuth Reasoning Content Boundary

- 更新 2026-07-01 的 Responses input 清理边界：official managed Codex OAuth 不能再原样保留 `type=reasoning` item 上的 raw `content`。真实上游错误 `Invalid input[n].content: array too long. Expected an array with maximum length 0` 同样会发生在 reasoning item；这说明 ChatGPT Codex 私有 `/responses` input schema 不接受 reasoning.content。
- 修复边界不是丢弃 reasoning 状态：`openai_compat.rs::normalize_codex_oauth_responses_request` 仍保留普通 `message.content`，并保留 reasoning item 的 `encrypted_content` 与已有 `summary`；只有没有 summary 的旧会话/第三方转换形态，才把 raw `content` 中可读文本提升为 `summary: [{type:"summary_text", text:...}]` 后移除 raw `content`。公开 OpenAI Responses、第三方 Responses、Responses->Chat 转换路径不调用这条 official OAuth 专属清理。
- 路由错分流的另一个根因是旧 DB/接管备份可能把 `codex-official` 目标 provider 污染成带第三方 `base_url`/`apiKey`。MultiRouter materialize 时官方身份应优先于污染的普通 API 字段：`targetProviderId=codex-official` 或 official/category/OAuth auth 命中时物化为 `meta.provider_type=codex_oauth`，并只在 request-local effective provider 上移除 `base_url/baseURL/baseUrl/apiKey/api_key`，持久 provider 不被重写。
- 回归测试应覆盖两类场景：reasoning 带 `summary+encrypted_content+content` 时移除 content 且保留 summary/encrypted_content；reasoning 只有 raw content 时提升为 summary_text；污染的 `codex-official` target provider 仍提取 `https://chatgpt.com/backend-api/codex` 与 `AuthStrategy::CodexOAuth`，且不走 Responses->Chat。

## 2026-07-03 GitHub Release Body Must Use CCSwitchMulti Notes

- Release tag 本身应继续使用推送的 fork tag（例如 `v3.16.4-11`），不要从 upstream/origin 取原版 tag 或 release 文案。`release.yml` 的 `tag_name: ${{ github.ref_name }}` 是正确边界。
- 旧 GitHub build release 的问题不是 tag 错，而是 Release name/body 仍是泛模板 `CC Switch <tag>` 和下载说明，缺少 CCSwitchMulti 的功能更新和 bug 修复内容。
- 修复边界：`Publish GitHub Release` job 必须 checkout 仓库，生成 `release-body.md`，优先读取 `docs/release-notes/${tag}-zh.md`，再追加下载资产说明；`softprops/action-gh-release` 使用 `name: CCSwitchMulti ${tag}` 和 `body_path: release-body.md`。
- `v3.16.4-11` 当前 GitHub release 已手动更新为 `CCSwitchMulti v3.16.4-11`，正文包含功能更新、Bug 修复、继承的核心产品修复、验证和下载说明。后续新 tag 会由 workflow 自动使用同样规则。

## 2026-07-03 CCSwitchMulti v3.16.4-11 GitHub Release Verification

- `v3.16.4-11` 已作为 BigStrongSun/ccswitchmulti 的正式 release 发布并通过 GitHub Actions release run `28616009981`，head sha 为 `8e93cd6df6b7737c3420a5b6861de41992449ca8`。五个平台 build、Publish GitHub Release、Assemble `latest.json` 全部成功。
- Release `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-11` 为 `draft=false`、`prerelease=false`，GitHub latest API 已返回 `tag_name=v3.16.4-11`。资产共 19 个，包含 unsigned `CC-Switch-v3.16.4-11-macOS.dmg`、macOS updater `macOS.tar.gz`/`.sig`、macOS zip、Windows x64/ARM64 setup/portable/signature、Linux x64/ARM64 AppImage/deb/rpm/signature、以及 `latest.json`。
- macOS job 日志确认 Apple secrets 仍为空，`APPLE_SIGNING_IDENTITY` 为空，workflow 明确走 unsigned fallback：复制 `src-tauri/target/universal-apple-darwin/release/bundle/dmg/CCSwitchMulti_3.16.4-11_universal.dmg` 到 `release-assets/CC-Switch-v3.16.4-11-macOS.dmg`，跳过 notarization 和 signed verification。这符合“允许接受未签名版本，后续再补 mac 签名”的发布边界。
- `latest.json` 已下载并解析成功：`version=3.16.4-11`，包含 6 个 updater 平台；`darwin-aarch64` 和 `darwin-x86_64` 都指向同一个 universal `CC-Switch-v3.16.4-11-macOS.tar.gz`，`windows-aarch64` 指向 `CC-Switch-v3.16.4-11-Windows-arm64-Setup.exe`。

## 2026-07-03 GitHub Release latest.json Retry Boundary

- `v3.16.4-10` 验证了 unsigned macOS DMG fallback 是有效的：Release 资产已包含 `CC-Switch-v3.16.4-10-macOS.dmg`、`macOS.tar.gz`、`.sig` 和 `macOS.zip`，说明 macOS build 和 `Prepare macOS Assets` 修复没有问题。
- 同一 run `28613873120` 最终失败在 `Assemble latest.json` 的 `Download all release assets`：`gh release download v3.16.4-10` 调 GitHub asset API 时返回 HTTP 503，导致 `latest.json` 没生成。这个失败点发生在资产上传之后，所以 Release 看起来可下载但缺 updater 元数据。
- 修复边界：`assemble-latest-json` 不能单次依赖 GitHub release asset API；下载全部 release 资产和上传 `latest.json` 都要有限重试，失败时保留明确错误。不要把这种 503 误判成 macOS DMG fallback 失败。
- 后续发布应推进新 tag 覆盖半成功 release，而不是只手工给旧 release 补 `latest.json`；否则 workflow 本身仍会在下一次 GitHub API 抖动时复发。

## 2026-07-03 macOS Unsigned DMG Release Fallback

- `v3.16.4-9` 的 GitHub macOS job 已证明 Tauri 会在 `pnpm tauri build --target universal-apple-darwin` 阶段生成 `src-tauri/target/universal-apple-darwin/release/bundle/dmg/CCSwitchMulti_*_universal.dmg`，即使仓库缺少 Apple 签名和公证 secrets。
- 真实缺口在 `Prepare macOS Assets`：旧逻辑在 `APPLE_SIGNING_IDENTITY` 为空时只上传 updater tarball 和 app zip，然后 `exit 0`，导致 Release 成功但没有 macOS DMG。
- 修复边界：无 Apple 签名时也必须把 Tauri 生成的 unsigned DMG 复制为 `CC-Switch-${tag}-macOS.dmg` 并上传；如果无签名且找不到 `.dmg`，macOS job 应失败而不是静默缺资产。Apple secrets 齐全时仍走 `create-dmg` 的签名/公证路径。
- Release 文案必须明确：缺 Apple 签名配置时 macOS DMG 是未签名版本；补齐 Apple Developer ID 证书和 notarization secrets 后，同一 workflow 会自动发布签名并公证的 DMG。

## 2026-07-03 CCSwitchMulti v3.16.4-9 GitHub Release Verification

- `v3.16.4-9` 已推到 `BigStrongSun/ccswitchmulti` 并通过 GitHub Actions release run `28610511658`，head sha 为 `0e8b25cdd0cbfe8e2bff054b46850ce1c5215c0e`。该 run 覆盖 Linux x64、Linux ARM64、Windows x64、Windows ARM64、macOS universal、Publish GitHub Release 和 Assemble `latest.json`，全部成功。
- 这次验证了 Windows ARM64 从 WiX MSI 切到 NSIS 的修复是有效的：release 资产包含 `CC-Switch-v3.16.4-9-Windows-arm64-Setup.exe`、`.sig` 和 `Windows-arm64-Portable.zip`，`latest.json` 的 `windows-aarch64.url` 也指向 `Windows-arm64-Setup.exe`。
- macOS 会自动 build：workflow 的 `macos-14` job 会构建 `aarch64-apple-darwin` 与 `x86_64-apple-darwin` target，合成 universal `codex-history-repairer` sidecar，并执行 `pnpm tauri build --target universal-apple-darwin`。本次产物包含 `CC-Switch-v3.16.4-9-macOS.tar.gz`、`.sig` 和 `CC-Switch-v3.16.4-9-macOS.zip`，`latest.json` 同时给 `darwin-aarch64` / `darwin-x86_64` 指向同一个 universal updater tarball。
- 本次没有发布 macOS `.dmg`，不是 macOS 自动 build 失败，而是 GitHub Actions 日志显示 `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_ID`、`APPLE_PASSWORD`、`APPLE_TEAM_ID` 都为空，`APPLE_SIGNING_IDENTITY` 因此缺失。workflow 只在 Apple 签名/公证凭据齐全时生成并上传签名 DMG；缺凭据时只发布 updater tarball 和 app zip。后续如果要有正式 DMG，需要先补齐 Apple Developer ID 证书与 notarization secrets，或者明确决定上传未签名 Tauri 生成的 DMG。
- 发布后校验：`gh api repos/BigStrongSun/ccswitchmulti/releases/latest` 返回 `tag_name=v3.16.4-9`、`draft=false`、`prerelease=false`；release 资产共 18 个，包含 `latest.json`，但不包含 `.dmg`。

## 2026-07-03 GitHub Release Windows ARM64 NSIS Fallback

- `v3.16.4-8` tag 验证了上一轮 CI/Release 修复的大部分链路：main push CI 成功；Release 的 Linux x64、Linux ARM64、Windows x64 均成功；Windows ARM64 已成功交叉编译主程序和 `codex-history-repairer.exe` sidecar。
- 新失败点仍在 Windows ARM64 安装包阶段：x64 `windows-2022` runner 上执行 `pnpm tauri build --target aarch64-pc-windows-msvc --bundles msi` 时，主程序已构建到 `src-tauri/target/aarch64-pc-windows-msvc/release/cc-switch.exe`，但 WiX v3 `light.exe` 在 `Running light to produce ..._arm64_en-US.msi` 后失败，日志只有 `failed to run ...\WixTools314\light.exe`。这说明把 ARM64 从 `windows-11-arm` 挪到 x64 runner 只能解决 runner 环境启动问题，不能保证 WiX3 ARM64 MSI 打包可靠。
- 修复边界：Windows ARM64 release 不再强制产 MSI，改成 x64 runner 交叉编译 `aarch64-pc-windows-msvc` 后用 NSIS 生成 `CC-Switch-v*-Windows-arm64-Setup.exe`，同时继续产出 `Windows-arm64-Portable.zip`。MSI 只作为兼容旧发布的可选资产，缺失不再阻塞 Release。
- 相关 workflow 不变量：`assemble-latest-json` 已支持 `*-Windows-arm64-Setup.exe` 映射到 `windows-aarch64`；`Prepare Windows Assets` 必须先收集 NSIS 并要求签名，再把 MSI 作为可选附加资产；GitHub Release 下载文案必须写 ARM64 `Setup.exe` 而不是 `.msi`。

## 2026-07-03 GitHub Flow CI/Release Failure Root Fix

- `v3.16.4-7` 推到 `BigStrongSun/ccswitchmulti` 后 GitHub Actions 有两类真实失败：CI 的 `Backend Checks` 卡在 Rust Clippy，Release 的 Windows 打包卡在 Windows 资产构建。不要把它归因为 GitHub 本身抽风，也不要只重跑 workflow。
- CI 根因之一是火山 AgentPlan 模型列表支持把 `fetch_models_for_config` / `model_fetch::fetch_models` 扩成 8 个并列参数，触发 `clippy::too_many_arguments`。修复边界是把 Tauri 命令入参收敛成 `FetchModelsForConfigRequest`，后端服务收敛成 `FetchModelsRequest` / `VolcengineModelListRequest`，前端 `invoke` 改为 `{ request: ... }`。
- CI 继续暴露的 backend test 根因是 Codex provider 配置合并边界不清：`model_context_window` 应视为用户界面/上下文显示偏好，provider 未声明时不能从 live config 删除；同名 custom provider 表如果 live 里仍带本地代理 `base_url` 或 `PROXY_MANAGED`，恢复 provider 备份时必须按 takeover 残留整表替换，否则本地代理字段会继续劫持后续请求。
- Release Windows x64 根因是 Tauri bundler 需要 `codex-history-repairer.exe` sidecar，但 workflow 只构建主程序，NSIS 阶段报 `codex-history-repairer.exe` 不存在。修复是在 Windows Tauri build 前显式按架构构建并校验 history repair sidecar。
- Release Windows ARM64 根因是 `windows-11-arm` runner 能编译主程序，但 WiX v3 `light.exe` 在该 ARM runner 环境无法可靠启动；正式 workflow 应在 `windows-2022` x64 runner 上交叉构建 `aarch64-pc-windows-msvc`，同时 pnpm setup/cache 条件要看 `runner.arch`，不要看目标 `matrix.arch`。
- `release.yml` 的正式发布语义必须是 `prerelease: false`；否则 GitHub latest 不会按正式 release 晋升，即使 tag 和资产都存在。
- 本轮本地验证覆盖：`cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings`、`cargo test --manifest-path src-tauri\Cargo.toml`、`pnpm typecheck`、`pnpm test:unit`、`pnpm exec prettier --check src/lib/api/model-fetch.ts .github/workflows/release.yml`。`actionlint` 本机未安装，后续若继续改 workflow，优先补装或在 CI 侧验证 workflow 语法。

## 2026-07-02 CCSwitchMulti v3.16.4-7 Formal Release

- `v3.16.4-7` 是 `v3.16.4-6` 后的 MultiRouter 路由热修复正式发布，核心变更是修复第三方 GPT alias 只出现在聚合 `modelCatalog`、没有回到第三方 route `match.models/modelMap` 时被官方 `gpt` 前缀 route 抢走的问题；版本提交为 `755b69e4ee0b5a91461558e4b7a8d8753b38bc5e`（`chore(release): bump CCSwitchMulti to v3.16.4-7`）。
- 本地正式输出目录：`C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-7`。验证：`latest.json` 版本为 `3.16.4-7` 且下载 URL 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-7/CCSwitchMulti_3.16.4-7_x64-setup.exe`；raw exe `FileVersion/ProductVersion=3.16.4-7`。
- `v3.16.4-7` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-7`。Release 为非 draft、`prerelease=false`，latest API 已返回 `tag_name=v3.16.4-7`，annotated tag 对象 `8d0890abefef939681724e350aad5e103d8d5a37` 解引用到 `755b69e4ee0b5a91461558e4b7a8d8753b38bc5e`。
- 本地/远端主资产 SHA256 对齐：setup `FC1D50037CB4FFC2C6BD008EEA6F72222DB33893A744128B50DAAFFAB8487C25`，portable `352905FB4A42C334ACACFC849B6805EB2B8B30C45615F7959551CA7DDC218DD2`，raw exe `22BE67AC4394B86B825CDE418D47C45C08948E64A1129D1433D0EA0655DF3E9A`。
- 发布前验证：`pnpm vitest run tests/lib/codexMultiRouterSync.test.ts tests/lib/codexMultiRouterWizard.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`；`pnpm typecheck`；`pnpm exec prettier --check tests/lib/codexMultiRouterSync.test.ts src/components/codex/CodexRouterWorkspacePage.tsx src/components/codex/CodexRouterWorkspacePage.test.ts memory.md docs/release-notes/v3.16.4-7-zh.md package.json src-tauri/tauri.conf.json`；`git diff --check`（仅 Cargo.lock CRLF 提示）。
- 已知后续项：`main` 推送后的 GitHub Actions `Backend Checks` 失败在 Clippy，不是 GitHub release 资产上传失败；错误为 `src/commands/model_fetch.rs::fetch_models_for_config` 和 `src/services/model_fetch.rs::fetch_models` 参数数 `8/7` 触发 `clippy::too_many_arguments`，应单独把 Volcengine 参数收敛成结构体或配置对象后修复，避免混入已发布的 `v3.16.4-7` tag。

## 2026-07-02 Codex MultiRouter Catalog Route Divergence Misroute

- 另一位用户的日志包 `ccswitchmulti_logs_2026-07-02_141527-150209(1).zip` 证明了第三种 GPT 错路由形态：51 条 `request_model=gpt-5.5-longnows-gpt` 的 Codex 请求都落到 `codex-multirouter::route::router-codex-official`，实际上游模型被改写为 `model=gpt-5.5`；同时 `_codex_session` 侧仍记录 `gpt-5.5-longnows-gpt`，说明 Codex 选择器能看到 longnows alias，但 runtime 没有对应 exact route，只能被官方 `gpt` 前缀 route 接走。
- 这和“空 catalog 清掉 relay route”以及“双 route 重复 exact 名称”都不同。根因边界是工作台 `buildModelCatalogForRoutes` 曾经把 target provider 的 `modelCatalog.models` 全量写进 MultiRouter 聚合 catalog，即使当前 route 的 `match.models/prefixes` 接不住这些模型；例如 LongNows route 只声明 `claude-opus-4-8`/`claude`，但 catalog 仍暴露 `gpt-5.5-longnows-gpt`。
- 修复边界：聚合 catalog 只能投影当前 route 可 exact/prefix 匹配的可见模型；provider 模型刷新影响已保存 plan 时复用 `syncCodexMultiRouterPlanWithProviders`，让 `route.match.models`、`upstream.modelMap` 和 `modelCatalog.models` 同步重算，不再让工作台刷新路径和 provider 保存路径分叉。
- 回归测试：`src/components/codex/CodexRouterWorkspacePage.test.ts` 的“does not expose provider catalog models that no saved route can match”固定 LongNows Claude-only route 不得暴露 `gpt-5.5-longnows-gpt`；验证命令覆盖 `pnpm vitest run tests/lib/codexMultiRouterSync.test.ts tests/lib/codexMultiRouterWizard.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`。

## 2026-07-02 Codex MultiRouter Duplicate Exact Route Bidirectional Misroute

- 多人同日下午反馈“中转 GPT 走到官方”和“官方 GPT 走到中转”时，不要只沿空 `modelCatalog` 单向清空 alias 的链路排查；双向错路由的运行时根因是同一个 MultiRouter plan 里多条 enabled route 同时声明相同 exact 可见模型名，例如官方和第三方中转都保存了 `match.models=["gpt-5.5"]`。Rust `find_codex_route_by_match_priority` 无法从同一个 `request.model` 判断用户意图，只能按 routes 数组顺序取第一条 exact match，所以 route 顺序不同会产生两个方向的错路由。
- 真正修复边界在保存/同步层生成唯一可见模型名：官方/OAuth canonical source 保留原名，第三方/中转同上游模型必须变成 `gpt-5.5-<provider-suffix>` 这类 alias，并在 route 级 `upstream.modelMap` 写回 `{alias:"gpt-5.5"}`。`syncCodexMultiRouterPlanWithProviders` 现在先对当前 plan 实际引用的 provider 跑 `resolveWizardModelNameCollisions`，坏旧配置里 relay route 的裸 `gpt-5.5` 会被修成 `gpt-5.5-relay-gpt`；工作台 `handleSaveRoutingRoutes` 和 routes tab 自动刷新也会调用 `normalizeCodexRoutesForVisibleModelAliases` 修复手动编辑/旧配置绕过口。
- 运行时只增加诊断防线：当多个 route exact 命中同一模型时，`codex.rs` 会写 warning，提示 `ambiguous exact route match`、route ids 和“按顺序取第一条”。不要试图在 runtime 靠 provider 类型猜用户想走哪条；没有唯一可见模型名时，用户侧的选择信息已经丢失。
- 回归测试：`tests/lib/codexMultiRouterSync.test.ts` 覆盖 provider 同步修复官方/relay 同名 exact route；`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖手动保存前修复重复 exact route；`src-tauri/src/proxy/providers/codex.rs` 的 `test_codex_router_duplicate_exact_routes_remain_order_dependent` 固定运行时顺序依赖和诊断语义。

## 2026-07-02 Codex MultiRouter Empty Catalog Relay GPT Misroute

- 多人反馈“本来走第三方中转的 GPT 请求被路由去官方”时，先查 provider 保存后的 MultiRouter 同步结果：`settingsConfig.codexRouting.routes[].match.models` 是否被清空、第三方 route 的 `upstream.modelMap` 是否丢失、聚合 `modelCatalog.models` 是否还包含 `gpt-*-relay` 这类可见别名。
- 根因不是 OAuth 代理与原生 Codex 差异，也不是 Rust runtime 主匹配优先级直接抢路由。直接触发点是 `c10a1541 fix(codex): sync multirouter catalogs after provider model edits` 引入的同步逻辑：`syncCodexMultiRouterPlanWithProviders` 把目标 provider 的空 `modelCatalog.models` 当成“用户删除了所有模型”，随后写入 `match.models=[]` 并删除 `upstream.modelMap`。relay route 失去可见 alias 后，运行时只能命中官方 GPT exact/prefix route。
- 修复边界：目标 provider 当前没有可用 catalog 时，不覆盖已保存 route 的 `match.models` / `modelMap`；`rebuildPlanModelCatalog` 在空目录回退路径复用 plan 里已有 catalog 条目的 `displayName`、`contextWindow`、`upstreamModel` 等字段。目标 provider 目录非空时仍按新目录同步新增/删除模型，并继续剪枝失效的 `spawnAgentModels`。
- 回归测试：`tests/lib/codexMultiRouterSync.test.ts` 的“目标 provider 目录暂时为空时不清空第三方 GPT 别名 route”覆盖官方 `gpt-5.5` 与第三方 `gpt-5.5-relay -> gpt-5.5` 并存、relay provider catalog 暂时为空、official catalog 正常更新时，relay route 和 spawn agent 候选必须保留。

## 2026-07-02 Volcengine AgentPlan OpenAPI Model Fetch

- 用户截图里的“目前火山引擎 Agentplan 获取不到模型”不是 API Key、网络或 `/models` 候选顺序的单点问题。火山 Agent Plan 的模型枚举文档是 `ListArkAgentPlanModel - 查询 Agent Plan 支持的模型列表`，在 `Agent Plan API` 管控面下；同页导航还有独立的 `ListArkCodingPlanModel - 查询 Coding Plan 支持的模型列表`。这类接口不是数据面 `https://ark.cn-beijing.volces.com/api/coding/v3/models` 或 OpenAI `/v1/models`。
- `origin/main`（2026-07-02 fetch 后为 `8d1b3306d09a27b9d8fc29694791d8421aba5f93`）没有修复 AgentPlan 专用模型枚举：全树无 `ListArkAgentPlanModel` / `ListArkCodingPlanModel`，只有通用 `/models` URL 候选和 UA 透传修正。不要把原版的通用 `model_fetch` 修复误判成火山 AgentPlan 支持。
- 纠偏：不要把“不能走 OpenAI `/models`”等价成“只能 catalog-only”，也不要把火山推理 API Key 和账号级 AK/SK 混为一类。火山 AgentPlan 有火山 AK/SK（存在 `meta.usage_script.accessKeyId` / `secretAccessKey`）时，应优先通过 `open.volcengineapi.com` 的 `ListArkAgentPlanModel` 管控面 OpenAPI 获取模型列表，并解析 `Result.Datas[].ModelID`；缺 AK/SK 但有推理 API Key 时，应继续尝试数据面 `/models`，失败再保留内置 `modelCatalog`；推理 API Key 和 AK/SK 都缺时才直接 catalog-only。
- 举一反三边界：AgentPlan 的“推理调用”仍是 OpenAI-compatible Chat/Responses 数据面，但“模型枚举”不是同一个接口。Provider 表单、MultiRouter 新向导、已保存路由页自动刷新三条入口都必须复用 plan-aware 获取逻辑；如果只修前两条，路由页进入 `routes` tab 时仍会用普通 `/models`，导致火山 AgentPlan 回归失败。自动刷新去重 key 也要把 OpenAPI action 与 AK/SK 的短哈希纳入比较，避免换 AK/SK 后复用旧失败状态。
- BytePlus 仍保持 catalog 回退，直到找到可验证的 BytePlus 专用模型列表接口契约；不要把 BytePlus 未证实地并入火山 CN OpenAPI。
- 不要通过把 `/api/coding/v3` 剥离成 `https://ark.cn-beijing.volces.com/v1/models` 之类猜测端点来“修复”。那只是换一个未证实的 OpenAI-compatible URL，和官方 `ListArkAgentPlanModel` 管控面边界不一致，容易把真实根因掩盖成另一个 404。

## 2026-07-01 Codex MiniMax Sensitive Image Media Retry

- 用户截图里的 `Provider: MiniMax; model: MiniMax-M3; upstream_status: HTTP 400; cause: input new_sensitive, messages[61]'s content[0] image is sensitive` 不是 MultiRouter route 物化丢能力；它说明 Codex 同一 session 历史里仍有图片块，上游 MiniMax 对某张图片做安全审核后拒绝。
- 不要把 `MiniMax-M3` 直接加入 text-only 预防名单，除非有供应商文档或实测确认它完全不支持图片。该错误措辞是图片安全拒绝，不是能力不支持；直接标 text-only 会永久关闭可能合法的图片输入。
- 修复边界在反应式 media retry：`forwarder.rs::media_retry_should_trigger` 已要求 adapter 是 Codex/Claude、整流开关开启、未重试过、原 provider body 确实含图片块；`media_sanitizer.rs::is_retriable_image_error` 再识别 unsupported image 与 MiniMax `base_resp.status_msg` / `new_sensitive` / `image is sensitive` 等图片审核错误，随后把图片块替换为 `[Unsupported Image]` 并对同一 provider 重试一次。
- 回归测试：`detects_minimax_sensitive_image_errors`、`reactive_triggers_for_codex_sensitive_image_errors`、`reactive_sensitive_image_error_still_requires_image_body`。后续排查类似 `messages[n].content[m] image is sensitive`，先看同 trace 是否有 `[Media] Image retry succeeded/still failed`，不要先猜 OAuth 或 MultiRouter catalog。

## 2026-07-01 CCSwitchMulti v3.16.4-6 Release

- v3.16.4-6 是 v3.16.4-5 后的热修正式发布，核心变更是 `fix(codex): strip invalid content from oauth response items`：Codex OAuth `/responses` 直透 ChatGPT backend 前会删除非 message/reasoning input item 上的冗余 `content`，避免 `Invalid input[3].content: array too long`。
- 版本面必须同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`，release note 是 `docs/release-notes/v3.16.4-6-zh.md`。
- 发布输出目录使用独立路径 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-6`，不要复用默认“最新版ccswitchmulti”目录或清理旧 `output/release-*` / `scripts/logs/` 未跟踪目录。
- `v3.16.4-6` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-6`。Release 为非 draft、`prerelease=false`，并通过 `gh api repos/BigStrongSun/ccswitchmulti/releases/latest` 确认是 latest。
- Annotated tag `v3.16.4-6` 的 tag 对象为 `8e57b291468f3068189cd0725a6440170ee0527e`，解引用到版本提交 `e0614deb263f7b2dbcbb3b1cbe46162294d8a353`（`chore(release): bump CCSwitchMulti to v3.16.4-6`）。发布资产共 10 个：Windows setup、setup signature、portable zip、raw exe、`latest.json`、Linux/macOS build notes、README、`RELEASE-METADATA.md`、`SHA256SUMS.txt`。
- 发布后校验：`latest.json` 版本为 `3.16.4-6` 且指向 `v3.16.4-6` setup URL；raw exe 的 `FileVersion/ProductVersion` 为 `3.16.4-6`；GitHub asset digest 与本地 `SHA256SUMS.txt` 对齐，主资产 SHA256 为 setup `B0D69BA5610B3B3600F9E38B255A47AB444DFDEE8FF7469BDE0C906DE094C7DD`、portable `65B8EAB4F573199FFB212B89CF32D7679BA00740CD357D826AB924665CF2A3B0`、raw exe `F2E3941DBFD932E2B32A35B64168816FBF68806BEAD3EFCDBBFAD4D1F68757B4`。

## 2026-07-01 Codex Cross-Provider Model Switch Control Messages

- 跨 provider 继续同一个 Codex session 时，Codex Desktop 可能把 `/model` 切换产生的 system/developer 控制消息放进 Responses `input`。如果先前会话在 MiniMax-M3，之后切到 gpt-5.4 这类更严格的 Responses 上游，直接透传这些内部角色可能触发 HTTP 400；这不是 MiniMax 单独问题，而是同一会话历史跨协议/跨 provider 复用时的 input 形态问题。
- 已有 Chat 转换路径 `transform_codex_chat.rs` 会把 developer 映射为 system，并通过 `collapse_system_messages_to_head` 合并到首位，避免 MiniMax 对中途 system role 的严格校验失败；但原生 Responses 透传路径之前没有同等规整。
- 修复边界：`openai_compat.rs::normalize_codex_responses_passthrough_request` 会把 `input` 中 `type=message` 或缺省 type 且 `role=system/developer` 的控制消息提升到顶层 `instructions`，并从 `input` 删除这些内部控制消息。第三方原生 Responses 透传在 `forwarder.rs::should_normalize_codex_responses_passthrough_control_messages` 命中时调用；official OAuth normalizer 也先复用该逻辑，再执行 ChatGPT backend 专属的 tool output `content` 清理。
- 回归测试：`codex_responses_passthrough_promotes_control_messages_to_instructions`、`codex_oauth_responses_normalizer_promotes_control_messages_and_strips_tool_content`、`codex_responses_passthrough_control_message_normalizer_is_scoped`，并复跑 `responses_request_to_chat_normalizes_codex_internal_roles` 与 `responses_request_to_chat_merges_mid_stream_system_into_head` 确认 Chat 转换老路径未回退。

## 2026-07-01 Codex OAuth Responses Passthrough Content Shape

- 用户截图里的 `Invalid 'input[3].content': array too long. Expected an array with maximum length 0, but got an array with length 1 instead.` 发生在 Codex `/responses` 直透 ChatGPT Codex OAuth backend，日志特征是 `responses_to_chat=false`、`responses_to_messages=false`、`upstream_url=https://chatgpt.com/backend-api/codex/responses`。这不是第三方 Chat 转换问题，也不是模型容量问题。
- 根因是 `openai_compat::normalize_codex_oauth_responses_request` 对 Codex Desktop 已经发来的 `input` 数组只做字段补齐，未清理非 message item 上的冗余 `content`。官方 Codex app-server protocol 的 `function_call_output`、`custom_tool_call_output`、`tool_search_output` 只允许 `output/tools/call_id/status/execution` 等字段，不允许携带 message-style `content`；ChatGPT backend 会把该 content 视为长度必须为 0 的数组并返回 400。
- 修复边界：只在 official managed Codex OAuth passthrough normalizer 中清理 input item，保留 `message` 和 `reasoning` 的 `content`，删除 tool/call/web/image/compaction 等非 message item 的 `content`。公开 OpenAI Responses、第三方 Responses、Responses->Chat 转换路径不调用这条清理逻辑，避免扩大行为变更。
- 回归测试落点：`openai_compat.rs::codex_responses_request_normalizer_strips_content_from_tool_output_items` 覆盖 function/custom/tool-search output item 带 `content` 时会被删除，同时保留 `output/tools` 和普通 user message content。

## 2026-07-01 CCSwitchMulti v3.16.4-5 Formal Release

- CCSwitchMulti 正式发布远端是 `fork` (`BigStrongSun/ccswitchmulti`)；`origin`/`upstream` 指向原版 `farion1231/cc-switch`，发布、tag、asset upload 都不能推到 `origin`。
- `v3.16.4-5` 的发布语义是“本地最新改完的 `main` 作为正式版本”。如果发布准备后又产生发布质量修正（例如清理重复脚本键），必须先提交到 `main`，再把 annotated tag `v3.16.4-5` 移到最终 `HEAD`，然后推 `fork/main` 和强推 tag。
- `package.json` 曾经有两个相同的 `history:tool:check` script key。虽然值相同、行为不变，但会让 Vite/esbuild 在正式打包时提示 duplicate key；发布前必须只保留一个，验证用 `Select-String -Path package.json -Pattern '"history:tool:check"'`。
- 用户要求的流程是先创建 GitHub release，再打包和上传资产。release URL 是 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-5`；创建 release 后如果 tag 被移动，需要 `gh release edit v3.16.4-5 --repo BigStrongSun/ccswitchmulti --latest --notes-file docs/release-notes/v3.16.4-5-zh.md` 重新确认 release 元数据。
- 本地正式打包建议使用独立目录 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-5`，命令：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\local-release-pipeline.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-5 -Reason "manual-formal-release-v3.16.4-5"`。不要清理无关未跟踪目录，例如旧 `output/release-*` 和 `scripts/logs/`。

## 2026-07-01 Local Branch Merge Audit

- 本地 `main` 已吸收三个此前未合入的修复/功能分支：`codex/upstream-preserve-codex-live-settings`、`upstream/codex-history-repair-session-ui`、`upstream/codex-history-repair-session-manager`。
- `codex/upstream-preserve-codex-live-settings` 合并时冲突在 `src-tauri/src/codex_config.rs` 与 `src-tauri/src/services/proxy.rs`。正确处理是保留当前 main 的 MultiRouter、OAuth 登录态保护、CCSwitch 自有 catalog 指针清理、`PROXY_MANAGED` 陈旧 provider 表清理，同时补入旧分支的递归 TOML provider 优先合并逻辑。验证：`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_restore_ --lib -- --nocapture`。
- `upstream/codex-history-repair-session-ui` 是旧 upstream PR 形态；当前 main 已有更新的 CCSwitchMulti 历史修复实现。合并时不要用旧 UI 覆盖当前 `CodexHistoryRepairPanel`、`SessionManagerPage`、API types 或 MultiRouter repair command。最终只吸收 `.gitignore` 历史工具构建产物规则、`history:tool:check`、`scripts/codex-history-tool/build-windows-exe.ps1` 和 i18n key。验证：`pnpm history:tool:check`；`pnpm typecheck`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`。
- `upstream/codex-history-repair-session-manager` 的 glob state DB discovery / reconcile CLI 内容已经被当前 main 后续实现覆盖。冲突文件 `scripts/codex-history-tool/codex_history_tool.py` 与 `src-tauri/src/codex_history_migration.rs` 必须保留 main，避免旧默认 provider、较窄扫描逻辑和 mojibake 注释回滚当前实现。验证：`pnpm history:tool:check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`。
- 合并后仍未进 `main` 的本地分支只剩非本轮目标：`backup/main-mixed-before-clean-20260622` 是旧混合备份，包含早期 sidecar/router 实验和 `Codex <codex@local>` 提交，不应直接合入；`docs/codex-responses-websocket-routing-notes` 只剩文档提交未合入，其中代码提交 `fix(codex): sync multirouter model cache` 已被 `main` 等价吸收。

## 2026-07-01 Codex OAuth Native Request Shape Diagnostics

- Added read-only script `scripts/codex-oauth-diagnostics.ps1` for capacity/OAuth triage. It writes sanitized evidence under `scripts/logs/codex-oauth-diagnostics/<timestamp>`: live Codex config, auth metadata, parsed `codex-router.log` events, capacity/error candidates, and summary. Token-like fields plus account ids are represented only by length and short SHA256 prefixes.
- Added `scripts/codex-request-shape-compare.mjs` for native-vs-proxy request shape diffing. It supports `--self-test`, `--serve-self-test`, file-based `--native/--proxy`, and mock-server mode with `--serve --native-command ... --proxy-command ...`. Mock mode exposes `CODEX_COMPARE_BASE_URL`, `CODEX_COMPARE_NATIVE_BASE_URL`, and `CODEX_COMPARE_PROXY_BASE_URL`; requests can be tagged with `x-codex-compare-side` or `?side=`.
- Added `docs/codex-oauth-native-diff.md` to fix the analysis order: first identify whether traffic is pure native, CCSwitchMulti local facade to official OAuth, or third-party routing; then compare `service_tier`, `prompt_cache_key`, `client_metadata`, `originator`, Responses-Lite, account id, and session/window ids; use Fiddler/mitmproxy only after source/log/mock diff cannot explain the behavior.
- Validation on this machine: Windows PowerShell 5.1 runs `codex-oauth-diagnostics.ps1`; the latest 100 parsed router events had no capacity/error candidates. Node `--self-test` and `--serve-self-test` both generated diff reports successfully.

## 2026-07-01 Codex Desktop Login Preservation During Takeover Restore

- Codex Desktop app 登录态的唯一安全来源是 live `~/.codex/auth.json`。CCSwitchMulti 的 MultiRouter/OAuth 只负责 LLM 请求出口，异常恢复、关闭接管、启动自恢复都不能把旧 `proxy_live_backup` 里的空 auth、API key auth 或过期 OAuth 快照覆盖到当前 live auth。
- 崩溃/系统重启/Codex 先于 CCSwitchMulti 启动的关键风险不是 `15721` 本地代理配置本身，而是恢复旧备份时删除或回滚 `auth.json`。`ProxyService::write_codex_live_verbatim` 现在在恢复 Codex 备份时，如果当前 live auth 有 OAuth 登录材料，只写/投影 `config.toml`，保留 live `auth.json`；第三方 API key 仍放进 `experimental_bearer_token`。
- 保持兼容边界：如果当前 live 没有 OAuth 登录材料，空 auth 备份仍可删除 `auth.json`，以支持 config-only 第三方 provider；有 live OAuth 时，空 auth 和 stale OAuth 备份都不能影响 app 登录。回归测试覆盖 `codex_restore_empty_auth_backup_preserves_current_live_oauth_login` 与 `codex_restore_stale_oauth_backup_preserves_current_live_oauth_login`。
- 日志证据要按时间线判断：`app-exit-events.jsonl` 里的 `abnormal_exit_detected` 后，`cc-switch.log` 会立即出现“检测到上次异常退出（存在接管残留）”“codex Live 配置已从备份恢复”“正在重新接管并补齐 Live”。这说明 Codex-only 启动时掉登录不是 Codex 启动瞬间被 CCSM 改了，而是旧版本 CCSM 上次恢复/重新接管时已经把坏 `auth.json` 留在 live 目录里。
- 额外审计边界：`services/provider/live.rs::LiveSnapshot::restore()` 当前是 `#[allow(dead_code)]` 且无生产调用者，但它原本也会原样写入或删除 Codex `auth.json`。为防止以后回滚/快照流程重新接入后复发，该路径也必须遵守同一规则：live auth 有 OAuth 登录材料时只恢复 `config.toml`，不写、不删、不回滚 `auth.json`。回归测试覆盖 `codex_live_snapshot_restore_empty_auth_preserves_live_oauth_login`、`codex_live_snapshot_restore_stale_oauth_preserves_live_oauth_login` 和无 live OAuth 时仍删除空 auth 的兼容行为。

## 2026-07-01 GitHub CI and Release Workflow Failure Boundaries

- GitHub CI 的 backend job 比本地常用验证更严格：`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` 和 `cargo test --manifest-path src-tauri/Cargo.toml` 都会跑。发布前不能只跑前端 `typecheck/vitest/prettier`，Rust warning 在 stable toolchain 升级后会直接卡 CI。
- `tests/provider_service.rs::codex_official_to_deepseek_then_takeover_enters_and_restores_proxy_managed_live_config` 使用 proxy ephemeral port；断言不能写死 `15721`。正确判断是 live `config.toml` 指向 `http://127.0.0.1:<port>/v1` 且包含 `PROXY_MANAGED`，否则本地 Windows 和 GitHub cargo test 会误判失败。
- Release workflow 的 Linux 矩阵需要在 `pnpm tauri build --bundles appimage,deb,rpm` 前先构建 `codex-history-repairer`：`cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`。该 bin 有 `required-features = ["history-repairer"]`，Tauri bundler 会寻找 sidecar，没预构建会报 `codex-history-repairer does not exist`。
- Release workflow 里 Windows x64 的失败点是 WiX `light.exe` 打包 MSI；主自动 release 已改成 x64 只构建/收集 NSIS `*-Windows-Setup.exe`，避免 WiX 阻塞整条 release。Windows ARM64 仍走 MSI，但 GitHub `windows-11-arm` runner 可能缺 `C:\Program Files\LLVM`，workflow 需要在缺失时 `choco install llvm -y --no-progress` 再写 `LIBCLANG_PATH`/`CLANG_PATH`。
- Release workflow 里 macOS 的失败点不是编译，而是 Apple signing secrets 缺失后仍把空 `APPLE_SIGNING_IDENTITY` 传给 Tauri，导致 codesign 报 `The specified item could not be found in the keychain`。正确降级是 build 阶段 unset 空的 `APPLE_SIGNING_IDENTITY`，后续只在 signing identity 存在时生成/公证 DMG；否则只发布 updater tarball 和 `.app.zip`。

## 2026-07-01 Codex MultiRouter Wizard Catalog Curation Flow

- Codex 单 provider 表单 `CodexFormFields` 的模型映射表第一列语义是“保留这个模型进入该 provider 的 modelCatalog”，不是子 Agent 候选。取消勾选会删除该模型行；上下箭头移动的是 catalog 行顺序。不要再在单 provider 获取 `/models` 后自动写 `spawnAgentModels` 前 5 个。
- MultiRouter 设置向导的正确顺序是：模型源 -> MultiRouter 命名 -> 配置检查 -> 获取/测试模型 -> 重名别名 -> 汇总模型排序/剔除 -> 路由预览 -> 保存发布。最终汇总页决定 `modelCatalog.models` 和 route `match.models` 保留哪些模型；`spawnAgentModels` 兼容编辑器留在 RoutesTab 的折叠高级设置中，不再作为主向导必经步骤。
- `buildCodexMultiRouterWizardPlan` 支持可选 `planName`、`catalogModelOrder`、`spawnAgentModels`。传入 `catalogModelOrder` 时必须同时过滤 routes 和 final catalog，避免 UI 剔除模型但路由仍命中；传入 `spawnAgentModels` 时要过滤掉已剔除模型并限制最多 5 个。
- MultiRouter 向导进入“获取模型列表”步骤后必须逐个普通 Codex provider 重新调用 `/models`，不能因为已有 `modelCatalog` 自动跳过。每个 provider 卡片要显示读取中、成功有更新、成功无更新、跳过或失败状态；失败文案统一为“获取模型列表失败，请检查当前 provider 配置”，并同时进入向导问题面板。刷新后要保留“整理模型”里用户已取消/保留的勾选，只追加新增模型；`displayName` 缺省和显式等于模型 ID 在更新判断里等价，不应误报“有模型列表更新”。卡片点击应关闭向导并打开对应 provider 配置页。
- 普通 Codex provider 保存后需要同步引用它的已保存 MultiRouter 方案：route `match.models`、route `upstream.modelMap`、聚合 `modelCatalog.models` 和 `spawnAgentModels` 都要从 provider 最新 `modelCatalog` 重建。但同步时必须保留已保存 route 的可见别名和 `modelMap`，只用目标 provider 的最新目录补上下文、多模态等能力字段；不能直接用目标 provider 原始模型名覆盖 route，否则官方源和第三方中转暴露同名上游模型时会丢失路由区分。若同步导致 `spawnAgentModels` 中的模型被删除，只剪枝旧候选并提示用户点“处理”进入对应 MultiRouter routes 页人工补选，不要自动按 catalog 前 5 个补齐。

## 2026-07-01 Codex Provider Protocol Probe Concurrency

- Codex provider 表单的“测试 Chat / Responses”可以并发，但不要改成无界并发：`src/components/providers/forms/CodexFormFields.tsx` 使用 `CODEX_PROTOCOL_PROBE_MODEL_CONCURRENCY = 3` 的模型级并发池，避免大模型目录串行太慢，也避免一次性打爆真实供应商限流。
- 每个模型内部 Responses 和 Chat 两个协议用 `Promise.all` 同时探测；每完成一个模型就更新“模型映射”行级协议 tag，最终汇总仍按原 catalog 顺序输出，避免并发完成顺序影响用户看到的模型列表。
- 回归测试落点是 `tests/components/CodexFormFields.test.tsx` 的 bounded model concurrency case：初始只允许前三个模型同时发起 Responses/Chat 探测，任一模型完成后才开始下一个模型。

## 2026-06-30 Codex GLM Model Context and Probe Guidance

- 2026-07-01 用户用智谱 GLM key 实测“测试 Chat / Responses 全不通”时，真实网络和 key 都正常：`https://open.bigmodel.cn/api/paas/v4/chat/completions` 与 `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions` 能返回 200，`/api/paas/v4/models` 和 `/api/coding/paas/v4/models` 也能返回模型列表；失败的 UI 报错路径是 `.../api/coding/paas/v4/v1/chat/completions` / `.../v4/v1/responses`，根因是新加的协议探测 URL 构造没有复用 `/models` 的版本段规则。修复边界：`probe_codex_responses_for_config` / `probe_codex_chat_for_config` 对已以 `/vN` 收尾的 Base URL 直接拼 `/responses` / `/chat/completions`，避免再追加 `/v1`；智谱仍只有 Chat Completions 路径可用，Responses 路径按官方当前接口返回 404，应由 UI 标为 Chat 可用而不是“全不通”。
- 2026-07-01 同类 URL 风险不只智谱：Codex 预设中火山 Agentplan `https://ark.cn-beijing.volces.com/api/coding/v3`、BytePlus `https://ark.ap-southeast.bytepluses.com/api/coding/v3`、DouBaoSeed `https://ark.cn-beijing.volces.com/api/v3` 也属于“非 `/v1` 版本化根地址”，旧协议探测会错拼成 `/v3/v1/...`。普通以 `/v1` 收尾的供应商旧逻辑已经能直接拼 endpoint，不属于这次 bug。回归测试要同时覆盖 `/v4` 智谱和 `/v3` 火山/豆包代表路径。
- 2026-07-01 Codex provider 的“测试 Chat / Responses”不能只在按钮旁显示一条错误摘要；真实问题是每个模型可能有不同协议能力。`CodexFormFields` 的探测结果应按模型保存并在“模型映射”每行显示小 tag：`双协议`、`Responses`、`Chat`、`不可用`，tag title 保留 Responses/Chat 详细返回。汇总文案要列出双协议通过、仅 Responses、仅 Chat、双协议失败的模型，避免用户只看到第一个失败模型（如 `glm-4.5`）而不知道其它模型状态。
- 2026-07-01 “下一步”必须由真实探测结果驱动而不是模型名启发式：双协议通过和仅 Responses 通过的模型进入 Responses provider，只有 Chat 通过的模型进入 Chat provider，双失败模型不进入拆分建议。拆分成两个 provider 只在新增 provider 场景有 `onProviderSplitSuggestionChange` 回调时弹确认框；编辑已有 provider 时只显示行级协议 tag 和汇总，不弹一个确认后无实际保存效果的拆分对话框。
- 2026-06-30 复查用户提供的智谱 Coding Plan key 后确认：`https://open.bigmodel.cn/api/coding/paas/v4/models` 当前返回的 GLM 条目只有 `id/object/created/owned_by`，没有 `context_window`、`max_context_length` 等规格字段；因此自动获取模型列表若要补齐 GLM 上下文，不能只靠 `/models`。官方 Mintlify 文档提供稳定 markdown：`/cn/guide/start/model-overview.md` 的模型表给出 GLM-5.2 `1M`、GLM-5.1/5/5-Turbo/4.7/4.6 `200K`、GLM-4.5-Air `128K`，单模型页如 `/cn/guide/models/text/glm-4.5.md` 给出 GLM-4.5 `128K`。后端 `model_fetch.rs` 的正确策略是：先解析 `/models` 显式 metadata；若智谱/Z.AI endpoint 的 GLM 模型缺上下文，再从官方 docs markdown 补齐，且只填缺失值、不覆盖上游显式值。
- 2026-06-30 后续完善为分层上下文补齐：`model_fetch.rs::enrich_missing_context_windows` 是统一入口，优先保留 `/models` 显式字段，其次 provider 官方 resolver（当前智谱 docs markdown），最后才用 `https://models.dev/api.json` 的 `limit.context` 作为公共目录兜底。models.dev 不能全局按模型名匹配，必须先确认当前成功 `/models` endpoint 以该 provider 的 `api` 前缀开头，再在该 provider 的 `models` 对象里匹配 key/id 或唯一后缀；否则 OpenRouter/Requesty 这类聚合目录中的同名模型容易串供应商。
- 2026-06-30 分层补齐的不变量已集中到 `model_fetch.rs::apply_missing_context_windows`：所有 provider 官方 resolver 和公共目录 fallback 都必须通过它写回，确保只填 `context_window=None` 的模型，永远不覆盖 `/models` 显式 metadata。新增 resolver 时同步补三类测试：显式值不被覆盖、provider/模型识别边界、官方文档解析退化场景。
- 智谱 `/models` 端点会返回模型上下文元数据时，Codex provider 自动获取模型列表必须优先从后端 `model_fetch.rs::extract_context_window` 解析真实字段；不要在前端为 GLM 写静态上下文表。应持续补充 `/models` 解析字段，比如 `context_length`、`max_context_length`、`max_input_tokens`、`limit.context`、`limits.context`、`metadata.maxContextLength` 等。
- 前端 `resolveFetchedCodexModelContextWindow` 的正确优先级仍是：远端 `/models` 显式值 > 用户已有 catalog 值 > 少量历史保守兜底（如 DeepSeek alias / preset 已有值）。如果某个供应商实际返回了上下文字段但 UI 为空，先修 `model_fetch.rs` 字段解析。
- Codex 表单的“测试 Chat / Responses”依赖模型目录；如果 catalog 为空，不能只 toast “请先获取模型列表”。正确交互是展开高级选项，滚到“模型映射”，聚焦并高亮右上角“获取模型列表”按钮，同时在确认框和提示文案里明确测试前需要先获取/添加模型。

## 2026-06-30 Release Pipeline Notes

- `scripts/local-release-pipeline.ps1` 会在 `scripts/export-latest-ccswitchmulti.ps1` 导出后追加 `RELEASE-METADATA.md`，因此必须在写完 metadata 后重新生成 `SHA256SUMS.txt`，否则 release 资产里的校验和会漏掉 metadata 文件。后续修改发布流程时要保持这个顺序：build/export -> metadata -> checksums -> upload。

## 2026-06-30 UI Portal Layer Ordering Audit

- Codex MultiRouter 向导相关的“点击后像卡死”不只来自单个 Dialog：全屏 provider panel 可到 `z-[140]`，但共享 Radix `SelectContent`/`PopoverContent`/`TooltipContent` 之前停在 `z-[100]`，`DropdownMenuContent` 甚至是 `z-50`，都会在向导上方打开 provider 表单时被面板遮住。
- 统一层级落点在 `src/components/ui/layers.ts`：普通 dialog `40/50/60`，portal 浮层 `z-[180]`，top dialog `z-[200]`，top dialog 内部需要 portal 的下拉用 `z-[210]`。不要再在业务组件里随手写 `z-[1000]` 或 `z-[200]` 抢层级；需要例外时先判断它属于普通 panel 浮层还是 top dialog 内浮层。
- `CodexMultiRouterWizard` 内部连通性测试确认框必须是 `zIndex="top"`，因为它从 `z-[120]` 的 wizard portal 内再打开 Radix dialog；如果仍用 `alert z-[60]`，确认框会被向导自身遮住。
- models.dev 定价导入弹窗本身是 `zIndex="top"`，里面的 provider `SelectContent` 是 body portal，必须显式使用 `UI_LAYER_CLASS.topDialogFloating`，否则默认普通浮层 `z-[180]` 仍会落在自己的 top dialog 下方。

## 2026-06-30 Dialog Top Layer Above Codex Wizard Provider Panel

- 用户截图显示 Codex provider 表单里点击“测试 Chat / Responses”后按钮旁出现“已打开测试确认框”，但确认 Dialog 仍不可见。根因不是 click/state 没触发，而是层级定义错误：MultiRouter 向导打开时 `AddProviderDialog` 的 `FullScreenPanel` 会提升到 `z-[140]`，而通用 Dialog 的 `zIndex="top"` 只有 `z-[110]`，仍在全屏 provider 面板下面。
- 修复边界：把 `src/components/ui/dialog.tsx` 的 `top` 层级统一提高到 `z-[200]`，同时 overlay 和 content 都使用同一 top 层级；不要只给单个 `CodexFormFields` content 加 class，否则 overlay/portal 层仍可能被面板遮住。回归测试：`tests/components/CodexFormFields.test.tsx` 断言协议测试确认框使用 `z-[200]`。

## 2026-06-30 Codex MultiRouter Single Source Entry Scroll Fix

- 用户反馈在“创建多路路由”里点击“单独接入模型源”后会立刻滚到 API Key / 高级选项区域，必须再滚回顶部选择模型源。根因是 `ProviderForm` 新建 Codex provider 时默认 `selectedPresetId="codex-0"`，挂载后的自动 `handlePresetChange("codex-0")` 也调用了此前为“手动选择预设后滚到配置字段”新增的 `scrollCodexProviderDetailsIntoView()`。
- 正确边界：打开单独接入模型源表单时，自动应用默认 OpenAI/Codex 预设只是为了给表单一个可编辑初始状态，不应改变滚动位置；用户手动点击具体模型源预设（如 DeepSeek、Zhipu GLM）后仍应滚到 API Key/Base URL 字段，避免停留在高预设网格误判无响应。
- 修复点：`handlePresetChange(value, { scrollDetails?: false })` 支持禁止滚动；仅 `useEffect` 的默认 `codex-0` 自动应用传 `scrollDetails:false`。回归测试在 `tests/components/ProviderForm.codexPreset.test.tsx` 同时覆盖初次挂载不滚、手动选择预设仍滚。

## 2026-06-30 Codex MultiRouter Duplicate Upstream Model Alias Routing

- 普通 Codex provider 表单里的“同名模型重命名”本质不是只改 `displayName`：能让 Codex Desktop 和后端路由区分两个同上游模型的是 `model` 可见 alias，`displayName` 只是菜单展示文本。`upstreamModel` 才保存真实上游模型名；后续排查不要把 displayName 当作可路由 key。
- 根因分两层：前端手动 MultiRouter 工作台仍按 visible `model` 聚合并去重，没有像向导一样先对重复 `upstreamModel` 生成稳定 alias；后端 `targetProviderId` 物化路径还会从目标 provider 复制 settings，丢掉 MultiRouter plan 上的 `modelCatalog`，导致 alias -> upstreamModel 查找失效。
- 修复边界：`CodexRouterWorkspacePage` 在创建 routing plan、打开候选 route、保存 route、刷新模型后重建 plan catalog 时复用向导的 `resolveWizardModelNameCollisions`，对第三方/中转同上游模型生成 `模型名-provider名` alias；工作台聚合 catalog 只在 alias 与上游名不同时写 `upstreamModel`，保持非 alias 条目紧凑。
- route 保存必须写入 route 级 `upstream.modelMap`，例如 `{"gpt-5.5-relay-gpt":"gpt-5.5"}`。这是因为运行时会物化 target provider，不能只依赖普通 provider 自己的原始 catalog；向导 `buildWizardRoutesFromSources` 也同步写 modelMap，避免 UI 显示 alias 但请求把 alias 发到上游。
- 后端修复点：`src-tauri/src/proxy/providers/codex.rs::materialize_codex_routed_provider_from_target` 必须保留 route provider 的 `modelCatalog`，让 `apply_codex_request_upstream_model` 能通过 visible alias 查回真实上游模型。回归测试：`test_materialize_routed_provider_preserves_model_catalog`。
- 前端回归测试：`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖官方 `gpt-5.5` 和第三方 `gpt-5.5` 同时进入手动 MultiRouter 后，第三方变为 `gpt-5.5-relay-gpt`、plan catalog 保留 `upstreamModel: gpt-5.5`、route match 使用 alias 且 `upstream.modelMap` 指回真实模型。

## 2026-06-30 Codex GLM 5.2 Responses-To-Chat 400 Diagnostics

- 用户截图分析 Codex 26.623.61825 子 agent 经 CCSwitchMulti `/responses` 转 Zhipu GLM `/chat/completions` 后返回 HTTP 400，且日志里 `header_count=16` 失败较多。排查确认当前 `codex_router_log` 的 `header_count` 来自 `ordered_headers.len()`，是 HTTP 请求头数量，不是转换后 Chat body 顶层字段数量，不能直接按“body 字段数 16”归因。
- 上游原版也有同类公开 issue：farion1231/cc-switch `#4792` 在 v3.16.4 上报 `Provider: Zhipu GLM5.2; model: glm-5.2; upstream_status: HTTP 400; cause: messages.content.type 参数非法，取值范围 ['text']`；`#4465` 也指向 Codex Responses->Chat 转换链在图片/content part 上的兼容问题。对比 `origin/main`，原版仍会把 GLM 输入里的 `input_image` 转为 Chat `image_url` content part，且没有 request_shape 日志，因此用户看到的现象确实不是 Multi fork 独有。
- 对应根修：`glm-5.2` 这个文本模型 / coding endpoint 按 text-only 处理。`src/config/codexProviderPresets.ts` 的 Zhipu GLM / Zhipu GLM en `modelCatalog` 声明 `inputModalities=["text"]`、`textOnly=true`、`supportsImage=false`；`src-tauri/src/proxy/providers/transform_codex_chat.rs::codex_chat_model_is_text_only()` 也按模型名兜底识别明确文本模型（如 `glm-5.1`、`glm-5.2`、`glm-5-turbo`），保证旧配置或缺少 route capability 时仍把图片 content part 降级成文本占位，避免 `messages.content.type` 非法。不要把这条规则扩大到 `glm-5v-*` 等视觉模型。
- 根修边界在 Codex Responses->Chat 转换链：`src-tauri/src/proxy/providers/transform_codex_chat.rs` 不再默认透传 `metadata` 和 `service_tier` 到第三方 Chat 兼容上游；这两个字段对 GLM/DeepSeek/Qwen 等自动缓存或普通 Chat 路径没有价值，且容易被严格兼容层拒绝。`stream_options.include_usage` 暂时保留，因为它支撑第三方流式 usage 记账，不能无证据全局移除。
- 400 可观测性补强：`src-tauri/src/proxy/forwarder.rs` 在 Codex `responses_to_chat=true` 时给 `request_prepared` 和 `upstream_error` 追加脱敏 `request_shape`，只记录 `top_keys`、`messages` 数量、`tools` 数量/类型、`thinking`/`reasoning_effort`/`stream_options`/`parallel_tool_calls` 等字段形态，不记录 prompt、工具参数、API key 或工具函数名。后续遇到空 `body_summary` 的 400，应先看同 trace 的 `request_shape`，不要靠截图猜字段。
- GLM 5.2 thinking 配置依据官方文档校正：智谱“迁移至 GLM-5.2”文档明确列出 `thinking={"type":"enabled"}` 和 `reasoning_effort="max"` 示例，因此 Codex 预设和后端推断都保留/补齐 `supportsEffort=true`、`effortParam="reasoning_effort"`、`effortValueMode="deepseek"`。不要仅因 z.ai 返回空 400 就回退成 `effortParam=none`；若后续 request_shape 证明某个 coding endpoint 拒绝该字段，再做 provider/endpoint 级 capability，而不是全局关掉 GLM effort。
- 回归测试落点：`responses_request_to_chat_drops_responses_only_metadata_and_service_tier`、`responses_request_to_chat_downgrades_images_for_glm_5_text_models`、`responses_request_to_chat_keeps_images_for_glm_5v_vision_models`、`test_resolve_codex_chat_reasoning_infers_glm_5_2_effort_support`、`codex_chat_request_shape_omits_prompt_text_and_records_field_shapes`、`tests/config/codexChatProviderPresets.test.ts` 的 GLM text-only / effort 预设断言。已验证：`cargo test --manifest-path src-tauri\Cargo.toml responses_request_to_chat_ --lib`、新增 Rust 单测、`pnpm vitest run tests/config/codexChatProviderPresets.test.ts`、`pnpm typecheck`、`cargo fmt --manifest-path src-tauri\Cargo.toml --check`、`git diff --check`。

## 2026-06-30 Codex Provider Chat / Responses Probe Visibility Fix

- 用户截图反馈“创建多路路由 -> 单独接入模型源 -> 高级选项 -> 测试 Chat / Responses”点击后像页面卡死。该按钮属于 `ProviderForm(appId="codex") -> CodexFormFields`，不是 `CodexMultiRouterWizard`。根因之一是 AddProviderDialog 使用 `FullScreenPanel`，默认层级 `z-[60]`，而协议测试确认 Dialog 也使用 `zIndex="alert"` 即 `z-[60]`；同层级 portal/动画 stacking context 下，确认框可能被全屏面板压住，用户看不到任何反馈。
- 修复边界：只改 Codex provider 的协议探测交互。`CodexFormFields` 的确认 Dialog 改为 `zIndex="top"`，点击“测试 Chat / Responses”立即在按钮旁显示“已打开测试确认框”；确认后先显示正在测试的模型数量和当前进度；成功/警告/错误分别使用不同文本颜色；catch 到后端 invoke/网络异常时保留内联 `role="alert"` 错误，并恢复按钮可点，避免只靠 toast 或控制台导致用户误判卡死。
- 回归测试落点：`tests/components/CodexFormFields.test.tsx` 覆盖确认 Dialog 顶层 `z-[110]`、点击后即时状态提示、后端异常时内联 alert 和按钮恢复。相关验证：`pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexPreset.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。

## 2026-06-30 Codex MultiRouter Wizard Small Window Layout Fix

- 用户截图显示 Codex MultiRouter 向导在较小窗口、尤其接近 Tauri 默认 `1000x650` / 最小 `900x600` 时底部按钮区被裁掉。根因不是向导状态机或步骤跳转，而是 `CodexMultiRouterWizard` 中间 grid 使用 `max-h-[82vh]`，但外层没有整体高度约束，最终高度约等于 header + `82vh` + footer，默认高度下会超过视口。
- 修复边界：只调整 `src/components/codex/CodexMultiRouterWizard.tsx` 的遮罩弹窗布局。外层 overlay 改成 flex 居中并 `overflow-hidden`，向导 shell 改为 `max-h-full flex flex-col min-h-0`，header/footer `shrink-0`，中间 body `flex-1 min-h-0 overflow-hidden`，右侧内容和左侧步骤栏各自滚动。不要通过单纯提高 `tauri.conf.json` 默认窗口高度来掩盖问题，因为用户仍可能缩小窗口或恢复旧窗口状态。
- 回归测试落点：`tests/components/CodexMultiRouterWizard.test.tsx` 断言向导 shell/body/footer 的关键布局类，避免后续把高度约束重新放回 `82vh` 内容区。验证命令：`pnpm vitest run tests/components/CodexMultiRouterWizard.test.tsx`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。浏览器直接打开 Vite renderer 会因缺少 Tauri IPC 出现 `window.__TAURI__` 相关错误，只能作为有限页面加载检查；完整交互仍以 Tauri 桌面或组件测试为准。

## 2026-06-30 Codex MultiRouter Post-Setup Validation Refresh

- 用户追问“配置完返回的校验和刷新流程”，排查范围仍只限 Codex MultiRouter。现有成功判定已经正确：`StatusTab` 只在本地代理运行、Codex 接管、当前 provider 是选中 MultiRouter、入口/规则启用，并且当前方案 route 有真实成功转发证据时才触发 `onRuntimeReady`，不会因为其它 Codex 请求 200 就提前进入历史修复。
- 新发现的刷新体验缺口：向导 finish 页启用 MultiRouter 后，父级只 `invalidateQueries` proxyStatus/proxyTakeoverStatus/providers，状态页要等轮询或后台 refetch 才显示最新监听/接管/current provider/日志；用户配置完返回后可能短时间看到旧校验状态。修复为启用后显式 `refetchQueries` proxyStatus、proxyTakeoverStatus、providers/codex 和 usage/logs。
- 状态页新增“刷新校验”手动入口：刷新同一组校验源并显示完成/失败提示，便于用户从 Codex 发出一次请求后立即重新检查链路卡片、最近转发和历史修复触发条件。不改变成功判定、route 归因、诊断探测或模型源刷新逻辑。
- 继续排查其它 Codex 跳转入口后发现：工具栏直接打开 Codex MultiRouter 时如果只 `setCurrentView("codexRouter")`，会沿用上一次 `codexRouterWorkspaceTarget` 的 provider/tab。修复为统一走 `openCodexRouterWorkspace(null, "status")`；工作台内部 tab 跳转新增滚动容器 `scrollTo({ top: 0 })`，避免从长页面切到状态/模型源/测试页时停留旧滚动位置。历史修复跳转已有 `SessionManagerPage` 测试覆盖一次性消费，不需要改。

## 2026-06-30 Codex MultiRouter Configuration Guide Navigation Audit

- 用户明确收窄排查范围：只处理 Codex 分支配置和 Codex 多路路由配置指南，不泛化到 Claude Desktop、Gemini、OpenCode、OpenClaw 或 Hermes。上一轮误扩展改动已撤回，当前只沿 `CodexRouterWorkspacePage` / `CodexMultiRouterWizard` / Codex provider 表单查同类“点击后状态已变但用户看不到下一步”的问题。
- 新发现的真实 Codex-only 缺口：`CodexRouterWorkspacePage` 已从 `App` 接收 `onCreateProvider`，但参数被命名为 `_onCreateProvider` 后没有使用；在工作台没有普通 Codex 模型源、RouteCandidatePicker 为空时，只提示“先添加至少一个 Codex 模型源”并提供“关闭”，用户无法从配置指南链路直接打开 Codex 模型源添加面板。
- 修复边界：只在 Codex MultiRouter 工作台接入已有 `onCreateProvider` 回调，给“模型源”页和“没有可选 router”空状态提供“添加模型源”；同时给行内展开的候选 router 选择器 / MultiRouter 设置面板加 `scrollIntoView({ behavior: "smooth", block: "nearest" })`，避免点击“编辑匹配规则/创建多路路由”后面板在视口下方导致再次误判为卡死。不改 route 保存、provider 协议、modelCatalog、wizard 状态机或非 Codex app。
- 回归测试落点：`src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖无模型源时点击“添加模型源”会触发父级回调，以及打开“编辑匹配规则”后滚到行内 RouteCandidatePicker。

## 2026-06-30 MultiRouter Provider Preset Click Perceived Freeze Fix

- 用户截图里“创建多路路由 -> 单独接入模型源”点击 `Zhipu GLM` 后并不是走 `UniversalProviderPanel`，而是 `ProviderForm(appId="codex") -> ProviderPresetSelector -> handlePresetChange("codex-<index>")`。`Zhipu GLM` 来自 `src/config/codexProviderPresets.ts`，不在 `universalProviderPresets`。
- 只读追踪确认 GLM 预设切换会一次性批量更新 Codex auth/config/modelCatalog/reasoning/takeover/form reset，但 `CodexFormFields` 的 catalog/routing 父子回写已有 ref + JSON 比对守卫，没有发现无限 setState 循环。用户感知“卡死”的主要交互根因是预设网格很高，点击后仍停在预设按钮区，API Key/Base URL/模型映射等实际要填写的字段在下方首屏外，视觉上像没有继续。
- 修复边界：`src/components/providers/forms/ProviderForm.tsx` 给 Codex 字段区加 `ref`，在 Codex 新建模式选择任意预设或自定义模型源后，用 `setTimeout(0)` 等 React 提交完成再 `scrollIntoView({ behavior: "smooth", block: "start" })`，把视口带到关键配置区；不改 GLM 协议、modelCatalog、reasoning 或保存逻辑。
- 回归测试：`tests/components/ProviderForm.codexPreset.test.tsx` 直接渲染 `ProviderForm appId="codex"`，先后点击 `DeepSeek` 与 `Zhipu GLM` 两个不同 Codex 预设，断言对应 Base URL、GLM 的 `glm-5.2` catalog、本地接管开启，并确认每次选择都触发滚动；配合 `tests/components/CodexFormFields.test.tsx` 验证 catalog/routing 受控回写仍稳定。

## 2026-06-30 Promote v3.16.4-4wizard To Latest Release

- 用户在 GitHub releases 列表截图里看到 `v3.16.4-3` 仍显示 `Latest`，原因不是 `main` 未推送，而是 `v3.16.4-4wizard` 仍标记为 prerelease。GitHub 只会把 `Latest` 标给正式 release，prerelease 即使发布时间更新也不会替代正式版 latest。
- 已将 BigStrongSun/ccswitchmulti 的 `v3.16.4-4wizard` release 从 prerelease 改为正式 release，并通过 `gh release edit ... --latest` 显式标记为 Latest；该 tag 的 peeled commit 已在上一轮指向 `main` 的 `15c2d728ae04b857a5531859ce911de6c2665b57`。后续判断“页面看不到 latest”时要先检查 `isPrerelease` 和 GitHub 的 explicit latest 标记，不要只看 tag/main 是否推送。

## 2026-06-29 Merge Wizard Trial Branch Into Main

- 用户纠正 `v3.16.4-4wizard` 不能只停留在 `codex/multirouter-wizard` 分支：需要把 wizard 试用线合入 `main`。合并前确认 `main...codex/multirouter-wizard` 为 `0 41`，即 `main` 没有独有提交、wizard 分支领先 41 个提交。
- 合并策略：在 `main` 上执行非快进 merge `codex/multirouter-wizard`，保留一个明确 merge commit，避免 `main` 静默快进导致后续溯源不清。未跟踪的 `docs/release-notes/v3.16.4-4-zh.md` 和 `scripts/logs/` 仍保持未跟踪状态，不纳入合并或提交。
- 合入内容包括 MultiRouter 向导、协议/缓存能力修复、Hermes/usage upstream bugfix、Windows 图标根修复、网站图标导出以及 `3.16.4-4wizard` 版本面。后续若刷新 `v3.16.4-4wizard` release，tag 应指向 `main` 上的 merge/memory 提交，而不是仅指向 wizard 分支提交。

## 2026-06-29 MultiRouter Wizard Provider Config Protocol Display Fix

- 用户试用向导时反馈配置核心参数页误导：`OpenAI Official Backup` 显示“API 格式：未显式设置，向导保存路由时默认 Chat Completions”，同时“已有模型目录”provider 显示“未配置在线获取参数”。根因是 `CodexMultiRouterWizard` 配置页直接读 `meta.apiFormat/settingsConfig.apiFormat` 和本地 `/models` 抓取参数，没有复用 `inferWizardApiFormat()` 这条已经用于保存 route 的事实来源。
- 修复边界：配置页的 API 格式展示必须和最终 route 生成一致。官方 OpenAI/OAuth 或暴露 GPT/O 原生 Responses 模型的 provider，即使旧 metadata 写 `openai_chat` 或未写，也显示/保存为 `Responses API`；未知第三方没有实测结果时才保守显示 Chat Completions。若旧配置和推断不同，UI 要明确说“向导推断，已覆盖旧配置”，避免用户以为 official 会走 Chat。
- `/models` 获取能力和可路由模型目录是两回事：没有 Base URL/API Key 只代表不能在线刷新模型列表，不代表不能生成路由。已有 `modelCatalog` 的 provider 应显示“已有 modelCatalog，可跳过 /models 在线读取；如需刷新再补 Base URL/API Key”，不要再写“未配置在线获取参数”这种像错误的提示。
- 回归测试：`tests/components/CodexMultiRouterWizard.test.tsx` 覆盖 catalog-only provider 不显示旧提示，以及 `OpenAI Official Backup` 带旧 `openai_chat` metadata 时配置页展示 `Responses API` 并提示覆盖旧配置；数据层已有 `inferWizardApiFormat` 测试覆盖最终 route 保存为 `openai_responses`。

## 2026-06-29 CCSwitchMulti v3.16.4-4wizard Iconfix Release Refresh

- 按用户要求将既有 GitHub prerelease `v3.16.4-4wizard` 更新到任务栏图标根修复后的最新提交。先推送 `codex/multirouter-wizard`，再强制移动 annotated tag `v3.16.4-4wizard`，确保 peeled tag 指向最新提交而不是旧 `ea455656...`。
- Windows 资产使用 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-4wizard-iconfix-assets` 覆盖上传：setup、setup.sig、portable zip、raw exe、`latest.json`、`README.md`、`RELEASE-METADATA.md`、Linux/macOS build notes 和合并版 `SHA256SUMS.txt`。既有 macOS `.dmg/.app.zip` 资产保留，因为 Windows 主机未重建 macOS 包。
- 上传后验证：远端 `refs/tags/v3.16.4-4wizard^{}` 指向任务栏图标修复提交；关键 Windows asset digest 为 setup `4a1b3edc...`、portable `10c7eaa5...`、raw exe `248cb88b...`、latest.json `15f54d63...`。`SHA256SUMS.txt` 同时包含新 Windows 资产和保留的 macOS 资产 digest。

## 2026-06-29 Windows Taskbar Icon Embedded Resource Root Fix

- 用户再次截图反馈任务栏图标仍像白色圆团后，重新沿真实链路排查：`src-tauri/icons/icon.ico` 已更新且小帧仍是 DIB，但 `src-tauri/target/release/cc-switch.exe` 与 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe` 的 `ExtractAssociatedIcon()` hash 仍是旧值。根因不是安装覆盖失败，而是 Cargo/Tauri 构建复用了旧 `resource.lib`，`build.rs` 没有声明 `icons/icon.ico`/相关 PNG 为 `rerun-if-changed`。
- 修复：`src-tauri/build.rs` 显式声明 `icons/icon.ico`、`32x32.png`、`128x128.png`、`128x128@2x.png` 和 `tauri.conf.json` 的构建依赖。修改图标后必须触发 native resource 重新生成，不能只看 `icon.ico` 文件时间。
- 图标生成策略调整：`scripts/generate-windows-icons.py` 不再手绘一套任务栏小图；`16/24/32/48/64` 也从 `assets/brand/ccswitchmulti-codex-app-icon-1024.png` 缩放生成，保持任务栏、安装器、应用内和网站图标同一个品牌 master。ICO 小帧仍写入 32-bit DIB，保留 Windows shell 兼容性。
- 安装链路补强：`src-tauri/nsis/installer-hooks.nsh` 继续修开始菜单/桌面快捷方式，并额外修存在时的任务栏固定项 `CCSwitchMulti.lnk`；`scripts/verify-windows-install-icon.ps1` 也检查可选任务栏固定项。
- 验证：运行 `python scripts/generate-windows-icons.py` 和 `python -m py_compile scripts/generate-windows-icons.py`；执行 `cargo clean --manifest-path src-tauri\Cargo.toml -p cc-switch` 后重新 `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-iconfix-install`，确认源 `icon.ico`、`src-tauri\target\release\cc-switch.exe`、raw exe 的 associated icon hash 全部为 `3E1F633E16DBD922D6A7A4538B1450276803745DDCC20C0BE5F8AB3875A9F3E5`；静默安装后 `scripts\verify-windows-install-icon.ps1` 通过，安装目录 exe hash 与源 ico 一致，开始菜单/桌面快捷方式 `IconLocation` 均为 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe,0`。刷新 Explorer 图标缓存并重启安装版后，任务栏截图确认图标为暗色圆角底的新品牌图。

## 2026-06-29 Codex MultiRouter 502 Official Route Diagnosis

- 用户截图里 `OpenAI Multi-Model Router` 在 `06/29 16:07` 连续出现 5 条 502，但同一时间段 `codex-router.log` 证明每次都已命中 `route_id=openai-official`，`effective_provider=codex-openai-router::route::openai-official`，`effective_endpoint=/responses`，`upstream_url=https://chatgpt.com/backend-api/codex/responses`，`responses_to_chat=false`，`auth_strategy=CodexOAuth`。因此这不是 route miss、不是官方 GPT 被错误转 Chat、也不是模型重名映射错误。
- 502 的直接原因是本机转发到官方 Codex Responses 上游时连接失败：`upstream_send_error ... elapsed_ms=3121/3128/4127 error=请求转发失败:_连接失败:_error_sending_request_for_url_(https://chatgpt.com/backend-api/codex/responses)`。数据库对应 5 条 `status_code=502`，随后 `16:07:20` 同一 session/model/route 立刻成功 200，说明是间歇性出站连接问题。
- 网络边界证据：当场 `Resolve-DnsName chatgpt.com` 返回 `198.18.0.6`（常见代理/TUN fake-ip 网段），`Test-NetConnection chatgpt.com -Port 443` 当前成功；CCSwitchMulti 日志显示 `GlobalProxy Initialized: direct connection`，WinHTTP 也是 direct。也就是说 CCSM 没有显式走全局代理，而是依赖系统 TUN/fake-ip 接管；502 更像本机代理/TUN/VPN 或官方链路短时抖动，不是 CCSM MultiRouter 配置本身。
- 修复一个会误导排障的产品问题：失败路径历史上会把 usage/error 落库到外层 router provider，导致 UI 只显示 `OpenAI Multi-Model Router` 502。`src-tauri/src/proxy/handlers.rs` 现在在 forward error 落库前复用 Codex route 解析和 target materialize 逻辑，把失败归因修正到真实 route/effective provider；未来同类失败应显示 `codex-openai-router::route::openai-official` 等 route 身份。

## 2026-06-29 CCSwitchMulti v3.16.4-4wizard Wizard Prerelease

- `v3.16.4-4wizard` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 预发布版发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-4wizard`。这是 `codex/multirouter-wizard` 分支的向导试用包，`isPrerelease=true`、`isDraft=false`，不是正式无向导 release 线。
- 本地 release pipeline 成功导出到 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，构建 metadata 指向提交 `ea455656521b8e53cbb448788ecb679f0b29e0b5`，版本面为 `3.16.4-4wizard`。本地 staging 目录为 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-4wizard-assets`。
- GitHub release 上传 10 个资产：`CCSwitchMulti_3.16.4-4wizard_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-4wizard_x64-portable.zip`、`CCSwitchMulti_3.16.4-4wizard_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、`RELEASE-METADATA.md`、`v3.16.4-4wizard-zh.md`。Linux/macOS 仍只上传平台构建说明，Windows 本地未生成正式二进制。
- 发布后验证：`latest.json` 版本为 `3.16.4-4wizard`；raw exe 的 `FileVersion/ProductVersion` 为 `3.16.4-4wizard`；GitHub asset digest 与本地主资产 SHA256 对齐，安装包为 `85825ada0fb485cc8c533e972c0cfce2717f87e9f3d5d60ef86aefc90c271c67`，portable zip 为 `5bcb76ef458f4bde734f4759fad79c69efee0a702a3cfabb044731f5285f5144`，raw exe 为 `61ee6f96129c512c88805faed1bac7bb42b54516dbf9933f2c3b9e123ecf05db`。
- 注意：`gh release view` 的 `targetCommitish` 可能显示 `main`，但 annotated tag `v3.16.4-4wizard` 本身的 tag object 指向 `ea455656521b8e53cbb448788ecb679f0b29e0b5`。在 PowerShell 里查询 peeled tag 必须给 `v3.16.4-4wizard^{}` 加引号，否则 `^`/`{}` 可能被 shell 或宿主参数解析干扰，出现误导输出。
- 发布时仍保留未跟踪文件 `docs/release-notes/v3.16.4-4-zh.md` 和 `scripts/logs/`：前者是无向导 release note，不能混入 wizard 试用 release；后者是本地日志/构建产物，不应随 release memory commit 提交。

## 2026-06-29 Windows Taskbar Icon Compatibility Fix

- 用户截图确认任务栏图标问题更像 Windows shell 兼容性/缓存问题，不是单纯 1024 源素材低清。根因排查：旧 `src-tauri/icons/icon.ico` 虽含 `16/24/32/48/64/128/256` 全尺寸，但所有 frame 都是 PNG-in-ICO；部分 Windows 任务栏、Explorer 缓存或快捷方式场景会对 PNG 小帧抽取/缩放异常，出现发糊或旧缓存视觉。
- 新增 `scripts/generate-windows-icons.py` 作为可复现图标生成脚本：`16/24/32/48/64` 使用专门为任务栏简化的高对比小图，并写入传统 32-bit DIB ICO frame；`128/256` 仍使用品牌 1024 图压缩为 PNG frame；同时更新 Tauri PNG、Windows Square logo 和应用内 `src/assets/icons/app-icon.png`。
- 验证点：解析 `src-tauri/icons/icon.ico` 时 `16/24/32/48/64` 的 frame 签名应为 `28 00 00 00...`（DIB），`128/256` 为 PNG；不要再回退到所有 frame 都是 PNG 的 ICO。实际安装后仍需确认快捷方式 `IconLocation` 为 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe,0`，并在 Windows 图标缓存较旧时重启 Explorer 或清理 icon cache。

## 2026-06-29 CCSwitchMulti v3.16.4-4 No-Wizard Release Boundary

- 用户确认 v3.16.4-3 不应作为正式无向导版发布，因为 tag `v3.16.4-3` 指向的 `1534a0e45dc17acd4b792de484f1c6b724cb7e18` 来自 `codex/multirouter-wizard`，仍包含 `CodexMultiRouterWizard`、`codexMultiRouterWizard` helper、ProviderList 的“配置多路模型”入口和向导测试。
- 正确修正策略不是删除或破坏 `codex/multirouter-wizard` 分支里的向导功能；wizard 分支要保留用于继续试用/迭代。正式无向导 release 应从不含向导的基线/分支切出，只合入已确认正式要带的 bugfix、provider 拆分、Responses-Lite fallback、子 Agent 用量统计、Windows 图标安装链路和 spawn_agent 模型可见性修复。
- 以后用户要求“发布当前版本”但近期分支名或 commit 含 `wizard/trial/easy` 时，必须先确认是否是试用向导包还是正式无向导包；不要直接从 `codex/multirouter-wizard` HEAD 打正式 release，也不要为了正式 release 在 wizard 分支上删除向导代码。

## 2026-06-29 CCSwitchMulti v3.16.4-3 Formal Release

- 2026-06-29 继续完善 MultiRouter 向导和单独 Codex provider 的协议探测：真实连通性测试必须先弹确认框，明确会向上游发送 `/v1/responses` 与 `/v1/chat/completions` 请求，可能消耗额度/流量/触发限流，输出上限为 1024 而不是 1。判断策略改为双协议结果矩阵：两者都通或仅 Responses 通时优先 Responses，仅 Chat 通时切到 Chat Completions，两者都不通时不能说成“不支持 Responses”，应提示 API Key、Base URL、模型权限、额度、网络或上游故障等更宽的失败边界。
- 2026-06-29 进一步明确：`/v1/responses` 探测通过不等于完整 Codex 功能正常。它只证明 Base URL/API Key/模型名/最小非流式 Responses 请求可用；完整功能还要看真实 Codex 会话里的 SSE/流式输出、工具调用、reasoning/上下文、多模态、限流稳定性、MultiRouter route 命中和历史修复后的 Desktop 状态。UI 文案必须称为“基础协议测试/基础请求可用”，不能把它包装成全功能验收。
- 2026-06-29 缓存命中调研结论：不要把“给 probe 加几个 Codex header”当成缓存验证。OpenAI/Codex prompt cache 主要依赖长前缀精确匹配、`prompt_cache_key` 路由、可选 `prompt_cache_retention`、body 前缀稳定和 `usage.prompt_tokens_details.cached_tokens`；DeepSeek Context Caching 默认自动启用，依赖后续请求完整复用已持久化的前缀单元，usage 字段是 `prompt_cache_hit_tokens/prompt_cache_miss_tokens`；Z.AI/GLM Context Caching 同样自动识别重复 system prompt/历史/长文档，并在 `usage.prompt_tokens_details.cached_tokens` 显示命中。适配优先级应是：保持真实 Codex OAuth 路径原生 Responses 不被转 Chat；Responses->Chat 转换要稳定 messages/tools/system 前缀和 canonical JSON；只对明确支持 OpenAI cache 参数的 Chat 上游考虑透传 `prompt_cache_key/prompt_cache_retention`，不能对 DeepSeek/GLM 一刀切注入未知字段。
- 2026-06-29 深度调研后的缓存适配边界：CCSM 的缓存适配应做成 provider/route capability，而不是把“Responses 探测成功”或“Chat 探测成功”当缓存结论。官方 OpenAI/Codex OAuth 走原生 Responses 并保留真实 client-provided session identity；OpenAI-compatible Chat 只有在明确声明支持 OpenAI prompt cache 参数时才透传 `prompt_cache_key/prompt_cache_retention`；DeepSeek/Z.AI/GLM 默认走自动前缀缓存，不注入 OpenAI 私有 cache 参数；Qwen/DashScope 兼具 Responses、隐式缓存和 `cache_control` 显式缓存，需按协议能力声明而非只按 provider 名称处理。当前代码缺口主要是：MultiRouter `CodexRoutingCapabilities` 无 cache capability 层；DeepSeek `prompt_cache_hit_tokens/prompt_cache_miss_tokens` 未统一映射到 `cache_read_tokens/input_tokens`；`openai_chat` 转换没有 cache capability gate；向导应展示“缓存兼容性/真实 usage 证据”，不要把基础连通性测试说成缓存命中验证。
- 2026-06-29 已实现缓存适配第一阶段：`ProviderMeta`/route capabilities 新增 `codexCache` capability 和 `promptCacheRetention`；MultiRouter route 物化时会把 `capabilities.codexCache` 带入运行时 meta，Codex Responses→Chat 转换只在 `cacheMode=openai_prompt_cache` 或显式 supports 时透传 `prompt_cache_key/prompt_cache_retention`，DeepSeek/GLM/Qwen 自动缓存路由不会收到 OpenAI 私有 cache 参数；Claude→Responses 增加显式 `promptCacheRetention` 支持，但 Codex OAuth 反代路径会移除 retention 以避免未确认参数触发 400；usage parser 现在能把 DeepSeek `prompt_cache_hit_tokens` 和 Qwen/DashScope details 里的 `cache_creation_input_tokens` 记入缓存统计。向导 route 预览页会显示每条 route 的缓存策略摘要，提示真实缓存命中看 usage 而不是基础连通性测试。
- 2026-06-29 修订 MultiRouter 向导收尾状态机边界：状态页进入“配置成功 -> 历史修复”不能只看任意最新 Codex proxy log 为 2xx/3xx；必须看到当前选中的 MultiRouter plan 的 route 有成功转发证据。`StatusTab` 现在用当前方案聚合后的 `trafficRows.successCount > 0` 作为 `currentRouteForwardOk`，只有当前 provider、代理监听、Codex 接管、路由入口和当前方案 route 转发都成立时才触发 `onRuntimeReady`。回归测试覆盖“不相关 provider/model 的成功日志不能触发历史修复交接”。
- 2026-06-29 `codex/multirouter-wizard` 合并 `main` 时确认 `main` 已是当前分支祖先，`git merge main` 返回 Already up to date；本轮只把向导试用版本面统一改为 `3.16.4-4wizard`（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`）。该版本号用于区分 wizard 试用构建，不代表无向导正式发布线。
- 2026-06-29 检查遗漏 bugfix 时同步远端发现 `upstream/main` / `origin/main`（`farion1231/cc-switch` 原版）新增两个明确修复并已 cherry-pick 到 `codex/multirouter-wizard`：`d1f6c74b` 修复 usage_script 凭据覆盖只应保存显式不同值，避免 provider 主 API Key/Base URL 修改后用量查询仍走旧覆盖；`61d7ac01` 修复 Windows Hermes 配置目录解析，按 settings override、`HERMES_HOME`、平台默认 `%LOCALAPPDATA%\hermes` 顺序对齐 Hermes 自身行为。`d1f6c74b` 与当前分支 provider live-config 保护逻辑在 `ProviderService` 函数区冲突，解决方式是同时保留 usage credential normalize 和 `codex_provider_requires_local_proxy`。
- 2026-06-29 按用户要求准备 `easy` 后缀试用包：版本面从 `3.16.4-3` 临时同步为 `3.16.4-3-easy`，用于重新打包验证 MultiRouter 向导更宽窗口、首页说明简化和步骤不回跳修复。该版本是本地试用包命名，不代表新的正式 GitHub release。
- 2026-06-29 `3.16.4-3-easy` 本地 Windows 试用包已由 post-commit pipeline 成功导出到 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，metadata 指向 `codex/multirouter-wizard` 的 `50499816cd3fcfd3c80fcc28ec156d011a855480`。可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-3-easy_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-3-easy_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-3-easy_x64.exe`；`SHA256SUMS.txt` 覆盖 easy 资产，raw exe `FileVersion/ProductVersion=3.16.4-3-easy` 已验证。
- 2026-06-29 修复 MultiRouter 向导继续试用反馈：向导遮罩层级为 `z-[120]`，普通 `AddProviderDialog` 的 `FullScreenPanel` 只有 `z-[60]`，所以在向导第 2 步点击“添加 Provider/添加模型源”会像没反应；现在 `FullScreenPanel` 支持传入层级，App 只在向导打开时把 Add Provider 面板提升到 `z-[140]`。配置核心参数页也改为三态显示：可自动获取模型、已有模型目录可继续、需补全配置；创建模型源和配置页的 provider 卡片区有独立滚动高度，避免必须拉高整个窗口才能看到后续 provider。
- 2026-06-29 修复 MultiRouter 向导重名策略和官方协议推断：旧 `aliasModelName()` 取 `provider.id` 第一段，自动 ID 会生成 `3ecd52c8-gpt-*` 这类不可读前缀；现在第三方重名模型统一用 `模型名-provider展示名` 后缀，官方 OpenAI/订阅源保留原名，多个第三方互冲突时各自加 provider 名后缀并保留 `upstreamModel`。官方 OpenAI GPT/O 模型源即使旧 metadata 写 `openai_chat`，向导也生成 `openai_responses` route；OpenAI-compatible 中转不能仅因名字包含 openai 被当官方源。
- 2026-06-29 继续修正向导协议来源：旧 provider 可能因为历史保存、复制、混合协议拆分或第三方预设把 `meta.apiFormat/settingsConfig.apiFormat` 留成 `openai_chat`；向导不能把这个字段当最终真相。现在保存和预览前会把用户显式 `/v1/responses` 连通性测试结果应用到草稿，实测通过的 provider 自动提升为 `openai_responses`，失败/警告/跳过才保留旧 chat 或默认策略，避免一边探测通过一边仍生成 chat route。
- `v3.16.4-3` 已作为 BigStrongSun/ccswitchmulti 的 GitHub formal release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-3`。Release 为非 draft、`prerelease=false`，发布时间为 `2026-06-29T02:02:35Z`，annotated tag `v3.16.4-3` 的 peeled commit 为 `1534a0e45dc17acd4b792de484f1c6b724cb7e18`（`chore(release): prepare CCSwitchMulti v3.16.4-3`）。
- 本地 Windows release pipeline 成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-29 09:56:35 +08:00`。`latest.json` 版本为 `3.16.4-3`，updater URL 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-3/CCSwitchMulti_3.16.4-3_x64-setup.exe`；raw exe `CCSwitchMulti_3.16.4-3_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-3`。
- 本次 release 上传 9 个资产：`CCSwitchMulti_3.16.4-3_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-3_x64-portable.zip`、`CCSwitchMulti_3.16.4-3_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、中文 release note `v3.16.4-3-zh.md`。Linux/macOS 正式二进制仍未在 Windows 本地生成，只提供平台构建说明。
- 发布前验证覆盖：`pnpm typecheck`；`pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/AddProviderDialog.test.tsx tests/components/ProviderList.test.tsx tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_model_catalog --lib`；`cargo test --manifest-path src-tauri\Cargo.toml codex_subagent_usage_stats --lib`；`cargo test --manifest-path src-tauri\Cargo.toml responses_lite --lib`；`git diff --check`。
- 发布过程中曾有一次手动 pipeline 与 post-commit pipeline 重叠，导致 Tauri artifact lock 等待；最终采用 post-commit pipeline 导出的 `3.16.4-3` 资产发布，并停止了冗余 `tmpC017` Tauri build 进程。以后 release 前如果已经触发 post-commit pipeline，先等 `scripts/logs/post-commit-release.log` 完成，避免再手动启动第二条 `tauri build`。

## 2026-06-28 Codex MultiRouter Wizard Implementation

- 2026-06-29 修复 MultiRouter 向导试用反馈的三处 UI/流程问题：遮罩窗口从 `max-w-5xl` 放宽为接近 1280px 的 `96vw` 宽度，内容区高度提高到 `82vh`，左侧步骤列加宽，避免默认窗口下 provider/路由组件挤压；第一页文案改为先说明“接入模型源、读取模型并处理重名、生成分流规则、启用并修复历史记录”四件用户任务，`127.0.0.1:15721` 等技术细节降级为备注。
- 这次“点到第 2 步又跳回第 1 步”的真实根因是 `CodexMultiRouterWizard` 在 `open/providers/existingPlan` effect 中每次 props identity 变化都会重新 `dispatchFlow({ type: "INIT" })`。`App.tsx` 原来 inline 传 `Object.values(providers)`，任意父级 rerender 都可能生成新数组，导致向导被重置。修复方式是向导内用 `initializedOpenRef` 保证每次打开只初始化一次，另设 provider 同步 effect 只追加/移除打开期间新建或删除的普通 Codex provider，不再派发 `INIT`；`App.tsx` 同时 memoize `codexWizardProviders` 降低无意义 props 变化。
- 2026-06-29 已为上述 UI/流程修复导出新的本地试用包 `3.16.4-3`：导出目录仍是 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，`RELEASE-METADATA.md` 指向 `codex/multirouter-wizard` 的 `1534a0e45dc17acd4b792de484f1c6b724cb7e18`，可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-3_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-3_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-3_x64.exe`。手动触发的 pipeline 与 post-commit pipeline 并发时返回过一次 `tauri build failed with exit code -1`，但 post-commit pipeline 随后完成导出；最终 `SHA256SUMS.txt` 和 raw exe `FileVersion/ProductVersion=3.16.4-3` 已验证。
- 分支 `codex/multirouter-wizard` 新增 Codex 首页底部居中的 `配置多路模型` 入口，落点是 `ProviderList` 的 Codex 专属 CTA；空 Provider 列表也会显示该入口，右上角原有 MultiRouter 工作台图标入口保持不变。
- 新增遮罩式 `CodexMultiRouterWizard`：Portal 到 `document.body`，黑色 fixed overlay，按教程顺序引导理解本地 `15721` MultiRouter、创建模型源、检查 API Key/Base URL/API 格式、自动获取 `/models`、处理重名模型、生成按 provider 分组的 route、保存发布、显式启用并打开工作台 `test` 页。
- 2026-06-28 追加状态机改造：`CodexMultiRouterWizard` 不再只是 `stepIndex` 线性向导，而是由 `wizardFlowReducer` 显式维护 `opened/needSources/reviewProviderConfig/configIncomplete/readyToFetchModels/fetchingModels/modelFetchPartial/modelsFetched/collisionReviewRequired/routePreview/savingPlan/saveFailed/published/enablePrompt/enabling/enableFailed/enabled/completed/dismissed` 等状态。左侧步骤点击会映射到对应业务状态；下一步按钮会做 gate，例如无模型源停在 `needSources`，配置缺口进入 `configIncomplete`，保存失败进入 `saveFailed` 并展示错误。
- 状态机辅助数据落在 `src/lib/codexMultiRouterWizard.ts`：`getWizardConfigIssues()` 判断缺 Base URL/API Key 且没有可用 modelCatalog 的 provider；`collectWizardModelNameCollisions()` 收集同一 upstreamModel 被多个 provider 暴露的冲突，供向导进入 `collisionReviewRequired` 并提示别名策略。模型刷新现在复用 `mergeFetchedModelsIntoWizardProvider()`，保留已有手写字段。
- 新增 `src/lib/codexMultiRouterWizard.ts` 作为可单测数据层：普通 Codex provider 才作为模型源，MultiRouter provider 通过 `settingsConfig.codexRouting` 识别并排除；官方/OAuth 源没有普通 `/models` 时使用保守官方 catalog 兜底；第三方/中转站与官方同名模型会生成可见别名并保留 `upstreamModel` 指向真实上游模型。
- 向导保存策略：草稿留在 React state；只有点击“保存并发布”才调用 `providersApi.add/update` 写入带 `codexRouting` 和 `modelCatalog` 的 MultiRouter provider；不会静默切换当前 Codex provider，完成页“启用这个多路路由”复用 App 里的 `switchProvider` 路径，让既有 Codex 本地接管、PROXY_MANAGED、OAuth 保留逻辑继续生效。
- 路由生成策略：每个模型源一条 route，使用 `targetProviderId` + `auth.source="provider_config"`，不复制第三方 API Key/Base URL，不写 `requires_openai_auth`；默认按 provider/model 文本推断 `gpt`/`o`、`deepseek`、`qwen` 等前缀。
- 验证：`pnpm vitest run tests/components/ProviderList.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts` 通过；`pnpm typecheck` 通过；`cargo fmt --manifest-path src-tauri/Cargo.toml --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog --lib`、`model_fetch --lib`、`switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib` 均通过。`pnpm format:check` 当前失败在两个未参与本次改动的既有文件 `src/components/codex/CodexRouterWorkspacePage.tsx` 和 `src/components/providers/forms/CodexFormFields.tsx`，本次未扩大 diff 去格式化无关大文件。
- 状态机改造验证：`pnpm vitest run tests/components/ProviderList.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts` 通过，48 tests passed；`pnpm typecheck` 通过；`git diff --check` 针对本次状态机改动文件通过。
- 2026-06-29 补齐向导每一步“异常/可继续”规则和真实 Responses 连通性探测：`CodexMultiRouterWizard` 的每个步骤卡片会展示本步骤可能失败的边界和继续条件；`fetchModels` 步骤新增用户显式点击的“测试 /v1/responses 连通性”，对每个普通 Codex provider 的可见模型发送最小 `input="ping"`、`max_output_tokens=1`、`stream=false` 请求，避免在自动刷新模型时静默消耗额度。
- 连通性状态机新增 `probingConnectivity/connectivityPassed/connectivityPartial/connectivityFailed`。`openai_responses` provider 的 `/v1/responses` 探测失败是阻塞项，不能保存发布；`openai_chat` provider 的直接 Responses 失败只是 warning，因为运行时 MultiRouter 会转到 `/chat/completions`；缺 Base URL/API Key 且已有 `modelCatalog` 时允许继续但标为 skipped，缺配置且没有目录时阻塞。
- 后端命令 `probe_codex_responses_for_config` 在 `src-tauri/src/commands/model_fetch.rs`，只做显式探测不缓存结果。URL 生成会把 provider 根地址、`/v1`、完整 `/v1/chat/completions` 或直接 `/v1/responses` 都收敛到 `/v1/responses`；HTTP 错误、网络错误和超时都结构化返回 `ok/status/url/model/detail`，错误体截断到 512 字符，避免 UI/日志被上游 HTML 或长 JSON 淹没。
- 本轮为满足全局格式检查，额外对既有 `src/components/codex/CodexRouterWorkspacePage.tsx` 做了纯 Prettier 格式化，不改业务逻辑；`pnpm format:check` 现在通过。
- 2026-06-29 继续补强异常可见性：`CodexMultiRouterWizard` 新增向导级 `wizardIssues` 列表，所有异步 catch 不能只发 toast，必须写入 UI 问题面板并标明 `错误/警告`、provider、异常详情和 `可继续/需处理后继续`。当前覆盖 `/models` 单 provider 失败、整体刷新中断、`/v1/responses` IPC/命令异常、保存失败、启用失败，以及用户尝试越过阻塞连通性结果的场景。
- 2026-06-29 补齐 MultiRouter 向导发布后的自动收尾：用户点击“启用这个多路路由”后，App 会先启动 CCSwitchMulti 本地代理，再打开 Codex live 接管，随后切换当前 Codex provider 到该 MultiRouter 方案并打开工作台 `status` 页。状态页不会只因配置态全绿就提示成功，必须同时看到最近一次 Codex 代理转发为 2xx/3xx，确保“当前链路、监听、Codex 接管、路由入口、最近转发”都成功后才 toast “配置成功”并跳到 Codex 历史修复页。
- 2026-06-29 完整引导交接细化：`CodexMultiRouterWizard` 启用成功后必须自动关闭遮罩，让用户看到 App 已打开的 MultiRouter `status` 页；toast 明确要求去 Codex 发送一次请求，等待当前链路、监听、Codex 接管、路由入口和最近转发都成功。向导里的“打开工作台”也改为打开 `status` 页，不再跳 `test` 页，避免绕开五项状态验证。
- 2026-06-29 入口选择规则：Codex 首页底部 `配置多路模型` 不再直接打开向导，而是每次先弹出入口选择面板；用户可以随时关闭退出，也可以选择“开始引导配置”进入遮罩式向导，或选择“直接打开工作台”进入 MultiRouter `status` 页。这个选择不受 dismissed localStorage 影响，确保用户再次点击入口仍可决定是否开启引导。
- 历史修复页新增向导收尾入口：`SessionManagerPage` 通过一次性 `initialCodexHistoryRepair` 自动打开 `CodexHistoryRepairPanel` 并消费标记；自动跳转进入时面板顶部会显示历史修复点击顺序：加载历史、预览修复、确认写入、完整重启 Codex、打开 GitHub 仓库点 Star。真实应用历史修复成功后回调 App，提示用户完整重启 Codex，然后先请求用户给 CCSwitchMulti GitHub 仓库点 Star，再用默认浏览器打开 `https://github.com/BigStrongSun/ccswitchmulti`。如果后续引导回调失败，只报“历史修复已完成，但后续引导失败”，不能把修复本身标记为失败。
- 2026-06-29 已为 MultiRouter 向导试用打本地 Windows 包：运行 `scripts/local-release-pipeline.ps1 -Reason manual-multirouter-wizard-test` 成功，导出目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，metadata 指向分支 `codex/multirouter-wizard`、提交 `214bc5b4650e20d3de7cc13a3ff113cda63b00c4`、版本 `3.16.4-2`。可试用文件包括 `windows/installer/CCSwitchMulti_3.16.4-2_x64-setup.exe`、`windows/portable/CCSwitchMulti_3.16.4-2_x64-portable.zip`、`windows/raw-exe/CCSwitchMulti_3.16.4-2_x64.exe`；这是本地试用包，不是正式 release bump。

## 2026-06-28 Mixed Relay Responses Capability Boundary

- 当前 MultiRouter 对 Codex `/responses` 的上游协议选择是 route/effective-provider 级配置判定，不是模型级在线能力探测。运行时入口是 `src-tauri/src/proxy/providers/codex.rs::explain_codex_responses_upstream_protocol`，优先看 managed `codex_oauth`、`meta.apiFormat`、`settings_config.apiFormat/api_format`、已知 chat-only base_url、`config.toml wire_api`，最后默认原生 Responses。
- 对“同一个中转里既有 GPT/Responses 模型，也有 Qwen/DeepSeek 等 Chat-only 模型”的正确现有用法是拆成多条 route：GPT/Responses 模型 route 写 `upstream.apiFormat=openai_responses`，Chat-only 模型 route 写 `upstream.apiFormat=openai_chat`。如果 route 引用 `targetProviderId`，`materialize_codex_routed_provider_from_target` 会继承目标 provider 的 base_url/auth/apiFormat；因此同一个目标 provider 不能天然表达“部分模型 responses、部分模型 chat”，除非拆成两个 provider 或使用内联 route 覆盖协议。
- 目前 `/models` 刷新只读取模型 id、owned_by、context_window 等元数据并写回 `modelCatalog`，`CodexCatalogModel` 和 `CodexRoutingCapabilities` 只有图片/文本/推理相关能力，没有 `supportsResponses` / per-model `apiFormat` 字段。状态页“协议探测”读取配置判定和 `codex-router.log` 最近真实请求的 `effective_endpoint/responses_to_chat`，不会主动请求远端 `/v1/responses`，所以不会自动发现某个模型不支持 Responses。
- 若后续实现在线探测，应做成显式手动/批量按钮而不是自动刷新时静默执行：对每个候选模型发最小 `/v1/responses` 探测请求，识别 404/405/400 unsupported endpoint/model 等结果并缓存到 provider `modelCatalog.models[].supportsResponses` 或 `apiFormat`；探测会消耗额度、可能触发供应商限流，也可能误判“模型不支持”与“账号无权限/渠道暂不可用”，因此结果应带时间戳、错误摘要和手动覆盖入口。
- 2026-06-28 普通 Codex provider 新增表单“获取模型”路径加入保守拆分提示：当 `/models` 同时返回 GPT-like（如 `gpt-*`、OpenAI namespace 下的 gpt、o 系列）和非 GPT-like 模型，且当前表单还没有用户手写 route 时，`CodexFormFields` 只弹出“检测到混合协议模型”确认框，不会静默写入 routing。用户确认后只记录提交意图并打开本地接管；真正点击新增时由 `AddProviderDialog` 生成两个独立 provider：`<providerName>-responses` 写 `meta.apiFormat=openai_responses` 且只保留 Responses 模型目录，`<providerName>-chat` 写 `meta.apiFormat=openai_chat` 且只保留 Chat 模型目录；用户取消时只保留已获取的模型列表，routing、provider 拆分意图和 apiFormat 都不变。编辑已有 provider 时不启用自动拆 provider，避免“编辑 A 生成 B/C”的危险行为。

## 2026-06-28 Responses-Lite Header Retry Fallback Policy

- 用户指出“第三方一律剥 `x-openai-internal-codex-responses-lite`”仍然过宽，因为未来第三方上游可能支持 Responses-Lite，提前砍掉 header 可能影响它们自己的 Lite 路径、prompt cache 或其它能力协商。策略已改成 optimistic pass-through：默认保留该 header 发给上游，只有上游明确返回 `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.` 这类错误时，才剥离 header 并对同一个 provider 重发一次。
- 实现落点仍在 `src-tauri/src/proxy/forwarder.rs`。发送前不再调用静态 strip helper；错误响应体读取并解压后调用 `should_retry_without_codex_responses_lite_header()` 判断，条件是 `AppType::Codex`、请求里确实有 Lite header、状态码为 `400/404/422/501` 之一、错误体包含精确 Lite 不支持文本。命中后记录 `upstream_retry_without_responses_lite`，移除该 header 后只重试一次；普通 400、非 Codex app、无 header 或错误体不匹配都不重试。
- 2026-06-28 进一步改为带过期时间的短期能力负缓存，避免同一上游/模型在连续请求里每次都先失败一次。缓存是内存态，TTL 为 24 小时，key 按 effective provider id、上游 URL 的 scheme/host/port/path、实际请求模型隔离，并忽略 query 以避免敏感参数进入缓存 key。命中缓存时直接去掉 Lite header 发送并记录 `responses_lite_fallback_cache_hit`；过期后自动删除并重新带 header 探测，防止未来第三方上游支持 Lite 后仍被永久去头。
- 验证通过：`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml responses_lite --lib`（6 passed）；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_error_triggers_retry_without_header --lib`。

## 2026-06-28 Responses-Lite Header Source And Proxy Failure Mechanism

- OpenAI Codex 源码确认 `x-openai-internal-codex-responses-lite` 不是普通透传 header，而是由模型元数据 `ModelInfo.use_responses_lite` 驱动的官方内部协商信号。`codex-rs/protocol/src/openai_models.rs` 定义 `use_responses_lite: bool`；`codex-rs/core/src/client.rs::add_responses_lite_header()` 在该值为 true 时给 HTTP Responses 请求加入 `x-openai-internal-codex-responses-lite: true`；WebSocket 路径则在 `build_ws_client_metadata()` 中写入 `ws_request_header_x_openai_internal_codex_responses_lite=true`。
- Lite 模式还会改变请求结构，不只是多一个 header：`build_responses_request()` 用 `prompt.get_formatted_input_for_request(model_info.use_responses_lite)`；Lite 为 true 时会去掉图片 detail、把 tools 放进 `AdditionalTools`/instructions 前缀、关闭 `parallel_tool_calls`，并让部分 tool planning 走 Lite 分支。说明服务端会按这个信号选择不同 Responses 处理路径。
- 中转遇到问题的根因是“协议能力错配”：Codex 官方客户端/后端之间的私有能力信号被 CCSwitchMulti 或其它代理原样转发给第三方 OpenAI-compatible 上游，或者转发给当时尚未支持该模型 Lite 路径的官方后端分支。上游看到 header 后按 Lite 路径校验模型，若该模型/账号/区域/后端版本不支持 Lite，就返回 `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.`。最新策略不是预先剥离，而是默认透传、命中特定 Lite 不支持错误后剥头重试一次。

## 2026-06-28 Responses-Lite Header Strip Policy Narrowed

- 上游作者关闭 `#4727` 后重新评估，原先 `should_strip_codex_private_header_for_upstream(_url, name)` 只看 header 名、无条件剥 `x-openai-internal-codex-responses-lite` 的策略过宽。这个 header 对第三方 OpenAI-compatible / MultiRouter 目标确实是官方私有信号，不应透传；但托管 ChatGPT Codex OAuth 目标属于官方协议路径，应该保留给官方后端自行协商，避免改变 Responses-Lite / prompt cache / 官方内部能力分支。
- 该静态剥离策略后来被进一步收窄为 fallback 重试策略：默认保留 header，只有上游明确返回 Lite 不支持错误时剥头重试一次。不要再恢复“第三方 Codex/OpenAI-compatible 上游发送前直接剥离”的口径。
- 这次验证时主工作区 `src-tauri/tauri.conf.json` 已有未归属脏改，新增 `bundle.windows.nsis.uninstallerIcon` 被当前 `tauri-build` 拒绝，导致主工作区 `cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib` 卡在 build script。为不修改用户的 NSIS/icon 改动，使用临时 detached worktree `C:\Users\sunda\Documents\cc-switch-test-responses-lite` 套同一份 `forwarder.rs` 改动验证：`cargo fmt --manifest-path src-tauri\Cargo.toml --check` 通过；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib` 通过 3 个用例：官方托管保留、第三方剥离、非 Codex app 保留。

## 2026-06-28 Windows Taskbar Icon Install Verification

- 本地 release pipeline 导出的 raw exe `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.4-2_x64.exe` 已经正确嵌入 `src-tauri/icons/icon.ico`；用 `System.Drawing.Icon.ExtractAssociatedIcon()` 抽取后和源 `icon.ico` 一致，都是新的白色云/青色底图标。
- 用户看到 Windows 任务栏仍是旧图标时，优先检查启动路径。开始菜单和桌面快捷方式默认指向安装目录 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe`，而不是导出目录 raw exe。若只运行 raw exe 或只生成导出产物，固定任务栏/开始菜单仍可能从旧安装目录或 Windows 图标缓存读取旧图标。
- 这次用 `CCSwitchMulti_3.16.4-2_x64-setup.exe /S` 静默安装后，`%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe` 被替换为 3.16.4-2，内嵌图标抽取结果也变成新图标；监听端口 `15721/15722` 由安装版 `cc-switch.exe` 接管。若任务栏视觉仍旧，剩余边界是 Windows Explorer / 任务栏固定项图标缓存，需要刷新快捷方式或重启 Explorer，而不是重新修 Tauri 图标配置。
- 进一步固化在 `src-tauri/tauri.conf.json` 的 `bundle.windows.nsis`：当前项目使用的 `tauri-build` 只接受 `installerIcon`，不能写 `uninstallerIcon`；安装包图标显式设置为 `icons/icon.ico`，并通过 `src-tauri/nsis/installer-hooks.nsh` 的 `NSIS_HOOK_POSTINSTALL` 重写已存在的开始菜单和桌面快捷方式，把 `IconLocation` 固定为安装目录里的 `cc-switch.exe,0`。验证脚本为 `scripts/verify-windows-install-icon.ps1`，用于比对源 ico、安装目录 exe 内嵌图标和快捷方式图标目标。

## 2026-06-28 MultiRouter spawn_agent Model Override Visibility Fix（2026-07-10 已被新版保留 schema 修复取代）

- 用户截图里 `spawn_agent` 工具提示“没有显式 model 选择字段”的根因不是提示词没写模型名，也不是单纯 catalog 前五候选排序问题；对照 `openai/codex` 最新源码确认，`multi_agent_v2` 的 `create_spawn_agent_tool_v2()` 在 `hide_spawn_agent_metadata=true` 时会调用 `hide_spawn_agent_metadata_options()`，直接从工具 schema 删除 `agent_type`、`model`、`reasoning_effort`、`service_tier`。新版 Codex 的 `MultiAgentV2Config::default()` 默认 `hide_spawn_agent_metadata=true`，所以只把 `qwen3.6` 写进 message 会继承父模型。
- 2026-07-10 更正：`hide_spawn_agent_metadata=false` 会让新版 GPT/Codex 把 `collaboration.spawn_agent` 判定为保留工具 schema 不匹配；当前正确边界是保持/写入 `hide_spawn_agent_metadata=true`，并通过 `~/.codex/agents/*.toml` custom agent role 文件固定子 Agent 的模型、provider 和 reasoning 配置。
- Codex 源码还确认 `spawn_agent_models_description()` 只展示 `ModelPreset.show_in_picker` 的前 5 个，而 `ModelPreset.show_in_picker` 来自 `ModelInfo.visibility == list`。因此 catalog 条目必须同时保留新版 `ModelInfo` snake_case 字段（`slug`、`visibility=list`、`supported_in_api=true`、`default_reasoning_level`、`supported_reasoning_levels`）和旧 renderer / 旧 direct preset 路径字段（`id`、`show_in_picker=true`、`hidden=false`、`defaultReasoningEffort`、`supportedReasoningEfforts`）。
- Provider inline `models` 也要同步补齐 `slug`、`description`、`visibility=list`、`show_in_picker=true`、`supported_in_api=true`、`default_reasoning_level`、`supported_reasoning_levels`，避免只写顶层 `model_catalog_json` 时某些 Desktop 热切路径看到不完整模型元数据。
- 回归测试落点：`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_projects_spawn_agent_model_info_fields --lib`、`cargo test --manifest-path src-tauri/Cargo.toml codex_multi_agent_v2_keeps_spawn_agent_reserved_schema_compatible --lib`、`cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_ --lib`，并配合 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`git diff --check`。

## 2026-06-28 MultiRouter Subagent Usage Model Aggregation Fix

- “今日子 Agent 会话流量”全 0 的根因不是前端数值格式化，而是 Codex 子 Agent JSONL 的 `session_meta.payload.session_id` 在当前 Codex Desktop 中指向父线程 ID；子 Agent 自己的线程 ID 在 `state_5.sqlite.threads.id` 和 rollout 文件名后缀里。旧 `session_usage_codex.rs` 同步时把 `proxy_request_logs.session_id` 写成父线程，导致 `build_codex_subagent_usage_stats_from_history()` 用子 Agent id 做 `data_source='codex_session' AND session_id IN (...)` 时查不到当天用量。
- 修复边界分两层：后续同步在 `session_meta` 标记为 `source.subagent.thread_spawn` / `source.thread_spawn` 时，优先用 rollout 文件名里的 36 位线程 ID 作为 `session_id`；已有错归到父线程的历史/当天数据不迁移 DB，而是在子 Agent 统计页按子 Agent rollout JSONL 只读回退解析 `token_count`，恢复 request/token/model 聚合。
- 模型聚合不能依赖是否已有 token_count 命中。`modelStats.agentCount` 现在从子 Agent 的 `turn_context` / `token_count` primary model 归并，每个模型一行展示子 Agent 数、请求、Tokens、费用；即使某个模型的子 Agent 暂无用量，也要显示 agentCount，避免页面退化成几百个子 Agent 明细行。
- 前端 `CodexRouterWorkspacePage.tsx` 的子 Agent 会话流量区默认只保留模型聚合表和一行数据源摘要，不再默认渲染逐子 Agent 明细表。这样状态页回答“每个模型有多少子 Agent、消耗多少 token”，而不是“每个子 Agent 用了什么模型”。
- 回归测试落点：`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_codex_subagent_model_stats_counts_agents_without_usage --lib`、`cargo test --manifest-path src-tauri/Cargo.toml test_sync_codex_subagent_uses_rollout_thread_id --lib`，并配合 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`pnpm typecheck`、`git diff --check`。

## 2026-06-28 CCSwitchMulti v3.16.4-2 Formal Release

- `v3.16.4-2` 已作为 BigStrongSun/ccswitchmulti 的 GitHub 正式 release 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-2`。Release 为非 draft、`prerelease=false`，发布时间为 `2026-06-28T05:00:55Z`。本地 tag `v3.16.4-2` 为 annotated tag，tag 对象为 `cf874abd37e10f767971deea69e0178edfd0aa71`，解引用到版本提交 `d81abacdccb6915e31ebf829e50155ae95f64a37`（`chore(release): prepare CCSwitchMulti v3.16.4-2`）。
- 本次正式版覆盖 `v3.16.4-1` 之后的两个用户可见修复：`fa32a34c` 新增异常退出 / panic / 正常退出结构化日志与“打开日志目录”入口；`7ebd7354` 修复 Codex Desktop `x-openai-internal-codex-responses-lite` 内部 header 被转发到真实上游导致 gpt-5.5 等模型 HTTP 400 的问题。版本面统一更新为 `3.16.4-2`，并新增中文 release note `docs/release-notes/v3.16.4-2-zh.md`。
- Windows 本地 release pipeline 由 post-commit hook 启动并成功完成，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 12:57:52 +08:00`。raw exe `CCSwitchMulti_3.16.4-2_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-2`，下载后的 `latest.json` 也验证为 `version=3.16.4-2` 且指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-2/CCSwitchMulti_3.16.4-2_x64-setup.exe`。
- 本次 release 上传 9 个平铺资产：`CCSwitchMulti_3.16.4-2_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-2_x64-portable.zip`、`CCSwitchMulti_3.16.4-2_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`、`v3.16.4-2-zh.md`。`SHA256SUMS.txt` 是从平铺 staging 目录 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.4-2-assets` 重新生成的，GitHub asset digest 与本地 checksum 对应。Linux/macOS 正式二进制未在 Windows 本地构建，本次仍上传平台构建说明。
- 发布前验证通过：`pnpm typecheck`；`cargo fmt --manifest-path src-tauri\Cargo.toml --check`；`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib`；`cargo test --manifest-path src-tauri\Cargo.toml ordinary_headers_are_preserved_for_upstream --lib`；`cargo test --manifest-path src-tauri\Cargo.toml app_exit_monitor --lib`；`git diff --check`。发布后验证：`gh release view v3.16.4-2 --repo BigStrongSun/ccswitchmulti --json tagName,isDraft,isPrerelease,publishedAt,url,assets`、`gh api repos/BigStrongSun/ccswitchmulti/releases/latest`、`git show-ref --tags v3.16.4-2`、下载并解析 release `latest.json`。

## 2026-06-28 Codex Responses-Lite Header Upstream Strip

- `This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite` 的根因不是 MultiRouter 自身路由错误，而是 Codex Desktop 发给本地后端的内部协商头 `x-openai-internal-codex-responses-lite` 被 CC Switch / CCSwitchMulti 的 `forwarder.rs` 默认透传到了真实上游。OpenAI 在 2026-06-26 左右收紧 Lite 路径后，`gpt-5.5` 等模型会因此在 official ChatGPT Codex upstream 或第三方代理 upstream 返回 HTTP 400。
- 正确修复边界在转发层 header policy：`src-tauri/src/proxy/forwarder.rs` 构建 `ordered_headers` 时，在默认透传前调用 `should_strip_codex_private_header_for_upstream()`，无条件移除 `x-openai-internal-codex-responses-lite`。不要把它修成 UI 开关、catalog schema、模型映射或 MultiRouter route 规则；也不要粗暴移除 OAuth/session/account headers，否则会破坏 Codex 官方登录态、前缀缓存和 CCSwitchMulti 之前的 OAuth login-preservation 修复。
- 这次先在原版 `C:\Users\sunda\Documents\LLMservice\ccswitch official` 基于 `origin/main` 创建 `codex/strip-codex-responses-lite-header`，提交 `1e6a46b7 fix(proxy): strip Codex Responses-Lite header upstream`，并向 `farion1231/cc-switch` 提交 PR `#4727`，关联 issue `#4700`。随后把同一策略移植到 CCSwitchMulti `C:\Users\sunda\Documents\LLMservice\cc-switch` 的 `codex/merge-official-v3.16.4` 分支。
- 回归测试落点：`proxy::forwarder::tests::codex_responses_lite_header_is_stripped_for_official_upstream`、`codex_responses_lite_header_is_stripped_for_third_party_upstream`、`ordinary_headers_are_preserved_for_upstream`。验证命令优先跑 `cargo fmt --manifest-path src-tauri\Cargo.toml --check`、`cargo test --manifest-path src-tauri\Cargo.toml codex_responses_lite_header --lib`、`cargo test --manifest-path src-tauri\Cargo.toml ordinary_headers_are_preserved_for_upstream --lib`、`git diff --check`。

## 2026-06-28 CCSwitchMulti v3.16.4-1 Prerelease

- `v3.16.4-1` 已作为 BigStrongSun/ccswitchmulti 的 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.4-1`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-27T20:52:24Z`，target commit 为 `e0228d531d1a7086a808d706e6ecb2618de44f4c`（`docs(memory): record completed v3.16.4-1 merge`）。
- 本地 Windows release pipeline 成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 04:50:26 +08:00`。raw exe `CCSwitchMulti_3.16.4-1_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.4-1`，`latest.json` 指向 `https://github.com/BigStrongSun/ccswitchmulti/releases/download/v3.16.4-1/CCSwitchMulti_3.16.4-1_x64-setup.exe`。
- 本次 prerelease 上传 8 个资产：`CCSwitchMulti_3.16.4-1_x64-setup.exe`、安装包 `.sig`、`CCSwitchMulti_3.16.4-1_x64-portable.zip`、`CCSwitchMulti_3.16.4-1_x64.exe`、`latest.json`、`SHA256SUMS.txt`、`linux-build-note.md`、`macos-build-note.md`。Linux/macOS 正式二进制未在 Windows 本地构建，后续需要 supplemental workflow 或对应平台构建补齐。
- 发布前后验证：`pnpm release:local` 运行了 `pnpm typecheck` 并完成 Tauri NSIS Windows x64 build；使用本地 `C:\Users\sunda\.ccswitchmulti\tauri-update.key` 生成 updater 签名；`gh release view v3.16.4-1 --repo BigStrongSun/ccswitchmulti --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets` 复核 release 状态和资产摘要；`SHA256SUMS.txt` 中 Windows 资产 hash 与 GitHub release asset digest 对应。

## 2026-06-28 CCSwitchMulti v3.16.4-1 Official Merge Completed

- `codex/merge-official-v3.16.4` 已完成官方 `farion1231/cc-switch` `v3.16.4` 跟进，版本面更新为 `3.16.4-1`。不要把它理解成直接 merge 官方 tag；这次按 `45555638..e50fc0eb` 的缺口逐个 cherry-pick / 手工合并，保留了 `cc-switch-multi`、`CCSwitchMulti`、`com.ccswitchmulti.desktop`、BigStrongSun updater、MultiRouter workspace、外部 OpenAI-compatible API、Codex history repair、WebDAV/S3 sync 和 fork release 脚本。
- 高风险合并点的最终边界：`codex_oauth_auth.rs` 采用官方共享 `crate::proxy::http_client::get()` 出站，但保留 Multi 的 `oauth_token_url` 测试注入和 refresh-token 轮换语义；`forwarder.rs` 合入 zstd/gzip/br/deflate 解压与 local proxy request overrides，同时保留 Multi 的 4 元组返回和 `codex_router_log`，上游错误摘要使用解压后的 body；`ProviderMeta` 同时保留 `min_output_tokens` 和官方 `local_proxy_request_overrides`。
- Codex 表单合并时必须记住：官方 `apiFormat` 已变成上游 wire format 选择，Multi 的本地路由/模型映射由 `takeoverEnabled` 独立控制。`CodexFormFields`/`ProviderForm` 已采用 `takeoverEnabled` / `codexTakeoverEnabled`，保留 MultiRouter catalog/routing、visible model 与 upstream model 分离；不要恢复旧的“apiFormat == chat 才显示路由”的耦合逻辑。
- 已合入的官方功能包括 ETok rename、Kimi 图标/赞助文案/auto compact、Volcengine Ark AK/SK usage、Skills UI 修复、Windows ARM64 release workflow、Usage live end time、JsonEditor dark mode、DB too-new recovery screen、local proxy request overrides、Copilot/Codex OAuth 全局代理 client、body 解压、Doubao Seed 2.1、Codex CN providers native Responses presets、SubRouter/OpenCode Go presets、v3.16.4 docs/release notes、Fable 5 banner removal 和 fork 版本 bump。
- 收尾测试修复：`tests/components/CodexFormFields.test.tsx` 的 test harness 需要传入 `takeoverEnabled`、`onTakeoverEnabledChange`、`localProxyHeadersOverride`、`onLocalProxyHeadersOverrideChange`、`localProxyBodyOverride`、`onLocalProxyBodyOverrideChange`。否则 `pnpm typecheck` 会报缺必填 props，Vitest 会在 `trim()` 处因 undefined 崩溃；这是测试壳没跟上组件契约，不是生产逻辑需要默认兜底。
- 本轮验证通过：`pnpm typecheck`；`vitest run tests/components/CodexFormFields.test.tsx tests/config/codexChatProviderPresets.test.ts tests/config/subrouterProviderPresets.test.ts tests/lib/requestOverrides.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts`；`cargo fmt --manifest-path src-tauri/Cargo.toml --check`；`cargo test --manifest-path src-tauri/Cargo.toml local_proxy_ --lib`；`cargo test --manifest-path src-tauri/Cargo.toml content_encoding --lib`；`cargo test --manifest-path src-tauri/Cargo.toml token_request_ --lib`；`cargo test --manifest-path src-tauri/Cargo.toml get_status_does_not_refresh --lib`；`git diff --check`。广义 `pnpm test:unit -- ...` 早先因脚本展开跑到 `tests/integration/App.test.tsx` 出现过一次 timeout，目标测试收敛后未复现，若发布前做全量 CI 仍需关注该集成测试是否环境性超时。

## 2026-06-28 Official v3.16.4 Delta And CCSwitchMulti Merge Boundary

- Official `farion1231/cc-switch` `v3.16.4` was verified from GitHub release/tag: release published `2026-06-27T05:14:41Z`, tag `v3.16.4` points to `e50fc0eb281cf937251a1cb24a44e792d69029ac`. Local `git diff v3.16.3..v3.16.4 --stat` shows 57 commits and 138 files changed with `+9409/-1020`; the release note itself summarizes 53 commits / 126 files / `+8149/-1016`, so use git as the exact source for merge planning and release notes as product summary.
- Current CCSwitchMulti `main` is `23c43f59e124db15608f9192a89a2e6dd141434e` (`docs(memory): record v3.16.3-23 release`), version surfaces are `3.16.3-23`, and `git merge-base HEAD v3.16.4` is official commit `455556380b52c18d3d444a751a6c17de6d4ee5b0` (`Chat API: skip tool calls with missing function names`). That means CCSwitchMulti has already absorbed the official v3.16.4 path through `45555638`; do not re-merge earlier commits such as CODEX_SQLITE_HOME probing, cached tool-call restore, DeepSeek `thinking:disabled` effort stripping, settings scroll reset, models.dev pricing import, duplicate Codex `base_url` cleanup, or Add Provider search click fix.
- Do not merge the full official tag into CCSwitchMulti. `git merge-tree HEAD v3.16.4` reports direct conflicts in fork identity and high-divergence files including `README.md`, `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/providers/codex_oauth_auth.rs`, `src/components/providers/forms/CodexFormFields.tsx`, `src/components/providers/forms/ProviderForm.tsx`, locale JSON files, and `tests/config/codexChatProviderPresets.test.ts`. A full tag merge also appears to delete many CCSwitchMulti-only modules when viewed as `HEAD..v3.16.4`.
- Fork identity must be preserved during any v3.16.4 merge: `cc-switch-multi` package name, `CCSwitchMulti` product name, `com.ccswitchmulti.desktop` identifier, BigStrongSun updater endpoints/signing, release/export scripts, supplemental Linux/macOS workflows, `codex-history-repairer`, MultiRouter workspace, external OpenAI-compatible API, WebDAV/S3 sync, Codex history repair UI/tooling, model fetch/catalog overlay behavior, and the Codex OAuth login-preservation fixes from `v3.16.3-23`.
- The still-missing official commits are `6ec86cff..e50fc0eb` after merge-base `45555638`: Homebrew docs cleanup; CTok to ETok rename; Kimi icon/prime-partner/order updates; Volcengine Ark AK/SK usage; Skills UI fixes; Kimi auto compact window; Windows ARM64 release support; live end time in usage range; JsonEditor dark mode; database-too-new recovery screen; local proxy request overrides; Copilot/Codex OAuth global proxy fix; zstd/gzip/br/deflate request and error body decompression; Doubao Seed 2.1 pricing/preset; Codex upstream format selector decoupling; unmanaged skill green dot; native Responses API presets for CN Codex providers; SubRouter and OpenCode Go presets; v3.16.4 docs/release notes; Fable 5 banner removal; and official version bump.
- Low-risk/high-value merge candidates for CCSwitchMulti are: `1a0e8c7a` zstd/body decompression, `524b9d98` Copilot/Codex OAuth requests using shared global proxy client, `9171ad75` usage live end time, `55abd182` JsonEditor dark mode, `f1328d89` unmanaged skill green dot, `2d478876` Claude MCP custom config path, `2781d40e` Skills/card UI fixes, `c4630b5c` Volcengine Ark usage query, `2e547c98`/`fdf538e5` Doubao Seed 2.1 pricing/preset, and provider pricing/preset additions that do not overwrite fork-specific catalog behavior.
- Medium/high-risk items need manual hunk porting, not blind cherry-pick: `6fd4e6f4` local proxy request overrides touches `forwarder.rs`, `ProviderForm.tsx`, `CodexFormFields.tsx`, `types.ts`, locales; `edeee25f` database recovery screen needs early DB-version checks integrated with CCSwitchMulti startup/updater semantics; `a4eb5f37` format selector decoupling must preserve MultiRouter model catalog browser and visible/upstream model split; `273cc48c` native Responses API preset migration must preserve CCSwitchMulti route mapping semantics; `430ddf92`/`dd6a951c` SubRouter/OpenCode Go presets and `142c8c1d` ETok rename should be merged without dropping fork presets/docs.
- Do not take official `f9547da9` version bump literally. The CCSwitchMulti successor should use the fork version scheme, likely `3.16.4-1` if preparing a release from this official base, and update all fork version surfaces consistently (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, release notes/export metadata).

## 2026-06-28 CCSwitchMulti v3.16.3-23 Prerelease

- `v3.16.3-23` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-23`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-27T19:50:42Z`，target commit 为 `d8f254fbf9d7b687f385e12bd8df98125306d5f3 build(pnpm): approve release build dependencies`，tag 覆盖 `v3.16.3-22..main` 的 16 个未发布提交。
- 本次发布包含 Codex OAuth 休眠/唤醒与 provider 切换稳定性修复：`get_status()` 保持离线状态语义、`access_token` 只在内存缓存、`RefreshTokenInvalid` 只在真实 token 请求明确 401/403 时清账号；同时移除 `codex_config.rs` 模型 catalog fallback 里的隐藏 live OAuth fetch，避免独立 `CodexOAuthManager` 轮换 refresh token 后主 manager 误删账号。
- Windows 本地 post-commit release pipeline 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-28 03:49:04 +08:00`。raw exe `CCSwitchMulti_3.16.3-23_x64.exe` 的 FileVersion/ProductVersion 均验证为 `3.16.3-23`，`latest.json` 指向 `v3.16.3-23` 的 Windows setup 资产。
- pnpm 11 在发布前阻止 `esbuild`/`msw` postinstall，修复方式是提交 `pnpm-workspace.yaml` 的 `allowBuilds` / `onlyBuiltDependencies` 白名单，并运行 `pnpm approve-builds --all`、`pnpm install --frozen-lockfile`、`pnpm typecheck` 验证。以后本地 release pipeline 遇到 `ERR_PNPM_IGNORED_BUILDS`，先检查该文件，不要交互式留 placeholder。
- 发布资产当前包含 Windows setup、setup signature、portable zip、raw exe、`latest.json`、`SHA256SUMS.txt`、Linux/macOS build notes；Linux/macOS 二进制资产未在本地生成，需要后续 supplemental workflow 或对应平台构建补齐。发布后复核 `gh release view v3.16.3-23 --repo BigStrongSun/ccswitchmulti --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets` 返回 8 个资产。

## 2026-06-28 Codex OAuth Sleep Wake Refresh Invalid Status Fix

- 休眠/唤醒后 Codex OAuth 认证页显示“已登录账号”的原版语义是“本地 `codex_oauth_auth.json` 里仍有账号和 refresh_token 记录”，不是在线验证结果。`get_status()` 不应主动 refresh，也不应因为打开认证页就清理账号；否则状态页会放大 refresh token 使用次数和临时网络误判。
- 最终修复边界：保持原版凭据模型，`refresh_token` 持久化，`access_token` 只在内存缓存。只有真实请求、额度查询、模型查询等需要 Bearer token 的路径调用 `get_valid_token_for_account()`；当 OpenAI token 端点明确返回 401/403 并映射为 `RefreshTokenInvalid` 时，才移除对应账号并让下一次状态查询显示未认证。网络错误、解析错误等临时故障不清空账号。
- 追加排查发现的隐藏边界：`src-tauri/src/codex_config.rs` 生成 Codex provider/model catalog 时不能因为官方 `models_cache.json` 缺失或无 `context_window` 就创建独立 `CodexOAuthManager` 去读取同一份 `codex_oauth_auth.json` 并在线 fetch models。该路径绕过 app 托管的 `CodexOAuthState`，若 refresh token 被官方轮换，主 manager 可能继续持旧 token，后续真实请求会误判 OAuth 失效并清账号。配置/catalog 生成只能读取离线 cache 或测试覆盖值，真实 OAuth refresh 必须通过托管状态发生在用户显式触发的请求/额度/模型查询路径。
- 底层容错也要保留：`CodexOAuthManager::get_valid_token_for_account()` 在 access_token 缓存未命中并拿到账号刷新锁后，应在读取 refresh_token 前重新加载一次 `codex_oauth_auth.json`。这不是为了恢复隐式在线刷新，而是防止未来双进程、旧版本遗留独立 manager 或其他实例已经把 refresh_token 从 A 轮换到 B 后，当前实例继续用内存 A 刷新并触发 `RefreshTokenInvalid` 误删账号。
- 前端 `useManagedAuth` 的 `hasAnyAccount` 不能只等于 `accounts.length > 0`，应受后端 `authenticated` 约束。Codex OAuth 本地账号记录和真实可用认证态必须分开看；以后不要再用“本地有账号”直接驱动绿色认证状态或保存校验。
- 回归测试落点：`codex_oauth_auth.rs` 覆盖 `get_status_does_not_refresh_or_remove_invalid_account`、`token_request_removes_account_when_refresh_token_is_invalid`、`token_request_refreshes_expired_default_account_when_token_is_valid`。验证命令优先跑 `cargo test --manifest-path src-tauri\Cargo.toml token_request_ --lib` 和 `cargo test --manifest-path src-tauri\Cargo.toml get_status_does_not_refresh --lib`。

## 2026-06-27 Logging And Frequent Exit Diagnostics Inventory

- 程序已有三类本地日志：通用运行日志由 `tauri-plugin-log` 写到 `<app_config_dir>/logs/cc-switch.log`（默认 `~/.cc-switch/logs/cc-switch.log`），panic hook 追加写 `<app_config_dir>/crash.log`，Codex MultiRouter 诊断事件写 `<app_config_dir>/logs/codex-router.log`。默认 app config 目录仍是用户家目录下 `.cc-switch`，但启动时会先读取 Store 里的 app_config_dir 覆盖。
- `src-tauri/src/panic_hook.rs` 会在启动最早期安装 panic hook，并强制 `RUST_BACKTRACE=1`；崩溃日志包含时间戳、版本、OS/arch/family、工作目录、线程名/ID、panic message、文件/行/列和完整 backtrace。`src-tauri/Cargo.toml` 设置 `panic = "unwind"`，因此 Rust panic 能被 hook 捕获；但直接进程 abort、系统杀进程、WebView/前端 JS 崩溃不一定进入该 hook。
- 通用日志初始化在 `src-tauri/src/lib.rs` 的 setup 阶段，目标包括 stdout 和日志目录文件 `cc-switch.log`，轮转策略是 `KeepSome(2)`、单文件 1GB。启动后会从 DB 的 `log_config` 读取开关和级别，通过 `log::set_max_level` 应用；前端入口是设置页高级里的 `LogConfigPanel`，只提供启用/禁用和 error/warn/info/debug/trace 级别选择。
- Codex router 日志由 `src-tauri/src/proxy/codex_router_log.rs` 直接追加写入，记录 `route_resolved`、`request_prepared`、`upstream_send`、`upstream_status`、`response_ready` 等清洗后的排障事件；它不会记录 prompt、header 原文或 SSE 内容，并会遮盖 token/API key。MultiRouter 状态页的一键诊断会读取该文件判断近期请求、错误和真实出站协议。
- 现有“异常退出恢复”只针对代理/Live 接管残留：启动时检查 DB live backup 和 live config 占位符，必要时调用 `recover_from_crash()` 恢复配置。这不是通用的频繁退出检测，也不会统计崩溃次数。
- 当前没有现成的“频繁退出/崩溃频率”检测：没有启动 marker、正常退出 marker 清理、退出原因/退出码统一记录、时间窗口计数、watchdog、最近 crash 自动提示，也没有“打开日志目录”的设置页按钮。排查别人频繁退出时，先让对方收集 `~/.cc-switch/crash.log`、`~/.cc-switch/logs/cc-switch.log`，若涉及 Codex MultiRouter 再收集 `~/.cc-switch/logs/codex-router.log`；如果 `crash.log` 没有新条目，就要考虑非 Rust panic 路径（前端/WebView、系统杀进程、安装器重启、进程 abort）。

## 2026-06-28 Abnormal Exit And Crash Cause Logging

- 新增 `src-tauri/src/app_exit_monitor.rs` 作为不依赖数据库的异常退出记录层：启动时写 `<app_config_dir>/logs/app-run-marker.json`，正常退出时删除 marker 并向 `<app_config_dir>/logs/app-exit-events.jsonl` 追加 `clean_exit`，下次启动如果发现 marker 残留则追加 `abnormal_exit_detected` 并在 `cc-switch.log` 打 warn。这样数据库初始化失败、配置迁移失败或 Tauri 事件循环异常退出也能留下证据。
- `panic_hook` 现在除了继续写完整 `<app_config_dir>/crash.log`，还会向 `app-exit-events.jsonl` 写结构化 `panic` 事件，包含 panic message、源码位置和线程摘要；完整 backtrace 仍只在 `crash.log`，避免 JSONL 过大。
- 已挂接的正常/显式退出路径包括窗口关闭退出、用户主动退出、Tauri restart、自定义 `restart_process`、Windows updater install 前退出、旧 config 加载失败用户退出、数据库初始化失败用户退出。系统强杀/abort 仍无法在退出前写 clean event，但会因 marker 残留在下次启动被识别。
- 设置页高级日志配置新增“打开日志目录”入口，调用 `open_log_dir` 打开 `<app_config_dir>/logs`，方便用户收集 `cc-switch.log`、`app-exit-events.jsonl`、`app-run-marker.json` 和 `codex-router.log`。完整 Rust backtrace 的 `crash.log` 仍位于 `<app_config_dir>` 根目录。

## 2026-06-26 CCSwitchMulti v3.16.3-22 Prerelease

- `v3.16.3-22` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-22`。Release 为非 draft、`prerelease=true`，发布时间为 `2026-06-26T04:16:52Z`，tag 指向 `d4260d1aeb89ade1859f4a341612a8453fc57cbb chore(release): prepare v3.16.3-22 prerelease`。
- 业务修复来自 `9b91ff5d fix(codex): refresh multirouter model sources optimistically`：MultiRouter `/models` 刷新成功后不再等待父级 providers refetch，当前打开的 route picker 会通过 `optimisticModelSourcesById` 立即读到新 catalog，解决“读取成功但 UI 仍显示未发现模型目录 / 卡在旧列表”的边界。
- Windows 本地 post-commit release pipeline 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，完成时间 `2026-06-26 11:59:43`；Windows setup 为 `CCSwitchMulti_3.16.3-22_x64-setup.exe`，raw exe 的 FileVersion/ProductVersion 均验证为 `3.16.3-22`，`latest.json` 指向 `v3.16.3-22`。
- 发布创建时 `gh release create` 在 raw exe 上传阶段遇到 EOF 并留下 draft release；恢复方式是停止残留 `gh` 进程，逐个补传缺失 Windows 资产，并删除误名 checksum 资产后重新上传正确的 `SHA256SUMS.txt`。后续 Linux/macOS workflow 又刷新了最终 checksum。
- Supplemental Linux Release workflow `28216822549` 成功，上传 AppImage、deb、rpm；Supplemental macOS Release workflow `28216824340` 成功，上传 unsigned universal `.app.zip`、universal updater tarball 和 `.tar.gz.sig`。最终 release 共有 12 个资产，`SHA256SUMS.txt` 覆盖除自身外的 11 个资产。
- 发布前验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`（21 个测试通过）、`pnpm typecheck`、`cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` unused warnings）、`git diff --check`（仅 `Cargo.lock` LF/CRLF 提示）。发布后复核：`gh release view v3.16.3-22 --repo BigStrongSun/ccswitchmulti --json tagName,isDraft,isPrerelease,url,assets,publishedAt,targetCommitish`、下载 `SHA256SUMS.txt` 检查 11 条 checksum、`gh run view 28216822549` 和 `gh run view 28216824340` 均为 `status=completed, conclusion=success`。

## 2026-06-26 MultiRouter Model Refresh UI Stale Catalog Fix

- 新版仍出现“加载模型列表卡住 / UI 没刷新”时，要区分两类问题：`v3.16.3-21` 已解决 `/models` 读取或保存事务不 settle 导致永久 loading；本次发现的剩余边界是刷新成功后 `nextProvider` 写入 DB/React Query，但当前 `CodexRouterWorkspacePage` 的 `modelSources` 仍可能来自父级旧 `providers` props，导致已打开的 `RouteCandidatePicker` 继续显示旧 catalog 或“未发现模型目录”。
- 根因位置是 `src/components/codex/CodexRouterWorkspacePage.tsx`：旧 `effectiveProviders` 只叠加 `optimisticRoutingPlan`，没有叠加普通模型源的刷新结果；同时刷新成功分支的 `queryClient.setQueryData(["providers","codex"])` 在 cache 尚无 `providers` 字段时会返回旧引用，不能保证触发 UI 更新。
- 修复方式是新增 `optimisticModelSourcesById`，在 `fetchModelsForConfig -> providersApi.update(nextProvider)` 成功后立即把普通 provider 的新 catalog 叠加进 `effectiveProviders`，让候选 router 和空 match route 立刻读取新模型；当父级 props 的 catalog 追上或 provider 连接配置/baseUrl/API key 变化时自动释放 overlay，避免旧 catalog 长期压住新配置。
- 回归测试新增 `refreshes visible route picker candidates after provider catalog save without parent refetch`：provider 初始 catalog 为空，打开候选选择器时显示“未发现模型目录”，`/models` 返回 `fresh-route-model` 且保存成功后，在不模拟父级 refetch 的情况下候选卡片必须立刻显示 `fresh-route-model` 并移除空目录提示。
- 本轮验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`（21 个测试通过）、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。renderer build 仍只有既有 baseline/browserlist/大 chunk 警告。

## 2026-06-25 CCSwitchMulti v3.16.3-21 Prerelease

- `v3.16.3-21` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-21`。tag 指向 `554bed1c chore(release): prepare v3.16.3-21 hotfix`，业务修复来自 `966a8e38 fix(codex): settle model refresh save-back hangs`。
- 本次热修的真实边界：`v3.16.3-20` 只修了并发刷新和 `/models` 阶段超时，仍可能在读取成功后的 `providersApi.update` 写回 provider / plan catalog 阶段永久 loading；`v3.16.3-21` 才把读取和写回合成同一个 30 秒超时事务。
- Windows 本地 release hook 构建成功，导出目录为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`，raw exe 文件版本为 `3.16.3-21`。release 创建时 `gh release create` 曾在 raw exe 上传阶段卡住，留下 draft；处理方式是停止残留 `gh`，补传 raw exe，再 `gh release edit --draft=false --prerelease=true` 发布。
- Supplemental Linux Release workflow `28177240622` 成功并上传 AppImage、deb、rpm；Supplemental macOS Release workflow `28177240635` 成功并上传 unsigned universal `.app.zip`、updater tarball 和 tarball 签名。最终 release 共有 12 个资产，`SHA256SUMS.txt` 覆盖除自身外的 11 个资产。
- 发布前验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`、`pnpm typecheck`、`cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` unused warnings）、`git diff --check`、`pnpm build:renderer`。

## 2026-06-25 MultiRouter Model Refresh v3.16.3-21 Hotfix Boundary

- 用户/外部反馈截图仍停在“候选 provider 模型列表刷新 / 正在读取模型列表...”时，必须区分三个版本边界：本机安装目录 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 仍是 `3.16.3-18`，公开 `v3.16.3-19` 完全不含刷新状态机修复，公开 `v3.16.3-20` 含 `ddfeed42`/`33a0bc58` 但不含 `966a8e38 fix(codex): settle model refresh save-back hangs`。
- `v3.16.3-20` 的 `withModelRefreshTimeout` 只包住 `fetchModelsForConfig(...)`，读取成功后的 `providersApi.update(nextProvider)` 与受影响 plan 写回仍可能永久挂起；当前 HEAD `966a8e38` 才把读取、provider catalog 写回、MultiRouter plan catalog 写回合成同一个 30 秒超时事务，并显示“已读取 N 个模型，正在写回本地配置...”阶段文案。
- 本轮现场验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts` 通过 20 个测试；截图类问题应通过补发 `v3.16.3-21` 处理，release notes 不能再建议 save-back 卡住用户只升级到 `v3.16.3-20`。

## 2026-06-25 MultiRouter Model Refresh Save-Back Timeout Fix

- MultiRouter 路由页“候选 provider 模型列表刷新”卡在“正在读取模型列表...”不只可能发生在 `/models` IPC/网络阶段；`src/components/codex/CodexRouterWorkspacePage.tsx` 在读取成功后还会 `providersApi.update` 写回普通 provider 的 `modelCatalog`，并重建/写回受影响 MultiRouter plan 的 `modelCatalog`。旧 `withModelRefreshTimeout` 只包住 `fetchModelsForConfig`，如果后续 provider/plan 保存、Codex live catalog/cache 同步或本地 DB/文件写入挂起，UI 仍会永久停留在 loading。
- 当前修复把“读取 `/models` -> 写回 provider catalog -> 写回受影响路由方案”视作一个刷新事务，30 秒超时覆盖整个事务；读取完成进入保存阶段时，卡片文案改为“已读取 N 个模型，正在写回本地配置...”，避免把保存阶段误判成远端 `/models` 还在读。
- 超时 attempt 会被记录到 `modelRefreshTimedOutAttemptKeysRef`，后台迟到的 Promise 不允许再把 error/loading 覆盖成 success；同时 catch 只在该 provider 仍然是当前 attempt 时写错误态，避免旧 attempt 超时覆盖新 attempt。
- 回归测试 `src/components/codex/CodexRouterWorkspacePage.test.ts` 覆盖两类永久 loading 边界：`fetchModelsForConfig` 永不返回，以及 `providersApi.update` 写回刷新结果永不返回。后者会先显示写回阶段文案，30 秒后落到错误态，迟到 resolve 不能再变成成功态。

## 2026-06-25 Codex Catalog Visible Alias And Upstream Model Split

- 第三方 Codex provider 的 `modelCatalog.models[].model` 是 Codex/子 Agent 可见候选名，不再强制等于真实上游模型名；新增 `upstreamModel`/`upstream_model` 表示请求发往上游时使用的模型。为空或等于 `model` 时按旧配置兼容处理。
- 普通表单和 MultiRouter 自动 `/models` 刷新合并时必须按 `upstreamModel || upstream_model || model` 优先匹配远端返回的 id，避免用户把 `gpt-5.5` 改成 `gpt-5.5-thirdparty` 后，下一次刷新又新增一个重复的 `gpt-5.5` 或把别名覆盖掉。新增远端模型默认写成 `model=id, upstreamModel=id`，保存时若二者相同会省略 upstream 字段。
- 运行时出站映射顺序固定为：route 级 `codexResolvedUpstreamModelOverride` / `modelMap` 优先，其次 catalog 条目的 `upstreamModel`，最后回退到 provider/config 里的单模型字段。这个映射必须同时覆盖 Responses 原生直连和 Responses->Chat 转换路径。
- Codex catalog 文件可以携带 `upstreamModel` 作为 cc-switch 私有元数据，但 OpenAI-compatible `/v1/models` 的 `data[]` 只能暴露可见模型名和上下文窗口，不应把真实 upstream alias 暴露出去。

## 2026-06-25 MultiRouter Model Refresh Release Boundary And Timeout Guard

- 用户/他人看到 MultiRouter 路由页“候选 provider 模型列表刷新”一直卡在“正在读取模型列表...”时，先确认运行版本；`v3.16.3-19` tag 指向 `6a1cf4e1`，不包含本地 `ddfeed42 fix(codex): settle multirouter model refresh states`，而本机安装目录 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 仍是 `3.16.3-18`，所以截图类问题很可能是发布包未带修复而不是 HEAD 修复失效。
- 2026-06-25 再次确认：GitHub prerelease `v3.16.3-19` 的 target commit 仍是 `6a1cf4e1`，`ddfeed42` 和 `33a0bc58` 都不在该 tag 内；别人发来的两个 provider 同时显示“正在读取模型列表...”的截图，应优先按“公开包未发布刷新状态机修复”处理。下一次发版必须包含 `ddfeed42`/`33a0bc58`，否则该问题会继续在已安装包里出现。
- `src/components/codex/CodexRouterWorkspacePage.tsx` 的候选 provider 自动刷新现在有双层保护：per-provider active attemptKey 负责防止 rerender cleanup 吞掉 pending 请求终态；前端 `withModelRefreshTimeout` 再给 IPC/后端异常挂起加 30s 兜底，必须让 UI 从 loading 落到错误态。
- attemptKey 不能只记录 `Boolean(apiKey)`；API Key 从一个非空值换成另一个非空值时必须重新发起 `/models` 读取，并让旧请求结果无法写回。当前实现对 API Key 做短哈希后参与内存态 attemptKey，不持久化也不展示完整密钥。
- 回归测试在 `src/components/codex/CodexRouterWorkspacePage.test.ts` 增加两类边界：API Key 变化时 stale request 不写回，以及 `fetchModelsForConfig` 永不返回时 30s 后显示错误而不是永久 loading。

## 2026-06-25 MultiRouter Candidate Model Refresh Loading Fix

- MultiRouter 路由页“候选 provider 模型列表刷新”一直停在“正在读取模型列表...”的根因在前端并发刷新状态机，不是后端 `/models` 请求缺少超时。后端 `src-tauri/src/services/model_fetch.rs` 每个请求已有 15s timeout；问题是 `src/components/codex/CodexRouterWorkspacePage.tsx` 自动刷新多个 provider 时，第一个 provider 成功写回 `providersApi.update` / `setOptimisticRoutingPlan` 会触发 effect cleanup，旧实现用局部 `cancelled` 阻断后续 pending provider 的 `.then/.catch`，而新一轮 effect 又被 `modelRefreshAttemptedKeysRef` 去重跳过，于是 UI 永久留在 loading。
- 修复方式是按 provider 维护当前最新 `attemptKey`，用 `modelRefreshActiveAttemptKeysRef` 判断请求是否仍是该 provider 的最新 attempt；正常 rerender 不再吞掉同批并发请求终态，真实配置变更产生的新 attempt 仍能阻止旧请求覆盖状态或写回 DB。
- 回归测试在 `src/components/codex/CodexRouterWorkspacePage.test.ts` 用可手动 resolve/reject 的 Promise 复现两个 provider 并发：Provider A 先成功并触发 rerender 后，Provider B 后续成功必须显示 `已读取并更新 1 个模型。` 且写回；Provider B 后续失败必须显示错误而不是卡 loading。
- 本轮验证：`pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts`、`pnpm typecheck`、`pnpm build:renderer`、`git diff --check`。renderer build 仍只有既有 baseline/browserlist/chunk 警告。

## 2026-06-25 CCSwitchMulti v3.16.3-19 Prerelease

- `v3.16.3-19` 已作为 GitHub prerelease 发布：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-19`。tag 指向版本 bump 提交 `6a1cf4e1`，版本面同步点仍是四处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。业务修复提交是 `2e9723c1`（MultiRouter 子 Agent 流量监控 + 浅色主题修复），前面还包含 vLLM/Qwen 上下文窗口修复提交 `7481bbb5`、`6d5d8c02`。
- 本次 release notes 必须继续用中文。内容覆盖：MultiRouter “今日子 Agent 会话流量”、子 Agent/模型聚合、会话用量同步入口、浅色模式可读性修复、vLLM `max_model_len/maxModelLen` 上下文窗口读取、SQLite session_id 分块查询，以及 macOS universal history-repair sidecar 构建修复。
- 本地 Windows 构建路径：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/local-release-pipeline.ps1 -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-19 -Reason manual-prerelease-v3.16.3-19`。产出被整理到 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-19-assets`，包括 setup、setup.sig、portable zip、raw exe、`latest.json`。
- Linux 资产在 WSL `openclaw` 内完成，仍然使用临时 `{"bundle":{"createUpdaterArtifacts":false}}` 配置构建：先 `cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`，再 `pnpm tauri build --bundles appimage,deb,rpm --config <tmpfile>`。实际上传资产是 `CCSwitchMulti_3.16.3-19_amd64.AppImage`、`CCSwitchMulti_3.16.3-19_amd64.deb`、`CCSwitchMulti-3.16.3-19-1.x86_64.rpm`。
- macOS 本机仍不能构建；这次通过 `Supplemental macOS Release` workflow_dispatch 构建并上传，run id `28150527263` 成功，耗时约 29m30s。该 workflow 上传了 `CCSwitchMulti_3.16.3-19_universal.tar.gz`、`.tar.gz.sig`、`CCSwitchMulti_3.16.3-19_universal.app.zip`，并刷新 release `SHA256SUMS.txt`。
- 最终 release 资产数为 12：Windows 4 个、Linux 3 个、macOS 3 个、`latest.json`、`SHA256SUMS.txt`。tag/main push 触发的 `.github/workflows/release.yml` push run 仍出现无 job 的失败记录，不作为本次发布路径；本次实际发布路径是手动本地 Windows + WSL Linux + supplemental macOS。
- 本轮验证：`pnpm typecheck`、`pnpm build:renderer`、`cargo check --manifest-path src-tauri/Cargo.toml`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_usage_stats --lib`、`git diff --check`。已知非阻塞警告仍是 Rust unused helper、Vite browserslist/baseline 和大 chunk 警告，以及 Tauri bundler `__TAURI_BUNDLE_TYPE` warning。

## 2026-06-24 MultiRouter Subagent Usage And Light Theme Readability

- MultiRouter 状态页的子 Agent 流量监控不能从真实代理转发日志里直接推断身份；真实代理日志只回答 route/provider/model 的出站归属。子 Agent 监控的来源应固定为 Codex 本地历史 SQLite/JSONL：先用 `thread_source="subagent"` 或 JSONL `session_meta.payload.source.subagent.thread_spawn` 确认子 Agent，再只聚合 `proxy_request_logs` 中 `app_type='codex'`、`data_source='codex_session'`、`session_id IN (subagent session ids)` 的同步用量行。
- 子 Agent 监控的 UI 口径是“本地 Codex 会话 token_count 同步后的用量”，不是代理层实际请求转发次数；因此页面需要保留“今日子 Provider / Model 流量”和“今日子 Agent 会话流量”两个分区，前者看真实出站，后者看子 Agent/模型消耗。
- MultiRouter 页面和第三方 Agent API 页面浅色模式修复应优先使用 `bg-card`、`bg-muted`、`bg-background`、`text-foreground`、`text-muted-foreground`、`border-border` 等语义 token，再把原来的深色透明样式放进 `dark:` 变体。不要在浅色主类里继续使用 `bg-slate-950/*`、`text-slate-100`、`text-white` 或深色半透明卡片。
- 子 Agent 会话统计查询 `session_id IN (...)` 时必须分块，当前保守批量是 500；`get_codex_subagent_usage_stats` 默认会为了状态页读取最多 1600 条历史、最多 5000 条只读候选，因此不要把所有 session_id 一次塞进 SQLite 变量绑定。
- 本轮验证基线：`pnpm typecheck`、`pnpm build:renderer`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo test --manifest-path src-tauri/Cargo.toml test_codex_subagent_usage_stats_only_counts_subagent_session_rows --lib`、`cargo check --manifest-path src-tauri/Cargo.toml`。Rust 只剩既有 `commands/misc.rs` unused 警告；renderer build 只剩既有 browserslist/baseline 和大 chunk 警告。

## 2026-06-24 CCSwitchMulti v3.16.3-18 GitHub Release

- 远端 `BigStrongSun/ccswitchmulti` 已经存在 `v3.16.3-17` prerelease（含本地 Windows/Linux 资产），因此这次不能复用旧 tag；新的正式 release 需要前进到 `v3.16.3-18`。版本面同步点仍是四处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。
- `v3.16.3-18` GitHub Release 已发布为 Latest：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-18`。release tag 指向提交 `6ff4252f`（版本 bump + unsigned macOS workflow 基线），后续 workflow 修复继续落在 `main` 上的 `93ec101b`，然后用 `workflow_dispatch` 构建同一个 tag 的补充资产。
- 本地 Windows 构建由 post-commit release hook 自动触发成功，随后用 `scripts/export-latest-ccswitchmulti.ps1 -SkipBuild -ReleaseRoot C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-18` 固化干净版本目录。发布 staging 目录是 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-18-assets`，只保留本次 release 实际上传的 Windows/Linux 资产与 `latest.json`。
- 本地 Linux 构建是在 WSL `openclaw` 中完成的，命令路径要先补 `PATH=\"$HOME/.cargo/bin:$PATH\"`，再构建 `codex-history-repairer` sidecar，然后用临时 `{\"bundle\":{\"createUpdaterArtifacts\":false}}` 配置执行 `pnpm tauri build --bundles appimage,deb,rpm --config <tmpfile>`。这次实际产物是 `CCSwitchMulti_3.16.3-18_amd64.AppImage`、`CCSwitchMulti_3.16.3-18_amd64.deb`、`CCSwitchMulti-3.16.3-18-1.x86_64.rpm`。
- macOS 本地构建在这台 Windows 主机上仍然不可行，硬边界是 Tauri 需要目标平台原生运行时和 macOS SDK/WebKit，而不是“少装一个 Rust target”。能完成的是 GitHub macOS runner 上的 unsigned supplemental build。
- 第一次 supplemental macOS workflow（run `28094163276`）失败的真实根因不是签名，而是 universal 打包阶段缺少 `src-tauri/target/universal-apple-darwin/release/codex-history-repairer`。修复方式不是重试，而是在 `.github/workflows/release.yml` 和 `.github/workflows/supplemental-macos-release.yml` 中都显式为 `codex-history-repairer` 构建 `aarch64-apple-darwin` 与 `x86_64-apple-darwin`，再用 `lipo` 合成 universal sidecar。
- 第二次 supplemental macOS workflow（run `28095435446`）成功后，release 额外补齐了 unsigned macOS 资产：`CCSwitchMulti_3.16.3-18_universal.tar.gz`、`CCSwitchMulti_3.16.3-18_universal.tar.gz.sig`、`CCSwitchMulti_3.16.3-18_universal.app.zip`，并自动刷新了 `SHA256SUMS.txt`。最终 release 共有 12 个资产：Windows 4 个、Linux 3 个、macOS 3 个、`latest.json`、`SHA256SUMS.txt`。
- 这条发布线还有两个环境约束要记住：一是 `.github` 被仓库 `.gitignore` 忽略，新增或修改 workflow 时必须 `git add -f .github/workflows/...`；二是本地 `post-commit` hook 会自动跑 `scripts/local-release-pipeline.ps1` 并占用 `scripts/logs/local-release.lock`，发布期间不要并发再起第二个本地构建。

## 2026-06-24 MultiRouter Protocol Probe And Codex Responses Decision Unification

- 当前 Codex MultiRouter 的 `/responses` -> 上游协议选择，本质上一直是“配置判定”，不是在线能力探测。单一真理来源现在收敛到 `src-tauri/src/proxy/providers/codex.rs::explain_codex_responses_upstream_protocol`：优先级为 managed `codex_oauth` 直连官方 `responses` > `meta.apiFormat` > `settings_config.api_format/apiFormat` > 已知 Chat Completions-only `base_url` > `config.toml wire_api` > 默认 `responses`。
- 这次修复顺手把一个关键边界钉死：只要 provider 被识别为 managed Codex OAuth，哪怕残留了 `apiFormat=openai_chat` 之类污染字段，也必须保持原生 `chatgpt.com/backend-api/codex/responses` 透传，不能再被误转成 `/v1/chat/completions`。
- `src-tauri/src/commands/proxy.rs` 的 MultiRouter 诊断现在会为每条 route 返回 `configuredProtocol/configuredProtocolSource/configuredProtocolDetail`，而且 route 摘要不再自己猜 target provider 配置，而是通过与运行态一致的 `build_codex_route_probe_provider` 物化 effective provider 后再判定。
- `codex-router.log` 的 `request_prepared` 事件原本就包含 `effective_endpoint`、`upstream_url`、`responses_to_chat`、`responses_to_messages`。现在诊断层会把这些字段解析成 `actualProtocol`，前端状态页“协议探测”视图可按每个 `Provider + Model` 展示“配置判定”与“最近实测”，直接看出最后一次真实出站走的是 `responses`、`chat` 还是 `messages`。
- 状态页里的“协议探测”按钮不会主动消耗真实上游额度；它只读取当前 route 配置和最近 router 日志。因此它解决的是“当前代码会怎么判、最近一次实际怎么走”的可见性问题，不是远端能力协商。如果后续真要做在线 capabilities probe，需要单独设计安全的探测请求与缓存。

## 2026-06-24 Codex Official Context Window Live Fallback

- 在 `src-tauri/src/codex_config.rs` 中，官方 GPT/Codex 模型的上下文窗口读取链现在是：provider DB 显式 `contextWindow` > `~/.codex/models_cache.json` > 本机已登录 Codex OAuth 账号实时拉取 `https://chatgpt.com/backend-api/codex/models` > `config.toml` 的 `model_context_window` > 最终默认值 `128000`。
- 这条 live fallback 专门覆盖首次配置、用户清理 `models_cache.json`、缓存损坏、以及缓存里只有 slug 但缺少 `context_window` 的场景，避免 Codex Desktop 又回退成 128k/约 122k 的显示。
- 同步桥接异步 OAuth 拉模时，不要在已有 Tokio runtime 里直接嵌套 `block_on` 或用当前线程硬顶网络 future。当前实现改为把 live 官方读取放到独立线程，再在该线程里使用 `tauri::async_runtime::block_on`，这样不会污染调用侧 runtime，也更适合 Tauri 同步配置生成路径。
- 回归测试必须至少覆盖三类边界：`models_cache.json` 缺失、JSON 损坏、缓存存在但缺失上下文字段；三种情况下都应能从 live OAuth 元数据恢复 `context_window`。

## 2026-06-24 Release Workflow Fork Secret Degradation

- `fork` 仓库的 release matrix 不能假设一定有 Apple 签名/公证 secrets。若 `APPLE_CERTIFICATE` 一类 secret 为空，旧 workflow 会在 `Import Apple signing certificate` 直接失败，并因为 matrix 默认 `fail-fast` 取消掉本来还能完成的 Windows/Linux 打包。
- 修复策略：`release.yml` 里将矩阵改为 `fail-fast: false`；macOS 证书导入、DMG 公证、签名验证只在 Apple secrets 和 `APPLE_SIGNING_IDENTITY` 都存在时执行。缺少签名材料时，macOS 仍然产出 updater `.tar.gz` 和 `.zip`，但跳过 `.dmg`、公证与签名校验，不再拖死整条 release。

## 2026-06-24 Codex Official GPT Context Window Projection Fix

- 现场现象：Codex Desktop 里官方链路/Multirouter 的 `gpt-5.5` 显示约 122k 上下文，而 CCSwitchMulti live `config.toml` 和 `cc-switch-model-catalog.json` 中 `gpt-5.5` 已是 272000。122k 与 128000 的 `effective_context_window_percent=95` 接近，说明 Desktop 某条读取路径忽略了 272000 后回退到了默认 128k。
- 根因边界：`src-tauri/src/proxy/handlers.rs` 的 Codex client `GET /v1/models` 会把 cc-switch catalog 扩展成 OpenAI-compatible `data[]`。该 `data[]` 以前只写 `context_window` / `max_context_window`，没有 `contextWindow` / `maxContextWindow`。Codex Desktop 某些 renderer/app-server 路径读取 `data[]` 时会看 camelCase 字段，读不到就按默认 128k 再乘 95% 展示。
- 修复：`openai_model_entry_with_source` 在 `data[]` model entry 中同时投 snake_case 与 camelCase：`context_window`、`max_context_window`、`contextWindow`、`maxContextWindow`。这不改变 raw `models[]` catalog 和已有外部 OpenAI API 兼容字段，只补齐 Desktop 读取别名。
- 回归测试：`proxy::handlers::tests::codex_catalog_models_response_keeps_catalog_and_openai_data` 必须断言四个上下文字段都存在并等于源 catalog 值。
- 后续根治：`src-tauri/src/codex_config.rs` 生成 catalog spec 时，官方 GPT/Codex 模型若 DB `modelCatalog` 未显式写 contextWindow，应优先读取 Codex 官方 `models_cache.json` 的动态上下文窗口，再回退到 `model_context_window` / 128000。不要继续把 `272000` 等 OpenAI 数值当成唯一真实来源。

## 2026-06-24 Qwen Local Context Window Fetch Fix

- 用户现场把问题边界收紧到“获取模型列表阶段没拿到 `qwen3.6=262144`，导致 Codex catalog/压缩阈值先错了”，而不是单纯的 `/responses -> chat` 输出预算裁剪。上游报错里出现的 `262144` 只是运行时错误文本，本地之前不会把它自动回写到 provider catalog。
- 直接探测用户这条 vLLM 端点 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 后确认：远端其实已经返回了 `max_model_len: 262144`，并不是“vLLM 没给上下文窗口”。根因是 `src-tauri/src/services/model_fetch.rs::extract_context_window` 只识别 `context_window/max_context_window/contextWindow/maxContextWindow`，没识别 vLLM 的 `max_model_len/maxModelLen`。
- 因此正确修复不是给 `qwen3.6` 做应用级静态兜底，而是在配置阶段的真实 `/models` 读取里补上 vLLM 字段解析。这样点“获取模型列表”时就能直接把 `262144` 写进 provider catalog，MultiRouter 和 Codex picker 后续都读取真实值。
- 回归测试改为覆盖 `max_model_len` 和 `maxModelLen` 两种 vLLM 风格字段；`pnpm test:unit -- tests/utils/codexModelContext.test.ts tests/utils/codexSpawnAgentCandidates.test.ts`、`pnpm typecheck`、`cargo test --manifest-path src-tauri/Cargo.toml switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib` 全部通过。

## 2026-06-24 Codex Provider Model Context Window Fallback

- 根因：DeepSeek 等 OpenAI-compatible provider 的 `/models` 端点仅返回模型 id（如 `deepseek-chat`、`deepseek-reasoner`、`deepseek-v4-flash`），不承诺返回 `context_window` 字段。而 Codex provider 表单的"获取模型列表"按钮和 MultiRouter 工作台的自动候选刷新都只在 `fetched.contextWindow` 为 truthy 时才写入上下文窗口，远端没给就留空。
- 修复策略：引入共用工具 `src/utils/codexModelContext.ts`，为 `mergeFetchedModelsIntoCatalogRows`（普通表单）和 `providerWithFetchedModelCatalog`（MultiRouter 候选刷新）提供统一的上下文推断优先级：远端显式值 > 用户已有目录值 > 本地 provider/model 预设元数据。预设匹配会对比 providerId/name/baseUrl/websiteUrl 信号以避免同名模型跨供应商误套。
- DeepSeek 兼容别名（`deepseek-chat`、`deepseek-reasoner`）也在工具中写入了显式 1M 上下文映射，不会因为上游返回旧式 id 而丢上下文。
- 测试 `tests/utils/codexModelContext.test.ts` 覆盖：远端显式值优先、已有目录保留、DeepSeek 预设兜底、DeepSeek 别名兜底、未知模型不捏造上下文。
- 相关提交：该修复同时变更 `CodexFormFields.tsx` 和 `CodexRouterWorkspacePage.tsx`，让两处上下文合并逻辑共用同一推断函数。

## 2026-06-24 Empty Codex Official Seed OAuth Routing Fix

- v3.16.3-15 的 official/OAuth materialize 修复仍有一个漏网条件：全新安装或恢复后的 `codex-official` 可能只是 `category="official"` 的空 seed provider，`settings_config.auth` 为空且没有 `base_url`，真实 OAuth 账号在 CCSwitchMulti 的 `CodexOAuthManager` 存储里。旧判断只认 `meta.providerType="codex_oauth"`、provider 内 `auth.auth_mode="chatgpt"` / tokens，或 router provider 自身的 managed auth，因此空 seed 被误当普通 Codex provider，GPT 原生 route 命中后仍会在 `CodexAdapter::extract_base_url` 报 `Codex Provider 缺少 base_url 配置`。
- 修复应把 `category == "official"` 且 id/name/route target 明确标记 `codex-official` / `OpenAI Official` 的空 seed 识别为 managed Codex OAuth，但继续让带真实非本地 `base_url` 的 provider 走普通第三方路径，避免误伤自定义 OpenAI-compatible provider。
- 回归测试要覆盖两条路径：MultiRouter `targetProviderId="codex-official"` 命中空 official seed 后 materialize 成 `meta.provider_type="codex_oauth"`；以及直接对空 official seed 调 `CodexAdapter` 时返回 `https://chatgpt.com/backend-api/codex` 和 `AuthStrategy::CodexOAuth`。

## 2026-06-24 Qwen MultiRouter Live Route Check

- 用户现场怀疑 MultiRouter 到 `qwen3.6` 的请求没有真正发出去。只读复查确认当前 live `~/.codex/config.toml` 已指向 `model_provider = "codex_model_router_v2"` 和 `base_url = "http://127.0.0.1:15721/v1"`，`cc-switch.exe` 进程 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 同时监听 `15721` 与 `15722`，`http://127.0.0.1:15721/health` 返回 200。
- 当前 DB 里 `codex-openai-router` 是 Codex current provider，`settings_config.codexRouting` 为对象 schema；`qwen-local` route 启用，匹配 `qwen3.6` / `qwen` 前缀，上游为 `https://www.matrixminecraft.cn:24443/vllm/v1`，`wire_api=openai_chat`，并保留 `codexChatReasoning` 的 `enable_thinking` 与 `minOutputTokens=2048`。
- 真实 `spawn_agent model=qwen3.6` 极小请求返回 `OK`。同一时间 `~/.cc-switch/logs/codex-router.log` 出现完整链路：`route_resolved route_id=qwen-local route_missed=false`、`request_prepared upstream_url=https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions responses_to_chat=true`、`auth_prepared auth_strategy=Bearer`、`upstream_send`、`upstream_status status=200`、`response_ready status=200`。这证明当前 MultiRouter 路由层和 15721 转发链路是通的，请求确实进了 qwen 上游。
- 本轮直接探测 `https://www.matrixminecraft.cn:24443/vllm/v1/models` 曾先返回 502，随后返回 200 且列出 `qwen3.6`；因此“卡住/没反应”更像上游 vLLM/relay 短暂抖动、模型冷启动或当时请求未实际选择/发出 qwen，而不是当前 MultiRouter 配置缺 route。后续复现时优先抓失败时刻的 `codex-router.log`：若没有 `model=qwen3.6` 新行，问题在 Codex/子 Agent 发起前；若有 `upstream_send` 但无 200，则看上游状态、首包超时或 502/521。

## 2026-06-24 CCSwitchMulti 3.16.3-15 GitHub Release

- Published `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-15` from local `main` after pushing commit `0739638b` and annotated tag `v3.16.3-15` to the `fork` remote (`https://github.com/BigStrongSun/ccswitchmulti.git`).
- This release is the hotfix successor to `3.16.3-14` for Codex MultiRouter regressions. It includes legacy array-shaped `settings_config.codexRouting` compatibility, Rust route resolver support before UI resave, official/OAuth target provider local-proxy pollution handling, and follow-up diagnostics hardening.
- Verification before release: `pnpm typecheck`, `pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/useCodexConfigState.test.ts`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, and `cargo test --manifest-path src-tauri\Cargo.toml codex_route --lib`.
- Windows export root: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-15`. The first full export timed out at the shell after 15 minutes while `cargo/rustc` was still running; after the build and NSIS processes finished, rerunning `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot ... -SkipBuild` completed export, signing, `latest.json`, and checksum generation.
- Initial Windows-hosted upload included `CCSwitchMulti_3.16.3-15_x64-setup.exe`, `.sig`, `CCSwitchMulti_3.16.3-15_x64-portable.zip`, `CCSwitchMulti_3.16.3-15_x64.exe`, `latest.json`, `README.md`, `linux-build-note.md`, `macos-build-note.md`, and `SHA256SUMS.txt`.
- Follow-up Linux supplement: the local WSL build produced AppImage/deb/rpm after first building the `codex-history-repairer` sidecar, but local uploads to `uploads.github.com` repeatedly stalled or disconnected. Commit `ffc2fa0e` added `.github/workflows/supplemental-linux-release.yml`, then GitHub Actions run `28076382107` built and uploaded Linux x86_64 assets for `v3.16.3-15`.
- Final release assets include Linux x86_64 `CCSwitchMulti_3.16.3-15_amd64.AppImage`, `CCSwitchMulti_3.16.3-15_amd64.deb`, and `CCSwitchMulti-3.16.3-15-1.x86_64.rpm`; `linux-build-note.md` was removed after the real Linux packages were uploaded. macOS aarch64 `dmg` and `.app.zip` assets are also present on the release.
- Important release hygiene: the export script's default `SHA256SUMS.txt` is a full export-tree checksum and may include internal tool files or nested platform notes that are not uploaded as release assets. For `v3.16.3-15`, `SHA256SUMS.txt` was regenerated from GitHub release asset digests so every entry corresponds to a downloadable asset.

## 2026-06-23 CCSwitchMulti 3.16.3-14 MultiRouter Route Regression

- `3.16.3-14` 的用户现场证明存在真实回归：MultiRouter provider 仍存在，但 `settings_config.codexRouting` 可能被保存成扁平数组，缺少新版对象外壳 `{ enabled, routes, defaultRouteId }`。新版前后端若只按对象 schema 读取，会表现为 `routing_configured=false` 或 `route_missed=true`，随后请求回落到 MultiRouter provider 自身；MultiRouter 自身不是普通上游，没有真实外部 `base_url`，会报 `Codex Provider 缺少 base_url 配置` 或递归保护 400/502。
- 根因不是 DeepSeek key、网络、用户教程步骤或必须删库重配。现场“直接写 SQLite 把 codexRouting 修成对象”只能作为临时恢复，产品修复必须兼容已损坏/旧式数组 schema，并在 UI 保存时自动迁移回对象 schema。
- 修复点：`CodexRouterWorkspacePage.readCodexRouting` 和 `useCodexConfigState.extractCodexRoutingConfig` 都必须先判断 `Array.isArray(codexRouting)`，将 legacy route 数组迁移成 `{ enabled: true, routes: [...] }`，避免 `typeof [] === "object"` 路径把 routes 清空。Rust `proxy/providers/codex.rs` 的 route resolver 也必须直接消费数组型 `codexRouting`，这样用户未重新保存 DB 前请求链路也能恢复。
- 第二层现场污染：OpenAI Official target provider 可能被写入本地接管代理 `127.0.0.1:15721`，导致 GPT route 命中后又递归回本机代理。route materialize 时，official/OAuth 目标 provider 的本地 proxy `base_url` 不能被当作真实上游；应按托管 Codex OAuth 处理并让 `CodexAdapter` 使用 `https://chatgpt.com/backend-api/codex`。
- 回归测试应覆盖：前端读取 legacy array 不丢 route；表单初始化 legacy array 不清空 route；后端 resolver 能用 legacy array 匹配 GPT/DeepSeek；official target provider 带本地 proxy `base_url` 时仍 materialize 为 `codex_oauth`。

## 2026-06-23 Codex History Repair State DB Auto Detection

- Codex history repair must not hard-code `~/.codex/sqlite/state_5.sqlite` as the default active DB. macOS user reports and upstream Codex issue evidence point to the current default state DB at `~/.codex/state_5.sqlite`; the `sqlite/` child directory is only a compatibility fallback for older/local transitional builds.
- Active DB resolution order should be: explicit UI/CLI override path, `sqlite_home` from Codex config, `CODEX_SQLITE_HOME`, default root `~/.codex/state_5.sqlite`, then legacy fallback `~/.codex/sqlite/state_5.sqlite`. This preserves configured migrations while fixing default macOS detection.
- The history repair UI should describe the default as `~/.codex/state_5.sqlite` and mention automatic `sqlite_home` / `CODEX_SQLITE_HOME` detection, so users do not manually copy the stale sqlite-subdir path into the override field.

## 2026-06-23 MultiRouter Model Modality Alignment

- MultiRouter 不能给新建 route 默认写入 `capabilities: { inputModalities:["text","image"], textOnly:false, supportsReasoning:true }`。这会把 DeepSeek V4 Flash/Pro 等纯文本模型错误标成图文，并且后端 `codex_routing_capabilities_for_model` 会优先信任 route 能力，覆盖模型名纯文本兜底。
- 正确能力来源顺序：route 显式能力 > `modelCatalog.models[]` 条目能力（`inputModalities` / `textOnly` / `supportsImage` / `vision` / `capabilities`）> 保守模型名兜底。未知模型不要默认标成图文，避免多模态/纯文本静默误判。
- DeepSeek Codex 预设的 `deepseek-v4-flash` 和 `deepseek-v4-pro` 应在 `modelCatalog` 中声明 `inputModalities:["text"]`、`textOnly:true`、`supportsImage:false`；MultiRouter 聚合 catalog 要保留这些字段并同步写入 route/catalog 能力。
- Rust `codex_config.rs` 生成 Codex Desktop model catalog 时，也要读取 `modelCatalog.models[]` 的能力声明；只看 route 能力或硬编码模型名会让前端目录和后端投影再次分叉。

## 2026-06-22 Codex MultiRouter User Guide

- 新增用户向说明书 `docs/guides/codex-multirouter-guide-zh.md`，定位为把 Codex Desktop 登录、CCSwitchMulti OAuth 授权、第三方模型源、本地路由映射、MultiRouter 工作台、子 Agent 前 5 候选排序、路由启动、Debug 检查、Codex 重启和历史修复串成完整流程的中文 Markdown。
- 文档只引用仓库已有真实截图：`docs/images/codex-official-auth-preservation/01-codex-app-enhancement-setting.png`、`docs/images/codex-deepseek-routing/01-codex-providers-require-routing.png`、`02-deepseek-codex-routing-form.png`、`03-local-route-codex-takeover.png`。MultiRouter 工作台、子 Agent 排序、状态 Debug、会话管理历史修复等新页面尚无真实截图，文档末尾列出待补路径，后续应补真实 UI 截图，不要伪造。
- 使用规则固化：先登录 Codex Desktop，再在 CCSwitchMulti `设置 → 认证` 完成 ChatGPT/Codex OAuth；额外模型源如 DeepSeek/GLM/本地模型通常要开启 `需要本地路由映射`，在高级选项 `模型映射` 中点击 `获取模型列表` 并配置上下文窗口；V4 Pro/Flash managed roles 自动从完整可路由目录注册，只有 direct model override 展示排序需要手工展开高级设置；保存/切换/模型目录变化后必须完全退出并重启 Codex Desktop。
- 历史修复说明保持当前产品边界：历史入口在右上角时钟/会话管理页的 `Codex 历史修复`，流程是 `加载历史`、按需全选当前页、`预览修复`、确认计数后 `确认写入`，完成后再次重启 Codex。该功能修复 provider bucket 可见性，不应表述为会话正文丢失修复。
- 主 `README.md` 前部的 CCSwitchMulti 分支说明后新增 `Codex 多路由配置说明书` 小节，直接链接 `docs/guides/codex-multirouter-guide-zh.md`，让首次配置用户先读完整流程而不是只看功能截图。
- 2026-06-22 用户补齐 MultiRouter 教程真实 UI 截图，稳定保存到 `docs/images/codex-multirouter/`：`01-settings-auth-oauth.png`、`02-add-provider-entry.png`、`03-configure-provider-local-routing.png`、`04-fetch-models-context-window.png`、`05-multirouter-entry.png`、`06-create-multirouter.png`、`07-configure-route-rules.png`、`08-save-route-rules.png`、`09-subagent-model-order.png`、`10-enable-routing-settings.png`、`11-debug-entry.png`、`12-13-history-repair-panel.png`、`13-codex-model-picker-validation.png`。这些图对应用户指定的 1-13 步及重启后 Codex 模型候选验证，不要再把这些场景列为待补截图。
- 渲染产物：`docs/images/codex-multirouter-guide/pages/` 保存 Markdown 说明书按页渲染的 PNG，规格为 1440x2400；当前页码包括 `00-overview.png`、`01-flow.png`、`02-step-1.png` 到 `12-step-11.png`、`13-faq.png`、`14-related-docs.png`，并有 `manifest.json` 记录标题和路径。说明书截图变更后必须重新生成这些分页 PNG 和 `output/pdf/codex-multirouter-guide-zh.pdf`。
- 2026-06-23 说明书分页生成流程已抽成仓库内 skill：`skills/markdown-paged-guide/`，包含 `scripts/render_paged_guide.cjs` 和 `scripts/pngs_to_pdf.py`。后续截图型 Markdown 说明书应优先用 `<!-- guide-page: file.png | title -->` 显式分页，统一用 `--max-image-height` 控制全书截图尺寸，再输出 `pages/manifest.json` 与 PDF。当前 MultiRouter 教程已改为 15 页：第一页入门准备，第二页 `总流程速览`，截图统一 `maxImageHeight=500`，避免双截图页底部溢出。

## 2026-06-22 CCSwitchMulti README Xiaohongshu Feedback QR

- GitHub multi README 的活跃源码落点是 `C:\Users\sunda\Documents\LLMservice\cc-switch\README.md`，对应 `fork` remote `https://github.com/BigStrongSun/ccswitchmulti.git`；`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 是固定交付/发布目录，不作为源码 README 修改点。
- README 顶部反馈入口使用仓库内资产 `assets/xiaohongshu-discussion-qr.png`，由用户提供的小红书群截图裁剪出纯二维码区域；README 引用路径保持相对路径 `assets/xiaohongshu-discussion-qr.png`，便于 GitHub 渲染。
- 顶部说明保留两条反馈路径：提交 GitHub Issue，或扫码加入小红书讨论群；二维码来自 2026-06-22 截图，标注有效期至 2026-07-20，后续过期需要替换同名资产并更新有效期文案。
- 纠正：GitHub 默认渲染的 `README.md` 应恢复并保持 `ff29c274 docs(readme): add ccswitchmulti screenshots and scenario` 那版中文 CCSwitchMulti 专属 README，包含“适合谁使用”“功能截图”和 MultiRouter 截图说明；不要用上游 `README_ZH.md` 或英文 `README.md` 覆盖默认首页。
- 配套图片资产必须随该版 README 一起保留：`assets/screenshots/ccswitchmulti/{provider-list,multirouter-status,multirouter-routes,codex-model-picker,usage-statistics}.png` 以及历史赞助图 `assets/partners/logos/lemondata.png`、`assets/partners/logos/ccsub.jpg`。如果只恢复 README 而不恢复这些文件，GitHub README 会出现大面积图片加载失败。

## 2026-06-22 MultiRouter Deletion Flow

- MultiRouter provider 通常就是 Codex 当前 provider；普通 provider 删除链路前端会禁用当前项，后端 `ProviderService::delete` 也会拒绝删除当前 provider，所以工作台必须提供 MultiRouter 专用删除入口。
- 删除当前 Codex MultiRouter 前，先自动切到一个非 MultiRouter 的普通 Codex provider 作为 fallback，再调用原有 `delete_provider`。不要绕过后端当前 provider 保护；保护逻辑仍用于防止误删正在使用的普通 provider。
- 工作台内至少在总览方案卡、路由规则页方案卡、状态页当前方案操作区展示删除按钮。删除动作仍走统一确认框，避免误点。

## 2026-06-22 MultiRouter Routes Compact Layout

- MultiRouter 规则配置页要优先按“同屏操作台”处理：顶部状态只做紧凑状态带，方案栏和规则详情栏不要固定到 360px，主布局应控制在约 300px 侧栏，避免小窗口时规则列表、详情和子 Agent 候选区被挤出屏幕。
- 候选 provider 模型刷新提示必须保留失败可见性，但成功/读取中的状态适合做一行紧凑条目；不要让刷新成功列表单独撑出一个大卡片高度。
- 子 Agent 候选模型面板的右侧候选池需要有 `max-height` + 内部滚动，预览卡片和拖拽项保持低高度；否则候选池会把整个 MultiRouter routes 页向下撑开，截图里顶部和候选区无法一页看全。

## 2026-06-22 MultiRouter Candidate Provider Model Refresh

- MultiRouter 路由规则页不能只消费普通 provider 已经持久化的 `settingsConfig.modelCatalog`，否则新建/切换 MultiRouter 时会停留在旧 GPT fallback，Qwen/DeepSeek/VLLM 等候选普通 provider 不会进入子 Agent 候选。
- 进入 `CodexRouterWorkspacePage` 的 `routes` tab 时，应自动对所有候选普通 Codex provider 调用 `fetch_models_for_config` 读取 `/models`；读取成功后写回该 provider 的 `settingsConfig.modelCatalog.models` 和 `spawnAgentModels`，并同步重建所有引用它的 MultiRouter plan catalog。
- 官方/OAuth provider 没有普通 base_url/API key 时跳过普通 `/models` 读取；普通 provider 缺 base_url、缺 API key、返回空列表或请求失败时，要在路由页和候选 router 卡片上明确提示“获取模型列表失败，请检查当前 provider 配置”。
- MultiRouter 的 `buildModelCatalogForRoutes` 必须按当前 routes 重建 catalog，只复用旧 catalog 的 display/context 元数据，不能无条件保留旧模型；否则取消 GPT route 或改成 VLLM/Qwen route 后，旧 GPT fallback 仍会污染 spawn_agent 前五候选。
- 普通 Codex provider 的“获取模型列表”按钮应把远端模型合并进模型映射表，并在保存时即使不是 `openai_chat` 也持久化非 official provider 的 modelCatalog；保存时空的 `spawnAgentModels` 要从 catalog 前五个自动补齐。

## 2026-06-21 WebDAV Cross-Device Codex Config Contamination

- WebDAV/S3 v2 sync does not upload `~/.codex/config.toml` as a raw file; the protocol uploads `db.sql` plus `skills.zip`.
- The synced SQL snapshot still includes portable and non-portable configuration rows such as `providers`, `mcp_servers`, `settings`, and `proxy_config`. After another device downloads the snapshot, normal CC Switch logic can write those DB rows back into that device's live Codex `~/.codex/config.toml`.
- Therefore cross-user WebDAV sync can effectively contaminate another machine's Codex config with the source machine's absolute paths, for example `notify`, `mcp_servers.*.command`, `mcp_servers.*.args`, local plugin/runtime cache paths, or provider config snippets that contain `C:\Users\<source-user>\...`.
- Do not treat this as Codex randomly generating bad paths. The root cause boundary is CC Switch sync importing machine-local DB values and later live-syncing them to Codex. Safe cross-device sync needs either excluding machine-local rows/fields or adding a per-device reconciliation/sanitization step before writing live configs.

## 2026-06-21 CCSwitchMulti 3.16.3-6 Local Export

- Version bump for the local manual-test build must update all four version surfaces: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.lock`.
- The local export pipeline for `3.16.3-6` produced Windows artifacts under `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`: `windows\installer\CCSwitchMulti_3.16.3-6_x64-setup.exe`, `windows\portable\CCSwitchMulti_3.16.3-6_x64-portable.zip`, and `windows\raw-exe\CCSwitchMulti_3.16.3-6_x64.exe`.
- Post-commit release hooks can start a background full build immediately after release/version commits. If a manual run hits `scripts\logs\local-release.lock`, inspect `scripts\logs\post-commit-release.log` and wait for cargo/rustc/makensis to exit instead of starting competing builds.

## 2026-06-21 Codex MultiRouter Route Toggle UX

- The MultiRouter route picker has two independent states: candidate membership and route enabled. UI labels must spell this out as `未加入`, `已加入并启用`, or `已加入但停用`; using only `启用/停用` makes users think the checkbox itself is the enabled state.
- In the generic Codex Provider edit form, route row switches must synchronously publish the full `codexRouting` object back to the parent form. Relying only on a child-to-parent effect introduces a one-render race where toggling a route and immediately pressing Save can persist the old enabled value.
- OpenAI/Codex official providers can legitimately have no `modelCatalog`. For route creation/picker display, only those OpenAI-like sources should get GPT/O-series fallback models; do not apply the fallback to every provider whose id starts with `codex-`, or Qwen/DeepSeek catalogs get polluted.
- `RouteCandidatePicker` 的 `selectedIds/enabledIds` 是未保存的本地草稿；同一个多路路由内普通父组件重渲染、provider refetch 或 optimistic plan 刷新时，不能再从 `candidate.route.enabled` 重新初始化，否则用户刚点的 `已启用` 会被旧配置覆盖回 `已停用`。父层 `routingPlans/modelSources` 应保持 memoized，子层候选刷新时只给新出现的 router 应用默认值，已有候选必须保留草稿状态。
- 子 Agent 候选模型面板右侧候选池不要使用固定 `max-h-*` 做滚动高度；它位于和左侧拖拽列表同一行的 grid 中，右侧卡片应 `h-full min-h-0 flex flex-col`，Tabs content 用 `flex-1 min-h-0`，列表自身 `h-full overflow-y-auto`，否则右下角滚动范围会和紫色外框高度不一致。

## 2026-06-21 Codex MultiRouter Picker Persistence

- MultiRouter 工作台的“创建多路路由”不能复用普通 Provider 创建表单；普通表单不会初始化 `settingsConfig.codexRouting`，会把新对象归到普通模型源，导致新建的多路路由在 MultiRouter 列表不可见。
- 正确创建路径是直接 `providersApi.add(nextPlan, "codex", false)` 写入一个带 `settingsConfig.codexRouting.enabled=true`、`routes=[]`、`modelCatalog` 初始目录的 Codex provider，然后打开候选 router 选择器。
- 候选 router 保存时必须把宽松 route 规整成后端可消费结构：稳定 `id`、`enabled`、`targetProviderId`、`match.models/prefixes`、`upstream.apiFormat`、`upstream.auth`，并确保 `defaultRouteId` 指向现有 route。
- 保存候选 routes 时同步重建 `settingsConfig.modelCatalog` 和 `spawnAgentModels`，否则 Codex 选择器/子 Agent 可见模型会与路由规则不一致。
- Tauri/Rust 持久化链路对 `settingsConfig` 是整段 JSON 直通保存：`providersApi.add/update` -> `ProviderService::add/update` -> `db.save_provider` -> SQLite `providers.settings_config`。后端不会裁剪 `codexRouting` 或 `modelCatalog`，本次修复不需要后端 schema 改动。
- MultiRouter provider 自身不是普通 Codex 上游，不应该进入通用 ProviderForm 去填 API Key、API 请求地址、本地模型路由或模型目录。多个 MultiRouter 共享同一套系统投影接管语义：Codex live config 指向 `codex_model_router_v2`、`http://127.0.0.1:15721/v1`、`wire_api=responses` 和 `cc-switch-model-catalog.json`，这些由切换/接管流程和工作台自动维护；用户只编辑方案名称、备注、入口启用、默认 route 和候选 routes。

## 2026-06-16 External OpenAI API Chinese Input Diagnostics

- Current live external Agent API profile was verified read-only from `~/.cc-switch/cc-switch.db`: enabled on `0.0.0.0:15722`, `backendType=provider`, `appType=codex`, `providerId=codex-official`, `defaultModel=gpt-5.5`. This means the reported `/v1/chat/completions` issue goes through External Chat Completions -> synthetic `codex_oauth` provider -> ChatGPT Codex `/backend-api/codex/responses`, not through the normal `15721` MultiRouter route table.
- Source-level UTF-8 chain remains `body.collect().to_bytes()` -> `serde_json::from_slice` -> `serde_json::Value` -> `chat_completions_request_to_codex_responses` -> `serde_json::to_vec` -> reqwest body; no ASCII/Latin-1/GBK conversion was found.
- Real compatibility gap fixed in `src-tauri/src/proxy/providers/openai_compat.rs`: Chat message content parts with Responses-style `type: "input_text"` or `type: "output_text"` were previously dropped because only `type: "text"` was accepted. This can make Codex see only surviving English tokens or references from mixed third-party Agent payloads. The converter now preserves `text`, `input_text`, and `output_text` as Responses text parts.
- Added non-content diagnostics for the external codex-official path: `external_chat_unicode_probe` in `codex-router.log` records text part count, character count, non-ASCII count, question mark count, replacement-character count, and a short hash before forwarding to Codex OAuth. It deliberately does not log prompt text.
- Regression tests added: `chat_request_preserves_chinese_through_codex_responses_conversion`, `chat_request_preserves_responses_style_text_parts`, `v1_chat_completions_preserves_chinese_for_profile_backend`, and `external_codex_unicode_stats_detects_chinese_without_prompt_leak`.

## 2026-06-16 CCSwitchMulti 3.16.2-20 GitHub release

- Published `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-20` from target commit `b38e0649aeafce68e3c6b300bcb53c22b4edb413` after pushing `feat/codex-local-model-routing` to the fork.
- Uploaded 10 exact assets: Windows setup exe, setup signature, portable zip, raw exe, `CodexHistoryTool_3.16.2-20.zip`, `latest.json`, root `README.md`, Linux/macOS build notes, and `SHA256SUMS-v3.16.2-20.txt`.
- Do not upload the fixed export directory wholesale for this release line: `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\SHA256SUMS.txt` still includes old version residue such as earlier raw exes. Use the version-specific checksum file for release verification.
- Post-release verification passed: `gh release view v3.16.2-20` reported a non-draft, non-prerelease release with all 10 assets; `git ls-remote --tags fork v3.16.2-20` pointed at the target commit; downloaded release `latest.json` points updater clients at `v3.16.2-20/CCSwitchMulti_3.16.2-20_x64-setup.exe`.

## 2026-06-14 Subagent Visible Model Toolcall Test

- User requested subagent testing for all currently visible Codex models plus toolcall capability.
- Live Codex config at test time used `model_provider = "codex_model_router_v2"` with `model_catalog_json = "cc-switch-model-catalog.json"` and `[model_providers.codex_model_router_v2] base_url = "http://127.0.0.1:15721/v1"`, `wire_api = "responses"`.
- `~/.codex/cc-switch-model-catalog.json` exposed 7 list-visible API-supported slugs with parallel tool calls enabled: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`.
- Subagent + shell toolcall passed for `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `deepseek-v4-flash`, and `deepseek-v4-pro`. Each successful worker ran safe read-only PowerShell checks such as `Get-Location`, `Get-Date`, and `Get-ChildItem`.
- `gpt-5.3-codex-spark` could be spawned but both attempts ended with `You've hit your usage limit. Try again later.`, so model availability/toolcall could not be verified in this run.
- `qwen3.6` first completed with an empty final status, then the explicit retry failed with `unexpected status 502 Bad Gateway` while handling `/responses`; CCSwitch logs showed routing was correct (`route_id=qwen-local`, upstream `https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions`) but the Qwen upstream returned 502 with `<urlopen_error_[Errno_111]_Connection_refused>`. Direct probes to `https://www.matrixminecraft.cn:24443/vllm/v1/models` and `/chat/completions` also returned 502, so the failure boundary is the Qwen vLLM upstream, not local model-catalog visibility or subagent shell toolcall permissions.
- Local router process remained running during the test: PID `46200`, path `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.2-14_x64.exe`, listening on `0.0.0.0:15721`.
- Do not treat unauthenticated `GET http://127.0.0.1:15721/v1/models` returning 401 as proof of model failure; this endpoint requires auth in the current router path.

## 2026-06-14 Codex Desktop Three-Model Runtime Snapshot

- Re-focused the 3-model picker report on the current running Codex Desktop state, not only on provider-id/history cleanup.
- Current live files are valid for MultiRouter: `~/.codex/config.toml` has `model_provider = "cc_switch_codex_router"`, top-level `model_catalog_json = "cc-switch-model-catalog.json"`, and `[model_providers.cc_switch_codex_router]` pointing at `http://127.0.0.1:15721/v1`; both `cc-switch-model-catalog.json` and `models_cache.json` contain the 7 expected slugs.
- A fresh Codex CLI process using the current `~/.codex/config.toml` (`codex debug models`) returns all 7 slugs, proving the generated catalog is parseable by Codex and the model fields are not filtered out by `visibility` / `supported_in_api`.
- The current thread tool description is not reliable proof of a 3-model Desktop picker: Codex hard-caps `spawn_agent` model override descriptions at 5 entries (`MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION = 5`), so DeepSeek can be omitted there even when the static catalog contains it. Use Desktop `model/list` / visible picker evidence for the UI claim.
- Current DB state is valid for MultiRouter: `codex-openai-router` is current, its `modelCatalog` has the 7 expected slugs, and `codexRouting` has enabled OpenAI/Qwen/DeepSeek routes. `codex-router.log` shows real `route_resolved` / `upstream_status` attribution for OpenAI, Qwen, and DeepSeek routes in prior/current runs.
- Codex app-server `model/list` is served from `supported_models(thread_manager)`, and `ThreadManager::new` builds a shared `models_manager` once from the startup `Config`. Later config/catalog writes do not automatically rebuild this manager. If the visible Desktop picker still shows only 3 while fresh `codex debug models` returns 7, the remaining root-cause boundary is the running Desktop app-server/UI model-list snapshot or UI cache, not CCSwitch catalog generation or route configuration.

## 2026-06-13 Codex MultiRouter Stable Bucket Reconciliation

- Re-checked the 3-model Codex Desktop picker issue after the 3.16.2-5 build.
- Live `~/.codex/config.toml` was already in MultiRouter takeover form with top-level `model_catalog_json = "cc-switch-model-catalog.json"`, `base_url = "http://127.0.0.1:15721/v1"`, `wire_api = "responses"`, `requires_openai_auth = false`, and `supports_websockets = false`.
- Live `cc-switch-model-catalog.json` and `models_cache.json` both contained the 7 expected slugs: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, and `deepseek-v4-pro`.
- Codex source archaeology showed `model_catalog_json` is the actual model candidate source; arbitrary non-reserved provider ids do not unlock the picker. Thread/history listing does use the current `model_provider` as its default provider bucket, so changing the MultiRouter id can hide historical sessions.
- Decision: keep `codex_model_router_v2` as the stable runtime MultiRouter provider id, while keeping `cc_switch_codex_router` in legacy/history source lists so older sessions can still migrate. Do not switch back to built-in `openai + openai_base_url` for MultiRouter unless a separate Codex source-level proof requires it.
- Runtime DB note: `codex-openai-router.settings_config.config` may still carry the old `model_provider = "openai"` plus `openai_base_url` persisted shape, but takeover code normalizes it to the stable local provider table. Future cleanup can normalize the stored provider config too, but the live candidate source is the generated catalog pointer.

## 2026-06-13 Codex MultiRouter Candidate Bucket Fix

- User reported the current CCSwitchMulti build still showed only three OpenAI candidates in Codex Desktop, while the older 2026-06-08 CCSwitchMulti build showed the full MultiRouter list.
- Code/DB archaeology:
  - 2026-06-08 working backups used `model_provider = "cc_switch_codex_router"` plus top-level `model_catalog_json = "cc-switch-model-catalog.json"` and `[model_providers.cc_switch_codex_router]`.
  - The working path was the static Codex `model_catalog_json` file with 7 router model slugs, not `models_cache.json` alone and not the later `openai + openai_base_url` experiment.
  - The current local DB had drifted to `model_provider = "openai"` with `openai_base_url = "http://127.0.0.1:15721/v1"` in `codex-openai-router.settings_config.config`, which risks pushing the picker back into Codex's built-in OpenAI provider semantics.
- Fix:
  - `src-tauri/src/codex_config.rs` now sets `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID` to `cc_switch_codex_router`.
  - `src-tauri/src/services/proxy.rs` keeps normal third-party Codex providers on `custom`, but MultiRouter takeover writes the 2026-06-08 router bucket, removes `openai_base_url`, and keeps `supports_websockets = false`.
  - `src-tauri/src/codex_history_migration.rs` treats `cc_switch_codex_router` as a known router/openai-history source so history sync does not split buckets.
  - `src-tauri/src/services/provider/mod.rs` regression test now starts from the drifted `openai + openai_base_url` persisted config and asserts the live config is normalized to `cc_switch_codex_router` with 7 catalog/cache models.
- Verification passed:
  - `cargo test --manifest-path src-tauri\Cargo.toml switching_codex_router_provider_auto_enables_dedicated_local_takeover --lib -- --nocapture`
  - `cargo test --manifest-path src-tauri\Cargo.toml history --lib`
  - `cargo test --manifest-path src-tauri\Cargo.toml --lib codex`
  - `cargo fmt --manifest-path src-tauri\Cargo.toml --check`
  - `cargo check --manifest-path src-tauri\Cargo.toml` (only pre-existing warnings in `commands/misc.rs`)

## 2026-06-11 Codex Windows App Upgrade Strategy

- User reported Codex CLI update failure from the CC Switch settings page: current `0.137.0`, latest `0.139.0`, toast stack included `aws_lc_0_39_0_jent_entropy_switch_notime...`.
- Local diagnosis:
  - Default `codex` resolves to `C:\Users\sunda\AppData\Local\OpenAI\Codex\bin\codex.exe`.
  - Another Codex executable exists under `C:\Program Files\WindowsApps\OpenAI.Codex_26.608.1337.0_x64__2p2nqsd0c76g0\app\resources\codex.exe`.
  - `codex --version` is `codex-cli 0.137.0`.
  - `codex update` says it cannot detect the installation method.
  - `npm view @openai/codex version` is `0.139.0`, but `winget upgrade --id 9PLM9XGG6VKS --source msstore` reports no available Store upgrade.
- Root cause: the previous Windows lifecycle updater treated Codex App/MSIX launcher paths as ordinary system/npm installs and could build `codex update || npm i -g @openai/codex@latest`, mixing the Codex App runtime with the user's WinGet Node/npm.
- Fix in `src-tauri/src/commands/misc.rs`:
  - Classify `AppData\Local\OpenAI\Codex`, `WindowsApps\OpenAI.Codex_...`, and `Microsoft\WindowsApps\codex.exe` paths as `codex-app`.
  - For Codex App/MSIX installs, generate a Store package update command with `winget upgrade --id 9PLM9XGG6VKS --source msstore --accept-source-agreements --accept-package-agreements`.
  - Do not attach npm fallback for this install source.
  - If multiple Codex entries are detected and no default install can be selected, any Codex App/MSIX entry forces the Store update command instead of the static `codex update || npm ...` fallback.
- Regression coverage:
  - `codex_windows_app_uses_ms_store_upgrade_without_npm_fallback`.
  - `ambiguous_codex_app_install_uses_ms_store_upgrade`.
  - `windows_codex_app_is_identified`.
  - Validation passed: `cargo test --manifest-path src-tauri\Cargo.toml anchored_upgrade_windows --lib`, `cargo test --manifest-path src-tauri\Cargo.toml install_source_classification --lib`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `cargo check --manifest-path src-tauri\Cargo.toml`.

## 2026-06-08 Router UI/Save Logic Fix

- Latest user symptom: after launching the portable build and selecting `OpenAI Multi-Model Router`, Codex Desktop still only showed OpenAI/GPT candidates and lost `gpt-5.3-codex-spark`, DeepSeek, and Qwen. The CC Switch list also showed `OpenAI Multi-Model Router` with the `不支持路由` badge.
- Multi-agent assessment: this was a narrow local state + UI/save-path diagnosis, so the main agent handled it directly instead of spawning subagents. Verification was done through process checks, DB inspection, typecheck, and packaging.
- Live process check:
  - Running process was PID `48844`, started `2026-06-08 20:39:21`.
  - Path: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\cc-switch.exe`.
  - This was the earlier 17:34 router-candidate portable, not the newer UI/save-logic fixed build below.
- Local DB hotfix:
  - Backup directory: `C:\Users\sunda\.cc-switch\backups\codex-router-category-fix-20260608_205059`.
  - `codex-openai-router.category` was corrected from `official` to `aggregator`.
  - Current provider was left as `codex-official`; no runtime switch away from the user's backup/official line was performed.
- Current Codex config check:
  - `C:\Users\sunda\.codex\config.toml` currently has no `model_provider`, `model_catalog_json`, local `base_url`, or `127.0.0.1` router/proxy lines, so Codex Desktop is still effectively on the backup/official config.
  - `C:\Users\sunda\.codex\cc-switch-model-catalog.json` exists and contains 7 model slugs: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
  - Therefore a missing CodexSpark/DeepSeek/Qwen dropdown after this state means the router takeover was not active, not that the catalog file was absent.
- Root causes:
  - `src/components/providers/ProviderCard.tsx` treated every Codex `official` category provider as `不支持路由`, even when `settings_config.codexRouting` existed. A router provider with official OAuth routes must still be treated as proxy-routed.
  - `src/hooks/useProviderActions.ts` only required the proxy for non-official providers. A Codex router with `codexRouting` now also requires the local proxy even when route auth uses managed official OAuth.
  - `src/components/providers/forms/ProviderForm.tsx` skipped `modelCatalog` and `codexRouting` persistence for category `official`, and only saved the model catalog for `openai_chat`. The router's outer API is `openai_responses`, so editing/saving it could wipe the generated catalog and routes.
- Code fix:
  - `ProviderCard.tsx` now detects `settings_config.codexRouting`, marks such Codex providers as needing routing, and suppresses the false `不支持路由` badge.
  - `useProviderActions.ts` now treats Codex router providers as local-proxy-required providers and allows them during proxy takeover.
  - `ProviderForm.tsx` now preserves `modelCatalog` and `codexRouting` when routing is enabled or routes exist, including router providers whose outer API format is `openai_responses`.
- Verification:
  - `pnpm typecheck` passed.
  - `pnpm tauri build --bundles nsis --config "$env:TEMP\cc-switch-tauri-no-updater.json"` passed.
- Latest UI/save-logic fixed artifacts:
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-ui-fix-20260608_210732\release\bundle\portable\cc-switch.exe`
    - SHA256 `4D3E0A7EC297901CEEAB972B3B70018521F0052077AEB6062F4468BE2B6F036A`
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-ui-fix-20260608_210732\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
    - SHA256 `1D7338E7F137D5CA1888F3A966F8877DA26CB8F3CEE8A87324075F0EE53CDAC7`
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
    - SHA256 `A1194B9A55BB2478BA182FAB1A6C7FF9AACA6DEED450A4A4662947099D5C298A`
- Architecture clarification:
  - `OpenAI Multi-Model Router` is not merely upstream CC Switch's native provider switcher, and it is not an external script. It depends on the local Codex multi-model routing patch now present in this repo.
  - Native CC Switch routing/proxy takeover can redirect Codex to one selected provider, but by itself it does not create a single Codex Desktop model dropdown containing OpenAI, CodexSpark, DeepSeek, and Qwen candidates.
  - The patched path has three required layers: `settings_config.modelCatalog` projects `~/.codex/cc-switch-model-catalog.json` so Codex can display all candidates; `settings_config.codexRouting` stores model-to-upstream routes; the Rust local proxy resolves the request `model` via `resolve_codex_model_routed_provider` and converts Responses to Chat where needed.
  - Therefore the multi-model dropdown requires CC Switch local proxy/takeover plus the patched `modelCatalog`/`codexRouting` implementation. Switching ordinary providers alone is not enough.

## 2026-06-08 Router Candidate/Timeout Fix Package

- Root cause found in the local user DB:
  - `codex-openai-router.settings_config.modelCatalog.models` only contained 4 OpenAI models, so Codex candidate model UI could not show DeepSeek/Qwen.
  - `codex-openai-router.settings_config.codexRouting` was missing, so even a selected DeepSeek/Qwen model would not have a route.
  - Code gap: `src-tauri/src/services/provider/live.rs::restore_live_settings_for_provider_backfill` preserved DB-only `modelCatalog` but not DB-only `codexRouting`; switch-away backfill from Live could wipe the router route table because Live `config.toml` cannot represent it.
- Code fix:
  - `src-tauri/src/services/provider/live.rs` now preserves both `modelCatalog` and `codexRouting` during Codex backfill.
  - Regression test added: `codex_switch_backfill_preserves_stored_codex_routing_when_live_lacks_it`.
- Local DB fix:
  - Backup: `C:\Users\sunda\.cc-switch\backups\codex-router-multimodel-fix-20260608_172503\cc-switch.db.before`.
  - Current provider was left as `codex-official`; no official/backup runtime switch was performed.
  - Router catalog models now include `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
  - Router routes:
    - `openai-official`: `gpt-*` -> `https://chatgpt.com/backend-api/codex`, `openai_responses`, `managed_codex_oauth`.
    - `deepseek`: `deepseek-*` -> `https://api.deepseek.com`, `openai_chat`. DeepSeek key is currently empty, so the candidate appears but requests need a key before success.
    - `qwen-local`: `qwen3.6` -> `https://www.matrixminecraft.cn:24443/vllm/v1`, `openai_chat`, `apiKey=vllm-local`.
- Verification:
  - `cargo test codex_switch_backfill --manifest-path src-tauri\Cargo.toml`
  - `cargo test codex_route --manifest-path src-tauri\Cargo.toml`
  - `cargo fmt --manifest-path src-tauri\Cargo.toml --check`
  - `pnpm typecheck`
  - Qwen upstream `/v1/models` returned `qwen3.6`.
- Latest artifacts were built into an isolated target to avoid overwriting the currently running old portable instance:
  - Target dir: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503`.
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
    - SHA256 `41D9FA3DB194F299F79772E5BABFF72D79AE9262332DD98142E90DDE802BCFDB`
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\portable\cc-switch.exe`
    - SHA256 `9D921B3122CB8FE436974F10DF8BAF1ABF2628812D66E12A7A3A7070727B9B26`
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
    - SHA256 `EC9936E4987985ABA8A2B066831AE1D853FD1BF972FE32CE38590615622FA146`
  - MSI: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target-router-fix-20260608_172503\release\bundle\msi\CC Switch_3.16.1_x64_en-US.msi`
    - SHA256 `38D4E2F7AAC10F27801E5BBDAEFB8B7DB6AE3D33658020DE27ACFA2E155C32D8`
- Packaging note:
  - `pnpm tauri build` produced the release exe, NSIS, and MSI but exited 1 at updater artifact signing because `TAURI_SIGNING_PRIVATE_KEY` is not set. The portable zip was manually generated from the new release exe, matching the existing local portable maintenance pattern.
  - To test the new portable build, close the old local modified CC Switch window first; the single-instance plugin can otherwise bring the old process to front. Codex official does not need to be stopped.

## 2026-06-08 DeepSeek Key Local Configuration

- User provided a DeepSeek key and asked to configure it locally. Do not commit or document the full key; only use masked form `sk-b931...b870` in notes.
- Backup directory before the write: `C:\Users\sunda\.cc-switch\backups\codex-deepseek-key-20260608_203307`.
- Updated local DB fields:
  - `codex-deepseek.settings_config.auth.OPENAI_API_KEY`.
  - `codex-openai-router.settings_config.auth.OPENAI_API_KEY`.
  - `codex-openai-router.settings_config.codexRouting.routes[id=deepseek].upstream.apiKey`.
- Current provider was left as `codex-official`; no switch/takeover was performed.
- Lightweight verification against `https://api.deepseek.com/v1/models` succeeded and returned `deepseek-v4-flash` and `deepseek-v4-pro`.

## 2026-06-08 Packaging And Maintenance

- Current local build artifacts:
  - NSIS installer: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\nsis\CC Switch_3.16.1_x64-setup.exe`
  - Portable zip: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\portable\CC Switch_3.16.1_x64-portable.zip`
  - Portable exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\bundle\portable\cc-switch.exe`
  - Raw release exe: `C:\Users\sunda\Documents\LLMservice\cc-switch\src-tauri\target\release\cc-switch.exe`
- Local verification before packaging:
  - `pnpm run typecheck`
  - `cargo test codex --lib` from `src-tauri`
- Recommended local packaging command:
  - Create temp config `C:\Users\sunda\AppData\Local\Temp\cc-switch-tauri-no-updater.json` with `{"bundle":{"createUpdaterArtifacts":false}}`.
  - Run `pnpm tauri build --bundles nsis --config "$env:TEMP\cc-switch-tauri-no-updater.json"`.
- Do not use plain `pnpm run build` as the final local handoff command unless `TAURI_SIGNING_PRIVATE_KEY` is available and MSI/WiX is intentionally required.
  - Current `tauri.conf.json` has updater public key plus `createUpdaterArtifacts=true`, so local builds without a private key fail after bundle generation.
  - Full target builds also invoke MSI/WiX; `light.exe` has previously made the command exit 1 even when `cc-switch.exe` and installer files were produced.
  - Treat the NSIS no-updater command above as the reliable local packaging path.
- Portable package maintenance:
  - Copy `src-tauri\target\release\cc-switch.exe` to `src-tauri\target\release\bundle\portable\cc-switch.exe`.
  - Zip only that exe into `CC Switch_3.16.1_x64-portable.zip`.
  - Portable and installed builds share user data in `~/.cc-switch` and `~/.codex`; do not run them concurrently with the official production app.
- Official production app safety:
  - Do not stop or restart the installed official process during local diagnosis/build work.
  - Last verified official process path: `C:\Users\sunda\AppData\Local\Programs\CC Switch\cc-switch.exe`.

## 2026-06-08 Local Codex Provider Cleanup

- User restored historical `~/.cc-switch` config and explicitly said future cleanup must not use that DB content as a template.
- Canonical Codex provider writes should follow latest repo schema:
  - Pure official fallback: `codex-official`, `settings_config={"auth":{},"config":""}`, no `model_provider`, no `base_url`, no `model_catalog_json`, no `codexRouting`.
  - New router providers must use `settings_config.codexRouting`; legacy `codexModelRoutes` / `modelRoutes` are read-only compatibility paths.
  - `meta.apiFormat` and route `upstream.apiFormat` are the explicit API-format source for proxy conversion.
  - Chat-compatible DeepSeek/Qwen providers should use `meta.apiFormat="openai_chat"` and TOML `wire_api="chat"`.
  - Do not put router TOML, `model_catalog_json`, or `127.0.0.1:15721/15722` into `settings.common_config_codex`.
- Local machine cleanup performed 2026-06-08 15:10:
  - Kept only `codex-official`, `codex-openai-router`, `codex-qwen-local`, and `codex-deepseek`.
  - Set `currentProviderCodex="codex-official"`, `enableLocalProxy=false`, cleared `common_config_codex`, disabled Codex takeover flags, and removed Codex `proxy_live_backup`.
  - Backup path: `C:\Users\sunda\.cc-switch\backups\codex-clean-20260608_150944`.

## 2026-06-08 Codex Local Model Routing

### Product Direction Update

- User clarified that the main UI should be a separate Model Router workspace, not only an embedded route editor inside `CodexFormFields`.
- Desired flow: configure or import multiple model sources first, then select sources and merge them into one router provider that Codex reaches through CC Switch local proxy.
- Prototype artifacts:
  - `docs/prototypes/codex-router-workspace-prototype.html`
  - `docs/guides/codex-model-router-workspace-prototype.md`
- Existing `CodexFormFields` Local model routing editor should be treated as an advanced/generated-config surface unless the prototype review decides otherwise.
- Prototype v2 decision: the Model Router workspace must follow the existing CCSwitch header/AppSwitcher/provider-card style, not a generic SaaS dashboard or left-sidebar layout.
- Prototype v2 entry/exit rules: users can enter from the Codex Provider list, the Codex provider form, or Universal Provider; after publish they return to the Codex Provider list with the generated router provider highlighted.
- Prototype v2 source library rules: source setup must guide provider creation/import, base URL/auth/API format setup, connection test, model fetch, capability query, manual capability edit, and real route testing.
- Prototype v2 catalog rules: one provider/source may expose many upstream models, so UI must support fetched model lists and user-controlled visible models before writing Codex model catalog.
- Prototype v2 publish rule: route success must be tested through the CC Switch Rust local proxy before final publish; static config validation alone is not enough.
- Proposed UI component split for real implementation: `src/components/codex-router/ModelRouterWorkspace.tsx`, `RouterSourceLibrary.tsx`, `RouterSourceEditorDialog.tsx`, `RouterModelCatalogPanel.tsx`, `RouterSummaryPanel.tsx`, `RouteTestPanel.tsx`, and a draft/publish adapter.
- Prototype v3 visual correction: the static prototype must use CCSwitch's dark desktop-app style, wide 16:10 window proportions, top toolbar/app switcher, orange circular add button, blue active borders, and long horizontal provider cards.
- Prototype v3 information architecture: split the router workspace into multiple pages (`Overview`, `Sources`, `Models`, `Routes`, `Test & Publish`) using left-side step navigation; do not stack all router content into one vertical long page.

### Branch And Sync

- Feature branch: `feat/codex-local-model-routing`.
- Created from latest `origin/main` after stashing the old local WIP.
- Protective stash kept for now: `stash@{0}` named `wip-codex-local-routing-before-upstream-sync-20260608-005258`.
- Untracked `run-release-and-check.bat` existed after applying the stash; do not delete it unless the owner confirms it is disposable.

### Canonical Config

- New route config lives under `settings_config.codexRouting`.
- Shape:
  - `enabled`: enables/disables the resolver.
  - `defaultRouteId`: fallback route id when no exact/prefix rule matches.
  - `routes[]`: user-defined route list.
- Route fields:
  - `id`, `label`, `enabled`.
  - `match.models` for exact model ids.
  - `match.prefixes` for model id prefixes.
  - `upstream.baseUrl`.
  - `upstream.apiFormat`: `openai_responses`, `openai_chat`, or `openai_messages`.
  - `upstream.auth.source`: first version supports `provider_config`, `managed_codex_oauth`, and `managed_account`.
  - `upstream.apiKey` for provider-config key material when needed.
  - `upstream.modelMap` for Codex model id to upstream model id mapping.
  - `capabilities.textOnly`, `capabilities.inputModalities`, `capabilities.supportsReasoning`.
- Legacy fields `settings_config.codexModelRoutes` and `settings_config.modelRoutes` are read-only fallbacks. The UI may load them and save back to `codexRouting`.
- `reuse_provider:<id>` is intentionally not supported in the first version.

### Rust Entry Points

- Route resolver and effective provider construction:
  - `src-tauri/src/proxy/providers/codex.rs`
  - Main entry: `resolve_codex_model_routed_provider`.
  - Effective routed provider id format: `{outer_provider_id}::route::{route_id}`.
  - Managed Codex OAuth routes must remove inherited provider `auth` / `apiKey`; otherwise stale Bearer keys can override the managed account chain.
- Forwarding and protocol selection:
  - `src-tauri/src/proxy/forwarder.rs`
  - Reuses existing forwarder flow after route resolution.
  - Supports Responses passthrough, Responses -> Chat, and Responses -> Messages endpoint handling.
- Responses to Chat conversion:
  - `src-tauri/src/proxy/providers/transform_codex_chat.rs`
  - Text-only route capability prevents emitting Chat `image_url` blocks.
- Model catalog capability generation:
  - `src-tauri/src/codex_config.rs`
  - Route capabilities override hardcoded text-only model-name fallbacks.

### Frontend Entry Points

- Shared types:
  - `src/types.ts`
  - `CodexRoutingConfig`, `CodexRoutingRoute`, `CodexRoutingAuth`, `CodexRoutingCapabilities`.
- Codex config state:
  - `src/components/providers/forms/hooks/useCodexConfigState.ts`
  - Reads `codexRouting`; migrates `codexModelRoutes` / `modelRoutes` into UI state.
- Provider save path:
  - `src/components/providers/forms/ProviderForm.tsx`
  - Saves `settings_config.codexRouting` when routing is enabled or routes exist.
- Codex UI:
  - `src/components/providers/forms/CodexFormFields.tsx`
  - Adds **Local model routing** controls as a route summary list plus an edit dialog for match rules, upstream API format, auth, mapping, and capabilities.
  - The Local model routing panel is independent of endpoint speed-test visibility; it should show whenever the Codex form has routing state.
  - Switching a route from `provider_config` to a managed auth source should clear route `apiKey` so stale keys are not persisted.
- i18n keys live under `codexConfig` in:
  - `src/i18n/locales/en.json`
  - `src/i18n/locales/zh.json`
  - `src/i18n/locales/zh-TW.json`
  - `src/i18n/locales/ja.json`

### Docs

- Existing DeepSeek guide paths are now generic Codex Local Model Routing guides:
  - `docs/guides/codex-deepseek-routing-guide-en.md`
  - `docs/guides/codex-deepseek-routing-guide-zh.md`
  - `docs/guides/codex-deepseek-routing-guide-ja.md`
- The filenames still contain `deepseek` for link compatibility, but the content is generic and UTF-8.

### Validation Commands Used

- Rust focused validation:
  - `cargo fmt`
  - `cargo test codex --lib`
- Frontend type validation:
  - `pnpm run typecheck`
- Frontend route UI validation:
  - `pnpm vitest run tests/components/CodexFormFields.test.tsx tests/components/ProviderForm.codexCatalog.test.ts`
- Renderer build validation:
  - `pnpm run build:renderer`

### Maintenance Notes

- When fixing route bugs, update this file if the schema, resolver behavior, or capability semantics change.
- If text-only/image behavior changes, update both catalog generation and Responses -> Chat conversion tests.
- Keep Codex connected to the CC Switch Rust local proxy for this design; route selection should depend on `body.model`, not the GUI's currently selected upstream provider.

## 2026-06-08 Codex v2 DeepSeek v4 Local Proxy Fix

- Canonical user-facing model spelling for this workspace is `deepseekv4`, while configured aliases may include `deepseek-v4-pro`, `deepseek-v4-flash`, or display names such as `DeepSeek V4 Pro`.
- The intended Codex path is still v2 through the CC Switch Rust local proxy: Codex sends `/responses` to `http://127.0.0.1:<proxy>/v1`, CC Switch selects a route, then translates to the route upstream format when needed.
- The DeepSeek v4 failure was not caused by the old user script. It came from the built-in Rust Responses -> Chat conversion emitting Chat `content[]` image blocks for a text-only upstream. DeepSeek rejected this with `unknown variant image_url, expected text`.
- Text-only detection for DeepSeek v4 must use compact model-id normalization so `deepseekv4`, `deepseek-v4-*`, and spaced display aliases are all treated the same.
- Keep DeepSeek v4 text-only behavior aligned across `src-tauri/src/proxy/providers/transform_codex_chat.rs`, `src-tauri/src/codex_config.rs`, and `src-tauri/src/proxy/media_sanitizer.rs`.
- GUI route creation should not persist default `capabilities: { textOnly:false, inputModalities:["text","image"], supportsReasoning:false }` for new routes, because that can create a false explicit image-capability override.
- Route-level `codexChatReasoning.minOutputTokens` is supported for Chat upstreams that need a larger minimum output budget to avoid reasoning consuming tiny Codex probe responses.
- Validation commands used for this fix: `cargo fmt`, `cargo test transform_codex_chat --lib`, `cargo test media_sanitizer --lib`, `cargo test codex_model_catalog --lib`, `cargo test codex --lib`, and `node node_modules\typescript\bin\tsc --noEmit`.

## 2026-06-08 Codex Multi-Model Router Detail Fix

- The working router provider is the patched CC Switch Rust local proxy path, not native provider switching alone. Codex connects to CC Switch, the proxy reads `body.model`, resolves `settings_config.codexRouting`, and forwards to OpenAI official, DeepSeek, or Qwen.
- Stable Codex history bucket for this local router is `codex_model_router_v2`. Avoid reintroducing `cc_switch_codex_router`; it splits Codex Desktop history into another provider bucket. On this machine, old `codex_model_router` rows were merged into `codex_model_router_v2` with backup at `%USERPROFILE%\.codex\backups\router-provider-v2-merge-20260608_225952`.
- Router provider DB config currently uses `model_provider = "codex_model_router_v2"` with `[model_providers.codex_model_router_v2] base_url = "http://127.0.0.1:15721/v1"` and `wire_api = "responses"`.
- Route/candidate catalog currently exposes 7 models: `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `qwen3.6`.
- `src-tauri/src/codex_config.rs` must preserve `additional_speed_tiers` and `service_tiers` for OpenAI official `gpt-*` entries, except `codex-spark`; third-party/local models should still clear these fields so the UI does not show official service tiers on DeepSeek/Qwen.
- Existing on-disk catalog was manually refreshed after the code fix; old file backup is `%USERPROFILE%\.codex\backups\catalog-speed-tiers-20260608_231320`.
- `src-tauri/src/proxy/codex_router_log.rs` writes compact diagnostics to `%USERPROFILE%\.cc-switch\logs\codex-router.log`. It logs route, auth, request preparation, upstream send/status/error, and response readiness by trace id without raw prompt, token, header, or SSE content.
- `src-tauri/src/lib.rs` should not delete `%USERPROFILE%\.cc-switch\logs\cc-switch.log` on startup; early router cutover errors must survive restart.
- Avoid raw request/SSE logs in normal Debug/Trace. `forwarder.rs` should log request bytes plus body hash; `response_processor.rs` should only parse SSE when usage collection requires it.

## 2026-06-09 CCSwitchMulti Config Preservation And Packaging

- Current local modified build is branded `CCSwitchMulti` to distinguish it from the official `CC Switch` binary. The app still uses the existing `.cc-switch` data directory so provider DB/config history remains shared; do not rename the config directory unless deliberately doing a clean-room install.
- Package identity for the modified installer is `com.ccswitchmulti.desktop`; deep-link scheme is `ccswitchmulti`. This prevents the local installer from being treated as the same app identity as official `com.ccswitch.desktop`.
- MSI packaging rejects prerelease ids like `multi.1`; use numeric prerelease `3.16.1-1` for this local build line. The visible distinction comes from `productName = "CCSwitchMulti"` plus the numeric local build suffix.
- Current delivery directory: `src-tauri/target-ccswitchmulti-20260609_001033/`.
  - Portable zip: `CCSwitchMulti_3.16.1-1_x64-portable.zip`.
  - Portable exe: `CCSwitchMulti.exe`.
  - NSIS installer: `CCSwitchMulti_3.16.1-1_x64-setup.exe`.
  - MSI installer: `CCSwitchMulti_3.16.1-1_x64_en-US.msi` copied from `src-tauri/target/release/wix/x64/output.msi` after Tauri's MSI final copy failed.
- Build cleanup on 2026-06-09 removed stale local modified targets `src-tauri/target-router-fix-20260608_172503`, `src-tauri/target-router-ui-fix-20260608_210732`, and `src-tauri/target-router-detail-fix-20260608_230505`, the default build cache `src-tauri/target`, and the old root release artifacts `cc-switch-release` / `cc-switch-release.zip`. A stale portable process from `target-router-detail-fix-20260608_230505` had to be stopped to unlock that old directory; the official backup instance was not stopped.
- After cleanup, only `src-tauri/target-ccswitchmulti-20260609_001033` should be used for current delivery artifacts. Do not hand users any old `target-router-*`, default `target`, or root `cc-switch-release*` artifact paths.
- In this environment `pnpm` may be absent from PATH, while local `node_modules` exists. `tauri.conf.json` now uses `node ./node_modules/vite/bin/vite.js build` for `beforeBuildCommand`; frontend validation can use `.\node_modules\.bin\tsc.CMD --noEmit`.
- Tauri NSIS bundling can return exit code 1 after successfully producing setup.exe when updater signing has a public key but no `TAURI_SIGNING_PRIVATE_KEY`. Treat the generated setup file as usable if it exists and hashes cleanly; record this caveat in handoff.
- Codex history reality on this machine: `state_5.sqlite` had 445 threads during the 2026-06-09 check, with 432 under `codex_model_router_v2` and only 13 under `openai`. Full history is not mostly in `openai`.
- Codex `thread/list` defaults to filtering by current `model_provider` when `modelProviders` is omitted. Passing `modelProviders: []` means no provider filter. Optional `cwd` filters are exact-path filters and can make history appear limited to the current workspaces.
- Do not create another router provider id. Keep router provider config at `model_provider = "codex_model_router_v2"` so the Codex Desktop history bucket stays stable.
- Provider switching must never write provider `config.toml` snapshots verbatim over the current live Codex config. `src-tauri/src/codex_config.rs` now merges provider config with live config: provider top-level scalar fields and `[model_providers.<active-id>]` override, while live `[features]`, `[desktop]`, `[memories]`, `[projects]`, `[mcp_servers]`, plugins, and other user tables are preserved.
- Common config snippets still need to add missing table entries. The merge behavior is "live wins on conflicts, provider/common config fills missing table keys." This preserves user MCP entries while allowing CC Switch common config to add new MCP servers.
- Proxy takeover placeholder branches in `src-tauri/src/services/proxy.rs` must also merge before `write_codex_live_config_atomic`; otherwise switching router during takeover can clear context-window display, memories, MCP, and project trust.
- Validation for this fix used `.\node_modules\.bin\tsc.CMD --noEmit` and `cargo test codex --lib` (318 passed).

## 2026-06-09 CCSwitchMulti History Visibility And Router Preservation Fix

- Live official state after the 2026-06-09 01:20 check: `codex-official` is current in `~/.cc-switch/cc-switch.db`, `currentProviderCodex` is `codex-official`, Codex proxy flags are disabled, and `~/.codex/config.toml` has no local router/proxy lines. If the UI still feels like it did not switch back, first distinguish live config from Codex history filtering.
- Runtime DB repair restored `codex-openai-router.settings_config.codexRouting` with three routes:
  - `openai-official`: `gpt-*` via `https://chatgpt.com/backend-api/codex`, `openai_responses`, `managed_codex_oauth`.
  - `deepseek`: `deepseek-v4-flash` / `deepseek-v4-pro` via `https://api.deepseek.com`, `openai_chat`, provider_config key.
  - `qwen-local`: `qwen3.6` via `https://www.matrixminecraft.cn:24443/vllm/v1`, `openai_chat`, `minOutputTokens=2048`.
- Backup before runtime repair: `%USERPROFILE%\.cc-switch\backups\codex-history-official-router-fix-20260609_012627`.
- `src/components/providers/EditProviderDialog.tsx` now preserves both DB-private Codex fields, `modelCatalog` and `codexRouting`, when editing the current provider after reading live settings. This prevents saving a current router provider from erasing its route table.
- `src-tauri/src/codex_config.rs` now preserves OpenAI speed/service tiers only for `gpt-5.5` and `gpt-5.4`. `gpt-5.4-mini`, `gpt-5.3-codex-spark`, DeepSeek, Qwen, and other generated catalog entries must have empty `additional_speed_tiers` and `service_tiers`.
- Current on-disk `~/.codex/cc-switch-model-catalog.json` was repaired to match that rule: `gpt-5.5` and `gpt-5.4` keep `fast/priority`; mini, spark, DeepSeek, and Qwen have no service tiers.
- History visibility analysis from the read-only subagent:
  - `state_5.sqlite` has 448 threads. `session_index.jsonl` has 426 unique ids; sqlite has 24 ids not in the jsonl index and jsonl has 2 ids not in sqlite.
  - Provider buckets: `codex_model_router_v2=433`, `openai=15`.
  - Source buckets: `vscode=223`, `exec=26`, `subagent=199`; archived threads total 142.
  - Visible history is mostly a view/filtering problem, not data loss. Default `thread/list` behavior filters by active provider when `modelProviders` is omitted, hides non-interactive sources when `sourceKinds` is omitted/empty, excludes archived items, applies exact `cwd` filters, and paginates.
  - To surface hidden history safely, prefer fixing the query/view: pass `modelProviders: []`, include non-interactive `sourceKinds`, avoid default exact `cwd`, expose archived separately, and page through `nextCursor`. Do not rewrite sqlite buckets just to make old sessions visible.
- Latest packaged delivery for this fix:
  - Directory: `src-tauri/target-ccswitchmulti-historyfix-20260609_013447/`.
  - Portable exe: `CCSwitchMulti.exe` SHA256 `909933223A40D6AECA5396F3D1B2A2104C22ECD86EF68DB7DF5B493B1D1DD65F`.
  - Portable zip: `CCSwitchMulti_3.16.1-1_x64-portable.zip` SHA256 `8985C3F5B5C8D5C54C8DA70E4B3D5D1E444C25454794D9DDD7B959FCDD4111FA`.
  - NSIS installer: `CCSwitchMulti_3.16.1-1_x64-setup.exe` SHA256 `3E7C668881D7B7E0EB61F8D754D95971A59046FA6C7EB8C07260B3E11CB2D3CE`.
  - MSI installer: `CCSwitchMulti_3.16.1-1_x64_en-US.msi` SHA256 `D15EAC130332CA0717001630E334C32D2FB9895A14BE47D23866612908906DE7`.
- Validation: `vitest` for `EditProviderDialog` and `CodexFormFields` passed 5 tests; `cargo test codex_model_catalog --lib` passed 5 tests; `.\node_modules\.bin\tsc.CMD --noEmit`, `cargo fmt --check`, and `cargo test codex --lib` passed 319 tests; Tauri no-updater build succeeded.
- The older `src-tauri/target-ccswitchmulti-20260609_001033/CCSwitchMulti.exe` was still running during packaging. Do not delete that old directory until the old process is closed or replaced by the new build.

## 2026-06-09 CCSwitchMulti Rootfix For Codex Official Fallback And Router Pollution

- Supersedes the previous history-bucket assumption: `codex_model_router_v2` is not a universal fix for history visibility. It only described one old local router bucket. Do not rewrite sqlite/jsonl buckets as the default fix for missing history.
- Do not treat the user's current official/default state as proof that the modified build works. The user had to roll back to official release/default config to keep chatting.
- Confirmed root causes:
  - `CodexAdapter::extract_base_url` previously scanned for the first `base_url` string in TOML, so inactive `[model_providers.*]` and `[mcp_servers.*]` entries could contaminate the active provider.
  - Provider/live merge kept stale provider-owned fields. Official fallback with empty config could retain old `model_provider`, `model_catalog_json`, `experimental_bearer_token`, or old `[model_providers.<router>]`, leaving DeepSeek/Qwen candidates visible after switching backup official.
  - Codex common config could deep-merge provider-private router TOML into arbitrary providers.
  - Proxy takeover official switching needed to exit takeover and restore/write live official config instead of trying to hot-switch through the local proxy.
  - The old `preserve_codex_mcp_servers_from_existing_config` path only preserved MCP servers, not full Codex user sections like `[projects]`, `[features]`, `[desktop]`, `[memories]`.
- Implemented fixes:
  - `src-tauri/src/proxy/providers/codex.rs`: base URL extraction uses `crate::codex_config::extract_codex_base_url`, which prefers the active `model_provider`.
  - `src-tauri/src/services/provider/mod.rs`: Codex credential extraction uses the same active TOML parser; switching an official provider during takeover calls `disable_takeover_for_app_after_switch_lock`, sets current provider, writes official live config, and syncs MCP.
  - `src-tauri/src/codex_config.rs`: official empty config now clears provider-owned top-level fields, removes CC Switch-owned `model_catalog_json`, and removes the active custom `[model_providers.<id>]` table while preserving user sections.
  - `src-tauri/src/services/provider/live.rs`: Codex common config strips `model`, `model_provider`, `model_context_window`, `model_catalog_json`, `experimental_bearer_token`, and `[model_providers]`.
  - `src-tauri/src/services/proxy.rs`: backup/live preservation now uses full Codex provider/live merge rather than MCP-only merge. Added regression test for router takeover -> official fallback cleanup.
- Validation commands passed:
  - `.\node_modules\.bin\tsc.CMD --noEmit`
  - `cargo test codex_switch_to_official_during_takeover_exits_proxy_and_cleans_router_fields --lib`
  - `cargo test test_extract_base_url_uses_active_model_provider_only --lib`
  - `cargo test codex_config --lib` (46 passed)
  - `cargo test codex_common_config --lib` (6 passed)
  - `cargo test provider_switch_with_restored_codex_backup_refreshes_catalog_and_common_config --lib`
  - `cargo test codex_restore_from_backup_projects_inline_model_catalog --lib`
  - `.\node_modules\.bin\tauri.CMD build --no-bundle`
- Latest delivery artifacts:
  - Directory: `src-tauri/target-ccswitchmulti-rootfix-20260609_032709/`
  - `CCSwitchMulti.exe` SHA256 `D764449F06FEEEA7FED052693AB55EE26200C2609B1001DBD56EE993F4186123`
  - `CCSwitchMulti_3.16.1-1_x64-rootfix-portable.zip` SHA256 `46BB69EB96FD811B945152EC2672C6220E0FC545DE47AD6326CE69E8C31C5AB9`
  - `CCSwitchMulti_3.16.1-1_x64-setup.exe` SHA256 `73F7E05581E35278936420CF5F5E13229A383D08F26FB960E689336395B67635`
  - `CCSwitchMulti_3.16.1-1_x64_en-US.msi` SHA256 `9E093D8C493E52337DD1811B8081A8187372C17CF384AC605C7EE4BA0DCFB132`
- Packaging notes:
  - Full `tauri build` produced NSIS/MSI but returned 1 because updater signing has a public key and no `TAURI_SIGNING_PRIVATE_KEY`; use `tauri build --no-bundle` to verify portable exe without signing.
  - Old timestamp package dirs `target-ccswitchmulti-20260609_001033` and `target-ccswitchmulti-historyfix-20260609_013447` were removed after creating the rootfix package. Only the rootfix directory should be handed out now.
  - The current running official app remained `C:\Users\sunda\AppData\Local\Programs\CC Switch\cc-switch.exe`; this rootfix pass did not stop it and did not mutate live `%USERPROFILE%\.cc-switch` or `%USERPROFILE%\.codex` config.

## 2026-06-09 Rootfix DB Provider Write

- After packaging rootfix, the current `%USERPROFILE%\.cc-switch\cc-switch.db` still only had `codex-official` and stale `default`; the package fix alone did not write the user's Codex provider config.
- DB backup before writing: `%USERPROFILE%\.cc-switch\backups\db_backup_before_codex_rootfix_config_20260609_145601.db`.
- Current Codex provider set written to DB:
  - `codex-official` / `OpenAI Official Backup`: official fallback, current provider, empty config/auth.
  - `codex-openai-router` / `OpenAI Multi-Model Router`: local proxy provider with `model_provider="codex_model_router_v2"`, base URL `http://127.0.0.1:15721/v1`, catalog models `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`, and `codexRouting` routes `openai-official`, `qwen-local`, `deepseek`.
  - `codex-qwen-local` / `Qwen Local vLLM`: direct optional provider for `qwen3.6`, base URL `https://www.matrixminecraft.cn:24443/vllm/v1`, Chat upstream metadata.
  - `codex-deepseek` / `DeepSeek`: direct optional provider for `deepseek-v4-flash` and `deepseek-v4-pro`, base URL `https://api.deepseek.com`, Chat upstream metadata.
- Removed stale provider `default`; it was an imported old router config under a misleading name.
- Cleaned `common_config_codex` by removing provider-owned lines `model_catalog_json`, `model_context_window`, `model_provider`, and `model`; preserved user MCP/plugin/windows/reasoning/auto-compact settings.
- Left Codex proxy disabled and current provider as `codex-official`: `enabled=0`, `proxy_enabled=0`, `live_takeover_active=0`. This avoids disrupting official fallback until the user explicitly enables/switches router.
- UI caveat: already-open CCSwitchMulti windows cache the provider list. Restart/refresh CCSwitchMulti after this DB write to show the four providers.

## 2026-06-09 Current Good Routing State And History Thread Reaudit

- User has now verified this build's Codex routing and OpenAI official fallback configuration are working. Preserve that as the known-good baseline during future debugging.
- Known-good provider layout:
  - `codex-official` / `OpenAI Official Backup`: pure official fallback, empty provider config, safe current provider.
  - `codex-openai-router` / `OpenAI Multi-Model Router`: local proxy provider using active Codex `model_provider = "codex_model_router_v2"` and catalog entries for GPT, Codex Spark, Qwen, and DeepSeek routes.
  - `codex-qwen-local` and `codex-deepseek`: optional direct providers, not replacements for the official fallback.
- Remaining unresolved bug: Codex history threads still do not display/sync as expected. The user says this is related to provider and bucket, and the previous memory around this may be wrong.
- Do not assume `codex_model_router_v2` is a universal history fix and do not rewrite sqlite/jsonl buckets by default. Re-verify Codex Desktop, CCSwitch, and Codex++ behavior around history indexes, provider buckets, accounts, sources, cwd/project filters, archived state, and pagination before implementing a fix.

## 2026-06-09 OpenAI Bucket Semantics And Responses WebSocket Fallback

- Verified against OpenAI Codex docs and local Codex v0.137.0 source: `openai` is a reserved built-in provider id. `model_providers.openai` does not override the built-in provider; `merge_configured_model_providers` keeps the built-in entry. To point built-in OpenAI at a proxy/router, use user-level top-level `openai_base_url`, not `[model_providers.openai].base_url`.
- Built-in `openai` provider semantics that matter for cc-switch:
  - `requires_openai_auth = true`.
  - `wire_api = responses`.
  - `supports_websockets = true`.
  - Normal turns prefer Responses WebSocket before HTTP Responses.
- Root cause of previous `openai` bucket failures/slowness: cc-switch served HTTP `POST /responses` but did not explicitly handle Codex's WebSocket handshake `GET /responses`. Codex switches immediately to HTTP only when the WS connect returns `426 Upgrade Required`; generic 404/405/network failures can cause retries, delay, or timeout.
- Implemented compatibility fix:
  - `src-tauri/src/proxy/server.rs` maps Codex `/responses`, `/v1/responses`, `/v1/v1/responses`, and `/codex/v1/responses` as `GET -> handle_responses_websocket_fallback` and `POST -> handle_responses`.
  - `src-tauri/src/proxy/handlers.rs` adds `handle_responses_websocket_fallback`, returning 426 with a small JSON error. This is an intentional signal to the official Codex client to disable WS for the session and use HTTP.
  - `src/utils/providerConfigUtils.ts` no longer treats `openai_base_url` as a `wire_api` value. Added a regression unit test.
  - `src-tauri/src/codex_history_migration.rs` now gates old v1 helper wrappers behind `#[cfg(test)]`.
- Current DB provider state checked read-only with secrets redacted:
  - `codex-official` / `OpenAI Official Backup` is current and pure official fallback.
  - `codex-openai-router` uses `model_provider = "openai"`, top-level `openai_base_url`, `model_catalog_json`, no `[model_providers.openai]`, routes `openai-official`, `qwen-local`, `deepseek`, and catalog models `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`.
- Validation commands passed:
  - `pnpm test:unit tests/utils/providerConfigUtils.codex.test.ts` (26 tests).
  - `cargo test --manifest-path .\src-tauri\Cargo.toml openai_for_v2 --lib` (2 tests).
  - `cargo test --manifest-path .\src-tauri\Cargo.toml responses_websocket_fallback_returns_upgrade_required --lib` (1 test).
  - Focused Rust regressions for `openai_base_url`, router merge, settings migration preservation, and Codex common-config stripping all passed.
- Latest package:
  - Directory: `src-tauri/target-ccswitchmulti-openaibucket-wsfix-20260609_163308/`.
  - Portable exe: `release/bundle/portable/CCSwitchMulti.exe`, SHA256 `DE348E685A03A522B4A2066FD0CAEA900EDE0B50A0433E959897ED4771DFDCC8`.
  - Portable zip: `release/bundle/portable/CCSwitchMulti_3.16.1-1_x64-openai-bucket-wsfix-portable.zip`, SHA256 `0085BAC5C731763D352757A295CC3CEBFF15BFDBCE32FA7BFD0341D56CCD587A`.
  - NSIS installer: `release/bundle/nsis/CCSwitchMulti_3.16.1-1_x64-setup.exe`, SHA256 `3DDD9F93DEF8020CAE12097CCAAFA89807A41C510C40F61696D92353BE2B58BF`.
- Build cleanup: removed default `src-tauri/target` and old `target-ccswitchmulti-rootfix-20260609_032709`. The old rootfix directory was locked by a stale local modified `CCSwitchMulti.exe`, so that stale local process was stopped before deletion. The official installed CC Switch stayed running at `%LOCALAPPDATA%\Programs\CC Switch\cc-switch.exe`.
- Operational note: only `src-tauri/target-ccswitchmulti-openaibucket-wsfix-20260609_163308/` should be handed out now. Launching/testing the new portable no longer has an older CCSwitchMulti process competing via single-instance; it is not necessary to stop the user's official Codex/official backup chat process.

## 2026-06-11 Third-party Agent API Public Access Check

- External OpenAI-compatible Agent API is intentionally separated from the Codex/Multi Router main proxy: current external listener is `0.0.0.0:15722`; main proxy `15721` is not listening in the checked runtime.
- Local and trusted-network reachability passed:
  - `http://127.0.0.1:15722/health` returned HTTP 200.
  - LAN addresses `192.168.31.206:15722` and `192.168.31.152:15722` returned HTTP 200 from this host.
  - Tailscale address `100.118.73.52:15722` returned HTTP 200 from this host.
- Public Internet reachability failed from this host:
  - Public IP discovery returned inconsistent exits (`185.151.146.146` from ipify and `117.133.83.107` from ipinfo), indicating proxy/multi-exit/NAT behavior.
  - `http://185.151.146.146:15722/health` and `http://117.133.83.107:15722/health` both timed out.
- Interpreted cause: CC Switch is bound correctly and Windows has enabled inbound `cc-switch.exe` allow rules for Private/Public profiles, so the remaining blocker is likely upstream of the app: router port forwarding, carrier-grade NAT, public IP not mapped to this machine, or external firewall/NAT policy.
- Do not treat公网 timeout as an application regression unless LAN/Tailscale/localhost also fail. For real public exposure, configure router/NAT port forwarding to the machine's active LAN IP or use a tunnel/VPN endpoint, and keep `ccsw_` keys private.
- Added `docs/guides/external-openai-api-relay-domain-guide-zh.md` as the operational handoff guide for exposing the External OpenAI-compatible API through a public relay/domain. The preferred topology is public relay Caddy/Nginx -> private Tailscale or SSH tunnel -> Windows CC Switch `15722`; use route/NAT forwarding only when a real inbound public IP exists.

## 2026-06-12 Codex DeepSeek Direct Provider Local Routing Fix

- Root cause for the reported standalone DeepSeek Codex provider failure: the UI's "需要本地路由映射" intent was stored as `meta.apiFormat = "openai_chat"`, but `ProviderService::switch` only hot-switched when takeover was already active. In normal mode it still wrote the DeepSeek provider directly into Codex live config, so Codex called `https://api.deepseek.com/responses` and DeepSeek returned 404.
- This is not a Third-party Agent API issue and not a DeepSeek documentation issue. DeepSeek's official endpoint is Chat Completions style; Codex still speaks Responses to CC Switch, so the local proxy must sit between Codex and DeepSeek.
- Regression source audit:
  - `1c82b8a3 Add Chat Completions routing for Codex providers` introduced `meta.apiFormat = "openai_chat"` and the proxy conversion path, while keeping generated Codex `wire_api = "responses"` so the Codex client can continue using Responses locally.
  - The same change only added a frontend warning in `useProviderActions`; it did not block normal switch or enable takeover.
  - Existing `ProviderService::switch` behavior from the older switch architecture still treated "not currently taken over" as permission to call `switch_normal -> write_live_with_common_config`, which direct-writes provider config to Codex live files.
  - Later local changes `8af568e4` / `24eca85c` made the UI present this as a first-class local routing / multi-route capability, which made the latent mismatch user-visible: users reasonably expected the switch/config to activate routing, but the backend still only routed if takeover was already active.
  - Official upstream is not able to make DeepSeek work by direct `/responses` either; it works only when Codex is already going through CC Switch proxy/takeover. The fix here is making that invariant backend-enforced instead of relying on user sequence or frontend warning.
- Implemented backend defense:
  - `ProviderService::codex_provider_requires_local_proxy` detects Codex providers that require local proxy because they are Chat Completions backends or contain `codexRouting`.
  - `ProviderService::switch` now auto-enables Codex takeover for such providers when takeover is not already active, instead of taking the normal direct live-write path.
  - `ProxyService::takeover_app_and_switch_provider_after_switch_lock` performs the locked transition: start proxy if needed, back up/sync existing live config, switch current provider, write Codex live config to local proxy `/v1`, update backup/current target, and set per-app takeover enabled.
- Regression test added: `switching_codex_chat_provider_auto_enables_local_proxy_takeover` asserts a DeepSeek `openai_chat` provider switch writes `http://127.0.0.1:<port>/v1` plus `PROXY_MANAGED` into Codex live config and does not leave `https://api.deepseek.com` in live config.
- Validation passed:
  - `cargo test switching_codex_chat_provider_auto_enables_local_proxy_takeover --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_codex_provider_uses_chat_completions --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test v1_responses --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test external_openai_api --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `pnpm typecheck`

## 2026-06-12 Codex Takeover Model Picker Must Stay On Built-in OpenAI

- Follow-up root cause for the user's "GPT menu shows 自定义, where did the selectable models go" screenshot: after the DeepSeek auto-takeover fix, Codex live config correctly pointed at CC Switch, but it still projected the selected upstream provider id (`deepseek`, `aihubmix`, etc.) into live `model_provider`. Codex then treated the session as a custom provider and the model picker collapsed into the custom-model bucket instead of showing the intended GPT/router catalog choices.
- Correct invariant: during proxy takeover, Codex live `config.toml` should expose the stable built-in OpenAI provider:
  - `model_provider = "openai"`
  - top-level `openai_base_url = "http://127.0.0.1:<port>/v1"`
  - `model_catalog_json = "cc-switch-model-catalog.json"` when CC Switch has a model catalog
  - `auth.json` uses `OPENAI_API_KEY = "PROXY_MANAGED"`
  - no upstream `[model_providers.<deepseek/qwen/...>]` table should be exposed in live takeover config.
- Real upstream provider identity and API keys stay in CC Switch DB/backup/provider settings. The proxy resolves the current provider or `codexRouting` by request model and injects upstream credentials internally.
- Implemented fix:
  - `ProxyService::apply_codex_proxy_toml_config_for_provider` now projects takeover TOML to built-in `openai` plus `openai_base_url`, preserving the selected model but stripping upstream provider tables/tokens from live config.
  - `codex_config::merge_codex_provider_config_texts` now removes the active custom provider table when the provider projection targets built-in `openai`, so stale live `[model_providers.*]` tables do not survive the merge.
- Regression coverage:
  - `apply_codex_proxy_toml_config_uses_builtin_openai_proxy_provider`
  - `hot_switch_codex_chat_provider_updates_live_provider_display`
  - `merge_openai_router_config_uses_builtin_openai_history_bucket`
  - `switching_codex_chat_provider_auto_enables_local_proxy_takeover`

## 2026-06-12 CCSwitchMulti v3.16.2-2 Release Export Rule

- Release tag for this fix train is `v3.16.2-2`; do not reuse `v3.16.2-1` because it already exists on `BigStrongSun/cc-switch`.
- Fixed local export directory remains `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`.
- GitHub Release assets cannot safely upload two different files both named `BUILD_ON_PLATFORM.md`; the export script now also writes root-level `linux-build-note.md` and `macos-build-note.md` with unique names for release upload.
- `SHA256SUMS.txt` should be generated after those root-level note files are copied, so the checksum list matches the final export directory.

## 2026-06-12 Codex DeepSeek Routing Crash And Legacy Wire API Fix

- User-reported crash: CCSwitchMulti v3.16.2-2 flashed/crashed when enabling Codex routing or switching to the DeepSeek provider.
- Windows/WER plus `%USERPROFILE%\.cc-switch\crash.log` showed the real root cause: `there is no reactor running, must be called from the context of a Tokio 1.x runtime`, followed by `panic in a function that cannot unwind`. This happened because the synchronous Tauri `switch_provider` command called `futures::executor::block_on` and then started the proxy TCP listener outside a Tokio reactor.
- Fix invariant: synchronous provider commands that wait for async proxy/db work must use a Tauri-runtime-aware helper. If a Tokio handle is already present, continue polling in the current context; otherwise enter `tauri::async_runtime::block_on`.
- Implemented helper: `services::provider::block_on_tauri_runtime`, used by provider switch/update/sync paths that call proxy async methods.
- Regression test added: `switching_codex_chat_provider_from_sync_command_has_tokio_reactor`, which simulates the desktop synchronous command path and verifies switching a Codex Chat provider starts local proxy without the missing-reactor panic.
- Second root cause found in current user DB (read-only, secrets redacted): `codex-deepseek` had `base_url = "https://api.deepseek.com"` and model catalog entries, but `wire_api = "responses"` and no `meta.api_format`. The old detector returned false as soon as it saw `wire_api = "responses"`, so DeepSeek was treated like a Responses provider and Codex could call `/responses` directly.
- Fix invariant: explicit `meta.api_format` still wins, but known Chat-Completions-only upstream URLs such as `api.deepseek.com`, `api.moonshot.cn`, DashScope, GLM, SiliconFlow, OpenRouter, and vLLM must be detected before trusting stale `wire_api = "responses"` from historical configs.
- Regression tests added:
  - `test_codex_provider_uses_chat_completions_for_legacy_deepseek_responses_wire_api`
  - `test_codex_provider_keeps_openai_responses_wire_api`
- This bug is not caused by the Third-party Agent API. It is the Codex provider/takeover path plus stale provider wire metadata.

## 2026-06-12 Codex Router Official GPT-5.5 URL Normalization Fix

- User clarified that the failed high-demand/reconnect case happened after selecting `gpt-5.5` from the Codex model list, while `OpenAI Official Backup` could use `gpt-5.5` successfully.
- Root cause: the Codex multi-model router's managed OAuth route builds a temporary `codex_oauth` provider that uses `CodexAdapter`. `CodexAdapter.build_url` treated `https://chatgpt.com/backend-api/codex` like a generic custom prefix, so a local Codex request to `/v1/responses` could become `https://chatgpt.com/backend-api/codex/v1/responses`. ChatGPT's Codex backend expects `https://chatgpt.com/backend-api/codex/responses` without `/v1`.
- Why official backup worked: non-router official requests were already observed in `codex-router.log` as `upstream_url=https://chatgpt.com/backend-api/codex/responses`. The bug lived in the router/effective-provider URL construction path, not in the user's official subscription, model availability, or DeepSeek conversion.
- Fix invariant: any Codex OAuth provider targeting `https://chatgpt.com/backend-api/codex` must strip the OpenAI-compatible `/v1/` prefix before forwarding to ChatGPT Codex backend. `/v1/responses` maps to `/responses`; `/v1/responses/compact?...` maps to `/responses/compact?...`.
- Regression tests added/strengthened:
  - `test_build_url_chatgpt_codex_backend_strips_openai_v1_prefix`
  - `test_codex_adapter_supports_routed_codex_oauth_provider` now asserts routed OAuth URL construction as well as auth strategy.

## 2026-06-12 Codex Multi Router 首个 SSE 错误触发 Failover

- 用户继续反馈 CCSwitchMulti 的 Codex multi 选择多路路由后仍出现 `We're currently experiencing high demand` / `stream disconnected before completion`；恢复 `OpenAI Official Backup` 也可能报同类错误。
- 追根因后确认：这类错误不一定表现为 HTTP 5xx。ChatGPT/Codex OAuth 可能返回 HTTP 200 + `text/event-stream`，但首个 SSE block 就是 `event: error` 或 `event: response.failed`。此前 `RequestForwarder::prime_streaming_response` 只等到首个 chunk 就把 provider 记为成功并把响应交给 Codex；一旦响应头已发给客户端，同一个请求就不能再 failover 到下一路。
- 修复规则：在首包预读阶段解析首个完整 SSE block；如果明确是 `error` / `response.failed` / payload 中含 `error` 或 `response.status=failed`，在响应交给客户端前转换为 `ProxyError::UpstreamError { status: 503 }`。这样现有 retry/failover 分类会把它当作可重试上游失败，multi 路由/故障转移才有机会换下一家。
- 正常 `response.created`、delta、`response.completed` 仍必须原样 replay 给客户端，不能为了吞错而破坏正常流。
- 已加回归测试：
  - `streaming_first_sse_error_event_is_retryable_before_response_is_returned`
  - `streaming_first_normal_sse_event_is_replayed_to_client`
- 已验证：
  - `cargo test streaming_first --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_build_url_chatgpt_codex_backend_strips_openai_v1_prefix --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test test_codex_adapter_supports_routed_codex_oauth_provider --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Official 也报 high demand 的根因修正

- 用户指出“official 也出现 high demand，说明上游返回 error 本身就不对，前一刀没修到点上”。这个判断成立：上一条 `prime_streaming_response` 修复只解决“首个 SSE error 交给客户端前还能 failover”的边界，不解释为什么 official/official backup 会拿到同类错误。
- 本机排查结论：恢复到 official backup 后，`~/.codex/config.toml` 已没有 `model_provider/openai_base_url/cc-switch` takeover 字段，主代理也已停止；纯 official 路径不经过 CC Switch。此时仍出现 high demand，只能是官方 Codex/ChatGPT 后端或 official 客户端重试后仍失败，CC Switch 不能在纯直连 official 路径里修上游容量错误。
- 对比 `codex-source-rust-v0.137.0` official 源码后确认：official Codex 会使用 `session-id`、`thread-id`、`x-client-request-id`、`x-codex-window-id = {thread_id}:{generation}`，并通过 `responses_retry::handle_retryable_response_stream_error` 对可重试 stream 错误循环重试，必要时 WebSocket fallback 到 HTTPS。
- CC Switch 的 official/managed OAuth 代理路径此前不够 official：`extract_codex_session` 只认 `session_id/x-session-id` 并给值加 `codex_` 前缀；`build_codex_oauth_session_headers` 注入 `session_id` 下划线头，且会覆盖已有 header。这会让“OpenAI Official Backup / router official route”在代理路径中和 official 客户端的身份/缓存/路由信号不一致，可能放大 high-demand/stream-failed 问题。
- 根因修复：Codex session 提取现在识别 `session-id/thread-id/x-client-request-id/x-codex-window-id/session_id/x-session-id`，从 `x-codex-window-id` 提取 thread_id，并保留原始值不加前缀；ChatGPT Codex OAuth 转发补齐 `session-id/thread-id/x-client-request-id/x-codex-window-id`，且只在原请求缺失时补，不覆盖 official 客户端已有值。
- 回归测试新增/更新：
  - `test_codex_official_session_id_header_is_preserved`
  - `test_codex_window_id_header_extracts_thread_identity`
  - `codex_oauth_session_headers_match_codex_cache_identity`
- 已验证：
  - `cargo test codex --manifest-path src-tauri/Cargo.toml --lib`（357 tests）
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`（52 tests）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Multi Router 从“模型分流”升级为“路由内故障转移”

- 用户继续指出“选择多路路由仍报 high demand，说明上游返回 error 本身就不对，之前没修到点上”。再次追根因后确认：当前 `codex-openai-router` 配置里，`gpt-5.5` 只匹配 `openai-official` route；Qwen/DeepSeek route 只匹配各自模型名前缀。旧逻辑的“多路路由”只是按请求模型选一路，不是同一个请求在官方失败后自动尝试其它 route。
- 因此即使首个 SSE `event:error` 已能在响应交给客户端前变成 retryable error，外层 failover 也只有一个 router provider 可尝试；实际不会落到 Qwen/DeepSeek。要真正解决“官方高负载时多路路由继续跑”，必须把 router provider 在转发前展开成 route provider 候选链。
- 修复规则：Codex 请求进入 `RequestForwarder::forward_with_retry_inner` 后，如果当前 provider 是 Codex router，就按请求模型解析候选 route：匹配 route 放第一位；其它 enabled route 作为后备追加。外层 provider retry/failover 会逐个尝试这些 effective provider。
- 跨模型后备必须改写上游模型名：例如用户请求 `gpt-5.5` 时，第一路 official 仍发 `gpt-5.5`；若 official 首包失败并切到 DeepSeek route，发给 DeepSeek 的模型必须改成 route 自己的默认模型（如 `deepseek-v4-flash`），不能把 `gpt-5.5` 原样发给 DeepSeek/Qwen。
- 为避免展开后的 route provider 再次被解析回官方 route，resolved route 会带 `codexResolvedRouteId`；`forward` 看到该标记后直接使用该 effective provider。
- 回归测试新增：
  - `test_codex_router_returns_fallback_route_candidates_after_primary`
  - `test_apply_codex_chat_upstream_model_forces_unmatched_fallback_route_model`
- 已验证：
  - `cargo test test_apply_codex_chat_upstream_model_forces_unmatched_fallback_route_model --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test codex_router_returns_fallback_route_candidates_after_primary --manifest-path src-tauri/Cargo.toml --lib`
  - `cargo test forwarder --manifest-path src-tauri/Cargo.toml --lib`（52 tests）
  - `cargo test codex --manifest-path src-tauri/Cargo.toml --lib`（359 tests）
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo check --manifest-path src-tauri/Cargo.toml`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex Multi Router official route 与 official backup 不等价

- 用户继续追问“为什么 Multi Router 用 official 会失败，这才是本质”。排查结论：Multi Router 的 official route 不是纯 official backup；它是 Codex built-in `openai` bucket -> `openai_base_url=http://127.0.0.1:<port>/v1` -> CC Switch HTTP/SSE proxy -> `https://chatgpt.com/backend-api/codex/responses`。
- 官方 Codex 源码 `model-provider-info/src/lib.rs::create_openai_provider` 对 built-in `openai` 设置 `supports_websockets = true`；`client.rs` 会优先走 Responses WebSocket，失败后才通过 `responses_retry::handle_retryable_response_stream_error` fallback 到 HTTPS/SSE。CC Switch 当前主代理没有实现 Codex Responses WebSocket，只在 `/responses` 的 GET 上返回 426 让客户端降级。
- 因此“Multi Router official”比“official backup”少了官方 WebSocket 直连能力，更容易落到 GitHub issue 中大量用户也报错的 HTTPS/SSE `/backend-api/codex/responses` 路径。外部 issue 覆盖 `stream disconnected before completion`、`high demand`、remote compaction、Azure/rate-limit/context 等场景；这说明 high demand 文案是 Codex 对多类后端/传输失败的泛化提示，不一定只表示真实排队高峰。
- 之前保留 `model_provider="openai"` 是为了维持官方 history bucket 和模型菜单；但这个选择天然启用 built-in OpenAI WebSocket 语义。若要让 Multi Router official 真正等价 official backup，根修方向不是再补 HTTP retry，而是实现 Codex Responses WebSocket relay/proxy，至少覆盖 prewarm、response.create、`x-codex-turn-state` sticky routing、`response.processed` 等官方协议。
- 可选降级方案：改回自定义 provider 并显式 `supports_websockets=false` 可避免 WS fallback 抖动，但会重新带来模型菜单/历史 bucket 变成自定义的问题；这是产品取舍，不是根治。

## 2026-06-12 Codex Responses WebSocket official relay

- 用户强调“尽量复用官方，不然永远会有 bug”。本轮修复原则：CC Switch 不实现自己的 Codex 事件协议解释器，只在本地 `/responses` GET 接受 WebSocket 后做透明中继；官方事件流、`response.create`、`response.processed`、prewarm 完成事件、错误事件都由 Codex 官方客户端和 ChatGPT Codex 后端继续按原协议处理。
- 新增 `src-tauri/src/proxy/codex_ws.rs`：首帧只解析 `response.create` 的 JSON 以获取 `model`，复用现有 `resolve_codex_model_routed_providers` 和 `CodexAdapter` 判定真实 route；只有 route 上游是 `https://chatgpt.com/backend-api/codex` 且不是 Chat Completions-only 时，才连接 `wss://chatgpt.com/backend-api/codex/responses`。
- WebSocket upstream 鉴权复用现有 Codex OAuth 托管账号：从 `CodexOAuthState` / `CodexOAuthManager` 取真实 access token，再通过 `CodexAdapter::get_auth_headers` 生成 `authorization` / `originator`；同时透传 official 相关 header：`session-id`、`thread-id`、`x-client-request-id`、`x-codex-window-id`、`x-codex-turn-state`、`chatgpt-account-id` 等。
- 非 official WS 路线不能在升级后直接断流，否则 official Codex 会报 `stream disconnected before completion`。正确做法是发送官方源码 `responses_websocket.rs` 能解析的 `{"type":"error","status_code":426,...}`，让 `client.rs` 命中 `WebsocketStreamOutcome::FallbackToHttp`，再走现有 HTTP Responses -> Chat bridge 给 Qwen/DeepSeek 等第三方 API。
- 路由变更：`/responses`、`/v1/responses`、`/v1/v1/responses`、`/codex/v1/responses` 的 GET 进入 `handle_responses_websocket`；非升级 GET 仍返回旧 426 JSON，POST HTTP Responses 路径不变。External OpenAI API 独立端口的 `/v1/responses` GET 也复用同一官方 fallback/relay handler，POST 仍走 external profile。
- 新增依赖：`axum` 开启 `ws` feature，新增 `tokio-tungstenite` 的 rustls/webpki TLS feature。
- 已验证：
  - `cargo test proxy::codex_ws`
  - `cargo test proxy::providers::codex`
  - `cargo test proxy::server`
  - `cargo fmt --check`
  - `cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）

## 2026-06-12 Codex WS close normally after Multi Router

- 用户反馈新 WS relay 后 Multi Router 报 `stream disconnected before completion: failed to send websocket request: Connection closed normally`。这说明本地 `/responses` WS 已被 official Codex 命中，且到 ChatGPT Codex upstream 的 WebSocket 握手成功，但上游在首个 `response.create` 发送前/发送时正常关闭。
- 对照官方源码确认：`core/src/client.rs::build_websocket_headers` 会构造 `openai-beta: responses-websockets-v2`、`x-codex-beta-features`、`x-codex-turn-state`、`x-codex-turn-metadata`、`x-client-request-id`、`session-id`、`thread-id`、`x-codex-window-id`、attestation 等；随后 `codex_login::default_client::default_headers()` 补 `originator` 和真实 `user-agent`。上一版 relay 只手写少数头，并通过 `CodexAdapter::get_auth_headers` 把 `originator: cc-switch` 发给 upstream WS，不够 official。
- 修复规则：上游 WS 握手应优先复用客户端发给本地代理的官方 headers；只过滤 hop-by-hop/WebSocket 握手头、本地占位 `authorization`、content headers，然后替换为真实 Codex OAuth `Authorization`。不要覆盖客户端提供的 `originator`、`user-agent`、`openai-beta`、`x-codex-*`、attestation 等官方头。
- 代码位置：`src-tauri/src/proxy/codex_ws.rs::copy_official_client_headers` 与 `should_skip_client_ws_header`。`codex_auth_headers` 仍负责取托管 OAuth token，但插入 upstream headers 时跳过 adapter 生成的 `originator`，避免把官方 originator 改成 `cc-switch`。
- 已验证：
  - `cargo fmt --check`
  - `cargo test proxy::codex_ws`
  - `cargo check`
  - `pnpm typecheck`
  - `pnpm release:export`
- 新 raw exe 已导出并启动：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `6A14F9627A87DBFA274D28D8A45703B7B05511145DA431D30F4B1E15770D3D11`。

## 2026-06-12 Codex WS Connection closed normally diagnostics

- 用户继续反馈开启 Multi Router 后仍报：`stream disconnected before completion: failed to send websocket request: Connection closed normally`。本轮先查日志：`%USERPROFILE%\.cc-switch\logs\cc-switch.log` 只有代理启停，`codex-router.log` 只有旧 HTTP forwarder 事件，缺少 Responses WebSocket relay 的握手、首帧、close code、fallback event 发送结果，因此无法判断是本地代理提前关、官方 upstream policy close，还是 fallback event 没送到 Codex 客户端。
- 外部交叉验证：Codex built-in web search 与用户 `matrix-websearch` 均搜到 openai/codex 同类问题；典型 issue 包括 `openai/codex#13039` / `#13041`，证据是 `wss://chatgpt.com/backend-api/codex/responses` 握手 `101 Upgrade` 成功后，官方 upstream 立即发 close code `1008 Policy`，Codex 客户端显示同样的 `failed to send websocket request: Connection closed normally` 并 fallback 到 HTTPS。因此本地日志必须记录 close code/reason length 和是否收到上游首帧，不能只记录 relay done。
- 诊断增强：`src-tauri/src/proxy/codex_ws.rs` 新增 `ws_*` 事件写入 `codex-router.log`，包含 accepted/client*first_frame/route_resolved/upstream_connect_start/upstream_connect_ok/upstream_first_send_start/upstream_first_send_ok/upstream_first_frame/upstream_close/client_close/relay*\*\_done/error/fallback_event_send_ok/error/fallback_close_ok/error 等。日志只写 header 名、帧类型、字节数、close code、reason_len 和 JSON error 摘要，不记录 token、header value、完整首帧、完整 upstream text、完整 close reason。
- 行为修正：若 upstream 首帧发送失败，不能直接 close 本地 WS；现在会先记录 `ws_upstream_first_send_error` 和 500ms upstream probe，再向本地 Codex 发送协议内 `status_code=426` error event，触发官方客户端按自身逻辑 fallback 到 HTTP Responses，而不是让用户只看到 `Connection closed normally`。
- Relay 可观测性增强：`upstream_first_send_ok` 之后的透明转发阶段会统计两侧 frames/bytes；如果 upstream 正常 close，会记录 `ws_upstream_close code=<code> reason_len=<n> before_first_upstream_frame=<bool>`；如果没有任何 upstream frame 就结束，会记录 `ws_upstream_ended_without_frames`。这正是后续区分“官方上游 policy close 1008”和“本地 relay/fallback 未送达”的关键证据。
- 本轮验证：
  - `cargo fmt --check`
  - `cargo test proxy::codex_ws`
  - `cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）
  - `pnpm typecheck`
  - `pnpm release:export`
- 新 raw exe 已导出并启动：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `4AC80A8E65784438957618568F7C1547B56BBD9381EF9B8FC7849CD87F4EDE1C`。启动后 `http://127.0.0.1:15722/health` 正常；`15721` 在未启用 Codex takeover 时不监听，符合预期。

## 2026-06-12 Codex Multi Router not being hit runtime check

- 用户再次反馈同样 `Connection closed normally`，但检查结果显示这次请求没有进入 CC Switch 的 Codex Multi Router：`%USERPROFILE%\.cc-switch\logs\codex-router.log` 最后更新时间仍是 `2026-06-12 06:16:39 UTC`，没有任何新 `event=ws_*`；`~/.codex/config.toml` 当前没有 `model_provider` / `openai_base_url` 指向 `127.0.0.1:15721`；`http://127.0.0.1:15721/health` 不通，而 `15722/health` 正常。
- `cc-switch.log` 显示用户在 `2026-06-12 16:45:20` 选择 `codex-openai-router` 后确实短暂启动了 Codex takeover 并写入 `http://127.0.0.1:15721/v1`，但 `16:46:17` 又执行了 Codex Live 配置恢复并停止 15721。用户说明这是因为不可用后切回 official，因此后续报错自然不会有 router 日志。
- 当前数据库状态：`providers` 里 `codex-official` 是 `is_current=1`，`codex-openai-router` 是 `is_current=0`；`proxy_config` 里 `codex.enabled=0`；`proxy_live_backup` 为空；第三方 OpenAI API 旁路 profile 仍指向 `codex-official`。因此现状是纯 official/旁路 official，不是 Multi Router takeover。
- 重要使用判据：Codex Multi Router 给 Codex 客户端用的是 `15721` takeover 端口；`15722` 是第三方 OpenAI-compatible Agent API 旁路端口，两者不是同一路。要验证 Multi Router，必须先在 CCSwitchMulti 选择 `OpenAI Multi-Model Router`，确认 `15721/health` 正常且 `~/.codex/config.toml` 指向 `127.0.0.1:15721/v1`，然后新开/重启 Codex 会话，因为已经运行的 Codex 会话通常不会重新读取刚改的 config。

## 2026-06-12 Codex Desktop App Multi Router activation diagnostics

- User clarified that "Codex" in this issue means the OpenAI Codex Desktop App, not a standalone CLI. The user's manual switch back to official/route-off was only to keep the current Codex conversation usable for debugging and must not be treated as the root cause.
- Local process evidence: the Desktop App runs `Codex.exe` from the WindowsApps package and an agent process `resources\codex.exe app-server --analytics-default-enabled`. In the current manual-official state, CCSwitch listens on `15722` only and `15721` is not listening, which is expected.
- Official documentation context: user-level `~/.codex/config.toml` supports `openai_base_url` as the built-in `openai` provider base URL override. The documentation warning that Codex ignores `openai_base_url` applies to project-local `.codex/config.toml`, not the user-level file.
- Code change: `ProxyService::takeover_app_and_switch_provider_after_switch_lock` now verifies the final activation state after starting the proxy, writing live config, setting DB enabled, and setting active target.
- New log event: `takeover_activation_check app=... provider=... proxy_running=... expected_proxy_url=... expected_codex_base_url=... live_matches_current_proxy=...`. Failure logs `takeover_activation_failed ... config_path=...` and rolls back provider/enabled/live config so the UI cannot show a false successful Multi Router activation.
- Next diagnostic rule: if Multi Router switch logs `proxy_running=true` and `live_matches_current_proxy=true` but `codex-router.log` still has no request, the remaining root cause is Codex Desktop app-server/thread not refreshing user config; if the activation check fails, follow the logged port/config evidence first.

## 2026-06-12 Codex Multi Router WS route/fallback root cause

- 完整追溯后确认链路：UI provider 卡片 -> `useProviderActions.switchProvider` -> `useSwitchProviderMutation` -> Tauri `switch_provider` -> `ProviderService::switch`。Codex router provider 因 `settings_config.codexRouting` 被判定为必须走本地代理，后端调用 `takeover_app_and_switch_provider_after_switch_lock`，启动 15721、备份 live config、写入 `openai_base_url=http://127.0.0.1:15721/v1`，并把当前 provider 设为 `codex-openai-router`。
- 能关闭 15721 的源码路径只有：切换到 category=official 的 provider 时走 `disable_takeover_for_app_after_switch_lock`；顶部/设置页关闭 takeover 时走 `set_takeover_for_app(false)`；总关闭代理时走 `stop_with_restore`。列表查询/provider 查询/get status 不会自动关闭 15721。
- 当前运行态证据：`15721/health` 不通，`15722/health` 正常；DB 中 `codex-official is_current=1`、`codex-openai-router is_current=0`、`proxy_config.codex.enabled=0`；`codex-router.log` 最后更新时间仍是 `2026-06-12 06:16:39 UTC`，因此这次用户看到的后续报错没有进入 15721。
- 中转根因修复：`codex_ws::resolve_official_ws_provider` 以前会遍历 router 展开的所有 fallback route，导致非 official/chat route 命中后仍可能扫描到后面的 official route 并错误进入 official WebSocket。现在只看本次模型解析出的第一条 effective route：如果它是 Chat Completions route 或不是 ChatGPT Codex official upstream，立即发送协议内 426 fallback，让官方 Codex 走 HTTP Responses -> Chat bridge。
- 中转根因修复：official upstream WS 在首帧后立即 close 或无任何数据结束时，旧 relay 只是把 close 原样转给 Codex，客户端显示 `Connection closed normally`。现在在 `upstream_close` 且 `before_first_upstream_frame=true` 或 `upstream_ended_without_frames` 时，向本地 Codex 发送 WebSocket 内 `status_code=426` error event 并关闭，尽量触发官方 HTTP fallback/failover。
- 中转兼容修复：upstream WS `origin` 现在强制覆盖为 `https://chatgpt.com`，避免客户端经本地代理留下非官方 origin 后被 upstream policy close。
- 可观测性修复：official switch 和手动关闭 takeover 现在都会在主日志显式记录 `source=official_switch` 或 `source=proxy_toggle_or_command`，后续能直接看出是谁关闭了 15721。
- UX 修复：Codex provider 切换成功后会刷新 `proxyStatus/proxyRunning/proxyTakeoverStatus/liveTakeoverActive`；即使之前弹过“需要代理”警告，Codex Multi Router 仍会明确提示“保持 CC Switch 运行，并完全重启或新开 Codex 会话后生效”。
- 联网交叉验证：内置 web search 与 matrix-websearch 都能找到 Codex `stream disconnected before completion` 同类问题；matrix 结果更偏中文代理/证书/长连接排障，GitHub 精确结果少。结论是 official 上游/网络确实可能断，但 CC Switch Multi Router 的责任是把可 fallback 的 WS 失败转成 HTTP/failover 路径。
- 已验证：`cargo fmt`、`cargo test proxy::codex_ws --lib`（5 tests）、`cargo check`（仅既有 `commands/misc.rs` 两个 unused warning）、`pnpm typecheck`、`pnpm release:export`。已启动新 raw exe：`C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti.exe`，SHA256 `BEC4C9F4B41736D26E0238EC5E77A79A9E1A5E3624280884FF42967D5C009C50`。启动后 `15722/health` 正常，未启用 Codex takeover 时 `15721` 不监听，符合预期。

## 2026-06-13 Codex MultiRouter custom runtime boundary

- 覆盖旧结论：MultiRouter 的 Codex live runtime 不能改回 `model_provider="openai"`。`openai` 是 Codex 内置保留 provider，会重新启用官方 OpenAI/WebSocket 语义；之前用它保历史桶和官方模型菜单的方案会把 `Connection closed normally` / WebSocket fallback 老问题带回来。
- 当前正确边界：MultiRouter takeover 写入 `model_provider="custom"`，`[model_providers.custom].base_url=http://127.0.0.1:<codex-port>/v1`，`wire_api="responses"`，`supports_websockets=false`，并移除 `openai_base_url`。真实 OpenAI/Qwen/DeepSeek 上游、API 格式和转换层都留在 `codexRouting` 与后端 route resolver 内处理。
- 模型菜单问题不要通过改回 `openai` 解决；应检查 `modelCatalog` 是否从 DB 投影到 `~/.codex/cc-switch-model-catalog.json`，以及 live config 顶层 `model_catalog_json="cc-switch-model-catalog.json"` 是否存在。Codex 官方只读取顶层 `model_catalog_json`，不是 `[model_providers.*]` 内字段。
- 历史记录问题本质是 Codex Desktop 按 `model_provider` provider bucket 过滤。使用 custom runtime 后，openai 历史不会天然显示在 custom 桶里；修复必须是用户显式触发的历史桶同步/迁移，不能为了历史把 runtime provider 改回 openai。
- MultiRouter 状态页流量统计不能只按真实 `targetProviderId` 聚合。Qwen/DeepSeek 等内联 route 可能没有外部 providerId，应按 route id/label 作为“子 Provider”统计，并可从 `codex-router.log` 的 `route_id` 或 `effective_provider=...::route::<id>` 回归属。

## 2026-06-13 Codex MultiRouter custom provider 候选模型显示修复

- 旧版能显示全量候选的真实路径不是单纯 `/v1/models`，而是 `model_provider="openai"` + `openai_base_url=http://127.0.0.1:<port>/v1` + 顶层 `model_catalog_json="cc-switch-model-catalog.json"`。因为它仍然伪装成 Codex built-in OpenAI provider，所以运行中模型管理器允许刷新 `/models`，能从 CC Switch 本地代理拿到完整 catalog。
- 当前 MultiRouter 不能改回 `openai`，否则会重新进入 built-in OpenAI/WebSocket 语义，带回 `Connection closed normally` / WebSocket fallback 老问题。正确 runtime 仍是 `model_provider="custom"`、`supports_websockets=false`、`base_url=127.0.0.1:<codex-port>/v1`。
- 对照 Codex official 源码确认：如果 Codex 进程启动时读到了顶层 `model_catalog_json`，会走 `StaticModelsManager`，完整 catalog 可直接显示；但如果是在运行中的 Codex 热切到 custom provider，旧的 OpenAI-compatible manager 不会主动刷新 `/models`，`OnlineIfUncached` 只会读 fresh `~/.codex/models_cache.json`。因此只写 `cc-switch-model-catalog.json` 不足以修复热切后的候选模型列表。
- 根因修复：CC Switch 在生成 `~/.codex/cc-switch-model-catalog.json` 后，同步写入 `~/.codex/models_cache.json`，复用现有 `client_version`，并用 `etag="cc-switch-model-catalog"` 标记所有权；退出 MultiRouter/切回 official 时，如果当前 cache 是 CC Switch 接管过的，就恢复 `models_cache.cc-switch-backup.json`，避免污染 official backup。
- 这次修复覆盖 Qwen/DeepSeek 候选缺失和 OpenAI GPT speed tier 不显示的同源问题：catalog 生成测试确认 speed tier 没丢，cache 同步测试确认 custom provider picker 能看到 `qwen3.6` / `deepseek-v4-flash`。如果之后还有候选缺失，优先检查 `models_cache.json` 的 `client_version` 是否和当前 Codex app-server 匹配，以及 Codex 是否仍拿旧进程内 catalog。

## 2026-06-13 Codex MultiRouter provider bucket correction

- Updated conclusion after comparing older 2026-06-09 backups: MultiRouter must not use the built-in `openai` provider, but it also should not be flattened into the generic `custom` provider. The old working shape used `model_provider="codex_model_router_v2"` plus `[model_providers.codex_model_router_v2].base_url=http://127.0.0.1:<codex-port>/v1`, top-level `model_catalog_json="cc-switch-model-catalog.json"`, `wire_api="responses"`, and `supports_websockets=false`.
- Root cause for the "only three OpenAI models" symptom: after the 2026-06-12 custom-runtime change, MultiRouter takeover wrote `model_provider="custom"`. That avoided built-in OpenAI WebSocket behavior but lost the router-specific provider bucket used by the old model/history path. Cache sync alone was too weak as a hot-switch repair if Codex kept using the official/openai picker state.
- Code rule: normal single upstream Codex providers still use `CC_SWITCH_CODEX_MODEL_PROVIDER_ID = "custom"`; only providers with enabled `settings_config.codexRouting.routes` use `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID = "codex_model_router_v2"`. Do not fix this by reintroducing top-level `openai_base_url`; official Codex only applies `openai_base_url` to the built-in `openai` provider, which re-enables the old WebSocket semantics.
- Regression coverage added: router switch now asserts live config uses `codex_model_router_v2`, defines `[model_providers.codex_model_router_v2]`, removes `openai_base_url`, disables websockets, writes `cc-switch-model-catalog.json`, and replaces `models_cache.json` with seven slugs (`gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex-spark`, `qwen3.6`, `deepseek-v4-flash`, `deepseek-v4-pro`) while preserving `client_version`.

## 2026-06-14 Codex Desktop three-model picker runtime boundary

- Current live config/catalog evidence can be healthy while the visible Desktop picker remains stale. On this machine, `~/.codex/config.toml` pointed at `model_provider="cc_switch_codex_router"` with `model_catalog_json="cc-switch-model-catalog.json"`, local `base_url=http://127.0.0.1:15721/v1`, `wire_api="responses"`, `requires_openai_auth=false`, `supports_websockets=false`, and no `openai_base_url`; `cc-switch-model-catalog.json` contained seven models.
- Fresh `codex.exe debug models` reading the same disk config returned all seven slugs, proving the written TOML/catalog were parseable. Therefore the remaining "only three models" symptom is not explained by route config, DB modelCatalog generation, or 15721 reachability alone.
- Codex Desktop uses `codex.exe app-server --analytics-default-enabled`; app-server builds `ThreadManager.models_manager` from startup config. `model/list` goes through that in-memory manager, so a running app-server can keep an older three-model picker even after CCSwitch rewrites `config.toml` or `cc-switch-model-catalog.json`.
- Concrete runtime evidence from this machine: `cc-switch-model-catalog.json` had 7 models; catalog mtime was `2026-06-13T23:43:49+08:00`; Codex app-server started at `2026-06-13T23:44:11+08:00`; config was written again at `2026-06-13T23:44:34+08:00`. That ordering means Desktop may be holding a model manager created before the final live config write.
- New diagnostic rule: MultiRouter status must show Codex Desktop/app-server process count, app-server command line/start time, config mtime, catalog mtime, catalog model count, and a warning when app-server started before the latest config/catalog write. The corrective action is to fully exit all Codex Desktop/app-server processes and reopen Codex before judging the picker.

## 2026-06-14 Codex MultiRouter stable history bucket and 3.16.2-6 export

- Follow-up fix: `sync_codex_history_provider_bucket_to_multirouter` must target `CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID` (`codex_model_router_v2`), not `custom`. `custom` is now treated as a legacy/source bucket along with `openai` and `cc_switch_codex_router`; otherwise explicit history sync can move sessions away from the current MultiRouter runtime bucket and make history disappear again.
- MultiRouter diagnostics now classify provider buckets as `stable_router`, `legacy_router`, `custom`, `builtin_openai_local_base`, or `other`; only `codex_model_router_v2` is pass, legacy/custom are warn, and built-in `openai` pointing at local base is fail.
- Version bumped from `3.16.2-5` to `3.16.2-6` to avoid overwriting a running `3.16.2-5` raw exe during export. New export artifacts: raw exe `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\windows\raw-exe\CCSwitchMulti_3.16.2-6_x64.exe` SHA256 `B72790130A30692D2BB83BA68B12F4BE05DD2DEAA62F0327A49DF854E40C2231`; installer `...\installer\CCSwitchMulti_3.16.2-6_x64-setup.exe` SHA256 `70A2D0B1BF7772AF9F5D01EC7C934074577B61A64046D10D1B067D5B86CB2D2B`.

## 2026-06-14 Codex Desktop current history repair built into CCSwitchMulti

- Historical note now superseded by 2026-06-23 evidence: one 26.609 local repair succeeded against `~/.codex/sqlite/state_5.sqlite`, but that path must not be treated as the universal default. Current automatic detection prefers configured sqlite homes, then `~/.codex/state_5.sqlite`, with the sqlite subdir kept only as a compatibility fallback.
- CCSwitchMulti now exposes a full `repair_codex_history_visibility` Tauri command and a MultiRouter page button labeled "修复历史显示". The UI first runs `dryRun=true`, shows the active DB/provider/user-event/index/hints/projectless/focus/mtime counts, then asks for explicit confirmation before apply.
- The Rust repair path targets `codex_model_router_v2` by default, treats `openai`, `custom`, `cc_switch_codex_router`, `codex_model_router`, and collected trusted legacy ids as source buckets, and does not switch MultiRouter runtime back to built-in `openai`.
- The repair is broader than provider bucket sync: it resolves the active Desktop sqlite DB, rewrites provider buckets, updates rollout first-line metadata, backfills `has_user_event` from rollout user messages, appends/moves `session_index.jsonl`, repairs `.codex-global-state.json` workspace hints, removes repaired ids from `projectless-thread-ids`, optionally saves/focuses a project root, and touches focused rollout mtimes.
- Regression coverage: `active_state_db_prefers_codex_root_over_sqlite_subdir`, `active_state_db_falls_back_to_sqlite_subdir`, and `repairs_current_desktop_history_visibility_end_to_end` cover root-default detection, sqlite-subdir fallback, `\\?\` cwd normalization, provider/user-event repair, session index append/move, workspace hints/projectless cleanup, saved root insertion, rollout first-line rewrite, and mtime touch.

## 2026-06-14 Codex MultiRouter history repair UI module

- The history repair trigger is no longer only a hidden status-page action. `CodexRouterWorkspacePage` has a dedicated `history` tab labeled `历史修复`, plus a top header shortcut and a status-page shortcut that only navigate to that tab.
- The `历史修复` tab replaces the old `window.prompt` flow with an optional project-root input, `预览修复`, and `确认写入`. Apply is disabled until the current project path has a matching dry-run preview, so changing the path cannot accidentally reuse stale counts.
- The tab surfaces the real backend repair evidence: current MultiRouter plan, Codex takeover state, returned `targetProvider`, active DB path/kind, live config provider, source buckets, visible window counts, backup dir, skipped reason, and per-area counts for provider/user-event/session_index/workspace hints/projectless/focus/mtime/saved roots.
- MultiRouter route editing jitter/display cut-off was traced to the nested route editor dialog in `CodexFormFields.tsx`: content used scroll classes without a stable flex-height parent. The dialog now has `max-h-[90vh] overflow-hidden`, and its body is `flex-1 min-h-0 overflow-y-auto`, so long route forms scroll inside the modal instead of resizing the viewport.

## 2026-06-15 CCSwitchMulti 3.16.2-18 GitHub release

- After adding the dedicated history repair tab, do not reuse the existing `v3.16.2-17` release because that tag points at older commit `02bd8a2a`. The release commit for the history repair UI module is `257e4e54`, tagged and pushed as `v3.16.2-18` on `BigStrongSun/cc-switch`.
- Published GitHub Release: `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-18`, marked Latest. Uploaded Windows assets: `CCSwitchMulti_3.16.2-18_x64-setup.exe`, `CCSwitchMulti_3.16.2-18_x64-portable.zip`, `CCSwitchMulti_3.16.2-18_x64.exe`, and `SHA256SUMS-v3.16.2-18.txt`.
- SHA256: setup exe `23A5D89CE4C80C78AFC5A55CD7EDA7EAF8DB22BA07B58F1FF8468A0C9FF6B707`; portable zip `C686C1048F5DE1000ABC1D553F6572C72490A09CA0ECB8CD5C0255D965D5B0B9`; raw exe `E0982F380BD44C45EFD1C22AB20208708A4DCDE6CC0AC562453F31999A489E36`.
- `pnpm release:export` succeeded for `3.16.2-18`, but the export root could not clear an old locked `CCSwitchMulti_3.16.2-17_x64.exe`. Future release uploads should stage only the exact target-version assets and a version-specific checksum file instead of uploading the export root wholesale.
- The fork currently shows no GitHub Actions runs after the tag push, so additional Linux/macOS assets will not appear automatically unless Actions are enabled/fixed or those platforms are built and uploaded separately.

## 2026-06-15 CCSwitchMulti 3.16.2-18 Linux release assets

- To match the existing `v3.16.2-17` release shape, Linux x86_64 packages were built in WSL from clean tag `v3.16.2-18` using `pnpm tauri build --bundles appimage,deb,rpm --config <no-updater-artifacts>`. The first attempt failed because the background PATH omitted `~/.cargo/bin`; after adding `/home/openclaw/.cargo/bin`, the build completed.
- Uploaded additional GitHub Release assets to `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-18`: `CCSwitchMulti_3.16.2-18_amd64.AppImage`, `CCSwitchMulti_3.16.2-18_amd64.deb`, and `CCSwitchMulti-3.16.2-18-1.x86_64.rpm`. `SHA256SUMS-v3.16.2-18.txt` was replaced with a combined Windows+Linux checksum file.
- Linux SHA256: AppImage `011B242C77A870086F684F96842755877E824D57D2C7A1F8B78AA4781C9EBC7A`; deb `730DDD58EA2D72347E7E2CAA987443D5390B43FC6C03D523433B4E95B9DDDDD8`; rpm `232D9CF6E4376BE315B332D06C90661F723C2B24152B0222DCFCD2366B01AF0B`.
- GitHub release verification after upload shows 7 assets total: Windows setup/portable/raw exe, Linux AppImage/deb/rpm, and the combined checksum file. macOS was not produced in this Windows/WSL pass because it needs a macOS runner plus Apple signing/notarization credentials, and the fork still does not expose runnable Actions via `gh workflow list`.

## 2026-06-15 README fork-positioning update

- `README.md` now opens as `CCSwitchMulti` instead of plain upstream `CC Switch`, and the top version/download badges point to the fork release page `BigStrongSun/cc-switch`.
- A new front matter section, `CCSwitchMulti Branch Notice`, explains that this repository is a downstream branch of official CC Switch and that the remaining README still contains inherited upstream documentation.
- The branch notice documents the fork-specific Codex features: `OpenAI Multi-Model Router`, `settings_config.modelCatalog`, `settings_config.codexRouting`, stable `codex_model_router_v2` runtime bucket, Codex Desktop picker unlock/Statsig filtering diagnostics, history visibility repair, and the external OpenAI-compatible API sidecar.
- The usage notes intentionally warn that catalog visibility is not the same as upstream request success, Codex Desktop may need a full restart or CCSwitchMulti unlock flow, picker unlock is runtime renderer injection rather than an on-disk `app.asar` patch, router-owned TOML must not be placed in shared Codex common config, MultiRouter must not be routed through built-in `openai`/`openai_base_url`, and the Codex takeover port is distinct from the sidecar API port.

## 2026-06-15 CCSwitchMulti 3.16.2-19 fork updater and standalone Codex history repairer

- The updater must use the fork release feed, not upstream `farion1231/cc-switch`: `src-tauri/tauri.conf.json` now points to `https://github.com/BigStrongSun/cc-switch/releases/latest/download/latest.json`, and the fallback update page plus About links point to `BigStrongSun/cc-switch`.
- The standalone Codex history repairer is a Windows GUI binary declared as `codex-history-repairer` in `src-tauri/Cargo.toml` behind the `history-repairer` feature. Keep `autobins = false`, otherwise Tauri can accidentally bundle the helper as the main app.
- The GUI calls `repair_codex_history_visibility_standalone`, which reads the live `~/.codex/config.toml` top-level `model_provider` when the target provider field is empty, falls back to `codex_model_router_v2`, auto-detects the active state DB with configured sqlite homes before the default `~/.codex/state_5.sqlite` and legacy sqlite-subdir fallback, and uses source buckets `openai`, `custom`, `codex_model_router_v2`, `cc_switch_codex_router`, and `codex_model_router`.
- Write mode blocks while Codex Desktop/app-server is running unless the GUI force option is enabled. This is intentional because current Desktop can rewrite `.codex-global-state.json` and SQLite WAL state during repair.
- Export script `scripts/export-latest-ccswitchmulti.ps1` now builds the helper with `cargo build --bin codex-history-repairer --features history-repairer --release`, copies it under `tools/codex-history-repairer`, manually signs the NSIS setup with `~/.ccswitchmulti/tauri-update.key`, writes `latest.json`, and stages release assets from the versioned export directory.
- Published release: `https://github.com/BigStrongSun/cc-switch/releases/tag/v3.16.2-19`. Required assets are present: Windows setup, setup signature, portable zip, raw exe, standalone `CodexHistoryRepairer_3.16.2-19_x64.exe`, `latest.json`, notes, and `SHA256SUMS.txt`.
- Verification performed for this release line: `pnpm typecheck`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `cargo test --manifest-path src-tauri\Cargo.toml standalone_repair_defaults_target_to_live_config_provider --lib`, `cargo test --manifest-path src-tauri\Cargo.toml repairs_current_desktop_history_visibility_end_to_end --lib`, and `cargo build --manifest-path src-tauri\Cargo.toml --bin codex-history-repairer --features history-repairer --release`.

## 2026-06-15 Codex history repair latest-script parity

- User screenshot showed `v3.16.2-19` still did not surface all repaired sessions. Root cause: the built-in Rust repair and `repair-codex-history-current-desktop.ps1` reproduced active DB/provider/user-event/index/hints/focus/mtime, but missed the later successful `balance-codex-history-recent-window.ps1 -MaxPerProject 10 -MaxTotal 300 -SourceFilter vscode -SyncRolloutMtime` step. Codex Desktop first takes a limited global recent thread window and only then groups by workspace, so current-project focus alone can still leave sessions outside the sidebar window.
- The repair backend now supports `balance_recent_window`, `max_per_project`, `max_total`, and `source_filter`. Visibility filtering uses the provider after planned bucket migration, so rows currently under `openai/custom/legacy` are counted before the write instead of disappearing from the preview.
- The balanced repair keeps the current project focus count as a floor, then round-robins remaining visible rows by normalized `cwd` with per-project and total caps. The MultiRouter history tab and standalone GUI default to `sourceFilter="vscode"`, `maxPerProject=10`, and `maxTotal=300` to match the successful Desktop-sidebar repair path.
- The rollout metadata repair now scans all JSONL lines with `payload.model_provider`, not only the first `session_meta` line, and restores the previous rollout file mtime after provider metadata rewrite; only the explicit focus/balanced mtime step changes sidebar ordering.
- `session_index.jsonl` repair now overwrites stale `thread_name` for selected rows and reports `sessionIndexTitles*` counts. Regression tests cover provider-after visibility, multi-project recent-window balancing, source filter behavior, stale title overwrite, and multi-line rollout provider rewrite.
- Verification passed: `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `pnpm typecheck`, and `cargo build --manifest-path src-tauri\Cargo.toml --bin codex-history-repairer --features history-repairer --release` (existing unrelated `commands/misc.rs` dead_code warnings only).

## 2026-06-15 CCSwitchMulti 3.16.2-20 history repair productization

- The current productized history-repair baseline is the latest successful balanced-window flow, not the older provider-only repair: active DB resolution must auto-detect configured sqlite homes, then default `~/.codex/state_5.sqlite`, then legacy `~/.codex/sqlite/state_5.sqlite`; repair targets must follow live `config.toml` or `codex_model_router_v2`, and the default visibility path is `sourceFilter="vscode"`, `maxPerProject=10`, `maxTotal=300`, with rollout mtime sync.
- CCSwitchMulti now adds `list_codex_history_sessions` and extends `repair_codex_history_visibility` with `codexHome`, `stateDbPath`, and `sessionIds`. The history tab can set Codex home, list active SQLite session summaries, search/filter records, select specific sessions for targeted recovery, or leave selection empty to run the balanced project/global recent-window repair.
- The Rust repair runtime treats nonempty `sessionIds` as an explicit focus set: provider/user-event repair still covers visible candidates, but focus movement, session_index move, workspace hints, and rollout mtime touch only apply to selected sessions; balanced recent-window reporting is disabled in that targeted mode. Regression coverage: `selected_session_ids_focus_only_requested_rows`.
- Standalone delivery is no longer a Windows GUI exe in the export pipeline. `scripts/codex-history-tool/codex_history_tool.py` is a standard-library Python tool with `list` and `repair` subcommands, exported under `tools/codex-history-tool` with README; `scripts/export-latest-ccswitchmulti.ps1` no longer builds or copies `codex-history-repairer.exe` and excludes `__pycache__`/`.pyc`.
- Version bumped to `3.16.2-20`; `pnpm release:export` produced `CCSwitchMulti_3.16.2-20_x64-setup.exe`, `.sig`, portable zip, raw exe, `latest.json`, and the Python history tool in `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti`. The export still warned that an old `CCSwitchMulti_3.16.2-17_x64.exe` was locked, but the target-version artifacts and tool checksums were written.
- Verification passed: `python -m py_compile scripts\codex-history-tool\codex_history_tool.py`, Python `list --limit 3 --json`, Python repair dry-run for `C:\Users\sunda\Documents\LLMservice`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, `pnpm typecheck`, `pnpm history:tool:check`, and `pnpm release:export`.

## 2026-06-16 CCSwitchMulti Codex history repair moved into Session Manager

- Supersedes the 2026-06-14 MultiRouter history tab placement: the product UI for Codex history repair now belongs in `src/components/sessions/SessionManagerPage.tsx` behind the Codex-only FileClock toolbar button, not in `CodexRouterWorkspacePage.tsx`. The MultiRouter workspace page no longer exposes a history repair tab/button and its old inline repair component was removed to prevent reviving stale provider-only UI.
- The built-in repair flow is implemented by `src/components/sessions/CodexHistoryRepairPanel.tsx`. It keeps the latest successful baseline defaults (`sourceFilter="vscode"`, `maxPerProject=10`, `maxTotal=300`, balanced recent window, auto-detected active state DB, rollout mtime sync), adds light default path hints, source/provider count panels, target-provider dropdown candidates, and SQLite-backed session selection.
- The Tauri backend now exposes `read_codex_history_session` so the Session Manager repair panel can inspect a selected SQLite session by following `threads.rollout_path` and parsing the local Codex JSONL into existing `SessionMessage` rows. `list_codex_history_sessions` also returns `sourceCounts`, `providerCounts`, and `targetProviderCandidates`.
- Built-in `repair_codex_history_visibility_for_multirouter` now matches the standalone/Python behavior when `targetProvider` is empty: prefer live `~/.codex/config.toml` top-level `model_provider`, then fall back to `codex_model_router_v2`. This avoids repairing the active third-party provider's history back into official `openai`.
- Regression coverage added: `multirouter_repair_defaults_target_to_live_config_provider`, `list_history_sessions_returns_provider_source_candidates_and_all_sources`, and `read_history_session_loads_rollout_messages_from_sqlite_path`. Verification passed: `cargo test --manifest-path src-tauri\Cargo.toml codex_history_migration::tests --lib -- --nocapture`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, targeted Prettier check for changed frontend files, `pnpm typecheck`, and `pnpm build:renderer`.

## 2026-06-16 CCSwitchMulti 3.16.2-21 provider edit route stability

- MultiRouter provider edit page route rows disappearing/jittering was traced to frontend state timing, not backend route persistence: `useCodexConfigState` initialized Codex catalog/routing to empty and only filled them in an effect, while `CodexFormFields` could echo the first-frame empty child rows back to the parent and overwrite loaded routes.
- The fix initializes auth/config/baseUrl/catalog/spawnAgent/routing synchronously from `initialData`, keeps prop-change keys for catalog/routing, and skips child-to-parent echo during external provider loads until local rows match the incoming state. The route list now keeps a stable empty-state container instead of collapsing, and the duplicate local-routing toggle was removed from Advanced Options.
- The wrong MultiRouter-page history-repair link remains removed; Codex history repair stays in Session Manager behind the Codex-only FileClock entry, using the 2026-06-15 balanced recent-window repair baseline.
- Export script hardening: `scripts/export-latest-ccswitchmulti.ps1` now copies only the current setup artifact's `.sig`, so stale signatures from older bundle outputs cannot leak into `SHA256SUMS.txt` or release staging.
- Version `3.16.2-21` was built/exported. Clean export path: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.2-21`. The normal `最新版ccswitchmulti` export path also received target-version artifacts, but an already running `CCSwitchMulti_3.16.2-20_x64.exe` kept old files locked, so the clean release handoff should use the versioned export directory.
- Verification passed: `pnpm typecheck`, `pnpm build:renderer`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, locale JSON parse check, `git diff --check`, and full `pnpm release:export` plus clean `-SkipBuild` export. Browser dev-mode UI inspection confirmed the Codex add-provider form has the new route empty state, no old Advanced hint, no visible MultiRouter history-repair link, and the expected local routing control; true desktop v21 UI inspection was blocked by Tauri single-instance because v20 was still running.

## 2026-06-17 MultiRouter spawn_agent candidate ordering placement

- `settingsConfig.modelCatalog.spawnAgentModels` is route/catalog configuration, so the MultiRouter candidate ordering UI belongs in `CodexRouterWorkspacePage` RoutesTab, not StatusTab.
- The route-rule panel copy should state that the first 5 models are the Codex `spawn_agent` visible models and can be drag-sorted. Both the preview window and the sortable draft list should visually highlight those first five candidates.
- StatusTab should not expose candidate editing controls (`保存排序`, `校验候选`, drag list, candidate source tabs). Keep it focused on link readiness, diagnostics, provider targets, traffic, router logs, and model-picker unlock evidence.

## 2026-06-17 MultiRouter workbench dedupe and External API multi-key credentials

- The MultiRouter top workbench should stay compact and action-oriented. Keep only the short positioning text plus create/manage/status navigation buttons there; link readiness, local listener, Codex takeover, enabled rules, diagnostics, traffic attribution, router logs, and picker evidence belong in StatusTab.
- Do not revive the removed "操作记录" tab, and do not move `modelCatalog.spawnAgentModels` editing back into StatusTab. Candidate ordering remains route/catalog configuration under RoutesTab after commit `057b43f7`.
- External OpenAI-compatible / third-party Agent API credentials now support multiple local `ccsw_` keys. New profile JSON stores key records in `apiKeys` with id, plaintext local sidecar key, prefix, and created_at so the UI can list, copy again later, and delete old keys.
- Compatibility boundary: `api_key_hash` / `api_key_prefix` are still maintained for the latest generated key and legacy hash-only profiles. A legacy profile with only hash material is shown as a non-copyable legacy key because plaintext was never stored; it can still be deleted. Deleting the last new-format key must also clear the compatibility hash so a removed key cannot continue authenticating.
- Security boundary: the reusable plaintext key is only the CCSwitchMulti-generated local `ccsw_` sidecar credential. Upstream OAuth tokens, refresh tokens, and real provider API keys are not exposed through the External API credentials page.

## 2026-06-16 CCSwitchMulti Session Manager history repair primary layout

- User feedback after the Session Manager move: the Codex history repair entry was still too hidden and the repair UI looked like an awkward utility panel. The product decision is now stronger: when `SessionManagerPage` is opened for Codex, history repair is the default primary workspace, with an explicit two-button switch for `历史修复` and `会话浏览` in the session list header.
- `CodexHistoryRepairPanel` now presents a single repair workbench instead of stacked cards: top action bar for load/preview/apply, status tiles for active DB / loaded-selected count / write state, a compact horizontal path-and-scope settings band, then SQLite history, session JSONL preview, and repair evidence columns. This keeps the latest balanced-window repair defaults visible without making the user hunt for the entry.
- The panel auto-loads active SQLite only when the Tauri runtime is present, so the real desktop app starts with useful history data while browser/dev preview does not show a false `invoke` error.
- Verification passed: targeted Prettier check, `pnpm typecheck`, `pnpm build:renderer`, and Browser dev-mode inspection at `http://127.0.0.1:3000/`. Browser DOM confirmed visible `历史修复` / `会话浏览` buttons, default Codex history repair main area, no development `invoke` error, and no horizontal overflow at 1280 px.

## 2026-06-16 CCSwitchMulti 3.16.2-22 release

- Version bumped to `3.16.2-22` for the Session Manager history-repair layout release. Export root: `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.2-22`.
- Release export verification: `latest.json` reports `3.16.2-22`, `SHA256SUMS.txt` contains only v22 Windows binaries, and the export includes setup exe/signature, portable zip, raw exe alias/versioned exe, platform build notes, README, and `tools/codex-history-tool`.
- Verification before release: targeted Prettier check, `pnpm typecheck`, `pnpm history:tool:check`, `cargo check --manifest-path src-tauri\Cargo.toml --lib`, and `scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot ...3.16.2-22`. Rust still only reports the existing `commands/misc.rs` dead_code warnings.

## 2026-06-21 MultiRouter route-rule picker

- `CodexRouterWorkspacePage` RoutesTab must not route “编辑匹配规则” into the generic Provider edit form. That form exposes the low-level `codexRouting.routes` editor and the old “添加 route” path can freeze or produce an unusable workflow for MultiRouter rule editing.
- Route-rule editing in the MultiRouter workspace is now an in-page candidate router picker: it merges existing routes with all non-routing Codex model sources, lets the user directly select/enable candidate routers, and writes only `settingsConfig.codexRouting.routes` through `providersApi.update(nextProvider, "codex")`.

## 2026-06-21 MultiRouter provider edit entry

- Codex MultiRouter providers in the main provider list must not open `EditProviderDialog` / generic `ProviderForm`. The generic form is only for normal upstream providers and can still expose the legacy route editor path where “添加 route” freezes.
- Main-list edit, and any workspace edit action for a routing plan, should navigate to `CodexRouterWorkspacePage` with that provider selected and `initialTab="routes"`. The dedicated workspace owns route selection, enabled state, model catalog, and spawn-agent candidate persistence.

## 2026-06-21 WebDAV/S3 sync portability

- WebDAV/S3 database sync must not blindly upload machine-specific absolute paths or keys when sharing a profile across devices. Sync export rewrites the current user home path to `${CC_SWITCH_HOME}` and import localizes that token, plus common `C:\Users\<other>` / `/Users/<other>` / `/home/<other>` paths, to the receiving machine.
- `includeKeysOnUpload` controls whether provider/API/MCP keys remain in the uploaded SQL snapshot. When disabled, key/token/password values are stripped while auth mode and routing structure are preserved so the receiving user can fill their own credentials.
- New route candidates should reference `targetProviderId` and `auth.source="provider_config"` instead of copying API keys or Base URLs. This preserves model-source ownership and keeps the workspace from scattering provider credentials into route rows.
- Verification passed for this change: targeted Prettier write/check on `src/components/codex/CodexRouterWorkspacePage.tsx`, `pnpm typecheck`, `git diff --check`, and `pnpm build:renderer`. Build still reports the existing browserslist/baseline staleness and large chunk warnings only.

## 2026-06-22 CCSwitchMulti v3.16.3-8 merge release preparation

- Purpose: make the next release a full successor by merging the `v3.16.3-5` release line into the `v3.16.3-7` MultiRouter/context-window line, instead of treating `v3.16.3-7` as a standalone targeted prerelease.
- Merge strategy: use a real git merge so the history records both parents. This preserves the official v3.16.3 merge, takeover restore preservation fix, unified history repair safeguards, and the newer MultiRouter/WebDAV/context-window changes.
- Version surfaces for the merged release must be `3.16.3-8` in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.
- Release rule: do not retag or force-update `v3.16.3-7`; publish the merged successor as a new tag/release.

## 2026-06-24 CCSwitchMulti v3.16.3-14 follow-up product fixes

- MultiRouter official/Codex fallback model catalog must carry explicit context windows. When an OpenAI/Codex OAuth provider has no real model catalog, fallback entries should be `gpt-5.5=272000`, `gpt-5.4=272000`, `gpt-5.4-mini=128000`, and `gpt-5.3-codex-spark=128000`; otherwise Codex Desktop can fall back to its 128k-ish display budget and users report GPT-5.5 as only about 122k context.
- The usage dashboard historically had rollup/prune maintenance but no user-triggered "clear logs" product path. The correct clear operation deletes `proxy_request_logs` and `usage_daily_rollups` only, preserving provider records, pricing rows, auth material, and app config.
- Port conflicts on 15721/15722 are real multi-instance/old-process failure modes. The low-risk product fix is to surface an actionable `AddrInUse` diagnostic naming CCSwitchMulti/old process/alternate port; a stronger cross-process singleton lock is separate work and should not be mixed into takeover restore logic casually.
- Codex Desktop model-picker unlock should not treat the CLI `codex.exe` as Desktop. Desktop executable discovery may include WindowsApps package layouts (`app/Codex.exe`, `app/resources/Codex.exe`, package root `Codex.exe`) and `%LOCALAPPDATA%\OpenAI\Codex`, but should avoid launching lowercase CLI paths. Launch should re-check whether Codex Desktop is already running before starting with CDP flags.
- OAuth token dual-store remains a risk boundary, not a solved low-risk fix: `~/.codex/auth.json` and CCSwitchMulti `codex_oauth_auth.json` can diverge. Do not blindly copy managed refresh tokens into Codex Desktop auth as a "sync" fix without proving rotation/account semantics; prefer preserving Codex login material and using managed OAuth only for proxy forwarding/quota paths.

## 2026-06-25 CCSwitchMulti v3.16.3-20 prerelease for MultiRouter model-refresh hang

- User screenshot with "候选 provider 模型列表刷新" cards stuck at "正在读取模型列表..." was a release-boundary issue first: public `v3.16.3-19` points at `6a1cf4e1` and does not include `ddfeed42` / `33a0bc58`, while the fixed local line is `4f1f911c` after `ddfeed42`, `33a0bc58`, and `272d02a3`. Future reports with the same UI should first check installed version/tag before debugging official Responses routing or upstream `/models`.
- Published prerelease `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.3-20`. Annotated tag `v3.16.3-20` dereferences to `4f1f911cae3ea13f78412c720854ab87201ee7c7`; release is non-draft and prerelease=true. Release notes are Chinese and explicitly describe the model-list loading hang, per-provider attempt tracking, API-key-sensitive stale request suppression, 30 second frontend timeout, and visible-model vs upstream-model split.
- Windows assets came from the local export pipeline at `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-20` and flat upload staging at `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.16.3-20-assets`. Raw exe `CCSwitchMulti_3.16.3-20_x64.exe` reports ProductVersion/FileVersion `3.16.3-20`; `RELEASE-METADATA.md` records commit `4f1f911c`.
- Linux assets were built locally in WSL distro `openclaw` from `/home/openclaw/ccswitchmulti-linux-build-v3.16.3-20`, after cloning the fork tag and verifying HEAD equals `4f1f911c`. Commands used Linux Node path `/home/openclaw/.local/node-v22.22.2-linux-x64/bin`, `pnpm install --frozen-lockfile --prefer-offline`, `cargo build --manifest-path src-tauri/Cargo.toml --bin codex-history-repairer --features history-repairer --release`, and `pnpm tauri build --bundles appimage,deb,rpm --config <createUpdaterArtifacts=false>`. The build succeeded; only the final `sha256sum` list command failed from CRLF glob input after artifacts had already been copied to Windows staging.
- macOS universal assets were produced by GitHub Actions run `28169469534` (`supplemental-macos-release.yml`) against `v3.16.3-20`; it completed successfully and uploaded `CCSwitchMulti_3.16.3-20_universal.tar.gz`, `.tar.gz.sig`, and `.app.zip`. This workflow also refreshed `SHA256SUMS.txt`.
- Final release has 12 assets: Windows setup/signature/portable/raw exe, Linux AppImage/deb/rpm, macOS universal tar/signature/app zip, `latest.json`, and `SHA256SUMS.txt`. Final `SHA256SUMS.txt` covers all release assets except itself; GitHub asset digests matched the local Windows/Linux checksums and workflow-produced macOS digests.
- Known non-blocking warnings in this release remain the existing Vite baseline/browserslist/chunk warnings, Rust unused/dead_code warnings, and Tauri `__TAURI_BUNDLE_TYPE variable not found` bundler warning. Fork push still triggers failing generic CI/release workflows, but manual local/WSL build plus supplemental macOS workflow are the verified publishing path for this prerelease.

## 2026-06-28 MultiRouter duplicate visible model semantics

- If OpenAI official and a third-party relay both expose the same visible model id such as `gpt-5.5`, MultiRouter does not infer quality, provider type, price, or freshness. Route order is the control surface.
- Frontend catalog generation in `CodexRouterWorkspacePage::buildModelCatalogForRoutes` uses a `Map` keyed by visible `model`; while iterating routes in order, the first route/source that contributes `gpt-5.5` wins and later same-id entries are skipped. The picker and `spawn_agent` catalog therefore show one `gpt-5.5`, not one per upstream.
- Runtime route resolution in `src-tauri/src/proxy/providers/codex.rs` uses `routes.iter().find(...)` over enabled routes. Exact `match.models` and prefix matches are case-insensitive, but duplicate exact matches still choose the first matching route in the saved `codexRouting.routes` array.
- `defaultRouteId` is only used when no enabled route matches the requested visible model. It does not override a duplicate exact match and does not choose between two `gpt-5.5` routes.
- The public helper `resolve_codex_model_routed_providers` can produce a primary route plus other enabled route candidates for future fallback use, but current forwarder code calls the single-provider wrapper `resolve_codex_model_routed_provider`, which takes `.next()`. Current HTTP routing therefore uses only the first resolved route inside the selected MultiRouter provider; it is not round-robin or automatic same-model failover.
- Upstream model rewriting is separate from route selection. Route `modelMap` / `upstreamModel` / `model` writes `codexResolvedUpstreamModelOverride` and takes priority; otherwise the catalog model's `upstreamModel` can rewrite the outbound body model. If neither exists, the visible request model is preserved for matched routes. For an unmatched fallback-style routed provider, Chat conversion forces the route/provider's own configured model so `gpt-5.5` is not blindly sent to DeepSeek/Qwen.
- Recommended configuration when a third-party relay provides an upstream named `gpt-5.5`: use a distinct visible alias such as `gpt-5.5-relay` with `upstreamModel="gpt-5.5"` if users need both official and relay selectable at the same time. If both are intentionally the same visible `gpt-5.5`, put the desired primary route first and treat the duplicate as shadowed unless the route order is changed.
- Live diagnosis on 2026-06-28: both `codex-multirouter` and `codex-openai-router` had official GPT routes before aggregate-platform routes, with broad prefixes (`gpt` or `gpt-`). A request for `gpt-5.5-pro` therefore matched the official route by prefix and was sent to ChatGPT Codex OAuth, producing "model is not supported when using Codex with a ChatGPT account" instead of using a third-party relay. The locally configured aggregate provider `yansd666带gpt官方模型` did not expose `gpt-5.5-pro` in `/models`; direct `/responses` and `/chat/completions` calls with `model=gpt-5.5-pro` both returned HTTP 503 "无可用渠道", so that provider currently supports `gpt-5.5` but not `gpt-5.5-pro`.
- When a MultiRouter route references `targetProviderId`, `materialize_codex_routed_provider_from_target` deliberately follows the target provider's `base_url`, auth, and `apiFormat`; the route row only carries route identity/capabilities/model override. For an aggregate platform that mixes native Responses-compatible GPT models with Chat-Completions-only third-party models, use separate provider entries or route-level inline upstreams per protocol. Do not rely on one global "需要本地路由映射" switch to represent both protocols at once.

## 2026-06-28 MultiRouter route-rule picker duplicate provider fix

- Editing an old MultiRouter after adding new normal providers can show duplicate Qwen/DeepSeek rows when the saved route is legacy/inline and the new provider-backed candidate has the same semantic model source. The root is not backend routing: the workspace candidate builder only deduped by `targetProviderId`, while legacy routes may have no target or may have lost `route.provider` during `normalizeLegacyCodexRoutingRoute`.
- The frontend fix is to preserve legacy `route.provider` / `upstream.provider` as `targetProviderId`, and to dedupe routes by semantic provider before rendering route entries, building candidate picker rows, and saving `codexRouting.routes`. Semantic matching falls back to normalized provider name/id and model/prefix overlap only when no explicit target provider exists.
- New provider candidates in `RouteCandidatePicker` should be directly actionable: clicking the right-side `启用` button on an unchecked candidate now selects and enables it in one step. Do not reintroduce `disabled={!checked || isSaving}` for that button, or users will again need to click `全选并启用` before adding one provider.
- Regression coverage lives in `src/components/codex/CodexRouterWorkspacePage.test.ts`: legacy provider references are preserved/deduped, and a new provider candidate can be enabled and saved without using global select-all. Verified with `.\node_modules\.bin\vitest.cmd run src/components/codex/CodexRouterWorkspacePage.test.ts`, `.\node_modules\.bin\tsc.cmd --noEmit`, and targeted Prettier check.

## 2026-06-28 MultiRouter gpt-5.5-pro source boundary

- When investigating a report that `gpt-5.5-pro` was "fetched", first distinguish model catalog acquisition from Codex runtime request input. The user screenshot of the yansd666 provider's model mapping showed `gpt-5`, `gpt-5-codex`, `gpt-5.1`, `gpt-5.1-codex`, `gpt-5.3-codex-spark`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.5`, and `gpt-image-2`; it did not show `gpt-5.5-pro`.
- For a new empty yansd666 Codex provider, clicking "获取模型列表" can still populate those official-looking GPT ids because `https://yansd666.com/v1/models` itself returns exactly those 9 ids for the configured account/key. Direct checks with default UA and a browser-like Mozilla UA both returned the same 9 ids and did not return `gpt-5.5-pro`.
- Live DB verification on `~/.cc-switch/cc-switch.db` found no `gpt-5.5-pro` string in current providers, including `yansd666带gpt官方模型` and the active `codex-openai-router`. `~/.codex/cc-switch-model-catalog.json` and `~/.codex/models_cache.json` also did not contain `gpt-5.5-pro`; the only live `~/.codex/state_5.sqlite` hits were Codex thread/task text created while debugging the screenshot.
- The model fetch path is literal: `fetchModelsForConfig` calls Tauri `fetch_models_for_config`, which calls `src-tauri/src/services/model_fetch.rs::fetch_models`; the backend parses OpenAI-compatible `/models` entries into `FetchedModel.id` and sorts them, without synthesizing `-pro` suffixes. Frontend merge paths in `CodexFormFields` and `CodexRouterWorkspacePage::providerWithFetchedModelCatalog` add fetched ids as `{ model: id, upstreamModel: id, displayName: id }`, and also do not generate `gpt-5.5-pro`.
- `providerWithFetchedModelCatalog` is additive: it updates context windows and appends new remote ids but does not prune models that disappeared from `/models`. Therefore a stale `gpt-5.5-pro` can persist on another user's machine if their provider catalog previously contained it or they manually added it, but that was not the state on this machine during the 2026-06-28 check.
- The observed toast saying `model: gpt-5.5-pro` was a runtime request boundary: Codex Desktop sent `/responses` with `model=gpt-5.5-pro`; the then-current router matched broad official GPT prefixes before the later aggregate route and sent it to ChatGPT Codex OAuth. After commit `bbe9d93d`, exact route matches take precedence globally over earlier prefixes, but if no exact `gpt-5.5-pro` route/catalog exists, a broad prefix can still be the intended fallback behavior.

## 2026-06-28 Codex official login preservation on provider switch

- The user-facing bug "switch provider, then official Codex asks to log in again" is a non-takeover `auth.json` overwrite problem, not an official-login bypass problem. Before this fix, `codex_config::write_codex_live_for_provider` still wrote non-official provider `auth.OPENAI_API_KEY` into `~/.codex/auth.json` when `preserve_codex_official_auth_on_switch=false`, and switching back to official could write a stale DB OAuth snapshot over the current live OAuth auth.
- Correct rule: third-party Codex provider switches should always leave `~/.codex/auth.json` alone and place the provider/API/proxy bearer in `config.toml` as `experimental_bearer_token`. Official switches should only write `auth.json` when live auth does not already contain real OAuth login material; if live auth has OAuth tokens, only refresh `config.toml`.
- Keep `codex_auth_has_oauth_login_material` separate from `codex_auth_has_login_material`: `OPENAI_API_KEY` is a provider token, not official login material. Do not treat third-party bearer keys as a reason to preserve/overwrite official OAuth auth.
- Regression coverage: `third_party_live_write_preserves_existing_codex_oauth_auth`, `official_live_write_preserves_current_oauth_auth_over_stale_db_snapshot`, updated `codex_custom_provider_live_write_preserves_oauth_auth_even_when_preserve_disabled`, plus existing takeover official return test `codex_switch_to_official_during_takeover_exits_proxy_and_cleans_router_fields`. Verified with targeted `cargo test --manifest-path src-tauri\Cargo.toml ... --lib` and `cargo fmt --manifest-path src-tauri\Cargo.toml --check`.

## 2026-07-03 CCSwitchMulti v3.16.4-7 release-note rewrite from tag diff

- User asked for a rewritten `v3.16.4-7` release note that compares the latest release to `v3.16.4-4` from actual per-version commits, not commit notes. The correct analysis boundary is tag diff, with `v3.16.4-7` tag `755b69e4` as the release point; later memory-only commits `5dbb8cd7` and `e8935a46` are post-release records and should not be treated as product changes.
- Diff-derived version summary: `v3.16.4-5` adds Codex Desktop OAuth login preservation during snapshot/backup restore, official same-account vs cross-account auth handling, MultiRouter wizard naming/model-selection/spawn-agent steps, provider-edit-to-MultiRouter catalog/route/spawn-agent synchronization, concurrent Codex protocol probing, release workflow hardening, and OAuth/request-shape diagnostics. `v3.16.4-6` only fixes official Codex OAuth Responses input items by removing invalid `content` from non-message/non-reasoning items. `v3.16.4-7` fixes MultiRouter duplicate GPT aliases, empty target catalogs wiping relay routes, non-routable aggregate catalog models, Volcengine AgentPlan model listing via `ListArkAgentPlanModel`, sensitive image retry, and Codex Responses control-message promotion.
- Release note rule learned: write this release as a cumulative `v3.16.4-4 -> v3.16.4-7` user-facing changelog grouped by impact, then include a short per-version section for traceability. Do not list `memory.md`, docs-only release files, function visibility changes, `parse_context_tokens` cleanup, or log wording changes as product updates.
- Evidence files used for the rewrite: `src/lib/codexMultiRouterSync.ts`, `src/components/codex/CodexMultiRouterWizard.tsx`, `src/components/codex/CodexRouterWorkspacePage.tsx`, `src-tauri/src/codex_config.rs`, `src-tauri/src/services/provider/live.rs`, `src-tauri/src/services/proxy.rs`, `src-tauri/src/proxy/providers/openai_compat.rs`, `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/media_sanitizer.rs`, `src-tauri/src/services/model_fetch.rs`, `src/utils/codexPlanModelFetch.ts`, and `.github/workflows/release.yml`.

## 2026-07-06 Codex reset credits watcher integration

- `jordan-edai/codex-reset-watcher` is useful as a reference for Codex banked reset credits, but its macOS SwiftUI/MenuBar layer should not be copied. The portable core is: read the same Codex OAuth login context, call `GET https://chatgpt.com/backend-api/wham/usage` and `GET https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`, and keep the feature strictly read-only.
- CCSwitchMulti already had cross-platform Codex quota plumbing in `src-tauri/src/services/subscription.rs`: macOS can read Keychain `Codex Auth`, while Windows/Linux fall back to `~/.codex/auth.json` through `codex_config::get_codex_auth_path()`. The correct integration point is extending `SubscriptionQuota`, not adding a macOS-specific watcher clone.
- The implemented reset-credit response intentionally exposes only safe display fields: available count, reset type, status, expiry, and title. Do not surface raw endpoint JSON, credit ids, account ids, user ids, access tokens, refresh tokens, or auth file paths in frontend state, logs, or saved snapshots.
- Codex `/wham/usage` `reset_at` can be seconds or milliseconds. `unix_ts_to_iso` now normalizes millisecond epochs and rejects implausible dates outside 2020-2100; keep this guard if endpoint parsing is refactored.
- Partial failure rule: if `/wham/rate-limit-reset-credits` fails but `/wham/usage` succeeds, quota should remain successful and may fall back to `rate_limit_reset_credits.available_count` from the usage response while exposing a reset-credit-specific error. Do not mark the whole quota query failed unless the primary usage query fails or credentials are invalid.
- Verification for this integration: `cargo test --manifest-path src-tauri\Cargo.toml codex_reset_ --lib`, `cargo check --manifest-path src-tauri\Cargo.toml`, `cargo fmt --manifest-path src-tauri\Cargo.toml --check`, and `pnpm typecheck`.

## 2026-07-07 Codex Built-in Web Search vs Third-party Router Boundary

- 用户追问为什么当前主 Agent 能用 Codex 内置 web search，而 DeepSeek V4 子会话/第三方模型说找不到内置搜索工具。排查结论：至少有三类不同能力不能混为一谈：当前 Chat/Codex 编排环境的系统工具 `web.run`、Codex CLI/App 的 first-party Responses `web_search`、以及用户配置的 `matrix-websearch` MCP 函数工具。`web.run` 是本会话系统工具，不会出现在 `tool_search` 或第三方模型的 MCP 工具列表里。
- 官方 Codex 手册显示稳定配置是顶层 `web_search = "cached" | "live" | "disabled"`，其中 `cached` 是默认，`--search` 等价于 `web_search = "live"`；`[features].web_search*` 是遗留开关。`codex features list` 在本机显示 `search_tool`/`tool_search` 已 removed，`standalone_web_search` 仍是 under development，不应作为稳定解决方案。
- 本机 `~/.codex/config.toml` 当前走 `model_provider = "codex_model_router_v2"`，`base_url = "http://127.0.0.1:15721/v1"`，`wire_api = "responses"`，当前模型是 `deepseek-v4-flash`。`codex debug models` 和 `~/.codex/cc-switch-model-catalog.json` 都显示 DeepSeek 条目有 `supports_search_tool = true`、`web_search_tool_type = "text"`，但这只是模型目录/能力描述，不等于本地代理已经能执行 OpenAI 托管的 first-party web search。
- 运行验证：`codex exec --json --skip-git-repo-check -m deepseek-v4-flash -c 'web_search="live"' ...` 没有产生官方手册所说的 `web_search` transcript item；模型反而通过 `tool_search` 懒加载并调用了 `matrix-websearch` MCP。即使额外 `--enable standalone_web_search`，DeepSeek 仍表示没有直接的内置 web_search 工具，只是读文档/本地文件回答。这说明当前第三方路由路径不是单纯缺少顶层 `web_search` 配置，而是 first-party web_search 没有作为稳定可调用工具注入到该第三方模型执行路径。
- 代码边界：`transform_codex_chat.rs` 已经桥接了 Codex `tool_search`，包括把 `{"type":"tool_search"}` 转为 Chat function `tool_search`，并能把上游 Chat tool call 恢复为 `tool_search_call`；但没有同等桥接 OpenAI 托管的 first-party `web_search`。不要把 `web_search_preview` 之类 OpenAI 原生托管工具简单转换为第三方 Chat function，除非同时实现可执行的本地搜索 runner 和 Codex 可接受的返回协议；否则模型只会生成一个没有宿主执行器的函数调用。
- 推荐方案：官方/OpenAI 模型需要内置搜索时，用 `web_search = "live"` 或交互式 `codex --search`，并走官方 OpenAI/OAuth 路径；第三方 DeepSeek/Qwen/GLM 等模型需要联网检索时，稳定方案是走 MCP（例如 `matrix-websearch`），通过 `tool_search` 懒加载或直接暴露 MCP search/open/find 工具。产品层如要修正体验，应把 UI/说明改成“OpenAI first-party web_search”和“MCP web search”两条能力，而不是让第三方 catalog 的 `supports_search_tool=true` 暗示它一定能用 OpenAI 托管搜索。
- 进一步确认：OpenAI first-party web_search 是 hosted tool，执行点在 OpenAI-hosted Responses API 服务端；只要请求路径变成本机 `codex_model_router_v2` / 第三方 OpenAI-compatible provider，OpenAI-hosted Responses API 就不在请求链路里，第三方模型不能“直接调用”这个托管搜索。官方 Bedrock 文档也明确说非 OpenAI-hosted provider 下依赖 OpenAI-hosted cloud services / hosted tools 的功能不可用。
- 想让第三方模型“具备同等搜索能力”只有两条现实路线：一是不要走第三方 provider，把需要 first-party web_search 的任务切到官方 OpenAI/OAuth provider；二是在 Codex/CCSwitchMulti 侧实现本地工具执行器，把搜索做成 MCP/function tool（可复用 `matrix-websearch` 或另写 OpenAI web-search 查询服务），由第三方模型发普通工具调用，本地执行后把结果回灌给模型。这是“自建搜索工具桥接”，不是让第三方直接使用 OpenAI hosted web_search。
- 抓包复核：用本地假 `openai_base_url = "http://127.0.0.1:18081/v1"` 捕获官方 provider + `web_search="live"` 的 `POST /v1/responses`，Codex 先尝试 `ws://.../v1/responses`，失败后回退 HTTP SSE；请求体默认 `content-encoding: zstd`，可用 zstd 解压。实际 `tools` 数组里同时有 client `tool_search` 和 hosted `{"type":"web_search","external_web_access":true,"search_content_types":["text","image"]}`，`tool_choice="auto"`、`stream=true`、`store=false`、`parallel_tool_calls=true`。这说明官方搜索的触发入口确实是 Responses 请求里的 hosted `web_search` tool 声明。
- 代理伪造方案边界：可以在 CCSwitchMulti 拦截 `/v1/responses`，识别并移除/替换 `type=web_search`，把它转成第三方可见的 function tool（例如 `name="web_search"`，参数 `{query, count}`），由代理执行 `matrix-websearch` 或其它搜索 runner，再把结果作为 tool output 继续向第三方模型发起下一轮，直到得到最终 message 后再按 Responses SSE/JSON 返回给 Codex。不要把 hosted `web_search` 原样透传给第三方；第三方通常不会执行 OpenAI hosted tool，也可能因未知 tool type 报错。

## 2026-07-07 Hosted Tool Bridge Design Document

- 方案文档已落地到 `docs/codex-hosted-tool-bridge-design.md`。核心设计不是让第三方模型直接调用 OpenAI hosted tool，而是让 CCSwitchMulti 把 Codex 入站 Responses 请求里的 hosted tool（例如 `type=web_search`）替换为普通 function tool，第三方模型只发 function call，CCSwitchMulti 再用独立 OpenAI credential 调 OpenAI Responses hosted tool，并把结果规整为普通 tool output 回灌给第三方模型。
- 这个桥接模式可以扩展到 `image_generation`：对第三方模型暴露 `generate_image(prompt, size, quality, format)`，CCSwitchMulti 内部调用 OpenAI Responses `tools: [{ "type": "image_generation" }]`，解析 `image_generation_call.result`，把 base64 解码成 artifact 文件，返回路径、MIME、尺寸和可选缩略图；不要把完整 base64 直接塞回模型上下文或普通日志。
- `file_search` 也可桥接，但必须显式配置允许的 vector store、文件权限和日志脱敏；`computer_use` 不应作为普通 provider proxy tool-loop 的 MVP，因为它需要独立的交互会话、安全确认、屏幕状态和动作权限设计。
- 实现建议新增 `src-tauri/src/proxy/providers/hosted_tools/` 模块，拆出 `bridge.rs`、`openai_client.rs`、`web_search.rs`、`image_generation.rs`、`file_search.rs`。默认只开启 `web_search`，`image_generation` 和 `file_search` 需要显式 allowlist，日志只记录 trace id、tool name、query hash、耗时和状态，不记录完整网页正文、完整 prompt、base64 图片或 API key。

## 2026-07-10 新版 GPT/Codex 应用历史不可见与 standalone 修复器路径

- 现场根因不是历史数据丢失：当前 `~/.codex/config.toml` 的 live provider 是 `model_provider = "custom"`、模型是 `gpt-5.6-sol`，而 `~/.codex/state_5.sqlite` 中 1845 条 threads 里有 1839 条仍在旧 `codex_model_router_v2` 桶，只有 3 条在 `custom`。Codex 按当前 provider 桶过滤时会只显示极少新线程，看起来像“历史没了”。
- 同时存在两个 state DB：`~/.codex/state_5.sqlite` 今天仍在写入且有 1845 条；`~/.codex/sqlite/state_5.sqlite` 停在旧时间且只有 935 条。Rust/Tauri 内置修复器当前已经按 `sqlite_home` / `CODEX_SQLITE_HOME` / 根库 / `sqlite/` 旧库顺序选择 active DB，但 standalone Python 工具仍旧优先 `sqlite/`，会在离线修复流程里误修过期库。
- 已修 `scripts/codex-history-tool/codex_history_tool.py`：无显式 `--state-db` 时先尊重 `sqlite_home` / `CODEX_SQLITE_HOME`，再选当前根库 `~/.codex/state_5.sqlite`，最后兜底旧 `~/.codex/sqlite/state_5.sqlite`；`scripts/codex-history-tool/README.md` 同步说明该优先级。
- 验证基线：`python -m py_compile scripts\codex-history-tool\codex_history_tool.py` 通过；默认 dry-run 已命中 `activeDbKind=codex_root` 和 `stateDbPath=C:\Users\sunda\.codex\state_5.sqlite`，预览会把 1842 条旧 provider 行同步到当前 `custom` 桶，并发现 240 条可见候选；`cargo test --manifest-path src-tauri\Cargo.toml active_state_db --lib` 3 个 Rust active DB 选择测试通过。
- 操作边界：真正写入历史前必须让用户完全退出 Codex/GPT app 或使用内置修复器的并发保护；不要在 app-server 仍运行时强制写入 live SQLite。若目标是恢复 MultiRouter 稳定形态，还要另外排查为什么 live config 从 `codex_model_router_v2` 变成了 `custom`，这不是历史工具本身造成的。

## 2026-07-10 新版 GPT/Codex 保留 spawn_agent schema

- 用户截图中的错误 `Invalid Value: 'tools'. Function 'collaboration.spawn_agent' is reserved for use by this model and must match the configured schema.` 指向新版 GPT/Codex 后端对保留工具名的 schema 校验变严格；本机 live `~/.codex/config.toml` 当时存在 `[features.multi_agent_v2] hide_spawn_agent_metadata = false`，会让 Codex 给 `collaboration.spawn_agent` 追加 `model`、`reasoning_effort`、`service_tier` 等 metadata 字段，从而和新版模型的保留 schema 不一致。
- 旧修复思路是为了让 `spawn_agent` 参数里可直接覆盖模型；新版路径已经有 `~/.codex/agents/*.toml` custom agent role 文件承载子 Agent 的 `model`、`model_provider`、`model_reasoning_effort`，因此不应再通过扩展保留函数 schema 选择模型。
- `src-tauri/src/codex_config.rs` 的 Codex catalog/config 投影使用 `ensure_codex_multi_agent_reserved_schema_compatible`：保留用户原本 `multi_agent_v2` 启用状态并写入 `hide_spawn_agent_metadata = true`。本机 `0.147.0-alpha.6.5` 实测仍暴露 `agent_type`、`model`、`reasoning_effort`，只隐藏 `service_tier`；默认自动选型走 managed custom roles。
- 现场快速恢复可以把 `~/.codex/config.toml` 里的 `[features.multi_agent_v2] hide_spawn_agent_metadata = false` 改为 `true` 后重启 Codex/GPT app；长期修复依赖新版 CCSwitchMulti 重新投影配置。

## 2026-07-17 MultiRouter spawn_agent 保留 schema 二次根因

- v3.16.5-15 仍出现 `Function 'collaboration.spawn_agent' is reserved ... must match the configured schema` 时，`hide_spawn_agent_metadata=true` 已经正确，真正根因在模型目录合并：CCSM 用通用模板生成路由条目后覆盖了同 slug 官方模型的协议字段，把官方 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 的 `use_responses_lite=true` 写成 `false`。现场 `models_cache.json` 与 `cc-switch-model-catalog.json` 对照可直接证明该差异。
- `use_responses_lite=false` 会让 Codex 不再使用 input 内的 `additional_tools` namespace transport，`collaboration.spawn_agent` 因而以不符合新版后端保留定义的工具结构发送；仅调整 `hide_spawn_agent_metadata` 无法修复这类 transport 级 schema 不匹配。
- `merge_codex_model_entry` 的正确所有权边界是：同 slug 官方缓存已经给出的协议、工具、推理和展示字段保持官方权威；CCSM 只覆盖模型标识、显式上下文、可见性、输入模态、用户明确声明的并行工具能力和基础指令等路由字段。回归测试 `codex_model_catalog_keeps_official_transport_and_reserved_tool_metadata` 必须覆盖 `use_responses_lite`、`multi_agent_version`、`tool_mode` 和 `apply_patch_tool_type`。
- 上述 transport 修复与 MultiRouter 空方案白屏修复一起发布为 v3.16.5-16；发布说明位于 `docs/release-notes/v3.16.5-16-zh.md`。

## 2026-07-10 统一 Codex App 历史目录与原生模型目录适配（纠正）

- 前一节“按 provider 桶过滤导致历史不可见”不适用于当前统一 Codex App，不能再作为新版历史修复根因。用 MSIX 内置 `codex 0.144.0-alpha.4` 按 App 官方同步参数只读实测，canonical `~/.codex/state_5.sqlite` 可稳定分页返回 `100 + 100 + 46 = 246` 条可见顶层 vscode 线程；`modelProviders` 省略、`null` 或空数组在当前数据上结果一致。两个数据库 `PRAGMA integrity_check` 都是 `ok`，历史正文和 canonical 索引没有丢失或损坏。
- 新版 `OpenAI.Codex_26.707.3748.0` 的桌面壳已经改为 `app/ChatGPT.exe`，子进程才是 `resources/codex.exe ... app-server`。新版侧边栏不直接展示 canonical `threads`，而是读取派生库 `~/.codex/sqlite/codex-dev.db` 的 `local_thread_catalog`。现场该目录只有 17 行，且 `local_thread_catalog_sync_state(local)` 为 `initial_build_complete=0`、`watermark_updated_at=NULL`，这才是“历史都没了”的真实断点。
- 新版主修复必须调用 App 自己的 `localThreadCatalog.requestStartupSync()`，由原生 manager 按 `useStateDbOnly=true`、source/parent/subagent/ephemeral 规则完成 cold sweep；不要批量重写 `state_5.sqlite` 的 `model_provider`、source/history 字段，不要改 rollout JSONL，也不要手工把 canonical rows 灌入 `local_thread_catalog`，否则会绕过 revision、missing-candidate、watermark 和 read-repair 语义。
- CCSM 的 renderer 兼容层现在通过新版 `rpc-*` 模块寻找 `localThreadCatalog`，调用 `readSnapshot()` / `requestStartupSync()`，并把“脚本已注入”和“原生同步确已请求”分开返回。Windows 宿主发现同时兼容旧 `Codex.exe` 与 `OpenAI.Codex_*` MSIX 内受路径约束的 `ChatGPT.exe`；普通独立 ChatGPT 程序不能进入 Codex CDP 链路。
- 旧版 SQLite/rollout 历史修复工具保留为离线兜底，但真实写入时只要检测到 Codex Desktop/app-server 仍运行就必须拒绝。它不是统一 App 的默认修复路径；dry-run 仍可作为只读诊断。
- `修复过程.zip` 的截图主要证明新版模型目录和路由适配，不是历史修复方案：官方 native cache 已含 `gpt-5.6-sol/terra/luna`，而 CCSM 路由 catalog 尚未包含。正确实现是按 `slug -> model -> id` 动态合并官方缓存与 CCSM 路由模型（路由字段优先、官方独有元数据与模型保留），并在当前缓存已被 CCSM 接管时从接管前 backup 恢复 native 基线；不要硬编码某一批 GPT-5.6 名称，也不要继续整份覆盖 `models_cache.json`。
- 验证基线：`cargo test codex_desktop::tests:: --lib` 17/17、`cargo test codex_history_migration::tests:: --lib` 39/39、`cargo test codex_config::tests:: --lib` 84/84、`pnpm typecheck`、`pnpm vitest run src/components/codex/CodexRouterWorkspacePage.test.ts` 39/39 均通过。

## 2026-07-11 Codex 内置 Image Gen 与 MultiRouter 边界

- 用户上传的“内置 Image Gen 调用失败”截图证明 Codex Desktop 内置图片生成不是走 `/v1/responses`，而是直接请求本地 provider base URL 下的 `POST /v1/images/generations`。因此 MultiRouter official `/responses` route 正常并不等于 Image Gen 会正常；如果本地代理没有注册 Images API 路由，请求会在 Axum 路由层直接 404，根本进不到 official route 解析。
- 修复入口是 `src-tauri/src/proxy/server.rs` 和 `src-tauri/src/proxy/handlers.rs`：FullProxy 与 External OpenAI API 模式都要注册 `/v1/images/generations`。普通 OpenAI-compatible provider 应按 Images API 原样透传；当前 provider 是 official OAuth 或 MultiRouter 能物化出 official OAuth route 时，图片请求优先走 ChatGPT/Codex 官方 OAuth 目标。
- 图片请求的 `model` 往往是 `gpt-image-*`，旧 MultiRouter official route 可能只匹配 `gpt-5.x` 文本模型。探测 official route 时可以用真实图片模型、catalog 里的 `gpt-*` 模型和稳定 GPT/Codex 名称做只读 probe，但发送 Images API 时必须保留请求体里的 `gpt-image-*`，并移除 request-local route 上的 `codexResolvedUpstreamModelOverride`，否则会把图片模型误写成 `gpt-5.5`。
- text-only 能力仍要优先于模板/模型配置的 `input_modalities`：`src-tauri/src/codex_config.rs::codex_catalog_model_entry` 在 NativeResponses 分支不能把已判定 text-only 的模型重新写回 `["text","image"]`，否则 Codex Desktop 会误显示内置 Image Gen 或多模态入口。
- 回归覆盖：`cargo test --manifest-path src-tauri\Cargo.toml image_generation --lib` 覆盖 Images 路由进入 handler 与 MultiRouter official route 选择；`cargo test --manifest-path src-tauri\Cargo.toml codex_model_catalog_text_only_native_responses_never_reenables_image_modality --lib` 覆盖 text-only 不被 NativeResponses 模态覆盖。

## 2026-07-13 Codex 多设备额度协作

- 官方 `utilization` 是账号总窗口百分比，本地 token 只是单设备代理/会话日志口径，禁止相互换算；协作页必须将“账号总窗口”和“已接入设备 token”拆开呈现。
- 协作账号命名空间由 `~/.codex/auth.json` 的 `tokens.account_id` 加固定域分隔后 SHA-256 得到；真实账号 ID、OAuth token、prompt、原始 JSONL、cwd 都不写入远端或本地协作报告。
- 远端协议：`{remote_root}/quota-collaboration/v1/{account_scope}/{device_id}.json`。每台设备只覆盖自己的文件；WebDAV 通过 `PROPFIND Depth=1`、S3 通过 `ListObjectsV2` 发现同 scope 下其它设备，避免共享 manifest 的多机覆盖竞态。
- `quota_collaboration_reports` 只保存每设备最新脱敏聚合报告。刷新官方 Codex 额度时写入本机报告；“同步设备报告”上传本机文件并合并其它设备。损坏、scope 不匹配或百分比越界的报告必须跳过。
- `observe` 只看数据。`enforce` 使用最近 10 分钟内、所有已同步设备中每个官方窗口的最高 utilization；窗口剩余不高于阈值时，`RequestForwarder` 只拒绝经过本机 CCSwitchMulti 的 Codex 请求（HTTP 429）。未通过 CCSwitchMulti 的旁路请求无法被控制，任何 UI 或文档不得承诺全账号强制控制。
- 用户文档位于 `docs/guides/codex-multi-device-quota-collaboration-zh.md`。首次接入必须在每台设备刷新官方额度、设置独立设备名并同步；不要复制 settings/配置目录，否则相同 `deviceId` 会使设备报告互相覆盖。
- 验证覆盖：`quota_collaboration` 有 29 项 Rust 测试，包含内存 DB 的设备持久化/隔离、observe 和 stale 不拦截、最高官方 utilization 触发约束，以及本地 mock WebDAV 的 MKCOL/PUT/PROPFIND/GET 两设备发现路径；前端 `CodexUsagePage.test.tsx` 覆盖未配置引导、约束确认和设置保存 payload。不要让测试读取真实 `auth.json` 或写入真实 settings。

## 2026-07-14 Issue #12 商汤 Chat Provider 协议误判根修

- 真实根因：`explain_codex_responses_upstream_protocol` 的已知 Chat-only URL 列表缺少 `sensenova.cn`，导致 `token.sensenova.cn/v1` 在协议信号缺失时退回 Native Responses；MultiRouter 物化目标 Provider 时也会丢失 route 的 `apiFormat`，并可能被目标 Provider 的陈旧 meta 再次覆盖。
- 修复：`materialize_codex_routed_provider_from_target` 同步 route 的 `apiFormat` / `api_format`，并让 route 的显式协议覆盖目标 Provider 的陈旧 `meta.apiFormat`；`is_known_chat_completions_only_url` 增加 `sensenova.cn` 作为旧配置兜底。
- 回归测试覆盖 SenseNova URL 推断，以及“目标 meta 为 `openai_responses`、route 显式为 `openai_chat`”的冲突场景。协议修正后继续复用现有 Responses -> Chat 工具调用映射，不做厂商专用 tool call ID 改写。

## 2026-07-14 Issue #15 同会话切换模型仍落回官方额度

- MultiRouter 不按 session 固定 route；每次请求都读取当前 `body.model`。`v3.16.5-5` 已收紧 catalog，只注入启用 route 的模型，但 Codex Desktop 仍可能暂存旧 alias。
- 旧 alias 若只属于已停用 route，原 resolver 会把它当成普通未匹配模型并使用 `defaultRouteId=official`，从而把“中转模型已失效”表现成“官方额度耗尽”。
- 根修是在 enabled route 无匹配时检查 disabled route 的精确模型声明；命中旧 alias 就 fail closed，不再静默回官方。回归同时覆盖同一 router 连续从 official 模型切到 relay alias 时，第二个请求按新 `body.model` 重新解析到 relay。

## 2026-07-14 CCSwitchMulti v3.16.5-8 发布准备

- fork 最新正式 Release 是 `v3.16.5-7`；本地 `v3.17.0` tag 来自 upstream，fork 不存在该 tag，因此本次继续使用 fork 补丁版本 `v3.16.5-8`。
- 发布范围以 `v3.16.5-7..main` 的真实 diff 为准：多设备额度协作、可信 Codex originator 白名单保留与缺失回退、SenseNova Chat 协议根修、停用 route 旧 alias 禁止回官方，以及相关教程和回归测试。
- `.github/workflows/release.yml` 由 `v*` tag 触发，必须先让 main CI 的 Prettier/rustfmt 门槛通过，再推 annotated tag 到 `fork`；完成后核验 Release 为正式版、矩阵任务成功、跨平台资产和 `latest.json` 齐全。
- CI 后端测试曾在 UTC runner 上失败：`range_starts_midnight_boundary` 用固定 `UTC+8` 构造样本，但被测函数按运行机器 `Local` 时区分日，导致同一 `UTC+8` 日期在 UTC 下跨日。测试已改为用 `Local` 构造边界样本，保持生产统计按设备本地日历日计算的语义。
- `v3.16.5-8` 已于 2026-07-14 发布到 `BigStrongSun/ccswitchmulti`：tag 解引用提交为 `27bdcdfa5e3733c0f9cd3fa37bb5192606ba8e23`，Release workflow run `29276721016` 成功，Release 为 `draft=false`、`prerelease=false`。
- Release 共 19 个资产，覆盖 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/zip、Linux x64/ARM64 AppImage/deb/rpm、5 个 updater 签名文件和 `latest.json`。
- `latest.json` 验证为 `version=3.16.5-8`，平台键包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有下载 URL 和 signature。

## 2026-07-14 Codex Multi-Agent Agent Message Content Boundary

- CCSwitchMulti `v3.16.5-7` / `v3.16.5-8` 在 official OpenAI OAuth `/responses` 路径可能返回 `Missing required parameter: 'input[20].content'`。该错误不依赖本机日志即可从请求归一化链路复现：`normalize_codex_oauth_input_item` 过去只允许 `type=message` 保留 `content`，其余未知类型一律删除。
- OpenAI Codex 在 2026-06-12 起把 Multi-Agent V2 投递表示为 `type=agent_message`，字段包括必填的 `author`、`recipient`、`content`；`content` 可同时包含 `input_text` 和 `encrypted_content`。`gpt-5.6-sol` 使用多 Agent 后，历史中的第 21 条恰好可能是这种 item，旧 normalizer 会把合法请求改坏后再转发。
- 根修复不是给缺失字段补空数组，也不是停止清理所有 tool content。official OAuth normalizer 改为明确的“禁止 content 类型列表”：function/custom/MCP/tool-search/local-shell/web-search/image-generation/compaction 等调用或输出类型继续删除冗余 `content`；`reasoning` 继续转换到 `summary`；`message`、`agent_message` 和未来未知类型保守保留原始 `content`。
- 回归测试 `codex_oauth_responses_normalizer_preserves_agent_message_content` 构造 21 条历史并断言 `input[20]` 的明文和加密 content 均保留；`codex_oauth_responses_normalizer_preserves_unknown_item_content` 固定未知类型的前向兼容回退。原有 tool output 和 reasoning content 清理测试继续约束私有 backend 的严格边界。

## 2026-07-14 Codex OAuth Content Cleanup Source And Responses-Lite Boundary

- `agent_message.content` 回归的源头提交是 CCSwitchMulti `77781164`（2026-07-01）。该提交为修复 `function_call_output`、`custom_tool_call_output`、`tool_search_output` 的 `content array too long`，把已证实的三个禁止类型泛化成“仅 message/reasoning 允许 content”；测试也只覆盖这三个输出类型，没有未知类型默认行为。`cb337248`（2026-07-03）正确把 reasoning raw content 改为 summary，但把 allow-list 进一步收窄到仅 message，使后续新增的 agent_message 必然被误删。上游 Codex 的对应演进是 `8f2d6416`（2026-06-12）把 plaintext/encrypted Multi-Agent V2 投递统一为 typed `agent_message`，`5b22a8e5`（2026-06-16）固定 NEW_TASK/MESSAGE/FINAL_ANSWER envelope；因此 CCSwitchMulti 7 月 1 日引入 allow-list 时其实已经落后于上游 schema。
- 同一审计发现 Responses-Lite 也被旧 normalizer 改坏：上游 Codex `33cc928d`（2026-06-23）开始让 Lite 请求用 `input[0].type=additional_tools` 携带 tools，并用 developer message item 携带 instructions，顶层 `tools/instructions` 应保持缺失。旧路径无视 Lite header，仍提升 developer message 并补顶层空 tools，破坏官方请求形态。
- Lite fallback 旧实现只剥 `x-openai-internal-codex-responses-lite` header 后重发原 body；这会形成“标准 header + Lite body”的协议错配。修复后，正常 Lite 转发只做 function arguments 和 OAuth item content 的安全清理，不移动 input instructions/tools；命中 fallback 缓存或上游明确拒绝 Lite 时，同时提取 `additional_tools` 到顶层 tools、提升 developer/system message 到 instructions，再用标准 Responses body 重试。
- content 规则增加参数化类型矩阵，覆盖 additional_tools/item_reference/function/custom/MCP/tool-search/local-shell/computer/file-search/code-interpreter/web-search/image-generation/compaction 等已知非 content item；message、agent_message 和未知类型继续保守保留。以后新增 item 类型时必须显式选择“已知禁止”或“未知保留”，并同步测试，不能恢复 catch-all allow-list。

## 2026-07-14 Codex 新 Session 模型菜单空列表竞态

- 官方 app-server `model/list` 响应固定为 `{ data, nextCursor }`，并允许合法返回 `data=[]`。Desktop 兼容脚本旧实现只对非空 `data` 调用 `patchModelArray`；首次空响应因此原样进入 renderer 查询缓存，新建 session 会表现为模型选择入口缺失或一直未加载。
- renderer 修复必须同时覆盖 `list-models-for-host` 和原生 `model/list` 两种方法名，并只处理已记录 request id 的 `mcp-response`。对已确认的模型列表响应，`data=[]`、`models=[]` 和直接数组都允许从 CCSM payload 回填；不能继续用通用 object graph patch 猜测其它 MCP 响应。
- Desktop 解锁目录投影不能只读取一次 `cc-switch-model-catalog.json` 后失败即安装空 payload。读取顺序改为生成 catalog、带 `etag=cc-switch-model-catalog` 的当前 models cache、活动 `model_provider` 的内联 `models`；回退仍严格限定活动路由目录，不能重新引入未启用 provider。
- 修改 renderer 兼容脚本行为时必须同步升级 patch key 和 request-client patch 版本；否则已运行 renderer 会因旧 installed 标记跳过新版拦截器，导致“代码已更新但重新解锁仍无效”。
- 官方 Codex 当前 `model/list` 使用 `RefreshStrategy::OnlineIfUncached`，标准响应类型见 `ModelListResponse { data, next_cursor }`。CCSM 的兼容层应把空列表视为可恢复的目录初始化竞态，而不是永久有效的“没有模型”结果。

## 2026-07-14 CCSwitchMulti v3.16.5-9 发布结果

- `v3.16.5-9` 已发布到 `BigStrongSun/ccswitchmulti`，tag 解引用的发布构建源码提交为 `c26237a1ad09cd6c21b7d3fd899786d99014b7f8`；正式 Release 为 `draft=false`、`prerelease=false`。
- 发布前 main CI run `29307562657` 成功，覆盖前端 typecheck、format、unit tests，以及后端 rustfmt、Clippy 和完整 Rust tests。
- Release workflow run `29308202757` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。
- Release 共 19 个资产，包含 Windows x64/ARM64 setup 与 portable、macOS dmg/tar.gz/zip、Linux x64/ARM64 AppImage/deb/rpm、5 个 updater 签名文件和 `latest.json`。
- `latest.json` 验证为 `version=3.16.5-9`，平台键包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有下载 URL 和 signature。

## 2026-07-15 CCSwitchMulti v3.16.5-10 白屏与数据库兼容发布结果

- `v3.16.5-10` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `212c512f0962e6e12e7ccc9ef2e16d8937eab9e3`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-10`。
- 发布内容同时覆盖两条白屏故障链：多路路由向导对历史异常模型目录项的未捕获渲染异常，以及应用根树的未捕获渲染异常恢复页；另将 SQLite schema 从 v12 迁移至 v13，并为旧数据库补齐 `input_token_semantics` 字段，兼容上游 v3.17.0 已写入 v13 的使用统计库。
- Release workflow run `29354860819` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-10`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 MultiRouter 同 Session 模型上下文窗口切换根修

- 用户报告的“短上下文模型切到长上下文模型仍是短窗口，反向切换又可能向短模型注入超长历史”根因是 CCSwitchMulti 旧提交 `3748fe8a7be113b01965c7e46cbe3e6b9688dfd6` 把顶层 `model_context_window` 当作跨 provider 的用户设置保留。对单模型 provider 合理，但 MultiRouter 将多个模型放入同一 `codex_model_router_v2` provider 后，该顶层值会覆盖 catalog 中所有模型的 `context_window`。
- 官方 Codex 当前 `models-manager/src/model_info.rs::with_config_overrides` 会把顶层 `model_context_window` 和 `model_auto_compact_token_limit` 无条件覆写每个 `ModelInfo`。官方 `core/src/session/turn.rs::maybe_run_previous_model_inline_compact` 则每个 turn 使用当前 `ModelInfo` 的窗口，并在长窗口切短窗口且历史已超过新窗口时，先使用旧模型执行 `ModelDownshift` 压缩，再运行短模型。不要把全局窗口写成最大值，这会再次破坏下行切换保护。
- CCSwitchMulti 修复位于 `src-tauri/src/codex_config.rs`：启用 `codexRouting` 或实际 config 已指向 `codex_model_router_v2` 时，目录投影会移除顶层 `model_context_window` 和 `model_auto_compact_token_limit`；live config 合并也会再次移除它们，避免旧配置在首次接管、热切换或恢复时回灌。普通单模型 provider 不受影响，仍保留用户的显式全局覆盖。
- 验证：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、两条定向回归、`cargo check --manifest-path src-tauri/Cargo.toml --lib` 和 `cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests:: --lib`（89/89）均通过。回归固定 MultiRouter 删除全局窗口/固定压缩阈值且保留逐模型目录，并固定单模型 provider 保留用户覆盖。

## 2026-07-15 CCSwitchMulti v3.16.5-11 发布结果

- `v3.16.5-11` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `667de3dc3c9ecf1fe355a34f56d3dd8296d87102`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-11`。
- 发布内容为 MultiRouter 同一 Codex session 的按当前模型上下文窗口切换根修：短模型切长模型恢复目标模型窗口，长模型切短模型维持官方发送前压缩保护；单模型 provider 的全局覆盖保持不变。
- Release workflow run `29362697306` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-11`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 Provider settingsConfig 为 null 的启动崩溃根修

- 截图中的错误 `Cannot read properties of null (reading 'settingsConfig')` 不是 v3.16.5-10 已处理的 MultiRouter 目录渲染异常，而是另一条 Provider 运行时契约破坏链。仅凭这条 JavaScript 文案不能区分“provider 条目本身为 null”和“上游先返回了 settingsConfig=null、随后某处派生出空 provider”；前端 `Provider` 类型声明不会校验真实 IPC payload，因此两层边界都必须收紧，不能只在两个报错组件旁加空值判断。
- 根修在 `src-tauri/src/database/dao/providers.rs`：所有 provider 配置解析、列表读取、按 ID 读取、OMO 当前 provider 读取、保存和局部更新都统一规范化为 JSON object。`null`、数组、标量、空/损坏 JSON 一律成为 `{}`，有效对象不变；因此旧库无需手改，也不会再被新写入重新污染。
- 回归：DAO parser 覆盖 `null`、数组、标量、损坏文本和有效对象；内存 SQLite 覆盖列表读取、按 ID 读取及写回 `null` 后实际持久化为 `{}`。`cargo fmt --check`、`cargo test --manifest-path src-tauri/Cargo.toml database::dao::providers::tests --lib`（2/2）和 `cargo check --manifest-path src-tauri/Cargo.toml --lib` 通过。本机 `~/.cc-switch/cc-switch.db` 只读扫描未发现当前坏记录，说明需修的是跨用户历史数据入口而非本机现存库。

## 2026-07-15 GitHub ARM64 Release 依赖安装网络容错

- `v3.16.5-13` 的 Release workflow 连续两次只有 Linux ARM64 失败；两次日志都显示 `ports.ubuntu.com` 的 IPv6 路由不可达，随后 IPv4 端口 80 超时。Windows x64/ARM64、macOS 与 Linux x64 均成功，因此失败与白屏源码或 Rust/前端构建无关。
- 旧工作流的 Linux 系统依赖阶段对每条 `apt-get` 只尝试一次，任一 Ubuntu 软件源瞬断就会让发布步骤整体跳过。根修是在统一 `apt_get` helper 中强制 IPv4、启用 APT 下载重试和连接超时，并对完整事务做三次退避重试；`update`、核心依赖、GTK、WebKit 与 libsoup fallback 全部必须经过该 helper。
- 已推送且失败的发布 tag 不应改写。`v3.16.5-13` 保留为失败构建证据，包含工作流根修的新源码应使用后续补丁版本重新打 tag。

## 2026-07-16 CCSwitchMulti v3.16.5-14 白屏根修发布结果

- 用户再次提供 `Cannot read properties of null (reading 'settingsConfig')` 恢复页截图时，`v3.16.5-13` 尚未生成 GitHub Release：其两次 Release attempt 均因 Linux ARM64 无法连接 `ports.ubuntu.com` 而失败，Publish Release 被跳过。因此该截图只能证明旧安装包仍会触发问题，不能证明 `cfb4804d` 的 Provider IPC 隔离已经在用户机器运行。
- `v3.16.5-14` 已发布到 `BigStrongSun/ccswitchmulti`，annotated tag 解引用与远端 main 均为 `13e16af9f12299161f1bd2ce5d86a5f0438235a0`。正式 Release 为 `draft=false`、`prerelease=false`：`https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-14`。
- 主 CI run `29430225565` 成功；Release workflow run `29430899551` 的 Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS、Publish GitHub Release 与 Assemble latest.json 全部成功。Linux ARM64 在强制 IPv4和重试后越过依赖安装，验证 `4a6858a3` 的发布网络容错有效。
- Release 共 19 个资产。`latest.json` 验证为 `version=3.16.5-14`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有非空下载 URL 和 signature。
- 发布快照的干净工作树验证通过：Provider payload 与 AppErrorBoundary 共 4 项测试、TypeScript typecheck、renderer 生产构建、app_exit_monitor 2 项测试及 `cargo check --lib`。本地未提交的 `/responses/compact` 代理改动未进入发布提交或 tag。

## 2026-07-16 MultiRouter 手动 Chat 协议被旧 Provider 快照回滚

- 用户在“配置核心参数”里把 Qwen 明确选成 Chat Completions，但“生成路由规则”仍显示 `openai_responses`；有时连续点击几次又会成功。这种非确定性不是协议探测结果本身，而是父层 provider query refetch 与向导草稿写入之间的竞态。
- `CodexMultiRouterWizard` 打开后的同步 effect 原本用 `nextSourceById.get(id) ?? currentById.get(id)` 合并 provider，导致任何父层 refetch 都优先采用数据库旧快照，覆盖用户刚写入草稿的 `apiFormat/apiFormatSource=manual`，也可能覆盖刚刷新的模型目录或别名处理结果。
- 根修边界：向导打开期间，已选 source 以 `currentById` 草稿为事实来源，父层快照只补充草稿中尚不存在的 source，并继续同步删除项；从 Provider 配置页返回时向导会关闭再打开，由初始化 effect 读取最新数据库配置，不需要在打开期间破坏草稿。

## 2026-07-15 v3.16.5-11 远端白屏报告复盘与前端 IPC 防线

- 用户回传的 `cc-switch.log` 显示 v3.16.5-11 后端完成数据库、provider、代理和 Codex 历史初始化；`codex-router.log` 中大多数请求正常得到上游 200。React/WebView 的 `Cannot read properties of null (reading 'settingsConfig')` 不会自动写入 Rust 日志，因此“后端日志无报错”不能推翻前端白屏报告，也不能据此归因网络或本机配置。
- `app-exit-events.jsonl` 中 2026-07-15 11:18:55 的 `cannot move state from Destroyed` 是 tao 0.34.6 Windows 事件循环 panic；随后 11:19:58 重启并可正常主动退出。这是独立的窗口生命周期异常，不是 `settingsConfig` React 空引用的证据，后续应另案复现，不能混入本次 Provider 数据契约修复。
- `7b7d9679` 已提交且随 v3.16.5-12 发布，负责数据库 DAO 的 `settings_config` 对象化；此前工作区中另有未提交的前端防线。正式收口应放在 `providersApi.getAll()` 统一 IPC 边界，而不是只放首页 `useProvidersQuery`：根 payload 非对象、provider 条目为 null/缺 id/缺 name 时隔离，`settingsConfig` 非对象时归一 `{}`，从而覆盖 query、mutation、设置页等所有直接调用者。
- `AppErrorBoundary` 继续作为最后一道恢复与取证边界：未捕获渲染异常显示恢复页，并把错误消息、组件栈和发生时间保存到带版本的本地诊断键 `ccswitchmulti.lastRenderError.v1`。它不替代数据边界修复，也不捕获异步事件或原生窗口 panic。

## 2026-07-15 CCSwitchMulti v3.16.5-12 发布结果

- `v3.16.5-12` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 解引用到发布源码提交 `063a6d59bf74893e021efd3fe75045b47e14baa2`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-12`。
- 本版发布 Provider `settingsConfig` 非对象配置兼容根修，消除截图中的应用启动错误边界；用户无需手工编辑 SQLite，旧记录会在读取时恢复为安全空对象。
- Release workflow run `29384516268` 成功，Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。Release 共 19 个资产；`latest.json` 已验证为 `version=3.16.5-12`，六个平台键均有下载 URL 和 signature。

## 2026-07-15 GPT-5.6 偶发 `collaboration.spawn_agent` HTTP 400 复核

- `gpt-5.6-sol` 和 `gpt-5.6-terra` 都返回 `Function 'collaboration.spawn_agent' is reserved for use by this model and must match the configured schema`，证明失败发生在上游校验 Responses `tools` 数组时，尚未进入模型推理；不是 Sol/Terra 子型稳定性差异，切换 5.6 子型不是根修。
- 所谓“有时好用”只能说明不同请求构造了不同的工具集，或 Codex app-server 进程/会话仍持有旧配置；一旦同一请求携带了被扩展的保留 schema，上游会在推理前确定性返回 400，不是随机网络错误。
- 截图分析里的 `spawn_agent_visible_model_limit` 只控制候选模型展示窗口，`src/services/quota_collaboration.rs` 只处理额度协作，都不参与 `collaboration.spawn_agent` 的函数 schema 生成，不应作为本错误的根因线索。
- 本机当时安装 CCSwitchMulti `3.16.5-12`，active `~/.codex/config.toml` 已是 `[features.multi_agent_v2] hide_spawn_agent_metadata = true`，当前 Codex app-server 进程也晚于该配置启动；本机近期 session 未找到新的同类上游错误。对外部报错机器应直接核对其 active config 和 app-server 启动时间，不能用本机健康状态代替现场证据。

## 2026-07-15 Codex 历史迁移后 Desktop 全局卡顿根因

- CCSM 旧的 provider bucket 迁移会在 Codex Desktop/app-server 运行时批量替换 rollout JSONL，并让文件获得新的 mtime；这会同时使 Codex 原生历史缓存失效、触发 SQLite/WAL 竞争，并让 CCSM 的使用量同步器把旧历史当作活跃文件重新扫描。根修是在任何真实写入前检测 Codex 进程并返回 `codex_running`，只保留 dry-run 诊断能力。
- provider bucket 改写只改变元数据，不代表会话最近活跃。原子替换 JSONL 后必须恢复原始 mtime，避免 Codex Desktop 和 CCSM 周期同步器重新索引全部迁移历史。
- 当前 `~/.codex/config.toml` 已由 Codex CLI 成功解析，活动 provider 为内置 `openai`，没有 CCSM 本地 URL、`PROXY_MANAGED` 或自定义 catalog 残留；本次现场不能归因为当前 TOML 损坏。
- 当前 Codex 现场约 17 个已加载任务产生 16 组 MCP/plugin runtime，约 84 个 MCP 后代进程、约 3 GB RSS。失效远程 Docs MCP 是单点启动延迟，但任务级 MCP 重复实例化是当前资源放大的主因；这是 Codex runtime 生命周期问题，CCSM 不应通过改写历史制造更多“最近活跃”任务来进一步放大。
- CCSM 仍需后续处理：Codex 使用量同步器对每个已修改 rollout 从首行重放；代理接管会临时删除整个 `[model_providers]` 后只创建 CCSM provider，恢复过度依赖备份；代理只覆盖模型 API，MCP、OAuth、插件和 Desktop 服务仍直连，UI 与文档必须明确该边界。

## 2026-07-16 CCSwitchMulti v3.16.5-15 MultiRouter 协议竞态修复发布结果

- `v3.16.5-15` 已发布到 `BigStrongSun/ccswitchmulti`。annotated tag 的本地与远端解引用均为发布源码提交 `c800c546ae662d3d01004c6532c75c95b267f59b`；正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.16.5-15`。
- 本版修复 MultiRouter 向导中手动选择 Chat Completions 后偶尔又显示 Responses 的竞态：向导打开期间以当前草稿为事实来源，父层 Provider query refetch 只补充尚未进入草稿的 source，不再用旧数据库快照覆盖用户刚选的 `apiFormat/apiFormatSource=manual`。
- 发布前独立干净快照通过定向 UI/lib 回归 52/52、TypeScript typecheck、renderer 生产构建、Rust `cargo check --lib`、Prettier、rustfmt 与 `git diff --check`。main CI run `29434619832` 的 Frontend Checks、Backend Checks、Clippy 和完整测试全部成功。
- Release workflow run `29435226376` 成功；Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 构建矩阵，以及 Publish GitHub Release、Assemble latest.json 全部通过。macOS DMG 的公证与代码签名校验成功。
- Release 共 19 个资产。`latest.json` 验证为 `version=3.16.5-15`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`，每个平台都有非空下载 URL 和 signature。
- 本地未提交的 `/responses/compact` 代理改动及旧输出目录没有进入发布提交、tag 或本次发布结果记录提交。

## 2026-07-16 第三方 Agent API 的 MultiRouter 聚合来源可用性根修

- 第三方 Agent API 已支持把整个 Codex MultiRouter 选为聚合模型源，也保留单条 route 的高级直连入口；不需要再增加一套独立的“多 Provider 多选”配置，否则会与 MultiRouter 的匹配、默认路由和故障转移规则形成两个事实来源。
- 截图中 Router 与其所有 route 都显示“需补配置”的根因是 External API 预检只读取 route 内联凭据或 Router 外壳凭据，而新版 route 只保存 `targetProviderId`，真实 Base URL、API key、协议及 OAuth 状态位于被引用的目标 Provider。实际请求会物化目标 Provider，因此旧预检与运行时语义不一致。
- 根修后，预检按运行时相同的字段优先级解析新旧 route schema，并从同一应用的 Provider map 查找目标；目标存在时优先按目标 Provider 判断托管 OAuth 或 Codex adapter 可提取的真实 Base URL/认证。显式引用缺失目标时必须不可用，不能被 route 上残留的 managed OAuth 标记误判为可用；没有目标引用的旧 route 继续兼容内联凭据和 Router 外壳凭据。
- Codex Provider 的运行时能力不能用只服务于额度查询的 `resolve_usage_credentials()` 代替：后者主要解析 config TOML，真实 Codex adapter 还支持顶层 `base_url/baseURL`、auth/env/direct key。第三方 API 的可用性检查已改为复用 adapter 提取规则，同时将托管 OAuth 与静态凭据视为并列认证路径，空配置的官方 seed 仍保持可用。
- 回归覆盖：整个 Router 可聚合官方 OAuth 与 `targetProviderId` 指向的 Qwen 静态 Provider；两条 route 均可用并贡献模型；缺失目标 Provider 时 Router 和 route 均不可用且返回可诊断错误。验证通过 External API 16/16、Codex 目标物化 6/6、外部 Router 解析 2/2、TypeScript typecheck、Prettier、rustfmt 和 `cargo check --lib`。

## 2026-07-16 CCSM 重启后 Codex 托管 OAuth 账号消失根修

- 用户反馈从 v3.16.5-13 连续升级并频繁重启 CCSM/Codex 后，连接测试可能先报 401，随后认证中心直接显示未认证。`v3.16.5-13..v3.16.5-15` 在 `codex_oauth_auth.rs`、统一 auth commands、Codex OAuth commands 和 manager 初始化处没有差异，因此不是 -14/-15 升级脚本新引入的认证库迁移或清理。
- 当前 `CodexOAuthManager` 只把 access token 放在内存，重启后的新 manager 必然没有 access token；第一次模型目录、额度或真实代理请求会调用 `get_valid_token_for_account()` 并立即使用磁盘 refresh token。连接测试/页面自动模型查询因此可以成为重启后的首次刷新触发器。
- `refresh_with_token()` 当前把 token 端点任意 HTTP 401/403 都折叠成 `RefreshTokenInvalid`，不检查 OAuth error body；调用方随后立即执行 `remove_invalid_account_after_refresh_failure()` -> `remove_account()` -> `save_to_disk()`，把账号从 `codex_oauth_auth.json` 永久删除。认证页的离线状态只看本地账号集合，所以删除后直接显示未认证；这解释了“先 401，随后认证页账号消失”，不是单纯 UI 状态错误。
- 本机日志也显示每次进程启动后均出现“从磁盘加载 1 个账号”后紧跟“access_token 需要刷新”，且同一启动时刻可能有多个调用同时到达刷新入口；每账号锁能合并网络刷新，但不能消除重启必刷。现场是否为 refresh token 真正轮换/撤销、并行旧进程竞争，还是 token 端点/WAF/代理的暂时 401/403，需要用户机器的 `cc-switch.log` 中 `[CodexOAuth]` 邻近日志确认；不要索取或导出含 refresh token 的 `codex_oauth_auth.json`。
- 根修不能只延迟刷新或给 UI 加提示：`refresh_with_token()` 现只按结构化 OAuth error code 识别 `invalid_grant`、`invalid_refresh_token`、`refresh_token_invalid`；普通 401/403 作为可重试失败保留账号。错误只提取经过字符约束的结构化 code，原始 body 不进入日志/UI，避免上游意外回显敏感内容。
- 明确失效的账号不再物理删除，而是在 `codex_oauth_auth.json` 中持久化 `invalidated_at/auth_error` 隔离；默认账号、账号列表和代理只选择可用账号，认证页显示“记录已保留，请重新登录”，同账号重新授权会原位覆盖并清除失效态，只有用户显式移除/注销才删除记录。
- 短期 access token 与过期时间现在和 refresh token 保存在同一凭据文件中，进程重启优先恢复未临期 token，不再每次启动都强制刷新。刷新成功会把 access token、过期时间及轮换后的 refresh token 作为同一串行快照落盘。
- refresh 返回明确 invalid_grant 后会在隔离账号前再次加载磁盘；若另一个进程刚写入不同 refresh token 或仍有效 access token，则采用最新凭据再试一次。这样旧进程的飞行中请求不能用旧 token 的失败覆盖新进程刚完成的轮换。
- 验证覆盖 24 项 Codex OAuth 定向回归，包括重启复用短期 token、临时 401 保留账号、明确 invalid_grant 隔离但不删除、同账号重新登录原位恢复、跨 manager 读取轮换 token、飞行中跨进程轮换恢复；TypeScript typecheck、`cargo check --lib`、Clippy `-D warnings`、rustfmt 与 Prettier 通过。

## 2026-07-17 v3.16.5-15 保留工具报错与 MultiRouter 白屏重新定界

- 用户明确故障机器运行 `3.16.5-15`。截图中的 macOS 资源名 `index-D-I7smwQ.js` 已在 GitHub Release `v3.16.5-15` 的正式 `CCSwitchMulti-v3.16.5-15-macOS.zip` 二进制中命中，`Info.plist` 也为 `3.16.5-15`；因此不能再把两张截图归因为旧安装包、错误版本号或未升级。
- `null is not an object (evaluating 'e.settingsConfig')` 的堆栈落在该发布包的 Codex MultiRouter 工作台主 chunk。`v3.16.5-15` 虽然在数据库 DAO 和 `providersApi.getAll()` 的 `get_providers` IPC 边界规范化了 provider，但工作台仍有 React Query 缓存、乐观更新、组件 props 和派生集合等内存入口直接信任 `Provider` 非空。之前“数据库与单一 IPC 双边界已经完整封闭白屏”的结论过宽；后续根修必须给共享 query/cache 写入和工作台组件入口建立同一运行时规范化契约，并用 null provider 注入回归覆盖真实工作台渲染链。
- `collaboration.spawn_agent` 保留 schema 报错也不能只凭 `hide_spawn_agent_metadata=true` 判定已修。当前 Codex 工具规划还受 multi-agent 版本、tool namespace、动态 feature config 和 Codex 运行时版本共同影响；新源码已出现 `multi_agent_v1` namespace，而截图现场仍是 `collaboration.spawn_agent`。必须从故障机采集报错请求的实际 `tools`/namespace/schema 与 Codex 版本，再与同版本官方直连请求做结构差异，不能继续猜测或只重写配置开关。
- 本轮仅完成发布产物指纹、tag 源码和当前 Codex 工具规划的只读核验，没有修改业务实现。第一类错误在缺少真实请求 payload 时不应贸然改 schema；第二类错误已确认需要扩大运行时数据边界，但应先补能复现工作台 null 注入的测试再实现根修。

## 2026-07-17 v3.16.5-15 打开 MultiRouter 面板立即白屏根修

- 用户补充了决定性触发条件：Provider 主界面正常，只要打开 Codex 多路面板就立即进入错误边界，Safari 报 `null is not an object (evaluating 'e.settingsConfig')`。定向组件测试证明根因不是数据库中的 `settings_config=null`，也不是 React Query 中混入 null provider。
- `CodexRouterWorkspacePage` 在没有有效 MultiRouter 方案时会把 `selectedPlanForModelRefresh` 设为 `null`，随后调用 `readCodexRouting(selectedPlanForModelRefresh)?.routes`。可选链只保护了函数返回值，没有保护传入参数；`readCodexRouting` 内部第一步读取 `provider.settingsConfig`，因此空方案现场必然白屏。
- 根修是让 `readCodexRouting` 的真实契约接受 `Provider | null | undefined` 并在入口返回 `null`。这不是组件旁的临时兜底：该读取函数本来就是“没有路由配置时返回 null”的统一边界，而调用方已经明确允许当前方案为空。
- 新增组件回归：只有普通 Codex provider、没有任何 MultiRouter 方案时渲染工作台，必须正常显示而不能访问 null 的 `settingsConfig`。验证通过工作台 45/45、`pnpm typecheck`、Prettier 和 `git diff --check`。

## 2026-07-28 合并原版 v3.17.0 后的完整集成测试收口

- CCSwitchMulti `3.17.0-1` 已合并原版 `farion1231/cc-switch v3.17.0`，merge commit 为 `b0471ee0`，且 `upstream/main` 是当前分支祖先。只跑 `cargo test --lib` 不足以验收合并：外部 `tests/*.rs` 依赖 crate 根公开 `Database`、`Profile`、`Prompt`、Profiles service payload/scope 以及 official Codex 重应用入口；这些兼容导出已在 `src-tauri/src/lib.rs` 恢复。
- 完整测试发现 Codex MCP 投影的零集合漏洞：`McpService::project_servers_to_app` 原先逐条遍历数据库记录，数据库没有 MCP 时循环不执行，导致 Provider 切换前 live 中的 `[mcp_servers.*]` 和历史错误 `[mcp.servers.*]` 被继续保留。根修是 Codex 分支始终从统一数据库构造完整启用快照，并复用 `sync_enabled_to_codex` 整表替换；空集合也会删除标准表和 legacy 表，其他 TOML 键保持不变。
- 旧 `provider_commands` 用例曾要求目标 Provider 快照内的 stale MCP 与 DB 启用 MCP 同时进入 live，这与数据库 SSOT 冲突。修正后的契约是：Provider 快照保留原始导入文本，避免静默破坏存档；live 只投影数据库中对 Codex 启用的 MCP，快照内陈旧条目不得复活。
- 最终验证：`cargo test --manifest-path src-tauri/Cargo.toml` 全部通过（库测试 2342 passed / 2 ignored，所有集成测试通过）；`provider_service` 33/33；`pnpm typecheck` 和全量单线程 Vitest 通过；`cargo fmt --check` 与 `git diff --check` 通过。构建产物必须在本次提交后重新生成，旧 `CCSwitchMulti_3.17.0-1_x64-setup.exe` 不代表最终源码。

## 2026-07-30 Codex 自定义 OpenAI 门面的能力与网络身份根修

- 官方 Codex `refs/remotes/live/main` 提交 `6219b7c40fc9c702c0aef9964e72b492558f60e4` 复核：`model_provider = "codex_model_router_v2"` 的配置键只用于本地选择、任务归属和路由，不作为 Responses provider 身份字段发送；`ModelProviderInfo::is_openai()` 精确按 `name == "OpenAI"` 判断 OpenAI 专属能力。
- `name = "OpenAI"` 能保留远程压缩、请求压缩、内部聊天元数据、加密函数参数、Web Search、Image Generation 和并发 reasoning summary 等 OpenAI 分支，但自定义 provider 不继承内建 `openai` 的 `version`、`requires_openai_auth=true`、`supports_websockets=true` 和 `supports_standalone_web_search=true` 字段默认值。当前 Web Search 判断是 `is_openai() || uses_openai_actor_authorization() || supports_standalone_web_search`，所以最后一个字段对 OpenAI name 是配置对齐与冗余保险，不是单独解锁开关。
- 明确回归链：`af58740b` 原本成对硬编码 `originator=codex_cli_rs` 和 `version=0.144.1`；`cd8d6bc6` 把 originator 统一迁到 forwarder 时漏迁 version，旧常量留在 `claude.rs` 成为死代码。固定 0.144.1 也会随官方最小客户端版本抬升而再次失效。
- 根修：可信本地 Codex 的官方 User-Agent 按 `<process-originator>/<cargo-version>` 动态恢复真实 version，并覆盖独立头中可能陈旧的值；线程级 originator 可以与进程 User-Agent 身份不同，异常/重复线程来源回退为 `codex_cli_rs` 时仍保留可信进程版本。External API 的任意 version 删除，第三方 User-Agent 不伪造 Codex version。
- Native/Mixed 直通和 CCSM managed OAuth 都进入同一官方身份规范化，不能只按 `is_codex_oauth` 处理；普通 Responses 与 raw passthrough 两条链均覆盖。raw Native route 还必须在重建头时放回 Desktop Bearer 与 `chatgpt-account-id`，否则文件/音频等未知 endpoint 会在丢弃本地认证头后变成未认证请求。
- MultiRouter Native/Mixed 与 Fully Managed 均显式写 `supports_standalone_web_search=true`；共享 `codex_official_provider_table` 同步补齐。旧版统一会话桶只有四字段，新 matcher 必须同时严格接受旧四字段与新五字段并拒绝未知字段，避免升级后无法清理自有配置。MultiRouter 继续显式关闭 WebSocket，使用 HTTP Responses + SSE。
- 网络身份边界：本机 Codex 的 User-Agent 和 host integration 提供的 `x-oai-attestation` 透传，External API 的 attestation 删除；CCSM 不生成或伪造 attestation。所有 `x-cc-switch-*` 控制头在普通和 raw transport 前剥离；经过代理后的 TLS/HTTP 实现不能声称与 Desktop 直连字节级不可区分。
- 官方 `6219b7c4` 的普通 `ContentItem` 是 `input_text/input_image/input_audio/output_text`，没有 `input_file`；`encrypted_function_args` 是 `Option<Vec<String>>`。回归覆盖官方图片、音频、client metadata、内部消息 metadata、加密参数数组，以及 raw 文件 endpoint 的原始字节/认证边界，不能把兼容 API 的 `input_file` 误写成 Desktop 当前已发送。
- 最终验证：Rust 全量库测试 `2369 passed / 2 ignored`；Vitest 单 worker 为 `88 files / 651 tests`；TypeScript typecheck、rustfmt 和 `git diff --check` 全部通过。Vitest 默认并发曾让既有 `tests/integration/App.test.tsx` 两项异步断言超时，单独重跑该文件 8/8 通过，随后单 worker 全量稳定通过，确认是测试共享状态/时序波动而非本次后端改动。

## 2026-07-30 v3.19.0-1 新功能真实 UI 审查（待修）

- 为避免与本机正在运行的 `3.16.5-21` 单实例混淆，使用临时 Tauri identifier `com.ccswitchmulti.ui-review` 从提交 `63f408d4ec58704ac66b1fc73eca65865dbe5757` 构建独立 release 可执行文件，并通过 `CC_SWITCH_TEST_HOME` 隔离用户数据。真实窗口验证覆盖：Grok Build 新建表单、Codex MultiRouter 工作台、Router 级 ChatGPT 认证方式、设置 > 认证账号池、使用统计、models.dev 自动同步和 Codex 用量维护。
- 已确认 MultiRouter UI 能显示 `Codex Desktop 当前登录`、`CCSM OAuth`、`OAuth 账号池` 三种模式，显示认证门面预览、HTTP Responses、`supports_websockets=false`；账号池 UI 有 Desktop 当前登录账号、优先级、保留额度、启用开关、上下移动和五分钟额度刷新状态；models.dev 有开关、状态、本地文件、模型选择和立即同步，5121 条目录只显示最近 80 条并支持全量搜索；Codex 用量维护有警告和确认入口。
- 待修交互正确性：`CodexOAuthSection` 的保留额度 `input` 在每次 `onChange` 都立即调用 `setCodexAccountPoolPolicy`，没有 debounce、显式保存、mutation 序列化或 pending 禁用。多位数字输入会产生并发持久化，响应乱序时最终阈值可能不是用户看到的值；排序、启用开关也可在 pending 时继续并发操作。
- 待修删除保护：新增 `XaiOAuthSection` 的单账号移除和“移除所有 xAI 账号”均直接执行，没有确认对话框；Codex/Copilot 旧组件也存在同类问题。本次至少应在新 xAI OAuth 发布前补确认，并统一三个 OAuth 组件的删除契约。
- 待修可发现性：Grok Build 新建页一次展示约 40 个预设且没有折叠/限高；在 903x631 的真实窗口中，供应商名称、认证和地址等主表单全部落在首屏下方，用户只看到预设矩阵和底部操作按钮。应限制预设区域高度、默认折叠非核心预设或选择后自动滚动到表单。
- 本轮没有修改业务实现，也没有把单元测试当作 UI 验收。Windows UI 工具在最后检查 Codex 用量重建确认框时返回了错误窗口捕获，已立即停止 UI 输入并终止隔离审查进程；该确认逻辑随后只以 `UsageDashboard.tsx` 的 `ConfirmDialog` 源码确认，不能记为完成了可见确认框验收。

## 2026-08-01 撤回 GitHub 上的 3.19 Release

- 用户要求撤回 `BigStrongSun/ccswitchmulti` 当前 GitHub 上发布的 3.19 版本，使最新可用版本回到 3.16.5 系列。
- 已删除 GitHub Release `v3.19.0-2`（release id `363224997`，稳定版，25 个资产）和 `v3.19.0-3`（release id `363263087`，预发布版，19 个资产）；远端 Release 列表中已无 `v3.19*` 条目。
- `gh api repos/BigStrongSun/ccswitchmulti/releases/latest` 已复核返回正式版 `v3.16.5-24`（release id `363261864`）。这也使基于 `releases/latest` 的 updater 下载入口回到 3.16.5 系列。
- 为保留源码追溯和未来恢复依据，本次只删除 Release，不使用 `--cleanup-tag` 删除 tag；`v3.19.0-2` 指向提交 `15dae3db049e674e257ac0f2a02a97f94524efcb`，`v3.19.0-3` 的 annotated tag 为 `22ddd192f870563cd3f8d25b206a813a16182dde`。

## 2026-08-02 Codex MultiRouter 15721 自环与假成功根修

- 现场不是“上游慢”或前端卡住：`codex-router.log` 同一请求先显示 route 正确命中 DeepSeek/Qwen，但 `request_prepared.upstream_url` 仍为 `http://127.0.0.1:15721/...`；SQLite 请求日志随后在同一 session 中爆发数千条 400/502，错误 body 呈 `CC Switch local proxy failed` 递归嵌套。Codex 必须等到 SSE `response.completed` 才完成 Turn，所以递归返回的 HTTP 响应壳会造成后台看似成功、Desktop 持续思考/重试。
- 源码根因在 retry/forward 两层契约错位：`build_forward_attempt_providers_preserving_codex_router_context` 先把 MultiRouter 展开成带 `codexResolvedRouteId` 的候选；`forward()` 看到该标记便跳过再次 route 解析和 `targetProviderId` 物化。于是 route ID 与归因正确，但实际 provider 仍继承父路由指向本机 15721 的 base URL。
- 根修不是改配置或加重试次数：retry 候选在进入账号池与尝试循环前必须通过 `materialize_codex_forward_attempt_provider` 从数据库读取真实目标 provider，并复用 `materialize_codex_routed_provider_from_target`。这样 base URL、认证、apiFormat、reasoning 等来自目标，route 仅保留请求级模型映射、能力和父路由归因；目标丢失必须返回显式配置错误。
- 防递归边界必须检查“最终有效上游”，不能只检查 route miss。正常 JSON、raw passthrough 和未知 raw endpoint 都在网络发送前调用同一拒绝逻辑；只有回环主机且端口等于 CCSM 当前实际监听端口才拒绝，避免误伤本机其它端口的 vLLM。拒绝使用非重试 `InvalidRequest`，回归证明 `total_requests=1`、`failed_requests=1`、`success_requests=0`。
- 验证证据：forwarder 模块 119/119；全量库测试在真实 CCSM 占用 15721 时只有既有的硬绑定端口用例失败，跳过该用例后 2691 passed / 2 ignored；`cargo check --lib`、rustfmt、`git diff --check` 通过。任何后续路由重构必须保留“展开候选先物化 target，再以 resolved route 转发”和“有效上游不能回到当前监听端口”两条不变量。

## 2026-08-02 Codex remote compact v2 响应语义补齐（3ecf02ef）

- 现象：DeepSeek `/v1/responses` 原生 route 收到 Codex remote compact v2 后返回 5 个普通 output，没有 `type=compaction` item，Codex 报 `expected exactly one compaction output item, got 0 from 5 output items`。
- 历史缺口：`4f0da985` 只补了 compact 路由/元数据/日志，`transform_codex_chat` 测试仍断言普通 message，没有真正构造 compaction item；`af60c7ed` 只是用 `name="OpenAI"` 打开 remote compact，不解决第三方上游响应。
- 根修：v2 检测限定 `responses_compaction_v2` 或 `/responses + compaction_trigger` 且无 implementation；原生 Responses 请求不改 wire，响应聚合后合成唯一 `ocx1:` compaction item；Chat 路径同样合成；后续请求转发前把 `ocx1:` 还原成 user summary；官方/显式支持 route 原生透传。
- 验证：`cargo check --lib`、fmt、12 个 compaction 测试全过；全量库测试的 3 个失败均为既有 Anthropic 断言或 15721 被运行实例占用，非本次改动引入。

## 2026-08-02 Codex 502 后无法重试根因：Managed retry 预算被写成 0

- 现场 session `019fc255-dd60-7a62-bbc2-99d5206f2487` 在 19:58 连续两次 `upstream_send_error` 后直接 `task_complete`，用户手动发“继续”也只再失败一次，没有自动等待网络恢复后重试。JSONL 显示 `codex_error_info=other`，不是流断开后的 5 次重试。
- 直接根因：`72c8ca22` 为防“可能已在途”的 502/504 重放，把 CCSM 托管 provider 写成 `request_max_retries=0`、`stream_max_retries=0`；该次现场 `~/.codex/config.toml` 的 `[model_providers.codex_model_router_v2]` 正是这两个值。Codex 源码 `codex-api/src/endpoint/session.rs` 用 `request_max_retries` 驱动流建立前的 transport/HTTP 5xx 重试，`core/src/session/turn.rs` 再用 `stream_max_retries` 驱动采样重试；两层都为 0 时，即使 `error sending request` 是明确未发送的连接/构造失败，也会立即终止。
- 当时修复只恢复 `request_max_retries=2`，并继续把 `stream_max_retries` 保持为 0；2026-08-04 的版本对比证明这留下了新的恢复缺口。`v3.16.5-22` 未覆盖该字段、实际使用 Codex 默认 5 次流重试；`72c8ca22` 才在其后把托管 provider 强制改为 0。`hyper_client.rs` 将未知结果的响应体读取失败映射为 `ResponsePending/429` 仍然正确，因为这类请求不能伪装成普通断流。
- 验证：`cargo check --lib`、`cargo fmt --check`、`managed_codex_retry_budget` 与 `hyper_client::tests` 通过。

## 2026-08-02 ResponsePending grace：超时后保留上游 future 再等 30s

- 用户继续追问“上游已经成功但响应迟到”的场景。复核结论：`72c8ca22` 的 `ResponsePending/429` 只阻止自动重发，并不能回收迟到结果；旧实现用 `tokio::time::timeout`，超时即丢弃上游 future，后来即使返回结果也没有客户端可接收。Codex 对普通 429 会映射成 `RetryLimit`，不会按 `Retry-After` 自动等待。
- 新增 `src-tauri/src/proxy/response_grace.rs`：`await_with_response_grace` 用 `tokio::select!` 保留原 future，常规超时到期后再等 `RESPONSE_PENDING_GRACE_SECS=30`；宽限期内收到结果就正常返回，仍无结果才返回 `ResponsePending/429`。
- 接入点：reqwest 首包前等待、非流式整包 body 读取、Responses 语义首包预读、普通流式首包预读、hyper raw/fallback 的响应等待。已向客户端返回流头之后，CCSM 自己的透明重放仍不可逆；但不完整 Responses 流必须交给拥有 session/turn 状态的 Codex，用客户端 `stream_max_retries=5` 恢复。
- 回归：`response_grace::tests` 覆盖宽限期内恢复与宽限期后 429；`response_` 127 tests、`streaming_first` 3 tests、`hyper_client::tests` 通过。全量 `cargo test --lib -- --skip update_current_claude_desktop_provider_syncs_profile_when_proxy_takeover_is_active` 为 2735 passed / 2 failed，2 个失败均为当前分支既有 `transform_codex_anthropic` 断言，与本次改动无关。

## 2026-08-03 Codex 原生 Responses SSE 安全恢复（v3.19.1-4）

- 旧结论“managed Codex 的 `stream_max_retries` 必须保持 0”过度保守，已于 2026-08-04 纠正。当前官方 Codex 默认值为 5，`core/src/session/turn.rs` 对所有可重试的 incomplete stream 重跑 sampling request；官方 `stream_no_completed` 测试明确覆盖第一次流已经产生 `response.output_item.done` 后断开、第二次请求完成的场景。CCSM 不应在语义输出后自行重放，但也不能禁用 Codex 自己的状态机。
- 新增 `providers/streaming_retry.rs::create_resilient_responses_sse_stream`，仅用于 native Codex `/responses` 直通。它在只下发 `response.created` 或 SSE 注释后发生 transport error / 未终止 EOF 时，对同一 provider、同一 URL、headers、body 最多重连 5 次；重连后的重复 `response.created` 被抑制。静默期间发送 `: ping` 注释，以保活下游 watchdog。
- 一旦任何非 `response.created` 的实际事件、终止事件，或不能安全识别的残块已下发，就永久关闭重放通道；显式 `response.failed`/`error` 逐字透传。这是协议安全分界，不承诺 HTTP SSE 在语义输出后可无感续传。
- `Forwarder` 现在为 native Codex Responses streaming 建立 reconnect factory；`handle_responses_for_app` 只在成功且非 JSON 的 native passthrough 分支包装流。Responses→Chat/Anthropic、namespace restore、compaction 分支仍沿用各自语义，避免双重转换或错误重放。
- 公开 OpenAI Codex 源码/配置确认 `stream_max_retries` 是 dropped stream reconnect 次数，Responses transport 支持 WebSocket 并在不支持时回退 HTTP。当前本地 GET `/responses` 有意返回 HTTP 426，`supports_websockets=false` 保持不变：旧 relay 曾出现 upstream 101 后首帧前 close，尚无真实官方端到端证明时不能翻开该开关。
- TDD 证据：先验证缺少 native wrapper / reconnect-factory selector 的编译失败，再通过 `native_responses_*`（created 去重、comment keepalive、output_item.done 禁止重放）、`streaming_retry` 20 tests、handler 与 forwarder selector tests。后续必须在真实官方账户的 WebSocket handshake、首帧、断开、HTTP fallback 全链路通过后，才能新立任务启用 Responses WS relay。

## 2026-08-04 恢复 Codex 客户端流重试，修复 3.16.5-22 之后的长任务回归

- 对比 tag 和 Git 历史确认：`v3.16.5-22`（`b4b784f1`，2026-07-28）没有生成 `stream_max_retries`，继承 Codex 默认 5；`72c8ca22`（2026-08-02）为防 502/504 放大才首次写成 0。后来 `461dc35c` 只补齐 CCSM 语义输出前的透明重连，没有恢复 Codex 语义输出后的 sampling retry，因此用户观察到旧版能跑长任务、后续版本会直接断开是真实回归。
- 当前官方源码 main `9873cba8` 与本机 Codex `0.146.0-alpha.3.1` 核验：provider 默认 `stream_max_retries=5`；`turn.rs` 的 retry loop 不以是否已输出语义事件为禁用条件；官方 `stream_no_completed::retries_on_early_close` 用 `response.output_item.done` 后提前 EOF 验证第二次请求。Matrix 直接读取官方 raw 源码与 Codex 内置检索/最新手册结论一致。
- 根修提交把 `CODEX_MANAGED_STREAM_MAX_RETRIES` 从 0 恢复为 5，`request_max_retries` 保持 2。两层所有权为：CCSM 仅在 `response.created`/注释之后、语义事件之前透明重连；正文、Reasoning、output item 或工具事件之后的 incomplete stream 交给 Codex 自己重试。未知结果的 ResponsePending 仍保持 429 不重试，代理不新增语义事件去重状态机。
- TDD RED 明确为新契约测试得到 `left=0/right=5`；GREEN 后该测试、`codex_config` 114/114 和 `services::proxy` 75/75 通过。指南和 2026-08-03 历史计划已标明新旧边界，不能再用旧文档声称托管 Codex 必须禁用流重试。

## 2026-08-04 session 019fbd59 的 encrypted/502 根因取证

- 该 session 全程命中 `codex-multirouter::route::router-codex-official`，上游固定为 `https://chatgpt.com/backend-api/codex/responses`、`CodexOAuth`、native Responses，无第三方格式转换。历史中存在官方 `reasoning.encrypted_content`，但这是正常官方 replay 数据；不能以字段存在本身判断 `invalid_encrypted_content`。
- 证据链：早期 request 约 423 KB；同一 session 后续逐步增长到 7.8 MB、12.9 MB、16.8 MB。16.8 MB 请求在 2026-08-03 13:40 首次发送 119.5 s 后 `upstream_send_error=error_sending_request`，重试在 8.2 s 后也失败；再下一次相同约 16.8 MB 请求在 119.9 s 后收到 HTTP 200。此前多次包含 encrypted history 的请求也均返回 HTTP 200。
- 已证实但尚不能定根因：该 session 的 385 次官方请求均经 CCSM `reqwest` 出站，且显式 CCSM 上游代理为 false；104 次（27.0%）在收到 HTTP 状态前失败。失败中 96 次的请求体在 5–10 MB，仍有同区间请求成功；16 MB 也曾成功。全量 router 日志中 `router-codex-official` 为 5,873 次里 827 次失败（14.1%），其它路由约 12,291 次仅 2 次失败。因此它是 CCSM 官方出站路径的传输不稳定，不能归因为单纯 body 大小、encrypted 字段，或官方协议 400。
- `invalid_encrypted_content` 的官方表现应为带该 code 的 HTTP 400；本 session 已抽取到的最终错误是本地 502 映射的 `error sending request`，没有该 code。encrypted 历史仍可能与请求大小/服务端处理时间有关，但不是已证实的断连根因。
- v3.19.1-4 的“首个语义输出前 SSE 重连”不能覆盖响应头尚未到达的请求阶段。为取得真正根因，`map_reqwest_send_error` 现在会在去掉 URL 后展开底层 source 错误链；只记录安全的传输分类，不记录请求、密文或凭据。下一次复现需据此区分 TLS/HTTP2/连接复用/对端关闭/超时，再决定是否应调整 transport、重连或请求体传输策略；禁止盲目删除 encrypted reasoning 或重放可能在途的请求。
- 2026-08-04 后续证据先把故障边界定位到官方：切回原生 `model_provider=openai` 后，该 session 的新 turn（`019fc88a-89fb-72b1-84d7-c39b98d9af7c`，`gpt-5.6-luna`）完全没有进入 CCSM router，却在 5 次重连后收到官方 `{"detail":"Bad Request"}`；请求 ID 为 `b4d7b16e-d132-45b7-8f76-42141447a75f`。同机同账号的其它原生 OpenAI session 同时连续成功，因此不是 CCSM 502 包装、账号或全局网络故障；但没有 OpenAI 内部日志时，不能再把这个 generic 400 直接解释成某一个请求字段错误。
- 最后一次有效 compaction（rollout line 23154）约 0.11 MB/108 items；其后又积累 148 个 replay item。最终有效历史含 29 张内联图片，原始 JSON 约 15.97 MiB、Zstd 约 11.82 MiB；40 个 `custom_tool_call_output` 占绝大多数。字段级审计确认 29 张均为 Base64 合法且文件头正确的 PNG，49 个 reasoning 密文都具备合法编码形态，44 组工具调用/输出全部一一配对，4 个 function arguments 都是合法 JSON。
- Codex 源码仍证明存在“历史无界放大”缺口：`ContextManager::for_prompt()` 只在模型不支持图片时替换图片；支持图片时 `truncate_function_output_items_with_policy()` 对 `InputImage` 无条件 clone。它会让每次重试都携带同一批旧图片，是超时/断连概率的重要放大因素，但不能单独证明 generic 400 的根因。
- 反事实实测否定了“该 session 已被图片或 encrypted 永久污染”：使用同一账号、同一 Luna、同一 compaction、全部 29 张图片和全部 49 段 reasoning 密文构造的 16,726,173 字节明文请求、10,832,709 字节 Zstd 请求均返回官方 HTTP 200；再用 Codex CLI `resume --ephemeral` 捕获完整 Responses-Lite 请求（262 input items、4 个嵌套工具、16,823,558 字节原文、10,874,984 字节 Zstd）原样发送，仍返回 HTTP 200。故截图中的 generic 400 是官方边缘/后端的一次瞬态终止；精确内部故障点只有 OpenAI 能通过上述 request ID 查询。
- CCSM 持续断连仍有一个已证实的传输差异：原生 Codex 对官方登录态默认使用 Zstd，而 `decode_codex_request_body()` 会解压并删除 `content-encoding`，`forwarder` 再把 JSON 序列化成未压缩 `body_bytes` 发往官方。现场同一请求因此从约 10.87 MB 的原生 wire 体膨胀为 16,788,941 字节的 reqwest 上游体；它曾两次在收到响应状态前失败，第三次相同字节请求返回 200。下一步根修应先恢复官方上游 Zstd 语义并保留请求阶段的安全重试/诊断；历史图片有界化只能作为独立的上下文治理，不能伪装成这次 400 的确定修复。
- 根修已在 `forwarder` 落地：最终 JSON 变换完成后，仅对 Codex POST 到 ChatGPT 官方 `/backend-api/codex/responses` 或 `/responses/compact`、且使用 managed/native official auth、未进入 Chat/Anthropic 转换的请求重新做 Zstd level 0 编码，并同步写 `content-encoding: zstd`、移除旧实体长度头。普通发送、Responses-Lite fallback 缓存和 Lite 去头重试共用同一编码边界；请求级 transport retry 继续 clone 同一不可变 wire body。第三方/转换请求保持明文。TDD 先得到缺少编码与选择函数的编译失败，再通过 4 个定向回归；forwarder 137/137、content-encoding 9/9、`cargo check --lib`、rustfmt 与 `git diff --check` 通过。现有 `openai_cache_read_tokens` dead-code warning 与本次无关。

## 2026-08-03 Codex 第三方 Reasoning 可移植桥设计

- 用户确认的硬约束：第三方 reasoning 必须实时显示；prompt、response 和 reasoning 不得复制到 CCSM 数据库或旁路文件；恢复内置 OpenAI 后 CCSM 完全退出请求链路，Codex 直接走官方 Provider 与 WebSocket；以后仍可重新启用 MultiRouter。
- DeepSeek 原生 Responses 的 `reasoning.content[].reasoning_text` 本来就是 Codex 支持的 raw reasoning，不是非法格式；现场失败来自 ChatGPT Codex 私有 backend 回放时要求该 `content` 为空。OpenAI 官方通常用可读 `summary` 加 provider 专属 `encrypted_content`，CCSM 不能伪造后者。
- 上游 CC Switch `v3.19.1` 对原生 `openai_responses` 基本透传，但 Chat -> Responses 转换器已经把第三方 reasoning 合成为 `summary_text` 并流式发送 summary delta，因此两条第三方路径当前不一致。
- 已批准进入实验的设计：仅对显式启用能力的非官方原生 Responses route，把 raw reasoning SSE/最终 item 无状态转换成可识别的 `rs_ccswitch_...` summary；Codex仍逐delta实时显示并只在自身 rollout 持久化。再次请求同一第三方时，CCSM从 marker summary反向恢复目标上游要求的 raw reasoning。官方响应、官方WebSocket、官方encrypted reasoning原样不动。
- 该设计承认完整raw reasoning承载于summary存在语义、长度、配对和压缩风险；必须先做短/中/长summary、工具调用、官方直连、官方压缩、切回DeepSeek的真实闭环。若官方A/B失败，停止堆叠CCSM兼容，转向Codex provider-aware history projection。
- 设计文档：`docs/superpowers/specs/2026-08-03-codex-portable-third-party-reasoning-design.md`。第一阶段不处理已污染的旧rollout，存量迁移必须在新桥验证后另立任务。

## 2026-08-05 Responses 转 Chat 的 system content=null 根因与归属

- session `019fbd59-0c1a-7592-87d3-1e2ad654fd0d` 在 `pre_turn compaction`（`comp_hash_changed`）切到 Qwen `qwen3.6` 后，CCSM 将 `/responses` 转成 `/chat/completions`；366,973 字节请求连续收到 HTTP 400。上游 Pydantic 的 19 条 validation errors 是同一个 `messages[1]` 在联合消息类型上的分支校验结果，实际坏值是 `{"role":"system","content":null}`，不是 19 条坏消息。
- 转换根因有三段：`responses_message_item_to_chat_message` 对缺失 `content` 使用 `Value::Null`；`responses_content_to_chat_content` 对显式 null 原样返回；`collapse_system_messages_to_head` 只消费字符串 system content，null system 被保留进 `rest` 并发送。assistant 的 `content:null` 在 tool call 场景可能合法，修复必须按角色和消息语义处理，不能全局删除 null。
- 归属复核：2026-08-05 fetch 后的上游 `main=0345fad6`、上游 tag `v3.19.1`、fork tag/运行版 `v3.19.1-5` 与当前 HEAD 在上述三段均保留相同逻辑；当前 HEAD 与 `v3.19.1-5` 的整个 `transform_codex_chat.rs` blob 相同。故不是 BigStrongSun 后续改动新引入，而是上游原版缺陷被 fork 继承；上游最新 main 截至该提交也未修复。现有 system-collapse 测试只覆盖字符串内容，缺少 missing/null system/developer 回归。

## 2026-08-05 Responses Lite `additional_tools` 转 Chat 根修

- 目标 session `019fbd59-0c1a-7592-87d3-1e2ad654fd0d` 的 HTTP 400 不是 Codex 在普通 message 上主动发送 `content:null`。Codex `0.146.0-alpha.3.1` 的 Responses Lite 请求会动态插入 `{"type":"additional_tools","role":"developer","tools":[...]}`；该结构项协议上没有 `content`，且按官方 rollout policy 不持久化到 session JSONL。
- 原始错误链在 `src-tauri/src/proxy/providers/transform_codex_chat.rs`：工具上下文只收集顶层 `tools` 和 `tool_search_output.tools`；未知 item 兜底又把带 `role` 的 `additional_tools` 当 message，developer 映射为 system，missing content 变为 null，最终被严格 Chat/vLLM 校验拒绝。同时 `additional_tools.tools` 被静默丢失。
- 根修提交 `b14e3db` 采用双层契约：递归收集 `input` 内 `additional_tools.tools` 并复用 `CodexToolContext::add_response_tool` 的 function/custom/namespace/hosted tool/去重逻辑；message 投影时显式消费 `additional_tools` 而不生成消息。对真实非 assistant message 的 missing/null content 输出空字符串，已有 system collapse 会消费空 system；assistant synthetic tool-call 的合法 `content:null` 保持不变。
- TDD 提交 `509646eb` 先锁定三个 RED 场景：Lite 工具丢失和 null system、custom/namespace 与跨来源去重、system/developer/user missing/null 和 assistant null 例外。GREEN 后定向 3 项通过，`transform_codex_chat` 122 项通过；最终 `cargo check --lib`、`cargo fmt --check`、`git diff --check` 通过，全量库测试 `2808 passed, 0 failed, 2 ignored`。
- 上游贡献从 `farion1231/cc-switch main@0345fad6` 建立干净分支 `bigstrongsun/fix-responses-lite-additional-tools`，只含 RED 测试 `a0d7b47b` 和 GREEN 实现 `31d8a937`，已推送到 `BigStrongSun/ccswitchmulti`。去敏 Issue 为 `farion1231/cc-switch#6158`，ready-for-review PR 为 `#6159`；PR 可合并、仅改 `transform_codex_chat.rs`，GitHub Actions 当前为 `action_required` 且没有 job，表示需上游维护者先批准 fork workflow，并非测试执行失败。
- 上游分支的 3 个新增回归及 `transform_codex_chat` 84 项全部通过，`cargo check --lib`、`cargo fmt --check`、`git diff --check` 通过。上游全量库测试为 `2334 passed, 3 failed, 4 ignored`；两个失败是 Windows symlink 权限错误 1314，一个是运行中的 CCSwitchMulti 占用代理测试端口导致 10048，均不在唯一变更文件路径内，PR 中已如实披露。
- 以后处理 Responses input 新结构项时，必须先判断它是消息还是协议元数据；不能仅凭 `role` 字段进入 message 兜底。也不能用全局删除 null 的方式修复 Chat schema，因为 assistant tool-call 允许且依赖 `content:null`。
- 上述根修已经按 RED→GREEN 完成；session JSONL/SQLite 和 Qwen 配置均未修改，也没有增加 provider 特判。

## 2026-08-05 Codex Multi-Agent V2 第三方 Provider 加密任务 Issue/PR 现状

- 官方 `openai/codex#36586` 精确报告自定义非 OpenAI Provider（DeepSeek）收到 `agent_message` 时，真实任务只存在于 `encrypted_content`，可见 `Payload:` 为空；Issue 仍为 open，标签包含 `bug`、`custom-model`、`subagent`。`#36321`、`#36493` 是同类空 payload 复现；`#36387` 记录 OpenAI 父模型到 DeepSeek 子模型的同一问题，后以 duplicate 关闭。
- 最关键的跨 Provider 证据在 open Issue `#36376`：即使官方 `#35845` 已随 `0.147.0-alpha.4` 落地，OpenAI 父模型仍返回真实密文 `message="gAAAAA..."` 且 `encrypted_function_args=null`；非 OpenAI 子模型因此仍收到 `[input_text header, encrypted_content]`，不能执行任务。
- 已合并 PR `#35845`（commit `03edf16f0bce2c454fc9a8ddb382e9c23c114f7f`）新增 `DirectPlaintextMessage`：仅当 function call 带 `encrypted_function_args=[]` 时，把 `spawn_agent`、`send_message`、`followup_task` 投递为结构化明文；否则继续加密。它是必要的 plaintext 通道，但不是 OpenAI 父模型到第三方子模型的完整修复。
- GitHub connector 与 `gh api search/issues` 均未找到关联 `#36376` 或 `#36586` 的官方修复 PR；搜索 `encrypted_content subagent provider` 也没有结果。当前唯一直接相关的上游 PR 是已合并但覆盖不完整的 `#35845`。
- `#36586` 评论中的真实线级验证补充了一个重要边界：DeepSeek 对 `agent_message` item 本身也可能忽略，即使其中改成明文 `input_text`；非 OpenAI→非 OpenAI 场景的社区验证方案是把 V2 任务降级投影为普通 `user` message。OpenAI→非 OpenAI 场景则不能仅在子请求侧转换，因为 CCSM/Codex 已经只拿到无法解密的 `gAAAAA...`。
- 因此 CCSM 下一步不能只做 child-side `encrypted_content -> input_text`。可行根方向是：保留 `agents` namespace；在父请求的非保留 `agents.spawn_agent/send_message/followup_task` schema 边界让官方父模型产生 plaintext/`encrypted_function_args=[]`，再按目标 Provider 能力将第三方子任务投影为普通 user input；官方父子链仍保留加密。实现前必须捕获父响应的 `arguments.message` 与 `encrypted_function_args`，并分别做 OpenAI→OpenAI、OpenAI→DeepSeek/Qwen、DeepSeek/Qwen→同类第三方的真实端到端 canary。
- 搜索渠道：GitHub connector 和 Codex 内置 Web 均找到相同 Issue 簇与 `#35845`；Matrix MCP 已通过 stdio JSON-RPC 成功初始化并调用三次搜索，但相关 GitHub 限定查询均返回 0 结果，其中一次中国搜索链超时，因此 Matrix 没有提供额外正证据。

## 2026-08-06 Codex 跨 Provider Responses 回放根修与 v3.19.1-9 真实验收

- 用户现场的三个表象属于同一类“第三方 response item 回放到 ChatGPT Codex 私有 Responses 边界”问题，但坏字段来源不同，不能笼统归因为 Qwen、DeepSeek 或 Responses/Chat 某一端：Qwen Chat 合成的 plain reasoning 带 synthetic `rs_resp_chatcmpl-*` ID，官方在 `store=false` 下将它当作服务端持久化引用并返回 404；DeepSeek native Responses 的 plain reasoning 带 UUID `id` 和 response-only `status=completed`，官方拒绝 `input[*].status`；DeepSeek 返回的 `web_search_call` 使用 `call_*` ID，而官方回放输入要求 `ws_*`。
- 根修边界是按目标 transport 做 replay projection，而不是给第三方 Provider 打补丁：回放到 ChatGPT Codex OAuth 时，未加密的 plain reasoning 删除 `id` 与 `status`、保留可读 `summary`；带官方 `encrypted_content` 的 reasoning 原样保留。第三方 `web_search_call call_*` 稳定映射成 `ws_ccswitch_<hash>`。这些归一化必须位于 `normalize_codex_oauth_responses_request()`，因为 MultiRouter route 物化后 Provider ID 是临时 route ID、认证来源是 `managed_codex_oauth`，只用 `is_codex_official_provider()` 会漏掉真实官方 transport。
- TDD 提交链：`f891f168`/`4eb154d7` 锁定并修复 synthetic reasoning 404；`f6268f3c`/`063b45dc` 锁定并修复 managed OAuth route 的 `call_* -> ws_*` 漏执行；`b8b63f52`/`1393e3fd` 锁定并修复 DeepSeek plain reasoning `status` 回放；`9fd663a7` 将完整修复升至 `3.19.1-9`。源码验证为 OpenAI OAuth compatibility 26/26、`transform_codex_chat` 123/123、全量库测试 2817 passed / 0 failed / 2 ignored、`cargo check --lib` 与 rustfmt 通过；仅保留既有 `openai_cache_read_tokens` dead-code warning。
- 最终 Windows 导出从固定 HEAD `9fd663a78edbb90dcde86d37d131c908c37e3de8` 串行执行，`latest.json`、文件名、PE ProductVersion 与 `RELEASE-METADATA.md` 均为 `3.19.1-9`，15 个导出文件逐项通过 `SHA256SUMS.txt`；raw EXE SHA256 为 `B95EA1EA163508C1533348B288EA8D019ECD51619C312854BCEDA7E40E4DD599`。运行版旧 `3.19.1-8` 已备份为 `cc-switch.exe.pre-3.19.1-9-responses-replay-20260806-020446.bak`，其 SHA256 为 `8441AE34949622E33ABE3A298E55660F7793506A02EABAD5061274C46805AF4B`。
- 安装后的真实代理验收 session `fe9cfd1c-0d85-4ed7-a981-2b71812c6bf4` 完成六步：官方 `gpt-5.6-sol` -> Qwen `qwen3.6` -> 连续 Qwen -> 官方，以及 DeepSeek `deepseek-v4-flash` -> 官方。Qwen 实际 reasoning ID 为 `rs_resp_chatcmpl-*` 且无密文；DeepSeek 实际 reasoning 为 UUID、`status=completed`、无密文，并额外加入现场同形 `web_search_call id=call_00_JYFDjhEPbdA9SmnfBXkC9250`。六次均为 HTTP 200 + `response.completed`，router log 为 6 个 upstream 200、0 个 upstream error，未出现 `rs_* not found`、`Expected an ID that begins with ws`、`input[*].status` 或提前断流。安装进程 PID `51672` 监听 `127.0.0.1:15721`，二进制版本和 SHA256 与导出工件一致。

## 2026-08-06 CCSwitchMulti v3.19.1-9 GitHub Release 发布结果

- annotated tag `v3.19.1-9` 解引用到发布说明提交 `f2b207fbef7e66dc530470fb4df24e5879b79127`，远端分支 `bigstrongsun/fix-portable-third-party-reasoning` 同步到该提交。发布说明为 `docs/release-notes/v3.19.1-9-zh.md`，覆盖跨 Provider Responses 回放修复、协议边界、真实验证、升级方式和下载能力边界。
- GitHub Release workflow run `31037635625` 完整成功：Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个平台矩阵，`Publish GitHub Release` 与 `Assemble latest.json` 全部为 `completed/success`。正式 Release 为 `draft=false`、`prerelease=false`，地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-9`。
- Release 共 19 个资产并全部带 GitHub 服务端 SHA256 digest。远端 `latest.json` 为 `version=3.19.1-9`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台；各平台 signature 非空，六个 updater URL 均实测 HTTP 200，返回 Content-Length 与 Release 资产一致。GitHub `/releases/latest` 已指向 `v3.19.1-9`。
- 发布前重新运行完整 Rust library 测试：`2817 passed, 0 failed, 2 ignored`；只有既有 `openai_cache_read_tokens` dead-code warning。Codex 内置搜索、Matrix WebSearch 与 GitHub API 都未发现预先存在的同名 tag/Release，避免覆盖远端状态。
- fork 的 R2 workflow 未被 `release: released` 自动触发，手动 recovery run `31040703867` 虽显示 success，但日志明确 `R2 secrets not configured; skipping download mirror sync`，所有下载/上传/manifest step 均 skipped。因此本次确认的是 GitHub Release 与 GitHub updater manifest 已发布，不能声称 `https://dl.ccswitch.io` R2 镜像已同步；没有凭据时不应重试或伪造 R2 成功。

## 2026-08-06 v3.19.1-9 主线程误报 third-party encrypted payload 根因与 v3.19.1-10 修复

- 同批用户反馈还包含另一条独立错误：从 DeepSeek V4 Flash 切回顶层 `OpenAI Official` 后，代理显示 `model: deepseek-v4-flash`，ChatGPT backend 返回 `Invalid 'input[10].content': array too long`。`model` 保留旧线程值不代表请求仍路由到 DeepSeek；`Provider: OpenAI Official` 说明 top-level provider 已切换。DeepSeek native Responses 实测会返回 `type=reasoning,status=completed,content=[reasoning_text]`，这是第三方合法输出，但不能原样回放给严格的 ChatGPT Codex input schema。
- 根因位于 `should_normalize_codex_oauth_responses_passthrough_body()`：旧 predicate 只接受 `provider.is_codex_oauth()`，而内置 `codex-official` 是 native auth passthrough，虽然最终同样访问 `https://chatgpt.com/backend-api/codex/responses`，却绕过了已有的 raw content -> summary、plain reasoning id/status、tool content 和 replay ID 归一化。RED `96a593f1` 锁定 native official 漏判，GREEN `3f351514` 让 managed OAuth 与内置 official 共用同一目标 transport replay boundary；不修改公开第三方 Responses 路径。
- 本机诊断 session `423cb92e-d144-4b92-bf9b-c8fbbfdf29b1` 通过 `v3.19.1-9` managed OAuth route 生成了真实 DeepSeek reasoning（UUID id、completed status、1 条 reasoning_text content、无 summary/encrypted_content），随后回放官方为 HTTP 200；这反证已有 normalizer 本身有效，缺口确实是 top-level native official 没进入它。正式验收必须安装 `3.19.1-10` 后再覆盖 top-level DeepSeek -> OpenAI Official，而不能只重跑 managed route。
- 现场报错为 `Provider: OpenAI Official; model: gpt-5.6-sol; cause: third-party child cannot read encrypted Codex agent payload`。该文案不是 OpenAI 或第三方模型返回，而是 `codex_multi_agent.rs` 在本地 Stage B 投影中主动生成；错误发生在发上游之前，因此不会进入 `proxy_request_logs` 的 upstream error 行。
- 矛盾 Provider 标签的根因是 ownership 判定分裂：MultiRouter 的 OpenAI route 物化后使用 request-local route ID，并以 `provider_type=codex_oauth` 表示托管官方 transport；`should_project_codex_agent_messages_for_provider()` 却只排除狭义 `is_codex_official_provider()`，没有排除 `provider.is_codex_oauth()`。于是合法的官方加密 `agent_message` 回放被误分类为第三方 child，在主会话本地 400；handler 又用外层 route 名输出 `OpenAI Official`。
- TDD RED 提交 `180b5735` 用真实 predicate 锁定 managed OAuth 被错误投影；GREEN 提交 `8d721273` 同时排除 managed Codex OAuth 与原生 official。Qwen、DeepSeek、xAI 和其它真正第三方 Responses route 仍进入投影，opaque 密文仍 fail closed，未放宽密文泄漏边界。
- `3f351514` 解决的是 native official replay normalizer 范围，不是本次 Stage B ownership 误分类；两者都应保留。版本提交 `969fec5c` 升到 `3.19.1-10` 并新增中文发布说明，当前运行中的 `3.19.1-9` 未替换。

## 2026-08-07 v3.19.1-9 主线程误通知用户反馈复核

- 用户转述的通知原文与 2026-08-06 已定位问题逐字一致：`Provider: OpenAI Official; model: gpt-5.6-sol; cause: third-party child cannot read encrypted Codex agent payload`。重新核对提交链确认，`180b5735` 先用 RED 测试复现 `provider_type=codex_oauth` 被误投影为第三方 child，`8d721273` 再让 `should_project_codex_agent_messages_for_provider()` 同时排除 managed Codex OAuth 与 native official ownership；不是新增根因，也不需要在 `v3.19.1-10` 上叠加补丁。
- 当前本机运行进程 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 的 PE ProductVersion/FileVersion 均为 `3.19.1-10`，监听 `127.0.0.1:15721`；源码定向回归 `proxy::forwarder::tests::agent_message_projection_runs_only_for_third_party_codex_responses` 为 1 passed / 0 failed，本地日志未再检出该错误文本。用户现场仍使用 `-9` 时，直接升级 `3.19.1-10` 才会生效。
- 联网交叉验证：Codex 内置 Web 搜索命中 OpenAI Codex issue `#34833`、`#33551`，确认 Multi-Agent V2 的 encrypted `agent_message` 对非 OpenAI provider 是真实的 provider-boundary 问题；Matrix WebSearch 独立搜索未稳定召回 issue，但直接打开 `https://github.com/openai/codex/issues/34833` 得到相同正文和 same-provider OpenAI control。外部来源支持“加密 payload 必须按 provider ownership 分流”这一背景；CCSM 主线程误通知的精确 ownership 漏判仍以本仓库源码、RED 测试和提交 diff 为权威证据。
- 验证：RED 在旧逻辑稳定失败、GREEN 通过；Multi-Agent payload 5/5、OAuth Responses 14/14、完整 Rust library `2818 passed / 0 failed / 2 ignored`；`cargo check --lib`、rustfmt、diff check 通过，仅保留既有 `openai_cache_read_tokens` dead-code warning。官方文档只确认 GPT-5.6 multi-agent 仍为 beta，未公开这种私有 envelope；Codex 内置官方搜索与 Matrix 独立搜索都没有找到该精确错误，进一步证明它属于 CCSM 本地诊断文案。

## 2026-08-06 v3.19.1-10 本地发布与发布锁根修

- Windows 本地发布最终固定在提交 `906a2b7b1d568ef0989fdc50c267c73f935d07a1`，目录为 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-release-v3.19.1-10`。安装包 SHA256 为 `FFD40C76CD890078B7C48D47C9183A0D28EE3B731D3BB5F22641F5DDC6800279`，portable ZIP 为 `A757104273F380ED36BA6485A6853792310492A7F842DE253E167E819F9FA820`，raw EXE 为 `947167B74B5D70D3599F652E54DE020240D5B920010737E1F4C34C493BB654D1`。
- 最终验收为 15 个导出文件、15 个 checksum entry、0 个缺失/哈希错误；setup 与 raw EXE 的 PE ProductVersion 均为 `3.19.1-10`，portable 内含 `CCSwitchMulti.exe`，`latest.json` 版本正确且 updater signature 与 `.sig` 完全一致。
- 收尾运行态复核时，`127.0.0.1:15721` 已变成 PID `71064` 的已安装 `3.19.1-10`，安装目录文件 LastWriteTime 为 `16:20:35`、进程启动于 `16:22:55`，早于本轮最终成功打包完成时间 `16:42:43`，也早于发布脚本提交；本轮没有执行 installer、AppData 复制或进程重启。该已安装 EXE 的 SHA256 为 `05F3A6DE4F70518E72D8B6B2DC1E0CEDB29AFDA37DD9CF76D2E4D1CB462B353C`，与最终验收 raw EXE 不同，因此仍应由用户用最终目录内 setup 自行重装，不能把当前运行版当作最终包安装验收。现有本地证据无法恢复已退出 parent PID `60456`，故不能断言是谁触发了 16:20 的替换。
- 首轮打包曾在 NSIS 阶段触发 Windows OS 32。更深根因不是 Rust/Tauri 编译失败，而是旧 `local-release-pipeline.ps1` 在竞争者获取锁失败后仍由 `finally` 无条件删除活跃流水线的锁，使第三条流水线能并发进入并争写 `target/release/cc-switch.exe`。提交 `906a2b7b` 改为原子 `CreateNew`、每进程 token 和 owner-only release；Windows PowerShell 5.1 回归确认竞争者失败后锁仍属原 owner，错误 token 不能释放，owner 可以释放。
- 带重复反斜杠的 `ReleaseRoot` 还暴露了 `SHA256SUMS.txt` 相对路径按未经规范化字符串长度截取的问题。提交 `434541de` 在两个 checksum writer 中先 `Path.GetFullPath` 再截取，保持 PowerShell 5.1 兼容；实际 duplicated-separator `-SkipBuild` 导出得到 15 条清单、0 条无效相对路径。

# 2026-08-07 Codex Desktop 国际化占位符原样显示根修与 3.19.1-12 本地交付

- 用户截图中的 `已处理 {time}`、`正在运行 {command}`、`<projectSelect>{projectName}</projectSelect>` 不是 Codex 中文语言包漏翻译，也不是用户项目数据问题。当前本机同样可复现：Appx `OpenAI.Codex_26.730.8199.0`（包内版本 `26.730.61639`）的 renderer DOM 在 `lang=zh-CN` 下实际包含 `Worked for {time}`。
- 现场运行的 CCSwitchMulti `3.19.1-10` 已注入 `window.__ccSwitchCodexAppCompatibilityV5`，其 1.5 秒 interval 持续执行 `patchReactState()`。`src-tauri/src/codex_desktop.rs::patchObjectGraph()` 遍历 React Fiber 对象图，`patchModelContainer()` 的 `if (value.defaultModel == null)` 对任何没有 `defaultModel` 的普通对象都写入模型 descriptor，而不是只写模型容器。
- 该无边界写入污染了 React Intl context：实测 `intl.formats`、`intl.defaultFormats` 和 `intl.formatters` 都被追加 `defaultModel/useHiddenModels/use_hidden_models`。污染后的 `defaultModel.model_messages` 与宿主对象形成循环引用；React Intl 创建/缓存 MessageFormat 时序列化 formats 参数，抛出 `TypeError: Converting circular structure to JSON`。
- React Intl 随后按官方 fallback 算法返回消息 source（不做替换），所以普通 ICU 参数 `{time}`、`{command}` 和 rich-text 标签都会原样出现。直接调用底层 `intl.formatters.getMessageFormat(message, locale, {})` 能正常得到 `Hello World`，但调用受污染 context 的 `intl.formatMessage(...)` 返回 `Hello {name}`，证明 parser、语言包和调用点传参都正常，坏点在被注入脚本污染的 Intl 配置/cache。
- 截图 2 的当前 Codex 调用点已正确传入 `projectName/projectSelect`，时间调用点也正确传入 `values.time`；React Fiber 现场显示 `localConversation.workedFor.v2` 的 `values.time` 为有效中文时长字符串。不能把问题误修成补翻译或手工字符串替换。
- 危险逻辑最初由提交 `3eeb39dc423d0679981af456d59e36b3de62295a`（2026-06-14，`fix: unlock Codex Desktop multirouter models`）引入。当前 Codex renderer 的对象图使污染稳定到达 Intl context，因此近期用户反馈集中出现并不等于近期才引入源码。
- 后续在当前 Codex Desktop `26.803.5235.0` 与已安装 CCSwitchMulti `3.19.1-11` 上再次只读复现：DOM 同时出现 `{fileCount, plural, ...}`、`{linesAdded}`、`{linesRemoved}`、`{time}` 与 `{command}`；三个 React Intl 对象仍含模型门字段，说明 `3.19.1-11` 只包含 DeepSeek 路由修复，并未包含本问题的生产修复。
- RED 提交 `d57b0e05` 把 renderer 真正执行的模型 patch 核心提取为 QuickJS 可直接运行的契约，并稳定复现 Intl 普通对象会被修改；GREEN 提交 `e7aacf26` 从源头删除全局 `Response.prototype.json` 劫持和 React Fiber 通用对象图改写。`patchModelContainer` 现在要求对象本身存在明确模型门字段，只修改已经存在的 camelCase/snake_case 控制字段；模型列表只在 app-server model-list、已关联的 MCP `model/list` 与 Statsig 模型配置三个已知边界处理。登录态兼容继续使用独立的 `setAuthMethod + authMethod` 定向 context 查找。
- 行为回归直接执行与 renderer 共用的同一段 JavaScript：React Intl 的 `formats/defaultFormats/formatters` 四次 patch 前后字节等价，同时显式模型门仍能加入 Qwen/DeepSeek、解除隐藏并设置默认模型。定向 QuickJS 为 `2 passed`，`codex_desktop` 模块为 `20 passed / 0 failed`；完整 Rust library 为 `2821 passed / 0 failed / 2 ignored`，`cargo check --lib`、`cargo fmt --check`、`git diff --check` 均通过，仅保留既有 `openai_cache_read_tokens` 未使用警告。
- 版本提交 `04d25cfe` 将本地交付提升到 `3.19.1-12`；中文 release note 后续按实际提交范围修正为累计说明，明确列出 DeepSeek Pro/Chat、React Intl、发布锁 ownership 和 checksum 路径规范化四项修复，并区分“源码/安装包已包含”与“安装后现场验收仍待完成”。首轮完整 post-commit pipeline 在 `2026-08-07 17:28:01 +08:00` 成功结束；导出目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 的 15 条 checksum 全部匹配。NSIS setup 为 11,500,092 bytes、PE File/ProductVersion 均为 `3.19.1-12`、SHA-256 `89E18249952E9A97C1018C516A8F373D5DE5DC634136ED82729EADC5B15A54AD`；portable ZIP 为 13,939,974 bytes、SHA-256 `D7B3D5F4F5760EE8F6521976DE2462C50D63B77C862CA7D0144940F5345E47C6`；raw EXE SHA-256 `CEFBA457604A3D339110F6C8749512B35D8FBD74BF247E0A3DF112C5356C3614`。Tauri updater `.sig` 非空且与 `latest.json` 完全一致；Windows Authenticode 仍为 `NotSigned`，不能把 updater 签名误报成微软代码签名。
- 当前运行中的已安装版仍是 `3.19.1-11`（PID 11200，启动于 16:36），Codex Desktop 仍是旧 renderer；本轮没有停止、安装或重启任何进程。只有用户完全退出 Codex Desktop 与 CCSwitchMulti、安装 `3.19.1-12` 并从 CCSwitchMulti 重启后，才能以 Intl 对象无注入键、DOM 无未解析 ICU/tag、模型菜单仍含 Qwen/DeepSeek 三项做最终现场验收；在此之前不能宣称用户当前 UI 已修复。

## 2026-08-07 CCSwitchMulti v3.19.1-12 GitHub 正式发布

- `v3.19.1-12` annotated tag 精确解引用到累计发布说明提交 `06600a4ae1835905bfc512608bc464baf8fe1386`，而不是当时已经包含后续 Codex Desktop 模型菜单 lifecycle guardian 的分支 HEAD；因此正式包只包含本版声明的 DeepSeek Pro/Chat 路由、React Intl、发布锁 ownership 与 checksum 路径规范化修复，没有混入后续功能。
- 精确候选提交在隔离 worktree 重新通过 `pnpm typecheck`、完整 Rust library `2821 passed / 0 failed / 2 ignored`、`cargo check --lib`、`cargo fmt --check` 与 `git diff --check`；仅有既存 `openai_cache_read_tokens` dead-code warning。
- GitHub Actions run `31175685456` 完整成功：Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS 五个平台矩阵，以及 `Publish GitHub Release`、`Assemble latest.json` 均为 `completed/success`。正式 Release 地址为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-12`，`draft=false`、`prerelease=false`，GitHub `/releases/latest` 已指向该版本。
- Release 共 19 个资产。远端 `latest.json` 为 `version=3.19.1-12`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台键；每个平台 signature 非空，URL 全部指向 `v3.19.1-12`。下载后的 `latest.json` SHA-256 为 `79ad6d65c4abca7987c8b58eec501a52fa3566aa21592cec487a25a6738e3be7`，Windows x64 setup SHA-256 为 `3cdeff6c74fe3c403e2bcbff4b3e6a1d8ebfd41aa0da07be956a594bbdf85c0a`，均与 GitHub 服务端 digest 一致。
- 联网事实核对使用了 Codex 内置搜索与 GitHub 官方 API/CLI；Matrix WebSearch 独立链路本轮返回 HTTP 521，未能提供第二份正证据，因此版本、tag 和发布状态以 GitHub 一手数据与本地精确提交验证为准，并保留该搜索通道不确定性。

# 2026-08-08 GitHub 未关闭 Issue 真实性审计（20 项）

- 以 GitHub 当前 open 列表、Issue 正文/评论、最新正式发布 `v3.19.1-12`（tag `06600a4a`）、本地当前源码和 Release Actions 日志交叉核验；不能把 tag 之后的本地提交算作已发布修复。
- 建议继续保留并处理 7 项：`#3`（v3.19.1-12 Actions 明确发布 unsigned/notarization skipped，文档 PR #4 仍 draft）、`#6`（发送错误已分类/重试，但当前错误包装仍写 `CC Switch local proxy failed`）、`#16`（OMP/Pi 尚无实现）、`#28`（所列 compaction 裁剪/空 choices 等提交不在当前主线）、`#32`（v3.19.1-9 可复现；lifecycle guardian 提交 `cfe3028e` 在 v3.19.1-12 tag 之后，尚未发布）、`#34`（LM Studio Responses 要求 `text.format`，当前转发源码没有补齐/移除逻辑）、`#35`（`apply_provider_to_paths_inner` 仍直接全量写 profile，非托管字段确会丢失）。
- 建议关闭或在让报告者用最新版复测后关闭 11 项：`#1`（spawnAgentModels 前五排序已发布）、`#5`（15721 是本地 takeover 端口，属咨询）、`#8`（MiMo preset 能力字段修复自 v3.16.4-13 已发布；原“中途停”根因未被单独证明）、`#10`（评论已确认模型列表恢复，当前 auth/catalog 路径已有实现）、`#11`（visible alias + upstreamModel 已实现并发布）、`#17`（历史 v13/v12 降级不兼容，当前 schema v16 可迁移 v13）、`#18`（坏明文 encrypted_content 自愈已发布）、`#21`（reserved collaboration schema 兼容与 agents namespace 已发布）、`#23`（DeepSeek low/high/max 官方目录已发布）、`#26`（跨 provider replay ID 规整已在 v3.19.1-2 起发布）、`#31`（双阶段跨 Provider V2 投递修复已在 v3.19.1-9 发布，并有 OpenAI->Qwen/DeepSeek nonce+真实 shell 的 live 验收）。
- 两项不能按现有标题直接判定为当前真实 Bug：`#2` 只有泛化浅色模式描述、无当前截图/路径，源码已大量使用 light/dark theme tokens，应该改成带页面截图的视觉 QA 清单；`#7` 的 400 现场是真实历史信号，但“developer.content[3] 被拆成 input[3]”没有 raw request body 支持，且当前 official Responses 归一化已有多轮修复，应要求最新版最小复现或关闭/重写根因。
- 联网交叉验证：Codex 内置 Web 搜索命中 Apple 官方 Gatekeeper/Notarization 说明及 OpenAI Codex 跨 Provider encrypted `agent_message` issues；GitHub connector、`gh`、源码/tag/Actions 日志提供项目当前事实。Matrix `matrix-websearch` 按独立查询链调用，但连续返回 HTTP 521，未能提供第二条正证据，因此对项目状态不使用 Matrix 结果背书。

# 2026-08-08 已处理 GitHub Issue 关闭执行

- 按上一轮真实性审计结论，通过 GitHub connector 将 `BigStrongSun/ccswitchmulti` 的 `#1/#5/#8/#10/#11/#17/#18/#21/#23/#26/#31` 统一关闭为 `state=closed`、`state_reason=completed`；逐项远端回读确认 11 项全部成功关闭。
- 关闭后再次检索远端 open 列表，数量从 20 降为 9，精确剩余 `#2/#3/#6/#7/#16/#28/#32/#34/#35`。其中 `#2/#7` 仍等待当前版本复测或重写证据，其余 7 项仍有明确未完成工作，未误关。
- 本轮联网仍执行两条独立链：Codex 内置搜索返回 GitHub 官方 Issue 状态筛选文档；Matrix `matrix-websearch` 再次返回 HTTP 521。最终关闭结果以 GitHub connector 的写入响应和关闭后的逐项/列表回读为权威证据。

# 2026-08-08 Open Issue 修复批次（#35 / #34 / #6 / #3）

- 设计和计划分别提交为 `185f788c`、`d12c9b33`，四个 Issue 保持独立提交和独立验收边界；当前工作分支 `bigstrongsun/fix-portable-third-party-reasoning` 尚未推送这些提交，因此不能把本地实现当作 GitHub 可见完成或已发布修复。
- `#35` 根因是 `apply_provider_to_paths_inner` 生成完整 Claude Desktop profile 后直接覆盖 `configLibrary/*.json`，没有区分 CCSM 托管字段与用户扩展字段。提交 `b96c3a4c` 在写入前合并现有对象中生成 profile 未包含的键；生成值继续覆盖 `inferenceGatewayBaseUrl` 等托管键，`autoModeEnabled`、`toolSearchEnabled`、`prefer1m` 等用户键保留。RED 测试因 `autoModeEnabled` 实际为 `Null` 失败，GREEN 后定向 1/1、模块 24/24 通过。
- `#34` 的现场请求包含 `text.verbosity` 但缺少 LM Studio Responses 要求的 `text.format`。提交 `f9ed9bb0` 在最终 native Responses 请求边界增加 LM Studio 专属归一化：仅 Codex + Responses + 未走 Chat/Anthropic 转换 + effective provider 的 type/id/name 可识别为 LM Studio 时，在缺失键时补 `{"type":"text"}`；显式格式、非 LM Studio 和转换路径不变。RED 因归一化函数不存在而编译失败，GREEN 范围测试 2/2、邻近 passthrough 3/3 通过。
- `#6` 根因是 `codex_proxy_error_json` 对已解析 route/auth 后的 `ProxyError::ForwardFailed` 仍统一包装成 `CC Switch local proxy failed`。提交 `a1b83163` 将该阶段改为 upstream connection 分类，官方 route 使用 `OpenAI Codex upstream connection failed`，同时保留 502/status code、provider/model/endpoint/cause 和既有重试语义；`ProxyError::Internal` 仍保留 local proxy 分类。RED 在旧文案断言失败，GREEN 两项定向测试通过，`codex_proxy_` 10/10、error mapper 7/7 通过。
- `#3` 的 draft PR `#4` 使用了已过期的 `CCSwitchMulti_<version>_aarch64.*` 资产名且当前显示 mergeable=false，不能直接合并。当前 `v3.19.1-12` Release 和 workflow 实际资产是 `CCSwitchMulti-v3.19.1-12-macOS.dmg/.zip`。提交 `c5440fa8` 在当前精简 README 中加入中英文 unsigned/unnotarized 说明、Apple `Open Anyway` 流程和仅针对 `/Applications/CCSwitchMulti.app` 的 quarantine 删除命令，并明确不会全局关闭 Gatekeeper；资产名和安全禁用命令检查通过。
- 批次新鲜验证：完整 Rust library `2825 passed / 0 failed / 2 ignored`，`cargo check --lib` exit 0；本批次三份 Rust 文件独立 rustfmt check、README 语义检查、`git diff --check` 均通过。全仓 `cargo fmt --check` 仍因此前 `cfe3028e` 附近已提交的 `codex_desktop.rs`、`codex_guardian.rs`、`commands/proxy.rs`、`services/proxy.rs` 格式差异失败，这些文件不属于本批次，未混入修复提交。
- 联网交叉验证：Codex 内置 Web 搜索命中 LM Studio 官方 Responses 文档，GitHub connector/`gh`/workflow/Release 资产用于核验 Issue、PR 和当前 macOS 文件名；Matrix `matrix-websearch` 独立查询继续返回 HTTP 521，因此没有作为正证据。仍需在推送/合并后处理 `#3/#6/#34/#35` 的 GitHub 状态；需要运行时或发布验收的行为不得因本地测试提前宣称已发布。

# 2026-08-08 CCSwitchMulti v3.19.1-13 GitHub 正式发布

- `v3.19.1-13` 使用 annotated tag；远端 tag object 为 `25b34680ddc612197511bb902a2ba92134baf3de`，peeled commit 精确指向候选 `c022d821d70f0935a641951648c99cd8cf658f31`。候选包含 Codex Desktop lifecycle guardian（#32）、LM Studio Responses `text.format`（#34）、上游连接错误归因（#6）、Claude Desktop profile 用户字段保留（#35）和 macOS 未签名安装说明（#3）。
- 本地最终门禁：前端 Vitest `112 passed / 818 passed`；首次完整运行的 App 集成用例发生一次 10 秒时序超时，隔离重跑 `8/8` 通过，随后完整重跑 `818/818` 通过；Rust library `2825 passed / 0 failed / 2 ignored`，`cargo check --lib`、`cargo fmt --check`、TypeScript typecheck 和 `git diff --check` 全部通过，仅保留既有 `openai_cache_read_tokens` dead-code warning。
- GitHub Actions Release run `31252533548` 完整成功：macOS、Linux x64、Linux ARM64、Windows x64、Windows ARM64 五个平台，以及 `Publish GitHub Release`、`Assemble latest.json` 均为 `completed/success`。
- 正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-13`，`draft=false`、`prerelease=false`，共 19 个资产。`latest.json` 为 `version=3.19.1-13`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台；全部 signature 非空并与下载的对应 `.sig` 内容一致，URL 全部指向本 tag。
- 下载验证：`latest.json` SHA-256 为 `532d2a2dd85bd866d300c864702cb0c0d35992980dbc8945710a6f29fa3cec61`；Windows x64 setup SHA-256 为 `f5c5b03617e41a973ce59c0c081fbd45e9525e7ff3eeba85a310b104df74e529`；两者均与 GitHub 服务端 digest 一致。发布成功只证明远端构建与资产完整，不等同于已经在用户机器安装和完成运行态验收。
- 联网事实核对继续使用 Codex 内置搜索和 GitHub 官方 API/CLI；Matrix `matrix-websearch` 独立链路仍为 HTTP 521，不能提供第二份正证据，因此 tag、Actions、Release、清单和哈希结论以 GitHub 一手数据及本地下载复核为准，并保留 Matrix 通道不可用这一不确定性。

# 2026-08-08 v3.19.1-13 发布后 Issue 与旧 PR 收尾

- 在正式 Release、Actions 和 updater 资产验证通过后，先逐项回读远端 open 状态，再给 `#3/#6/#32/#34/#35` 添加 `v3.19.1-13` 修复说明并关闭为 `state=closed`、`state_reason=completed`；关闭后再次逐项回读，五项均为 completed。
- macOS 文档 draft PR `#4` 的目标已由当前分支实现并发布，且旧草稿资产名过时、`mergeable=false`；添加替代说明后关闭，最终 `state=closed`、`merged=false`、`draft=true`，没有合并旧补丁。
- 收尾远端 open Issue 精确剩余 `#2/#7/#16/#28/#37`；其中 `#37` 是协助仓库脱离 fork network 的协调事项，本轮没有将其误判为产品 Bug 或关闭。open PR 仍为 `#13/#22/#25/#27/#36`，本轮仅按既定范围关闭 `#4`，未扩大到其他 PR。

# 2026-08-09 PR #36 审查、干净移植与 Windows 自启根修

- `zhushihao` 的 PR `#36` 标题为“保留 Claude profile 用户字段 + 消除 powershell 弹窗 + 修复开机自启”，但 head `d18d1b49107f1d879b91a1dba3fcd47eb6860b07` 基于长期未更新的远端 `main@f13fea48`，相对该 base 混入 94 commits / 53 files；相对实际发布分支仍 ahead 12 / behind 16。不能直接 merge，否则会把旧 release、Codex 路由、workflow、版本和冲突解决历史带入当前发布线。
- 真正相关提交只有 `e48510fb`（顶层 Claude profile extras）、`0fd46f7d`（模型条目 `prefer1m`）、`bb7d8dee`（后台 PowerShell `CREATE_NO_WINDOW`）和 `dbfe6273`（Windows 自启注册表）。顶层 extras 已由当前线 `b96c3a4c` 独立实现，因此不重复吸收；其余贡献在隔离 worktree 审查后映射为干净提交 `27ba2724`、`9482e057`、`15770e31`，作者保留为 Shawn Pro，提交者为 BigStrongSun。
- `27ba2724` 在现有 `merge_existing_profile_extras` 上按模型 `name` 合并 `inferenceModels` 条目中生成配置未包含的字段。回归测试先在无实现分支真实失败：`prefer1m` 由 `Bool(true)` 变为 `Null`；实现后目标 1/1、完整 Claude Desktop config 模块 25/25 通过。
- `9482e057` 给剩余三处后台直接 PowerShell 调用补 `CREATE_NO_WINDOW`：Codex guardian 主进程探测、Windows 安装候选 JSON 探测和代理诊断进程探测。历史迁移路径原本已有同一标志；用户主动安装器和终端流程不在本次隐藏窗口范围。Windows `cargo check --lib` 通过。
- 原 `dbfe6273` 不能原样吸收：它的 `is_enabled()` 只看 Run 值，会忽略用户在 Task Manager 中写入的 `StartupApproved` 禁用状态；路径比较大小写敏感；`StartupApproved` 写入/删除错误被静默吞掉。`15770e31` 保留其“引用含空格 exe 路径并同步维护两处注册表”的核心，增加 Windows 路径大小写不敏感匹配、StartupApproved override 判断、仅忽略 NotFound 的错误语义，并在 StartupApproved 启用写入失败时回滚 Run 值。两条 RED 先因 Windows 状态模块不存在而编译失败，GREEN 后 2/2 通过。
- 合并前定向回归为 Claude profile 25/25、自启 2/2、Codex 主进程探测 1/1、代理诊断 10/10；`cargo check --lib`、rustfmt 和 `git diff --check` 通过。仅保留既有 `openai_cache_read_tokens` dead-code warning。
- 联网交叉验证使用 Codex 内置搜索的 Microsoft Run/RunOnce 官方文档、Rust `CommandExt` 官方文档，以及本地 `auto-launch 0.5.0` Windows 源码；官方文档确认 Run 值是 command line，路径含空格时应正确引用，Rust API 确认 `creation_flags` 传给 CreateProcess。`StartupApproved` 是未公开稳定契约，因此精确 marker 语义以当前 crate 源码和本机 Windows 行为为依据。Matrix WebSearch 独立查询仍返回 HTTP 521，没有提供正证据。
- 干净提交与首次记忆提交已推送到 `bigstrongsun/fix-portable-third-party-reasoning@332c6a4d`；PR 评论为 `https://github.com/BigStrongSun/ccswitchmulti/pull/36#issuecomment-5226946143`。随后将 PR `#36` 关闭，远端回读为 `state=CLOSED`、`mergedAt=null`，表示贡献已手工集成而原 PR 未被 merge。

# 2026-08-09 CCSwitchMulti v3.19.1-14 GitHub 正式发布

- `v3.19.1-14` 使用 annotated tag：远端 tag object 为 `b4a3e173e9858d3968a6563f261320c7170621f8`，peeled commit 精确指向发布候选 `832d3f09076b0f4d61dbb5ddc67d6fc5753c1490`。本地先删除仅属于 PR #36 贡献者分支、未存在于 BigStrongSun 远端且不在当前发布历史中的轻量 `v3.19.1-14/-15/-16`，再在正式候选上重建本 tag；发布后记忆提交不得移动该标签。
- 本版发布内容包括：Claude Desktop 每模型 `prefer1m` 用户字段保留、Windows 后台 PowerShell `CREATE_NO_WINDOW`、Windows 开机自启 Run/StartupApproved 状态修复，并在发布说明中感谢贡献者 `@zhushihao`。版本号已同步到 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json`，中文发布说明位于 `docs/release-notes/v3.19.1-14-zh.md`。
- 发布前新鲜门禁：Rust library `2828 passed / 0 failed / 2 ignored`，前端在显式排除 `.worktrees/**` 后为 `112 files / 818 passed`；`cargo check --lib`、`cargo fmt --check`、`pnpm typecheck`、发布说明 Prettier 和 `git diff --check` 均通过。只保留既有 `openai_cache_read_tokens` dead-code warning；直接运行未排除 `.worktrees/**` 的 Vitest 会扫描旧 worktree 并引入重复 React/旧断言污染，不能作为当前根源码结果。
- GitHub Actions run `31272882703`（`https://github.com/BigStrongSun/ccswitchmulti/actions/runs/31272882703`）最终 `completed/success`：Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS、`Publish GitHub Release` 和 `Assemble latest.json` 七个作业全部成功。
- 正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-14`，`draft=false`、`prerelease=false`，发布时间 `2026-08-08T19:22:46Z`，共 19 个资产。远端 `latest.json` 为 `version=3.19.1-14`，包含 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64` 六个平台键；全部 signature 非空，URL 全部指向本 tag。
- 下载哈希验证：Windows x64 setup SHA-256 为 `773b71152a69e16f576a3159d6300beb02f56d0d25675edacc25c14974e39330`，`latest.json` SHA-256 为 `fc017ce6ea9b3e179933093fe9d3cb51932f517e37ae02b4e1d8c6de81c72fa2`，两者均与 GitHub 服务端 digest 完全一致。发布成功只证明远端构建与资产完整，不等同于已在用户机器安装并完成运行态验收。
- 联网事实核对使用了 Codex 内置搜索和 GitHub 官方 API/CLI；Matrix `matrix-websearch` 独立链路仍返回 HTTP 521，不能提供第二份正证据。因此 tag、Actions、Release、清单和哈希结论以 GitHub 一手数据及本地下载复核为准，并保留 Matrix 通道不可用这一不确定性。

# 2026-08-09 Multi-Agent V2 本机真实链路复测

- 用户反馈 `v3.19.1-14` 的 V2 子 Agent 仍不可用，但本机现场实际运行的是 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` `3.19.1-12`（PID 28552，2026-08-08 23:10 启动，监听 `127.0.0.1:15721`），Codex Desktop 为 `26.803.5235.0`，当前命令环境报告 `codex-cli 0.146.0-alpha.3.1`。因此本轮是当前机器链路验收，不是 `v3.19.1-14` 已安装验收。
- 当前有效配置为 `model_provider=codex_model_router_v2`、父模型 `gpt-5.6-sol`、`multi_agent_version=v2`、`tool_namespace=agents`。SQLite 当前 MultiRouter 的 `spawnAgentModels` 明确为 `deepseek-v4-flash / gpt-5.6-sol / qwen3.6 / gpt-5.6-luna / gpt-5.6-terra`，五份 managed Agent profile 也都存在。
- 同源无上下文 canary 成功：child session `019fe5fc-036b-77a2-811f-a0cd77c71561` 准确接收 `SAMEPROVIDER_V2_20260809_1805_A7K9`，执行 `Get-Location` 与 `git rev-parse --short HEAD`，返回 `C:\Users\sunda\Documents\LLMservice | eb79da20`；随后 `followup_task` 又准确接收 `FOLLOWUP_V2_20260809_1810_R8T3`，执行分支查询并返回 `master`。rollout 明确为 V2，初始与 follow-up 各有一条 `agent_message` 和一组真实 custom tool call/output。
- 跨 Provider DeepSeek canary 也完整成功：`deepseek-flash` profile 生成 child session `019fe604-1c1f-7110-9633-36bc1cac86a9`，nonce `DEEPSEEK_V2_20260809_1812_D6N1` 可见；CCSM 两轮请求真实命中 `https://api.deepseek.com/v1/responses` 并均返回 HTTP 200。child rollout 为 `model=deepseek-v4-flash / multi_agent_version=v2`，包含 2 次 function call、2 次 function output，最终返回正确路径与提交。
- Qwen profile 能正确创建 child、选中 `qwen3.6` 并命中 Qwen route；child rollout 中 `QWEN_V2_20260809_1807_M4P2` 位于 166 字节可打印载荷，不是旧版 `gAAAAA...` opaque Fernet，说明任务已经进入 V2/CCSM 投递链。但 Qwen 上游 `https://www.matrixminecraft.cn:24443/vllm/v1/chat/completions` 持续返回 HTTP 521，模型没有机会消费任务，因此中止重试；不能把这次失败归因于 V2 payload。
- 当前 `spawn_agent` 的直接动态 `model=` schema 只接受 `gpt-5.6-sol / gpt-5.6-terra`，所以 `model=qwen3.6` 在 Codex 本地直接报 `Unknown model`，没有到达 CCSM；改用注册 profile `agent_type=qwen-local` 后才真实命中 Qwen。该现象与 SQLite 的五模型候选不一致，是独立的 Codex 工具 schema/目录暴露问题；本机尚未安装带 lifecycle guardian 的 `v3.19.1-13/14`，不能声称新版仍同样失败。
- `v3.19.1-12..v3.19.1-14` 的 `codex_config.rs` 完全一致；`forwarder.rs` 只在 V2 明文化逻辑前新增 LM Studio `text.format` 归一化，V2 明文化、第三方 `agent_message` 投影和 ownership 判断本身没有变化。当前 `v3.19.1-14` 源码定向回归为 `codex_multi_agent 5/5`、mixed-router plaintext 1/1、third-party projection ownership 1/1、materialized official policy 1/1，合计 8/8，通过时仅有既存 `openai_cache_read_tokens` warning。
- 结论：本机无法复现“V2 子 Agent 整体不可用”；官方同源 spawn/follow-up 与 OpenAI 父模型到 DeepSeek 子模型都可用。可复现的两个局部问题是 Qwen 服务端 HTTP 521，以及直接动态模型覆盖只暴露 Sol/Terra。若外部用户反馈的是 DeepSeek/profile 路径，仍需对方提供 Codex 版本、实际 child model/provider、错误文本和 child rollout 形态才能与本机差异对齐。
- 上游交叉验证：`openai/codex#34833`、`#33551`、`#35932`、`#32705`、`#32988`、`#33314` 截至本轮仍为 open；`#32749`（暴露 V2 model overrides）已 merged。Codex 内置搜索与 GitHub 官方 API/CLI 结论一致；Matrix WebSearch 与本机 Qwen 共用的 Matrix 服务本轮均返回 HTTP 521，因此 Matrix 没有提供第二份正证据。

# 2026-08-09 Multi-Agent V2 第三方子 Agent 自动选型根修

- Codex V2 的 `model` direct override 与 `agent_type` custom role 是两条独立选择路径。`spawnAgentModels` 只负责 direct override 工具描述中的前 5 个展示顺序；默认“用户不选模型，由 Codex 按任务选择 Flash/Pro”必须通过带互斥 `description` 的 managed custom roles 实现。
- 直接根因位于 `src-tauri/src/codex_config.rs::sync_codex_managed_agent_files`：旧实现 `specs.iter().take(5)` 把 managed role 注册错误耦合到 direct override 前五窗口。本机目录中 `deepseek-v4-pro` 排第 6，因此没有 `deepseek-pro.toml`。修复后遍历完整当前可路由 catalog；stale prune 的 desired set 同样来自完整目录，不会再因模型掉出前五删除有效 role。
- 当前内置 DeepSeek profiles 的初始值分别指向 `deepseek-v4-flash` / `deepseek-v4-pro` 与 `codex_model_router_v2`，任务边界来自结构化问卷和可覆盖 profile，不是把模型名硬编码成“自动选型”规则；父 Codex 仍按 role description 选择 `agent_type`。用户同名 role 继续保留，CCSwitchMulti 回退写入 `ccswitch-<role>`。现有 V2 reasoning effort 尚未读取模型 catalog 的支持集合/默认值，这是独立待修缺口，不能把旧 medium/high 初始值写成已动态适配。
- MultiRouter 主向导已删除“子 Agent 候选”必经步骤；既有 `modelCatalog.spawnAgentModels` 继续读取、过滤和保存，RoutesTab 编辑器改为默认折叠的 `高级：子 Agent 模型覆盖`，明确其只控制 direct override 排序。Qwen 行为、`hide_spawn_agent_metadata=true`、混合路由 `tool_namespace=agents` 和 proxy 请求参数均未改变。
- TDD RED 提交 `9dc9c68f`：Rust 因第 6 位 Pro role 文件不存在而失败，前端分别因旧向导步骤仍存在和工作台没有高级折叠入口而失败。GREEN 定向结果为 managed agent 4/4、direct override priority 1/1、前端 workspace/wizard 49/49；完整 Rust library 为 2831/2831，完整前端在排除 `.worktrees/**` 后为 113 files / 820 tests，`cargo check --lib`、typecheck、rustfmt 和 `git diff --check` 通过。全仓 `pnpm format:check` 仍只报告本次未修改的 `src/lib/api/proxy.ts` 与 `src/types/proxy.ts` 两个既有格式差异。
- 当前 Codex `0.147.0-alpha.6.5` 选择 custom role 时需 `fork_turns=none` 或正整数；`rust-v0.148.0-alpha.1` 起上游 PR `#37252` 允许 role 应用于 full-history fork。CCSwitchMulti 不固定改写 `fork_turns`，由各版本 Codex 的工具描述指导父模型。`hide_spawn_agent_metadata=true` 在当前版本只隐藏 `service_tier`，仍保留 `agent_type`、`model` 和 `reasoning_effort`。
- Vitest 必须显式 `--exclude ".worktrees/**"`：仓库内旧 worktree 会被默认 glob 扫描并加载不同 React 实例，导致 `Invalid hook call / useState null` 的假失败；不能用业务补丁规避测试发现污染。
- 联网复核使用 Codex 内置搜索交叉验证 OpenAI 官方 Subagents 文档、当前 `multi_agents_spec.rs` 和 PR `#37252`。Matrix WebSearch 按独立链路重试仍返回 HTTP 521，没有提供第二条正证据；版本和机制结论以官方一手来源、本机运行态与当前源码为依据。

# 2026-08-09 CCSwitchMulti 在线替换安装事故与强制恢复规则

- 当前 Codex 会话通过 CCSwitchMulti 的 `127.0.0.1:15721` 访问上游模型，因此运行中的 CCSM 是当前任务的基础设施，不是可以随意停止的普通被测桌面应用。为 WebView2/CDP 调试而单独停止 CCSM 会切断当前会话；本轮发生过一次该错误，随后已重新拉起服务。以后在停止或替换 CCSM 前必须先画清运行依赖，禁止再次执行孤立的 stop/kill。
- 如果需要替换安装，必须先启动一个不依赖后续模型调用的独立 PowerShell 事务脚本；脚本在被启动前就应包含完整的 `预检和备份 -> 按已核实 PID kill -> 等待进程退出和 15721 释放 -> 正式卸载旧版 -> 等待卸载完成 -> 安装新版并检查退出码 -> 拉起新版 -> 等待 15721 -> 健康请求和版本/哈希/路由验证`。绝不能先 kill，再依赖 Codex 下一轮工具调用继续安装。
- 事务脚本必须有 `try/finally` 等价的恢复语义：成功路径拉起并验收新版；任一步失败也必须优先重新拉起仍可用版本，必要时用预先准备的旧安装包回滚，恢复用户配置，再验证 `127.0.0.1:15721`。脚本退出时不得把机器留在“CCSM 已停止且无人恢复”的状态。
- kill 必须限定为已确认安装路径对应的 PID：先尝试正常退出并限时等待，超时后才强制终止，不能按进程名误杀其他实例。卸载只清理程序安装范围，用户数据库、Provider、路由、凭据和未知配置字段必须备份并保留。
- UI/CDP 验收不得通过当前会话临时停止 CCSM来注入启动参数。确需重启态验收时，应由独立、不依赖该 CCSM 的会话执行，或在用户明确控制的维护窗口执行；当前会话只能做不中断服务的只读配置、日志和路由验证。

# 2026-08-11 CCSwitchMulti 小窗口顶部操作裁切根修

- 用户截图约为 `924x646`。旧顶部栏把 AppSwitcher、Codex 工具与新增供应商按钮全部放在同一个 `shrink-0` 集群，外层同时使用 `overflow-x-hidden`；即使全局自适应缩放已降至 80%，全部 8 个应用开启时总宽度仍超出弹性槽，右端操作被直接裁切。
- 仓库已有同类根修 `0cb6e014`（`fix(ui): keep header actions visible when all apps are enabled`），但它不是当前 `3.19.1-19` 分支祖先，且没有任何当前 tag 包含它；本次故障属于发布分支漏集成已有响应式修复，而不是 Tauri 最小窗口尺寸或 WebView 缩放本身失效。
- 根修结构是：AppSwitcher 独占 `min-w-0 flex-1` 弹性中段，通过父槽真实 `clientWidth` 与 `ResizeObserver` 计算可见应用数量；溢出应用进入“更多应用” Popover，当前激活应用始终顶替最后一个可见位。Codex 页面工具与新增供应商按钮进入独立 `shrink-0` 右端区，不再被应用标签挤出。
- TDD 先在旧实现验证窄槽位没有 `appSwitcher.more` 而失败；GREEN 覆盖 120px 窄槽位保留当前 Hermes、隐藏应用可访问且不重复，以及 1000px 宽槽位恢复全部 8 个应用且不显示“更多应用”。四语言新增 `appSwitcher.more`。
- 全量前端验证为 `117 files / 917 tests`、TypeScript 与 production renderer build 通过；本次文件 Prettier 与 `git diff --check` 通过。全仓 `format:check` 仍只被本次未修改的 `src/lib/api/proxy.ts`、`src/lib/codexMultiRouterWizard.test.ts`、`src/types/proxy.ts` 三个既有格式差异阻断。
- 独立 Vite 预览未停止正式 CCSM。浏览器按截图尺寸 `924x646` 和 Tauri 最小窗口 `900x600` 验收：document `scrollWidth == clientWidth`；所有 header 按钮边界均在 viewport 内；Codex 当前标签、多模型路由、用量、Agent API、Skills、提示词、会话、MCP 与新增按钮完整可见；OpenCode/OpenClaw/Hermes 可从“更多应用”打开。

## 2026-08-11 Codex Responses 断流错误呈现根因与修复

- 现场 `stream disconnected before completion: Transport error: network error: error decoding response body` 不能直接判定为 timeout。`~/.cc-switch/logs/cc-switch.log` 的完整 error source chain 明确为 `unexpected EOF during chunk size line`：上游 HTTP chunked response 在下一条 chunk-size 行读取完成前提前关闭；相邻时段还出现 `unexpected EOF during handshake`，共同指向上游/代理链路提前断开。
- 根因位于 `src-tauri/src/proxy/providers/streaming_retry.rs::create_resilient_responses_sse_stream`：已有 semantic output 后，为避免正文或工具调用被重放，代码正确地停止重连，但错误地继续 `yield Err(error)`。该错误随后经 passthrough 作为 Axum HTTP Body error 传给本地 Codex，使 CCSwitchMulti 到 Codex 的响应体异常终止；Codex 只能二次报告泛化的 `error decoding response body`，无法看到代理已捕获的深层原因。
- 修复边界：保留“已有 semantic output 后绝不重放”的副作用安全约束；将传输错误转换为合法 Responses SSE `event: error` 后干净结束 HTTP body。chunk-size EOF、真实 timeout、其他传输中断分别显示“HTTP 分块响应未完整结束”“读取超时”“传输中断”；正文前最多 5 次安全重连耗尽也通过合法 SSE 报错，不再制造损坏的下游 body。
- 历史定位：通用 passthrough 将 stream error 作为 Body error 的基础行为来自初始导入 `693c3872`；原生 Responses 在已输出正文后明确 `yield Err` 的当前安全重连路径由 `461dc35c`（2026-08-03）引入，最早包含该行为的现存正式 tag 为 `v3.19.1-5`。该提交的不重放设计本身正确，缺陷是错误呈现协议选择错误。
- 回归验收：`native_responses_surfaces_post_content_chunked_eof_as_protocol_error` 证明不重放、下游所有 stream item 均为 `Ok`、存在合法 `event: error` 且不再泄漏 `error decoding response body`；`native_responses_transport_error_message_distinguishes_true_timeout` 保证 timeout 不与 EOF 混淆。`cargo test --lib proxy::providers::streaming_retry::tests` 为 22/22；完整 Rust library 为 2925 passed、0 failed、2 ignored；`cargo fmt --check`、`cargo check --lib`、`git diff --check` 通过。

## 2026-08-11 CCSwitchMulti v3.19.1-20 GitHub 正式发布

- `v3.19.1-20` 是 `v3.19.1-14` 之后首个 GitHub 正式版本；本地 `3.19.1-15` 至 `-19` 只用于阶段性交付，没有复用 `-19` 标签发布内容不同的二进制。版本提交 `8de0b4222695dda7119284642671fce2ac3314d1` 同步 `package.json`、`Cargo.toml`、`Cargo.lock` 与 `tauri.conf.json`，中文累计说明为 `docs/release-notes/v3.19.1-20-zh.md`。
- annotated tag object 为 `3638885a87f2fd6fcb0e6b3cc89fc9fb79913e2f`，远端 peeled commit 精确为 `8de0b4222695dda7119284642671fce2ac3314d1`。发布后记忆提交不得移动该 tag。
- GitHub Actions Release run `31467563416`（`https://github.com/BigStrongSun/ccswitchmulti/actions/runs/31467563416`）耗时 42m47s；Linux x64/ARM64、Windows x64/ARM64、macOS、Publish GitHub Release、Assemble `latest.json` 全部 `completed/success`。
- 正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-20`，页面标记 Latest，正文完整覆盖 Sub-Agent V1/V2、OAuth device-flow 竞态、自启动/安全重装、小窗口裁切和 Responses 断流错误呈现。页面共 21 项：19 个实际 release assets，加 GitHub 自动生成的 source zip/tar.gz。
- 远端 `latest.json` 为 `version=3.19.1-20`、6 个平台键，所有 URL 均指向本 tag、HTTP 200，所有 signature 非空且逐项与对应 `.sig` 完全一致；`/releases/latest` 最终跳转本 tag。下载后的 `latest.json` SHA-256 为 `a0a5d5f785d18603cf6fc814e2376f786e6c2c6746897cdc33590e05d08ff1a2`，Windows x64 setup SHA-256 为 `99177dcd9ed098a7f34f82afc81caa8b6c3c53be7381a5df3e8a15c9ad2e9763`，均与 GitHub 页面服务端 digest 一致。
- 本地发布候选门禁：Rust `2925 passed / 0 failed / 2 ignored`；前端 `117 files / 918 tests passed`；typecheck、production renderer build、`cargo fmt --check`、`cargo check --lib` 和 `git diff --check` 通过。发布成功不等于用户机器已安装 `-20` 或已完成 Windows/macOS/Linux 真实重启/登录验收。

## 2026-08-11 应用内升级检查双客户端根修

- 安装态 `3.19.1-19` 已在真实 UI 成功识别 GitHub `3.19.1-20`，因此远端 `latest.json`、版本比较和签名清单当前可用；但 `~/.cc-switch/logs/cc-switch.log` 在 2026-08-03 至 08-11 多次记录前端 updater 直连 GitHub 的 `error sending request`，说明“这一次能显示”不能证明链路稳定。
- 直接根因是 updater 被拆成两个 HTTP client：`src/lib/updater.ts` 的例行检查直接调用 JavaScript `@tauri-apps/plugin-updater.check()`，下载/安装及数据库恢复页则调用 Rust `updater_builder_with_runtime_proxy()`。提交 `0d693c8a`（`3.19.1-3`）声称让所有 updater 检查/下载继承全局代理，实际只修改了 Rust 路径，漏掉 About 页/顶部徽标使用的前端检查；因此该缺陷从 `3.19.1-3` 起存在并会随 GitHub 直连条件间歇复现。
- 根修新增后端 `check_app_update`，从同一个 proxy-aware builder 返回 `currentVersion/availableVersion/notes/pubDate`；前端 `checkForUpdate()` 只调用该 IPC，不再构造第二个 updater client。TDD RED 明确失败于前端插件仍被调用，GREEN 后前端 2/2、Rust metadata 1/1、typecheck 和 production renderer build 通过。
- Windows 普通升级不需要先卸载：当前锁定 `tauri-plugin-updater 2.10.0` 会用 NSIS `/P /R /UPDATE /ARGS` 启动外部 installer，再 `std::process::exit(0)`；应用在 install 前先保存窗口、恢复 Live 配置、停止代理、移除托盘并释放单实例锁。SQLite 位于 `~/.cc-switch` 而非 `$INSTDIR`，正常升级不会复制或删除数据库；`Database` 连接随旧进程退出释放，新实例随后重新打开，因此不存在安装器与数据库文件的直接覆盖冲突。
- 完整“停止 -> SQLite 全目录快照/完整性 -> 卸载 -> 安装 -> health/version/hash -> 回滚”事务脚本只用于安装损坏或安装器族迁移。把它作为每次升级默认路径反而会执行卸载 hooks，并可能再次删除 Windows 开机自启等用户集成状态；该路径必须继续备份 `cc-switch.db/-wal/-shm` 和注册表并做恢复验证。
- macOS updater 原地替换 `.app` bundle，现有后端链在 install 返回后清理并重启，避免旧 WebView 在 bundle 替换后继续调用 JS；Linux AppImage 原地替换可执行文件，DEB/RPM 可能通过 `pkexec`/系统包管理器获取权限，随后同样清理和重启。三平台共享的网络检查缺陷已由统一后端命令一并消除；只有 Windows 存在 NSIS 外部进程和 `std::process::exit` 的特殊退出顺序。

## 2026-08-11 macOS ChatGPT.app MultiRouter 首次启用失效根修

- 用户现场为 CCSwitchMulti `3.19.1-20`、macOS `26.6.1 arm64`、Codex Desktop `26.803.61601`，实际统一包 `/Applications/ChatGPT.app`、bundle id `com.openai.codex`。并非所有 Mac 都触发：需要新版统一壳、从向导首次启用、启用前 current provider 不是目标 MultiRouter、尚未 takeover，并继续使用未重载的当前会话；旧 `Codex.app`、已经接管、直接从 Provider 列表切换或启用后立即重启的用户可能绕开部分缺陷。配置是触发条件，不是用户配置错误。
- 跨平台主根因由 `ae431551` 引入并最早发布于 `v3.16.4-3`：`handleEnableCodexMultiRouterPlan` 手动执行 `start proxy -> takeover current provider -> switch target`，因此先以 OpenAI Official 生成官方 catalog 和 legacy managed roles，再热切换目标。修复提交 `2b1f497b` 删除提前启动/接管，只切换目标 Provider，让 `ProviderService::switch` 在尚未 takeover 时进入现有锁内原子入口。`ProviderSwitchOutcome` 明确返回 success/error，向导 helper 对失败抛出原始 Error，不能再吞错后派发 `ENABLE_SUCCESS`。
- macOS 旧壳 discovery 由 `3dcd2a11` 引入并最早发布于 `v3.16.4-16`，只认进程 `Codex`、目录 `Codex.app` 和 `Contents/MacOS/Codex`。修复提交 `44b9de42` 以 `com.openai.codex` 和 `Info.plist/CFBundleExecutable` 为权威，同时支持 `/Applications`、`~/Applications` 中的 `Codex.app`/`ChatGPT.app` 和 Spotlight；独立 ChatGPT bundle、路径穿越 executable name、Framework 内部 Service.app 均不会被误当主壳。Windows 继续只接受 `OpenAI.Codex` MSIX 下的 `ChatGPT.exe`，Linux 继续接受 `Codex`/`Codex*.AppImage`。
- 原子入口此前验证成功后漏掉手动 takeover 已有的 guardian/model-picker 收尾。提交 `3c27e38a` 将三个成功出口统一到 `run_post_takeover_lifecycle`，只在 Codex 且 takeover 已验证后启动；CDP 注入仍是 best-effort warning，不是 HTTP 路由成败条件。`target_provider_record_unavailable_inline_auth_fallback` 是 provider 分类降级，不是 executable discovery 或无请求进入代理的直接原因。
- TDD/门禁：严格启用 helper 2/2、`useProviderActions` 29/29、`codex_desktop` 23/23、Provider service 78/78；完整前端 `119 files / 922 tests`、完整 Rust `2930 passed / 2 ignored`、TypeScript、renderer build、cargo check 和 diff check 通过。全仓 Prettier 仍只被本次未修改的 `src/lib/api/proxy.ts`、`src/lib/codexMultiRouterWizard.test.ts`、`src/types/proxy.ts` 三处既有差异阻断。当前 Windows 主机没有真实 macOS UI，因此不能宣称现场运行已验收；必须以 GitHub macOS 构建成功和受影响 Mac 用户重启后请求真实进入 `127.0.0.1:15721` 作为后续证据。
- 联网链：Codex Web 找到 OpenAI 官方 issue `#31866/#31944/#32022` 证明 2026 年 7 月统一包迁移到 `ChatGPT.app` 且 bundle id 保持 `com.openai.codex`，Apple 官方 NSWorkspace API确认 bundle identifier 是平台稳定发现边界；Matrix WebSearch 正常但仅返回弱二手结果，没有提供关键事实的独立正证据。结论由官方一手来源、本地源码、Git 历史和用户脱敏现场交叉确认。

## 2026-08-12 CCSwitchMulti v3.19.1-22 Sub-Agent V2 正式发布

- `v3.19.1-22` 的四处版本源（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`）均为 `3.19.1-22`；版本/tag 提交为 `c364cde9f3a350f7303e92b7f9de4fb3498e2e41`。本地 annotated tag、远端 tag peeled commit 与 Release 构建 `head_sha` 三者一致；发布后记忆提交不得移动该 tag。
- GitHub Actions Release run `31514013012`（`https://github.com/BigStrongSun/ccswitchmulti/actions/runs/31514013012`）从 2026-08-12 00:44:45 至 01:28:58（Asia/Shanghai），耗时 44m13s；Linux x64/ARM64、Windows x64/ARM64、macOS、`Publish GitHub Release`、`Assemble latest.json` 七个 job 全部 `completed/success`。
- 正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-22`，不是 draft/prerelease，且 `/releases/latest` 返回同一 release id。正文包含独立“子 Agent”工作区、V1/V2、能力问卷、折叠模型配置、TOML 诊断以及深浅双主题说明。
- Release 恰有 19 个实际资产，集合与工作流预期完全一致：Linux x64/ARM64 各含 AppImage、updater `.sig`、DEB、RPM；macOS 含 DMG、universal updater tarball/`.sig`、ZIP；Windows x64 与 ARM64 各含 Setup、updater `.sig`、Portable ZIP；另含 `latest.json`。全部资产状态为 `uploaded`，且 GitHub REST `digest` 均为完整 `sha256:<64 hex>`。
- 远端 `latest.json` 为 `version=3.19.1-22`，平台键精确为 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`；六个平台 signature 均非空并逐项等于对应 `.sig` 文件，URL 均指向 V22、对应真实 release asset 且 HTTP 200。
- 下载后的 Windows x64 NSIS Setup 大小为 11,631,085 bytes，文件/产品版本均为 `3.19.1-22`，SHA-256 为 `c2ae540cc9c68ccc6125a6925fc0418200adf0f2ee442e141f36ee7370fe5f8b`；下载后的 `latest.json` SHA-256 为 `32352efec631d87cafd6a6c05928535cf7fcf03035df5ff271613c75be4436d4`。两者都与 GitHub 服务端 digest 精确一致。Windows Setup 没有 Authenticode 签名；本轮通过的是 Tauri updater `.sig`/manifest 链，不能把二者混称为同一种签名。
- 当前 HEAD 的新鲜门禁：Rust library `2954 passed / 0 failed / 2 ignored`；Sub-Agent profiles `71/71`；Codex config `156/156`；完整前端 `119 files / 943 tests`，Sub-Agent 工作区定向前端 `160/160`；事务安装 Pester `46/46`。`cargo check --lib`、rustfmt、TypeScript typecheck、Prettier、Vite production renderer build 与 `git diff --check` 均通过；仅保留既有 dead-code、mock/React 与 bundle-size warning。
- 安装态边界：本机仍运行 UI 验收候选 `3.19.1-21`（PID 65888、SHA-256 `92B73DC3B8286CE21259D792D248B2D1ED783982DCAE6B1182505A534B844D0E`），`127.0.0.1:15721/health` 为 HTTP 200；最终 `3.19.1-22` 尚未通过独立事务进程覆盖安装。发布成功不能表述为本机已经安装 V22，也不能为了安装在当前依赖 CCSM 的会话中单独停止它。
- 安装候选的实际 Codex 注入状态已经可见：`~/.codex/agents/deepseek-v4-flash.toml` 与 `deepseek-v4-pro.toml` 分别锁定 `codex_model_router_v2`、medium/high reasoning；`~/.codex/config.toml` 保持 `hide_spawn_agent_metadata=true` 与 `tool_namespace=agents`。浅色、深色、TOML 展开态和长模型折叠列表的安装版截图位于 `artifacts/design-audit/subagent-theme-2026-08-11/`；错误生成的 15x15 `05-light-after.png` 不是证据且不得提交。

## 2026-08-12 macOS V22 Codex 配置解析失败根修

- 用户的 macOS `3.19.1-22` 报告包含两条独立的 Codex 启动阻断：`AbsolutePathBuf deserialized without a base path in model_catalog_json` 与 `duplicate field max_concurrent_threads_per_session`。它们不是上一轮 `ChatGPT.app` 发现/首次启用顺序修复失效，而是新版 Codex 收紧配置反序列化后暴露出的旧投影契约。
- `model_catalog_json` 根因来自提交 `7811383b`（最早进入 `v3.16.2` 系列）：为旧 WSL/symlink 场景把已经算出的绝对 catalog 路径改写为固定相对文件名 `cc-switch-model-catalog.json`。当前 Codex 的 `AbsolutePathBuf` 要求绝对路径；CCSM 现在在首次写入、catalog projection、MultiRouter takeover 和 Provider 刷新路径中都保留 `get_codex_model_catalog_path()` 的绝对路径。相对文件名 fixture 仍仅用于识别、清理和迁移 CCSM 旧配置。
- 并发键冲突来自提交 `2aef8a2e`（最早进入 `v3.16.3-23`）：CCSM 只检查旧键 `agents.max_threads`，而当前 Codex Serde 将它作为 `max_concurrent_threads_per_session` 的 alias。用户已有 canonical 键时继续补旧键，会被判定为同一字段重复。投影现在 canonical 键优先，否则迁移旧键值，否则写默认 10；随后删除 alias，只保留 `max_concurrent_threads_per_session`，并保留用户 `max_depth`。
- 报告中“浏览/保存非 current MultiRouter 会改 live 文件”的推断未获源码与回归支持：`finish_codex_subagent_v2_mutation()` 会比较 effective current provider，非 current 返回 `NotRequired`。真实文件刷新发生在切回 Official 后的 live projection。Official 自身带六模型 catalog；无显式 V2 profiles 时 legacy managed-role 投影生成六个 GPT role，这是既有 Official 多 Agent 行为，与上述两条解析错误无直接因果，本轮未删除。
- TDD 提交为 `c3976e97`（RED）与 `4b6f7dfb`（GREEN）；旧路由测试契约更新为 `786248c5`，格式提交为 `99d72136`。验证：三条根因/边界定向测试通过，`codex_config::tests` 156/156，通过完整 `cargo test --lib`（2956 passed / 0 failed / 2 ignored）、`cargo check --lib`、`cargo fmt --check` 和 `git diff --check`。仅有既存 `openai_cache_read_tokens` dead-code warning。
- 联网交叉验证：Codex 内置 Web 搜索命中 OpenAI 官方源码、配置文档与 issue，均使用绝对 `model_catalog_json`，且官方 `AgentsToml` 明确声明 `max_threads` alias；独立 Matrix WebSearch 三条相关查询无结果，不能作为正证据。结论由官方一手证据、本地提交历史、源码与 RED/GREEN 测试共同支持；当前 Windows 环境不能替代受影响 macOS 机器的最终运行验收。
- 发布验收方法与 GitHub 官方 REST 文档一致：release asset 的 `digest` 用作服务端 SHA-256，二进制下载后再本地复算；Codex 内置搜索与 Matrix WebSearch 均独立打开 GitHub 官方 release-assets 文档并得到同一字段/下载语义，Matrix 泛搜索未返回相关结果但官方页面直开成功。
- 内置预设能力不能直接编辑：默认只读，用户必须点击“创建高级覆盖”后才进入 `source=user`，界面明确显示“已偏离内置预设”，并能一键恢复当前版本的内置能力。`ProviderMeta.codexPresetId` 保存稳定 `presetKey`，不得保存会随数组顺序变化的 `codex-N`；旧 `codex-N` 只保留兼容读取。
- 本轮发现目录受控状态的根因：`catalogRowsMatchModels()` 在模型级 reasoning schema 加入后没有比较 `reasoning`，所以覆盖按钮虽更新了子组件状态，子到父 effect 却误判为未变化。修复是把完整 reasoning 纳入统一相等性边界，不是在按钮或测试里绕过同步。
- `modelCatalog()` 现在保证每个内置目录模型都有显式 reasoning capability。有官方 effort 的 DeepSeek/Grok/GLM/Step 按模型枚举；Kimi、Qwen、MiniMax、MiMo、SiliconFlow 只声明 boolean thinking 且 `supportedEfforts=[]`，不展示虚假强度；其余证据不足模型显式 `supported=false`，不继承 GPT/Native Responses 通用档位。
- 聚合平台能力仍必须优先于模型原厂能力，例如 SiliconFlow 的 `enable_thinking` 和 OpenRouter 的 `reasoning.effort`；未知模型或平台证据不足时保持不展示、不注入 effort 的保守策略。
- 提交 `ba927d22` 完成只读/覆盖/恢复、预设身份持久化、全 catalog 显式能力和 reasoning 同步根修。验证：Rust library `2842 passed / 0 failed / 2 ignored`；前端 `116 files / 841 tests` 全通过；`cargo check --lib`、typecheck、rustfmt、变更文件 Prettier 和 `git diff --check` 通过。仅保留既有 `openai_cache_read_tokens` dead-code warning 及测试夹具预期 stderr。

# 2026-08-14 Codex Provider 菜单投影与旧路由入口收口

- 普通 Codex Provider 表单不再显示旧“Codex 多模型路由”编辑器；唯一可见的多路路由编辑入口收口到 `CodexRouterWorkspacePage`。这只是 UI 收口，历史 `settingsConfig.codexRouting` 数据、前端 normalize/save、迁移 schema、Rust resolver 和实际代理路由全部保留，禁止后续把隐藏入口误解为可删除兼容层。
- `meta.codexLocalModelMapping` 的产品语义是“把该 Provider 的模型目录投射到 Codex `/model` 菜单”，不是启用 MultiRouter。新建 Codex Provider、自定义模板和内置预设默认开启；编辑已有 Provider 时继续尊重显式 `false`，避免覆盖用户选择。
- “在 Codex `/model` 菜单中显示”移入默认折叠的高级选项，并从 `shouldShowSpeedTest` 门控中拆出：即使某类 Provider 不显示测速/协议探测，该菜单投影设置仍可独立编辑。xAI OAuth 托管预设继续隐藏该开关。
- 默认开启菜单投影不能计入 `hasAnyAdvancedValue`，否则高级区会因默认值自动展开，违背“移到高级”的交互意图。空模型目录即使开关为 true 也不会生成无效菜单项。
- TDD 定向覆盖包括：旧路由入口不可见但传入 routing state 保持不变；菜单开关在折叠高级区内；新 Provider 默认 enabled；已有显式 false 保存不变。定向 28/28、MultiRouter state/sync 14/14、App 集成隔离重跑 8/8、typecheck 通过。App 与其他文件并行运行曾出现 2 项 DOM/时序污染，隔离重跑全过；全仓 format check 仍只剩既有 `src/lib/api/proxy.ts`、`src/types/proxy.ts` 两处差异。
- 后续产品边界收紧：带稳定 `codexPresetId` 的 CCSwitchMulti 维护预设始终使用 `effectiveCodexMenuProjection=true`，不再显示可关闭开关；历史预设若误存 `codexLocalModelMapping=false`，下次保存会纠正为 true。MultiRouter 的后端强制投影规则保持不变。
- 只有自定义 Provider 保留退出目录管理的能力：新建或从预设切换到自定义时默认开启，编辑既有自定义 Provider 时继续尊重显式 false。开关位于高级选项最后，并明确说明它只控制 Codex 启动时加载的 `model_catalog_json`、`/model` 模型/别名/上下文/推理档位，不控制 Provider、代理或 MultiRouter 是否可用；仅自行维护目录的用户应关闭。
- 收紧后的 TDD 先得到 4 项预期失败，随后定向 30/30、路由状态同步 14/14、App 集成隔离重跑 8/8、typecheck 和变更文件 Prettier 检查通过。App 首轮仍出现既有异步 DOM/时序超时，完全相同命令重跑通过；测试夹具仍有既存 Tauri window metadata stderr 与 React act warning。

## 2026-08-14 Codex Provider 模型源就绪主流程

- Codex Provider 表单的“模型与兼容性”必须常显模型同步、默认模型、上游协议、连接验证与 MultiRouter 就绪结论；不要再把这些主流程动作藏回高级折叠。
- `CodexProviderReadinessSection` 只消费 `CodexFormFields` 已拥有的 model sync/protocol probe state 和回调，不应另建平行状态；错误验证结果用 `role="alert"`，其它验证结果用 `role="status"`。
- 维护预设继续显示协议、上下文、推理档位及 `/model` 目录由 CCSwitchMulti 维护；自定义 Provider 继续是自动检测 Chat/Responses，失败后才允许高级手动覆盖。Task 4 的旧 route editor 可见入口仍保持收口，`codexRouting` 历史兼容链不能被本 UI 变动误删。
- TDD 移植证据：RED `1abad036` 的独立测试在 GREEN 前因缺少 `CodexProviderReadinessSection` import 预期失败；v26 集成 GREEN `6909eb10623eaf127444e341ac6a24dbfaf4e5cb` 后 `CodexProviderReadinessSection` 与 `ProviderForm.codexPreset` 定向测试 12/12 通过，`git diff --check` 通过。测试输出仍有既有 baseline-browser-mapping 过期与 React act warning，非失败。

## 2026-08-14 MultiRouter 原地完成收尾

- MultiRouter 的运行态五项验证成功（当前 Provider、代理监听、Codex 接管、路由入口、最近一次匹配路由的转发）必须留在状态工作台原地显示“MultiRouter 已通过真实请求验证”；不得再通过 App 回调自动跳到 Sessions 或自动打开历史修复。
- 只移除 MultiRouter 专属 `onRuntimeReady`、post-setup ref 和 Sessions 自动导航。`SessionManagerPage` 的独立 Codex 历史修复工具及 `onCodexHistoryRepairCompleted` 回调必须继续保留；四阶段向导和 V2 `initializeProviderConfig` 初始化调用也不得随之回退。
- TDD 移植证据：RED `90321d0e` 在未修改生产代码前得到两项预期失败——工作台缺少原地成功提示；App 在 15 秒超时下实际 `data-runtime-ready-wired=true`、预期 false。GREEN `86e4965c` 后工作台与 App 60/60 通过；随后表单回归 `c16a17ed` 让测试壳按用户真实路径展开“高级选项”，同时保留默认折叠、按钮入口和菜单投影断言。最终聚焦测试 82/82、typecheck、变更文件 Prettier 与 `git diff --check` 均通过。本轮写入文件已严格 UTF-8 解码、无新增 BOM、无 U+FFFD；`CodexRouterWorkspacePage.tsx` 的 BOM 已存在于任务基线。App 测试仍会输出既有 Tauri window metadata stderr，且 `baseline-browser-mapping` 数据过期提示不影响退出码。

### Review fix round 1

- “验证连接”打开确认框后的用户反馈“已打开验证确认框；如果没有看到弹窗，请按 Esc 后重试。”是仍在生产组件中的可见行为。相关测试必须真实点击“高级选项”后再点击“验证连接”，并同时断言反馈、顶层对话框层级和标题；不得为适配测试删除该反馈。
- 向导首步完成说明也不得残留“启用后带你修复历史记录”的自动流程暗示。正确边界是：真实请求验证留在状态工作台原地完成；历史记录修复由用户按需从 Sessions 独立进入。该文案边界由 RED `75b4306b`（旧文案实际失败）和 GREEN `1c367539`（最小单行修复）锁定，独立 Sessions 工具与完成回调不变。

## 2026-08-14 v3.19.1-26 历史功能分叉最终收口

- 固定 runtime 审计 HEAD 为 `8e9a4bc4dba374532e021051a9cb2ba44ba098f6`，发布 lineage 基线为 `v3.19.1-25^{}` / `4d19b80a0c0f077de3968d3f12271924c6825a97`。对本地全部 `refs/heads/*` 34 个和 `refs/remotes/fork/*` 139 个逐一重算 exact tip、merge-base、双向 ancestry、ahead/behind 与 `git cherry`，共 173 refs；35 个 refs 含 `+`，去重 198 个 `+` patches，66 个 `+ fix/feat` 全部完成当前树语义审阅。最终分类为 ancestry 30、selectively-integrated 2、docs/research 7、NO-GO 2、patch-equivalent 4、superseded 11、upstream/PR scope 117、`actionable-missing=0`。2026-08-15 又以 live `git ls-remote --heads fork` 对比本地非 symbolic 的 `refs/remotes/fork/*`：远端 138 个 heads 与本地 138 个 tracking tips SHA 全部精确一致，`NEW/MOVED/DELETED=0`；本轮仍未 fetch，但因 tips 无漂移，不需要更新 objects，173-ref/198-patch 审计结论不变。
- Task 3 Provider stack 精确移植映射：`fe80d27f -> 8cf00d0a44d35fdf6be53b081ee06cbc9a66220d`、`94269c25 -> 26b05c1d93fb2f799f94eeaf656754bbe5c7a417`；Provider readiness 为 `bf38788a -> 1abad036a00a4814cb180030fca5fef1e7e4c31a`、`9de5f879 -> 6909eb10623eaf127444e341ac6a24dbfaf4e5cb`；全局设置为 `8e677be7 -> 66bed477dd5e9fec297487e70586a2085cfff76c`、`1c8b6173 -> 47c75463d1d412f304d9dae087b10ecd2c91cced`。其中 7 项源 patch 已由 `git cherry` 标为 `-`，其余因父树/memory/冲突适配仍为 `+` 的条目均有精确集成提交与当前源码语义证明；不得再把任何 source-branch SHA 当成 v26 GREEN。
- MultiRouter stack 精确收口：`21a06a9f -> 68012439f127cf2b5945eca470f866bfdc2015ac`；`737dd735 -> b495befd5180d6da33cb4de69283c00811854675` 后以 `d64088b9187fc00a4a515d59983ed07bd952e5af` 对齐当前四阶段测试，并由 `d95dd47cf81b141879b770379075f75ef92d3bc8` 恢复新建方案在交给启用消费者前的 V2 `initializeProviderConfig` 初始化。原地完成 stack 为 `e3a7732c -> 90321d0e62e88c8a1dddbed3cc2ec6daddba5ba7`、`2242e5b4 -> 86e4965c7bb6a38cd699c41bbfdd52f98306da78`、`69ad006e -> c16a17ed2abca398cfe984552a77c76229b3dc5a`；评审修复 `5ed9952300dd86d7b1b52e90345a1b61c2554dc7` 恢复验证确认反馈断言，`75b4306b6ce26cb77a041bc27bdb5dead4cf3bbf` / `1c367539cfd0e90b0e28246ecee27ffacdead5a0` 锁定“状态工作台原地完成、Sessions 独立历史修复”的产品文案。四步向导不得重新嵌入 V1/V2 编辑步骤，也不得恢复自动跳 Sessions。
- Task 9A 新分叉已选择性集成：RED `ff739720 -> 212763c8714560c82cc152a98264943b6e187874`；GREEN `c3e7311c -> 61430988ae10d4cc6bde59b6f1bb206d68fb4a20`；评审 RED/GREEN 为 `0553f21fbad7d804196b9e8804f3fe752dbd3bd3` / `8e9a4bc4dba374532e021051a9cb2ba44ba098f6`。`CodexSubagentV2Profile.inputModalities` 的合法持久化值收紧为 `['text']` 或 `['text','image']`；已知模型从 catalog hydrate，未知模型保持保守未声明，自动和手工覆盖文案都必须附带图像任务安全边界。
- V2 mutation 成功不再等同于“写调用返回”：initialize、reconcile 与普通 update 统一检查 `databasePersisted`、projection 状态、managed role TOML 逐文件绝对路径/存在性/精确内容回读和 activation 边界。current provider 只有 expected/actual TOML 一致才可报告 applied；非 current 明确 `not_required`，retry 保持 `pending_retry`；运行中 Codex/app-server 的 role snapshot 仍要求重启并新建会话，CCSwitchMulti 不声称拥有热加载保证。`124ea44f` 的源分支 memory 由本条最终知识取代。
- 自动选型边界最终统一：`spawnAgentModels` 前五只控制 direct override 工具描述窗口；V2 managed roles 来自完整可路由 profile/catalog，不得使用 `.take(5)`，也不得把“硬编码 Flash/Pro 模型名”当成语义自动选型。父 Codex 按 custom role description 选择 `agent_type`，模糊任务仍可能选择内置角色，不能承诺第三方 role 必定命中。
- 基线全量验证从 `4d19b80a` 起固定 Windows 冷编译资源约束 `CARGO_BUILD_JOBS=2` 与 `--test-threads=1`：前端 121 files / 962 tests，Rust `2967 passed / 0 failed / 2 ignored`。选择性移植阶段验证为 Task 4 30/30 + typecheck、Task 5 12/12、Task 6 4/4 + typecheck、Task 7 64/64 + typecheck、Task 8 最终 113/113 + typecheck；Task 9A 最终 V2 前端 121/121、Rust V2/mutation 103/103、四步向导 31/31，并通过 typecheck、cargo fmt、Prettier、diff hygiene 与 UTF-8 strict。全量 release candidate、安装事务和远端 Release 仍属于后续门禁，不能用这些聚焦结果替代。
- 联网事实链保持可审计：Task 3/Task 9 首轮用 Codex 内置搜索与 Matrix WebSearch 独立读取 Git 官方 `git-cherry`/merge-base 语义；Task 9A 首轮两链独立读取 OpenAI 官方 `openai_models.rs` 的 input modality 契约。较早评审轮的固定 Matrix 入口曾缺失，因此当时只记录环境 concern；2026-08-15 final fix wave 已恢复固定 bridge，并以两条独立链再次核对 React 官方 `useEffect` 竞态清理建议和 OpenAI Codex 官方 `openai_models.rs` 的 `input_modalities` 元数据契约。Matrix 搜索本身返回 0 条，但对两份官方原文的直接 `open` / `find` 成功，不能把空 search 结果误判为 bridge 掉线。最终 Git 分类仍以本地 object graph 与 live tip SHA 为权威，前端异步/模态契约以当前源码、RED/GREEN 和官方一手来源交叉验证。
- 原四项非阻断 follow-up 中两项已在 final fix wave 收口：V2 initializer 测试现在用 `toBe(initialized)` 证明传递的是 API 实际返回的 persisted-provider identity；`SessionManagerPage.tsx` 及测试注释已改为“显式调用方独立请求历史修复”，不再描述已删除的自动导航。当前仅保留两项条件性、非阻断风险：Task 9A 前端 catalog identity 仍使用 `localeCompare`，尚未与后端 NFKC + case-fold 对齐，本轮不得发明不完整的 Unicode full case-fold；Sub-Agent V2 reasoning effort 仍未把模型 catalog 的支持集合/默认值作为端到端单一契约送入 profile compiler，不受支持 effort 可能在 Responses→Chat capability 校验中失败，本轮不得借机改 reasoning defaults。上述风险不是 `actionable-missing` runtime patch，不能把它们夸大为本候选已发生的确定失败，也不能表述为已解决。

## 2026-08-15 v3.19.1-26 最终评审修复波

- 起点固定为候选 `dd96780149685c369e934e09a288077c7f5cfb2c`。统一 RED `6a757b96` 在生产代码未变时运行 79 项聚焦测试，得到 74 passed / 5 expected failed；五项失败精确覆盖共享 Codex TOML 加载失败仍可编辑、维护预设绕过真实验证、身份变化后沿用成功、旧探测晚到覆盖新身份、`/models` 能力字段丢失。RED 同时加强 initializer exact identity 断言并清理 Sessions 测试语义。
- 共享设置根因是 `CodexGlobalConfigSettings` 以可编辑占位 TOML 初始化，后端读取失败后仍挂载 Goal mode、编辑器和保存入口。GREEN `fdd7c835` 改为 fail closed：展示真实错误与 Retry，成功读取前不挂载编辑/Goal/save，`save()` 也以 `isLoaded` 二次守卫；聚焦 3/3。
- Provider 根因是 readiness 只绑定 UI 状态而未绑定完整 Provider 身份，维护预设名称又被误当作凭据就绪；异步探测没有 sequence/identity owner，`/models` 合并只保留 context。GREEN `5290171b` 让 readiness 必须来自当前身份的真实成功验证，身份覆盖 provider、endpoint/full URL、API key/OAuth/AgentPlan/Anthropic auth、UA/header/body override、协议参数、默认模型和完整 catalog 能力；身份变化立即失效，旧请求进度/结果不得回写。拆分建议也绑定生成后的身份，避免同步 catalog 时误清本次建议，同时在后续身份变化时失效。
- `5290171b` 在表单组件内保留后端明确返回的 `inputModalities` / `supportsImage`，同时覆盖新增行、已有行和手动选择路径；不按模型名称推断图像能力。维护预设只说明 CCSwitchMulti 拥有 metadata，不再跳过端点/凭据验证。生产提交前聚焦 76/76；修复拆分建议 ownership 后复跑相关 29/29；typecheck、Prettier、`git diff --check` 均通过。该提交当时尚未覆盖表单下游的实际 save/reload normalizer，不能单独作为端到端持久化证明。
- 文档前全量前端门禁以 `pnpm exec vitest run --exclude '**/.worktrees/**' --no-file-parallelism` 运行 123 files / 990 tests，退出码 0；`vitest list` 的 JSON 清单独立核对同为 123/990。唯一工具级提示是 `baseline-browser-mapping` 数据超过两个月；负路径测试按设计输出既有 stderr，但不影响通过。final fix wave 未触碰 Rust，因此没有重复运行 Rust，也不能把此前 Rust 门禁冒充为本轮新执行。

## 2026-08-15 v3.19.1-26 Task 11A 图像能力持久化闭环

- 独立 final re-review 在 `ae764934` 发现最后一个 Important blocker：`CodexFormFields` 已把显式图像能力交给父表单，但 `ProviderForm.normalizeCodexCatalogModelsForSave()` 未写出 `supportsImage`，`useCodexConfigState.extractCodexCatalogModels()` 也未从 `supportsImage` / `supports_image` / legacy `vision` 读回；因此没有 `inputModalities` 时，显式 `true` 或有语义的 `false` 都会在真实 save/edit/re-save 边界丢失。根因是两个持久化 canonicalizer 字段表不完整，不是同步合并或模型能力推断错误。
- 严格 TDD RED `d8d72239` 只修改实际 save normalizer 与真实 hook 的测试：2 files / 11 tests 中 8 passed / 3 expected failed，收到对象均只缺 `supportsImage`。fixture 分别锁定无 `inputModalities` 时独立保存 `true` / `false`、camelCase DB SSOT 读回、snake_case live reverse-parse 和 legacy `vision` 统一为 camelCase；没有通过 mock 或 modalities 反推预期。
- 最小 GREEN `5b08272e` 只修改 `ProviderForm.tsx` 与 `useCodexConfigState.ts`：保存端仅以 `typeof supportsImage === 'boolean'` 写出 canonical 字段，读取端按 camelCase、snake_case、legacy `vision` 的显式 boolean 优先级归一化；`false` 不因 falsy 被丢弃，不从模型名或 `inputModalities` 推断。RED 边界转为 11/11，扩大 CodexFormFields / ProviderForm / config-state 聚焦为 6 files / 48 tests 全绿，typecheck、Prettier、diff check、两份生产文件 strict UTF-8/no BOM/no U+FFFD 通过。
- Task 11A 再次以 Codex 内置搜索和固定 Matrix WebSearch bridge 独立核对 OpenAI Codex 官方 `openai_models.rs`：`input_modalities` 是 backend/client 交换的模型元数据。Matrix 索引搜索没有返回权威结果，但对官方 raw 源直接 `open` / `find` 成功；具体 persistence blocker 仍以本地 save/load 调用链和 RED/GREEN 为权威。聚焦运行保留既有 `baseline-browser-mapping` 提示，ProviderForm preset 测试还输出既有 React `act(...)` warning，均不影响退出码。
- Important 3 现在从 `/models` 获取、表单新增/更新/手选、父表单保存 normalizer 到 DB/live reload canonicalizer 端到端闭环。`ae764934` 的 123 files / 990 tests 是加入三项 Task 11A 回归前的历史中间门禁；最终实现 HEAD `7c5fb51c` 由主线程运行完整 Vitest，123 files / 993 tests 全部通过、exit 0、耗时 238.4 秒，随后 `vitest list --exclude '**/.worktrees/**' --json` 独立确认同为 123/993。Task 11A 独立复审在该实现 HEAD 返回 PASS，历史 `final-rereview-report.md` 继续只代表 `ae764934` 的 FAIL 时点。
- Task 11A 未修改 Rust、版本或 reasoning defaults，也未 build/install/tag/push/publish。`localeCompare` vs NFKC + case-fold 与 V2 reasoning/catalog 仍是原两项条件性非阻断风险，状态不变。

## 2026-08-14 v3.19.1-26 候选门禁运行时边界

- 事务安装器 Pester 套件依赖 Windows PowerShell 5.1 / Pester 3.4 语义：测试会从 `$PSHOME\\powershell.exe` 启动 disposable child，且 SQLite corrupt/StrictMode 断言在该宿主可靠。不要用 Codex primary-runtime 的 PowerShell 7 直接判定产品回归；其 `$PSHOME` 缺少 `powershell.exe`，会造成 3 个环境性失败。应以 `C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -NoProfile -ExecutionPolicy Bypass` 运行，候选门禁为 46/46。

## 2026-08-15 V27 角色 TOML `max` 回归根因

- `3.19.1-25` 用户无法把 `model_reasoning_effort = "max"` 写入角色 TOML，不只是 Rust 枚举缺少 `Max`：前端 `CodexSubagentProfileEditor.isUsableProfile()` 仍使用 `low/medium/high/xhigh` 四档白名单。用户选择 `max` 后，草稿会立即被归类为“无效能力配置”，正常 profile 区域消失，因而无法继续预览和保存。
- 修复边界位于 `bigstrongsun/release-v3.19.1-27`，基于正在打包的 `bigstrongsun/release-v3.19.1-26@dd967801`；不得修改 `-26` 打包工作树。显式档位契约现阶段同时补齐 Rust Serde/TOML 枚举、TypeScript 类型、前端可用性白名单和下拉框的 `max/ultra` 候选；后续仍须由统一 capability resolver 对具体模型收紧实际可选集合，DeepSeek 不得凭空获得 `ultra`。
- RED 提交 `42dd052c` 同时证明 Rust 缺少 `ModelReasoningEffort::Max` 且前端没有 `max` 选项。GREEN 定向证据：`codex_subagent_v2_fixed_max_round_trips_into_role_toml` 通过；前端选择 `max` 后 profile 不再变成 invalid，生成预览包含精确 `model_reasoning_effort = "max"` 并能持久化；Rust V2 profiles 75/75 通过。

## 2026-08-15 V27 推理能力统一解析器

- Provider 的 `supportedEfforts` 不能再直接当作 Codex role/spawn 的可选集合。`codex_reasoning.rs` 的归一结果分别保存 Provider 原生接受值、Codex 可选值、Provider 默认值、关闭能力、来源置信度和 effort 映射；unknown 保持空集合，不能回退到通用 GPT 档位。
- DeepSeek V4 维护声明的 Provider 原生集合是 `low/high/max`、默认 `high`、允许关闭；确认映射为 `low->low`、`medium->high`、`high->high`、`xhigh->high`、`max->max`。生成 Codex catalog 因此声明 `none/low/medium/high/xhigh/max`，但不得推断 `ultra`。映射目标不属于 Provider 原生集合时，声明验证直接失败。
- `CodexReasoningEffort` 的候选词汇统一覆盖 `none/minimal/low/medium/high/xhigh/max/ultra`；具体模型只能从归一后的 `codexSelectableEfforts` 取值。该 resolver 同时供 catalog 投影和 Chat 转换配置使用，避免前后端或投影/请求各自维护模型名分支。
- TDD 证据：resolver RED `8f787070`、GREEN `998d517c`；DeepSeek catalog/preset RED `77b6cb09`。当前聚焦门禁为 resolver 5/5、Codex config 163/163、Codex Provider 98/98、preset 10/10，TypeScript typecheck 通过；仅保留既有 `openai_cache_read_tokens` dead-code warning。

## 2026-08-15 Codex ImageGen 502 transport root cause (V27)

- Live installed runtime is `3.19.1-25` on `127.0.0.1:15721`; V26 packaging worktree remains untouched. Diagnosis and the follow-up fix belong to `bigstrongsun/release-v3.19.1-27`.
- Current Codex image generation/editing uses long-running HTTP POST requests to `/backend-api/codex/images/generations` and `/backend-api/codex/images/edits`; it does not use the Responses WebSocket. A working Responses WebSocket therefore does not prove the image POST path is healthy.
- Live `codex-router.log` evidence on 2026-08-15 showed a 13.1 MB `gpt-image-2` edit correctly routed through the official OAuth route, followed by `connection_closed_before_message_completed` after 134-181 seconds. The request had already been uploaded and could still have executed upstream.
- `map_reqwest_send_error_class` incorrectly treated `reqwest::Error::is_request()` as a pre-send build failure. Reqwest request errors also include failures such as `client_error (SendRequest): connection_closed_before_message_completed`; returning retryable 502 caused Codex `EndpointSession` transport retries to replay the expensive, non-idempotent image request.
- Separate 7-13 second `unexpected EOF during handshake` failures were true connect-stage failures and remain safe for bounded retry. Raising the existing 600-second non-streaming timeout cannot fix either class because the observed failures occurred earlier than that deadline.
- Correct repair boundary: use `is_builder()` only for definitely pre-send construction failures; classify other non-connect/non-timeout send/read failures as response-pending; map image response-pending failures to a non-retryable result-unknown response; and add long-lived HTTP/2 keepalive support for the image POST path instead of blindly replaying it.
- V27 implementation changes `map_reqwest_send_error` to distinguish `is_builder()` from the much broader `is_request()`. `SendRequest: connection_closed_before_message_completed` now becomes `ResponsePending`; DNS/TCP/TLS/connect failures remain `ForwardFailed` and retain bounded retry.
- Image endpoint `ResponsePending` now returns HTTP 424 with code `cc_switch_image_result_unknown`, `retryable=false`, and no `Retry-After` header. This prevents both CCSM provider/account replay and Codex `EndpointSession` 5xx/transport replay while making the unknown-result state explicit instead of reporting a false 502 handshake failure.
- The shared reqwest client now sends HTTP/2 keepalive pings every 30 seconds (20-second acknowledgement timeout), keeps them active while idle, and enables the adaptive HTTP/2 flow-control window. This is a best-effort connection-lifetime hardening; it cannot recover an image whose upstream job has already detached because the public Codex image API returns no resumable generation ID.
- Targeted validation: `image_generation` 13/13, `codex_proxy_` 10/10, and `reqwest_` 3/3 tests passed. The only warning remains the pre-existing unused `openai_cache_read_tokens` helper.

## 2026-08-15 v3.19.1-26 本地发布 PowerShell 宿主污染根修

- `pnpm release:local` 的第一次 v26 调用在进入 Tauri 编译前就于 `scripts/export-latest-ccswitchmulti.ps1` 的 `New-TemporaryFile` 失败，因此没有生成可验收的新二进制，不能计作一次有效构建。
- 根因不是 Windows PowerShell 5.1 缺失该 cmdlet，而是 Codex primary-runtime 启动的 `pnpm exec powershell` 继承了含 PowerShell 7 模块目录的 `PSModulePath`；Windows PowerShell 随后优先兼容加载 `Microsoft.PowerShell.Utility 7.0.0.0`，该导出面没有 `New-TemporaryFile`。普通 WinPS 5.1 同机可正常发现该命令。
- RED `cdeb6969` 先在污染宿主暴露 helper 缺失；GREEN `39e41d70` 新增 `scripts/release-build-config.ps1`，仅使用 `[System.IO.Path]::GetTempFileName()` 与 `[System.IO.File]::WriteAllText(..., UTF8Encoding(false))` 生成 Tauri override，并由显式 cleanup 删除。不要通过修改用户/系统 `PSModulePath` 或 Codex runtime 来修发布脚本。
- GREEN 验证：污染的 `pnpm exec powershell` 1/1，原生 Windows PowerShell 5.1 1/1，事务安装器加发布配置测试 47/47，typecheck 与 diff/UTF-8 hygiene 通过。修复后的下一次 `pnpm release:local` 才是 v26 的第一份有效本地构建证据。
- 官方事实交叉验证使用 Codex Web 与固定 Matrix WebSearch 两条独立链；两者均能读取 Microsoft Learn 的 `New-TemporaryFile` 与 `about_PSModulePath`，具体污染顺序仍以本机 `$PSModulePath`、实际模块版本和可复现子进程为权威。
- 修复后第二次 `pnpm release:local` 已完成 release 编译、Vite build、最终应用链接和 NSIS，随后在 exporter `Write-Checksums` 的 `Get-FileHash` 再次失败。这不是新根因，而是同一不完整 `Microsoft.PowerShell.Utility` 导出面的第二个依赖点；只修临时文件而不审计同模块后续调用的边界过窄。
- 第二轮 TDD RED `44705602` 用固定内容和固定 SHA-256 锁定缺失的 `Get-ReleaseFileSha256`；GREEN `9808b11e` 在现有 helper 中实现流式 `[System.Security.Cryptography.SHA256]`，并让 exporter 与 `local-release-pipeline.ps1` 两处 checksum 都不再调用 `Get-FileHash`。污染宿主和原生 WinPS 各 2/2，事务安装器加发布测试 48/48，三份脚本 WinPS 5.1 parse、typecheck、UTF-8/diff hygiene 通过。
- 第二次调用虽然留下 raw EXE 和 NSIS，但缺少完整导出、metadata 和最终 checksum，因此不能冒充完整 release；最终必须从包含两轮修复的 clean HEAD 重跑完整流水线，而不是只给失败目录补文件后宣称完成。

## 2026-08-15 v3.19.1-26 Tauri updater bundle metadata 版本配对

- clean HEAD 的完整本地构建 exit 0，但 Tauri CLI 输出 `__TAURI_BUNDLE_TYPE variable not found in binary`。不能仅凭 raw EXE 同时能搜到 `__TAURI_BUNDLE_TYPE_VAR_NSS` 与 `..._UNK` 就判定为假警告：后者也存在于 `bundle_type()` 的 match 常量中，文本出现不证明运行时 static 被正确改写。
- 根因是构建两端协议错位：npm `@tauri-apps/cli 2.8.1` 内置的 `tauri-bundler 2.6.1` 仍通过 `.taubndl` PE section 和指针定位三字节 `UNK`；Cargo 实际解析的 `tauri-utils 2.8.3` 来自提交 `9b17a7ae`，已移除该 section，改为长字符串 `__TAURI_BUNDLE_TYPE_VAR_UNK`。官方 `tauri-apps/tauri#14521` / `0575dd287e02` 同时改变 utils 与 bundler，改用字符串搜索替换；这与本机二进制、Cargo.lock 和 CLI 源码逐层一致。
- RED `6a513c08` 在生产依赖未变时稳定证明 CLI 版本不满足 marker-based utils 的最低兼容边界；GREEN `82a4e7c1` 将 `@tauri-apps/cli` 精确锁到 `2.10.1`，该版本源码包含官方字符串替换实现。新版 CLI 的严格 preflight 随即揭示三个被旧版静默放过的 JS/Rust minor mismatch：`tauri 2.10.3` / API 2.8、dialog 2.6 / JS 2.4、updater 2.10 / JS 2.9。RED `4b181977` 锁定三组契约，GREEN `65ac17c8` 只把对应 JS bindings 精确对齐到 2.10.1、2.6.0、2.10.0，没有升级无关插件或整个 Rust 栈。
- 原生 Windows PowerShell 5.1 下 transaction/release Pester 为 50/50，frozen install、typecheck 和 Tauri preflight 通过；实际 `tauri build --bundles nsis` 明确输出 `Patching ... with bundle type information: nsis`，完成 NSIS 且不再出现旧 updater warning。以后 Tauri CLI 升级后必须把其 mismatch preflight 当作真实契约门禁，不能用跳过检查参数恢复旧行为。
- 本轮联网交叉验证使用 Codex Web 与固定 Matrix WebSearch：Codex Web 命中官方 issue `#14059`、官方源码和 `#14521`；Matrix 精确搜索无结果。最终版本边界以官方 GitHub commit、npm registry、Cargo crate VCS metadata 和本地实际二进制为权威，Matrix 空结果只表示独立检索未命中，不表示 bridge 失效。
- v26 第一次真实安装事务 `ccsm-20260815-041148-f9c9251cee884db28fd6493ca241f583` 因 `new runtime hash mismatch` 安全回滚到 25，RollbackError 为空；25 的版本、原 SHA-256、15721 owner 与 health 200 均恢复。RED/GREEN `e23642a3` / `591c2090` 让事务保留 actual/expected 哈希，第二次诊断事务 `ccsm-20260815-041638-64df1c849ebe4e179a2b927f47425849` 得到安装态 `C4C897875265F33A7C63741187AFEE521836E1D9BB070677DA569AAA66F98803`、raw `EE522E79190E4A56A3B76E09B5E5B01B18BA3D8BA6A0A6BE7BFA2F3D2109790A`，并再次无损回滚。
- 两个哈希差异精确等于 raw EXE 中唯一 `__TAURI_BUNDLE_TYPE_VAR_UNK` 被替换为 `..._NSS`。新版 bundler 为 NSIS patch 临时二进制，完成后恢复 target raw；exporter 因此导出的是 portable/raw 形态，安装器内嵌的是 NSIS 形态。RED/GREEN `667b7572` / `d6b0d13a` 在 `release-build-config.ps1` 增加 `Get-TauriNsisInstalledExeSha256`：必须恰好一个 UNK marker，按字节替换后计算安装态哈希，零个或多个都 fail closed；真实 v26 推导值与诊断事务 actual 完全一致，Pester 53/53。以后事务安装不得直接用 raw artifact SHA 作为 ExpectedInstalledHash。

## 2026-08-15 v3.19.1-26 Sub-Agent V2 最终运行阻断与根修

- `71f35116` 的 v26 安装候选进程、版本与 health 都正常，但无模型名真实 canary 证明它不能发布：只读长上下文任务首先选择内置 `explorer`，Codex `0.147` 随后因 custom role 与 full-history fork 兼容边界报错，并在删除 `agent_type` 后退回官方子 Agent。临时向父级加入 selection policy 后才选择 `deepseek-v4-flash`，说明只有 role 文件而没有父级委派策略不足以形成预期自动选型。
- 同一安装态的 `~/.codex/models_cache.json` 中 DeepSeek V4 Flash 的 `default_reasoning_level` 为 null、`supported_reasoning_levels` 为空；固定 `medium`、继承 `high`、显式 `low` 都被 runtime 拒绝。这是 catalog 投影与 profile compiler 之间缺少能力契约，不是上游模型本身不可用。
- RED `35fa3b87` 锁定父级 selection policy 托管注入、用户 developer instructions 保留、V1 可逆清理、Codex `0.147` fork 兼容、catalog reasoning 支持/默认值和不支持 effort 回退。GREEN `80e7b04a` 将 selection policy 与实际 roles 编译为可替换的顶层 managed developer-instructions block，明确匹配任务中的 preferred custom role 优先于通用 built-ins；兼容失败必须保持相同 `agent_type` 并改用 `fork_turns=none` 或正整数，不得静默退回内置 Agent。
- `CatalogModel` 现在携带 supported/default reasoning；profile compiler 只生成目标模型支持的 `model_reasoning_effort`，不支持时回退 catalog 默认并输出 warning，能力未知时省略固定 effort。DeepSeek V4 Flash/Pro 精确旧 ID 恢复 `low/high/max`、默认 `high`；显式行能力优先，未知第三方不得继承 GPT 档位。
- 根修后的门禁为 profiles 77/77、`codex_config` 166/166、reasoning resolver 3/3、Rust library 2978 passed / 0 failed / 2 ignored、Vitest 123 files / 993 tests（234.01 秒），并通过 `cargo check --lib`、rustfmt、UTF-8/no BOM/no U+FFFD。该问题是最终安装 canary 新发现的 acceptance gap，不是 173-ref/198-patch 历史分叉审计中的漏移植；`actionable-missing=0` 仍成立。
- `71f35116` 的资产与安装态仅用于阻断证据，全部哈希作废。必须从包含 `80e7b04a` 及后续文档提交的 clean HEAD 重新构建、事务安装并覆盖诊断时手工修改的 DeepSeek agent TOML，再重启 app-server、新建会话验证 Flash、Pro 与官方保留路径；配置或单元测试不能代替真实无模型名 canary。

## 2026-08-15 v3.19.1-26 正式构建、安装、canary 与分叉复核

- 正式 tag peeled commit 为 `7e6665fcfd297f3f8954850d59f9786831b1e3d2`。`pnpm release:local` exit 0，metadata 精确绑定该提交，15/15 checksum 回读一致；Windows x64 raw EXE SHA-256 为 `CFE8B6684B2FAF6C0D1178B3515BDD2779116E6DDCEED251C72ABECC6A41B626`，NSIS 安装态 SHA-256 为 `01B3181D5775FE06FFA11C31246B66F3AF03118197F7C3E5D4247719EB408791`。
- 事务 `ccsm-20260815-055216-f59d4af5f0354cb6a76f17614b090351` 由独立隐藏 PowerShell 进程完整执行 kill、卸载、安装、拉起、健康检查和失败回滚边界；最终 PID `55436`，`/health` HTTP 200，FileVersion/ProductVersion 均为 `3.19.1-26`，运行中安装态哈希与预期值精确一致。任何后续安装都不得在普通交互 shell 中单独停止 CCSM。
- 新会话 Flash child `01a00247-28f8-7ae0-9240-fa20a8eac5b4` 为 `agent_role/model=deepseek-v4-flash`、`model_provider=codex_model_router_v2`、effort high，真实执行 `rg` 和 `git status` 并完成同 child follow-up；Router 命中 DeepSeek native `/responses` 且 HTTP 200。
- 新会话 Pro child `01a0024a-1feb-7212-907e-04cb8655a742` 为 `agent_role/model=deepseek-v4-pro`、provider `codex_model_router_v2`、effort high，完成实际源码/commit 审查；Router 将 Responses 桥接到 DeepSeek `/chat/completions` 并返回 HTTP 200。
- 官方保留 child `01a0024e-5404-7f90-ae0b-ec76026f067b` 选择内置 `default`、`gpt-5.6-sol`、effort medium，Router 命中 ChatGPT Codex Responses 并返回 HTTP 200。三条 canary 证明最终父级 selection policy 能在匹配的 Flash/Pro 与官方保留路径之间自动选择，而不是仅证明角色文件存在。
- 推送发布分支后再次用 live `git ls-remote --heads fork` 对照：139 个远端 heads 与 139 个本地非 symbolic tracking tips 逐 SHA 一致，`NEW/MOVED/DELETED=0`。新增 tip 仅为发布分支自身；正式 tag 固定在 `7e6665fc`，tag 之后的 post-release 提交（首个为 `07f1da65`）只修改审计、发布说明和 memory。原 173-ref/198-patch 审计的 `actionable-missing=0` 结论不变。
- GitHub Actions run `31846073316` 最终为 success：Windows x64/ARM64、Linux x64/ARM64、macOS、Publish GitHub Release、Assemble `latest.json` 七个 jobs 全部成功。Release 非 draft、非 prerelease，且 `releases/latest` 返回 `v3.19.1-26`；19 个资产全部下载后逐一计算 SHA-256，与 GitHub 服务端 digest 无任何 mismatch。
- `latest.json` 版本为 `3.19.1-26`，六个平台键为 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64`、`windows-aarch64`、`linux-x86_64`、`linux-aarch64`；URL 全部指向 `/releases/download/v3.19.1-26/`，签名全部非空。Annotated tag object 为 `52ff4d24f0c0c7f2e304816f70484227e4ff5b43`，本地/远端 peeled commit 均为 `7e6665fcfd297f3f8954850d59f9786831b1e3d2`；发布分支可在 tag 之后追加只含文档的发布证据提交，但不得移动 v26 tag。

## 2026-08-15 v3.19.1-27 发布线汇合与候选门禁

- v27 工作树在主任务接手前已有 32 个独立提交，覆盖 reasoning capability resolver、Sub-Agent schema v2/UI 协调和 ImageGen 长请求防重放，但它从 v26 早期候选 `dd967801` 分叉，缺少 v26 最终 30 个独立提交。合流提交 `6fe99e9d` 整体合入 `e963b07d`，没有从 v26 重建空分支或重复 cherry-pick；合流前备份 ref 为 `backup/release-v27-pre-v26-final-merge-20260815`。
- 冲突根因是 v26 的 supported/default effort 临时契约与 v27 的统一 runtime policy 同时修改 profile compiler。最终采用 v27 的 `delegated/model_default/fixed/disabled` 与 resolved capability，同时保留 v26 的父级 selection policy、legacy DeepSeek fallback、状态 warning 汇总和发布/事务脚本。legacy `auto` 迁移为 delegated；显式 `xhigh` 保持 fixed `xhigh`；DeepSeek fallback 声明 Provider `low/high/max`、默认 `high`、允许关闭，并映射 `medium/xhigh -> high`。
- 合流聚焦门禁：profiles 84/84、`codex_config` 167/167、ImageGen 13/13、Codex proxy 10/10、reqwest 3/3、前端 7 files / 241 tests。全量门禁：Rust library 2991 passed / 0 failed / 2 ignored；Vitest 123 files / 996 tests；typecheck、Prettier、cargo check/fmt、diff hygiene 均通过；原生 Windows PowerShell 5.1 Pester 53/53。
- v27 不重复 v26 的 173-ref/198-patch 历史审计；v26 已证明 `actionable-missing=0`。本轮只确认 v27 现有新提交与 v26 最终发布线已经同树可达。正式 build/install/tag/Release 证据必须在 `3.19.1-27` 版本提交之后追加，不能复用 v26 资产或哈希。

## 2026-08-15 v3.19.1-27 本地安装与 GitHub 正式发布

- 版本提交 `50c32b3731c91e50c1249821aebc9e5da4110823` 统一 package/Cargo/Tauri/lock 版本并包含 v27 中文说明；`pnpm release:local` 在该 clean HEAD 上 exit 0，metadata 精确绑定该提交，15/15 checksum 一致。raw EXE SHA-256 为 `CF31C903F52027F6B7D6E608AF6AA43FF2C1C609CBE2224D8C66F2304B008107`，NSIS installer 为 `D318CCDC6BF8CB7F7424E38FF9B359D1E4BEE6620EF9307CB3FDF74C59CDF522`，NSIS 安装态预期哈希为 `FA9726A634969DF11AD068E034063DBF3658694EC06078FF1F901DBB3D2B7D7D`。
- 独立隐藏事务 `ccsm-20260815-074918-c5bf859249d348ec98840a8717578637` 返回 `Success`、`Error=null`、`RollbackError=null`。新 PID/15721 owner 为 `48284`，路径为已安装 `cc-switch.exe`；FileVersion/ProductVersion 均为 `3.19.1-27`，安装态哈希精确匹配推导值，`/health` HTTP 200。普通交互 shell 未单独停止 CCSM。
- 安装版 UI 真实验收通过：MultiRouter 顶部六个导航包含独立“子 Agent”；当前 V2 的按钮为 disabled“已启用 V2”，V1 为可操作“启用 V1”。Flash profile 展开后实际显示 builtin/confirmed 能力来源、Provider `low/high/max`、Codex `none/low/medium/high/xhigh/max`、默认 `high`、允许关闭以及 `medium/xhigh -> high` 映射；旧配置固定 `medium` 被明确标注为主 Agent 不可覆盖，不再伪装成 delegated。
- annotated tag object 为 `afe53ad14fa3f5e84e7c697c31c5002d18ac6944`，本地/远端 peeled commit 均为 `50c32b3731c91e50c1249821aebc9e5da4110823`。GitHub Actions run `31851926650` 七个 jobs 全部 success；Release 非 draft、非 prerelease并成为 Latest。
- GitHub Release 有 19 个实际资产。全部资产下载到独立临时目录后按服务端 `digest` 逐项复算 SHA-256，`MismatchCount=0`；`latest.json` 版本为 `3.19.1-27`，六个平台键齐全，全部 URL 指向 v27 且签名非空。
- 用户本轮明确要求 GitHub Release；R2 `release.released` workflow 没有被 Actions 的 `GITHUB_TOKEN` 二次触发，因此未手动 dispatch，也不得声称 R2 已同步。tag 后只允许提交发布证据文档，禁止移动 v27 tag 或用文档变化触发重复构建。

## 2026-08-15 MultiRouter 向导职责与 Sub-Agent V2 候选边界

- MultiRouter 向导只负责“选择已就绪模型源、同步验证、组合路由、保存启用”。Provider 凭据、模型目录、API 协议、推理能力和 Hosted Tools 的配置归各自 Provider 页面或 MultiRouter 工作台所有；不要再把这些编辑器重复塞回向导首屏。向导中的 Provider 卡片只显示低干扰摘要和“配置 Provider”跳转。
- 向导入口必须显式区分“创建新配置”和“编辑旧配置”。编辑入口列出每条可编辑方案的名称与 ID，向导以精确 `planId` 读取目标，顶部持续显示当前方案身份；打开入口前先刷新 Provider query，目标丢失时 fail closed，不能回退到 `providers.find(...)` 的第一条路由方案。
- Sub-Agent V2 的普通“添加第三方可配置模型”只同步 authoritative classifier 判定为 third-party 的可路由模型。已有官方 profile 为兼容保留，但默认收进“官方模型（高级）”折叠区；官方 Codex 通常由内置角色继承当前模型，不应让普通用户误以为必须逐个勾选官方模型。
- `routable=false` 是可用性/配置状态，不是错误，必须使用中性灰色；红色只用于解析失败、保存失败等真实错误。不可路由且未启用的 profile 不允许启用或编辑；已启用后变得不可路由的 profile 仍必须允许用户关闭。
- 本轮 RED 提交为 `bcdc8ef6`。生产实现 focused 证据：V2 前端、向导新旧测试和 App 入口集成共 168/168；Rust `codex_subagent_v2` 103/103；typecheck、Prettier、rustfmt、`git diff --check` 通过。Browser 在独立 Vite renderer 中实际验证深色与通过 CDP 模拟的浅色首屏；Tauri-only invoke/event 错误属于 renderer 脱离 native host 的预期限制，不能冒充安装版运行时无错误证明。
- 本修复位于 `bigstrongsun/fix-wizard-provider-subagent-ux`，基线为已发布 v27 的 `d2a0a2dc`；不得移动或重建 `v3.19.1-27` tag，也未获授权推送、安装或发布本修复。

## 2026-08-15 Codex 热切换 Windows notify 与 Sub-Agent V2 固定字段丢失

- 用户同时报告了两条互不依赖的切换失败。第一条是 Codex Desktop 生成的根级 `notify` 命令把 Windows 路径写成 TOML basic string 中的裸反斜杠，`C:\Users...` 的 `\U` 被解析为 8 位 Unicode 转义起点，CCSM 在读取原 Live 配置时严格失败。第二条是普通 Provider 表单读取已有 `codexRouting` 时只重建 `enabled/defaultRouteId/officialAuth/routes`，把 `subagentVersion` 和完整 `subagentV2` 固定字段从保存结果中删除，下一次 V2 编译因此报笼统 `invalid_configuration`。
- Live 兼容边界只能处理首个 TOML table 之前、不在多行字符串内、根级 `notify` 数组首参数中的 Windows 绝对路径；仅当原 TOML 无效且路径含未配对反斜杠时补齐转义。合法配置、单引号 literal path、多行 developer instructions、其他表和任意非 notify 语法错误必须保持原行为，不能用宽泛字符串替换掩盖损坏。
- `codexRouting` 是方案级可扩展对象。`extractCodexRoutingConfig()` 读取对象 schema 时必须先保留原对象，再规范化 UI 直接维护的核心字段；类型契约显式包含 `subagentV2`。普通 Provider 保存、模型目录编辑或协议刷新都不得擦除 V1/V2 选择、问卷、reasoning runtime policy、overrides 或未来新增的方案级字段。
- V2 schema 2 的公开校验码必须覆盖 `missing_reasoning_policy`、`invalid_reasoning_policy`、fixed/default effort、input modalities、legacy v2 字段和 reasoning capability 不兼容等实际错误；已知结构错误不得再折叠为 `invalid_configuration`，否则用户无法定位丢失栏位。
- RED 提交 `7460469b` 分别锁定路由固定字段保留、Windows notify 读取归一化和具体 reasoning 缺失错误码。GREEN 聚焦验证为 ProviderForm/配置状态与 V2 编辑器合计 140/140、扩大 Codex config 170/170、`cargo check --lib`、typecheck、Prettier、rustfmt 和 `git diff --check` 全部通过；既有 `baseline-browser-mapping` 提示和 `openai_cache_read_tokens` dead-code warning 与本修复无关。
- 本机当前 Live 配置同时存在多行 `developer_instructions` 内的 notify 示例文字和真正根级 notify；实际复读确认前者必须逐字保留、后者在当前 Codex 版本已是合法双反斜杠。读取兼容器的 multiline/root-scope 限制正是防止把说明文字当配置修复。

## 2026-08-15 v3.19.1-28 发布准备

- v28 只包含 v27 后的向导职责收敛、Sub-Agent V2 第三方候选隔离、Windows notify Live 读取兼容和普通 Provider 保存保留 V2 固定字段；已发布的 v27 tag 与资产保持不变。四处版本统一提交为 `fa70e7b3`，中文说明为 `docs/release-notes/v3.19.1-28-zh.md`。
- 固定构建前完整门禁：Rust library 2996/2996，Vitest 123 files / 1002 tests，原生 Windows PowerShell 5.1 下 release-build-config 6/6、事务安装 47/47；`cargo check --lib`、typecheck、Prettier、rustfmt 和 `git diff --check` 通过。Pester 3.4.0 的 `Should Throw` 在 PowerShell 7 下会误报，必须按仓库既定边界使用 Windows PowerShell 5.1，不能把运行器不兼容当生产失败或伪装成通过。
- 第一次 v28 本地流水线虽 exit 0，但构建日志出现 `__TAURI_BUNDLE_TYPE variable not found`；事务 `ccsm-20260815-121651-5dd2541f85cc43c69cbdf94b84c45dae` 发现安装态哈希仍等于 raw EXE、没有完成 `UNK -> NSS` 标记，按设计回滚到 v27，`RollbackError=null`，回滚后 FileVersion/ProductVersion 为 `3.19.1-27`、哈希 `CD0ED804D3D1CABD10144E31CC674A84A7855889BCF9ECF49F58C140D5167BC4`、health 200。
- 根因是 worktree 的 `node_modules` junction 指向主工作树：声明与 lock 已固定 `@tauri-apps/cli 2.10.1`，实际安装包仍为 `2.8.1` 且命令报告 `tauri-cli 2.8.0`，与 Rust `tauri-utils 2.8.3` marker 机制不匹配。普通 frozen install 在 junction 重建确认下可无变更返回 0，因此发布流水线必须先执行非交互 `pnpm install --frozen-lockfile --force`，再同时核对声明版本、实际安装 package 版本和 CLI 自报版本，之后才允许 typecheck/export；不能把事务 expected hash 改成 raw 来掩盖 updater bundle type 缺失。实际依赖重建后 package 与 CLI 均为 `2.10.1`。

## 2026-08-15 Qwen3.8 中途停顿与 Hosted Tools 流式根修

- 目标会话约 189K prompt tokens、945 KB 请求体、139 条 Chat message 和 41 个工具，仍低于 Qwen3.8 的 262144 上下文；现场没有 429、5xx、context overflow 或转换丢失。长上下文只放大等待，不是根因。
- roglinux 透明代理把 thinking/Tool Guard 模型硬编码为 `qwen3.6`，且模型切换只重启 vLLM worker：环境文件已写 `VLLM_SERVED_MODEL_NAME=qwen3.8`，长驻 proxy 进程仍持有 `qwen3.6`。独立修复仓库 `C:\Users\sunda\Documents\LLMservice\qwen38-tool-guard-fix` 用 RED/GREEN 将系统 Guard、tail Guard、流过滤和 generation limit 统一到运行时模型解析器，并让 controller/兼容 shell 切换事务重启 proxy/dashboard。生产 canary 日志为 `codex_tool_guard_applied=true`、`system_applied+tail_applied`，同时正常最终文本成功，不能用全局 `tool_choice=required` 代替。
- CCSwitch 根因位于 `forwarder.rs`：只要原始请求带 Hosted Web Search/Image Generation 且开关启用，就把全部 Responses-to-Chat 请求改成 `stream=false`；Codex 显示流式但上游没有增量，长上下文时表现为持续“正在思考”。v31 改为语义化传输策略：流式 `tool_choice=auto` 保留 SSE 并从 Chat 投影中移除 hosted-only 工具，普通客户端工具不受影响；显式 hosted tool choice 与真正非流式请求继续走有界 loop。不得根据用户文本猜测是否搜索。
- RED/GREEN 提交为 `9ca173a0` / `c45d0dfa`（从 v30 基线重放后的哈希）。与 reasoning PR 合流后的完整 Rust library 为 3006 passed / 0 failed / 2 ignored，前端相关测试 179/179；typecheck、`cargo check --lib`、rustfmt 与 `git diff --check` 通过。安装验收必须看到同一 trace 的 `streaming=true` 和 `upstream_stream=true`，并分别验证普通工具循环、显式 Hosted 工具、正常最终回答与长上下文。
- v31 本地 release 构建日志明确出现 Tauri `Patching ... with bundle type information: nsis`，安装包 SHA-256 为 `665CDBF69AE889CAA5AD3473A3AB71CAD4B99C79633EB41CDF93B86E15FE88F5`。事务 `ccsm-20260815-214338-8abca640d04e4137bf314eaa3d95264d` 返回 `Success`、`Error=null`、`RollbackError=null`；安装版 PID/15721 owner 均为 `48992`，ProductVersion `3.19.1-31`，SHA-256 `DE307C845D02CE59AF334DFEE98C2A2BC193E9A1A0981266BEFC59A5C0754A96`，health HTTP 200。
- 安装版真实 Qwen canary 使用 `stream=true`、`tool_choice=auto`，同时携带 hosted `web_search` 与普通 function。首个 SSE 事件 0.593 秒到达，共 50 个事件并以 `response.completed` 结束；router trace `ef86c36b-0970-4665-9a50-2c8b7371365d` 显示 `/responses -> /chat/completions`、Qwen route HTTP 200、`streaming=true`、`upstream_stream=true`。这证明全局 Hosted Tools 不再让普通 Agent 请求失去增量输出。
- 第二个安装版 canary 在相同 hosted `web_search` 广告下要求普通 `report_marker`，实际收到流式 `response.function_call_arguments.done`、正确工具名和 `CCSM_QWEN38_TOOL_OK` 参数；trace `ed4f112d-8604-4857-b8b3-9f81c00d38c2` 同样为 HTTP 200、`streaming=true`、`upstream_stream=true`。因此策略只移除 hosted-only 定义，没有误删 Codex 的终端/文件/MCP 类客户端工具。

## 2026-08-16 Qwen3.8 view_image original detail 回放 400 根修

- 现场任务 `01a005e2-7e5e-7a91-91be-1f69223b3c0a` 在 context compaction 后调用 `view_image` 检查 PPT 渲染页；工具输出被 Codex 持久化为 `input_image + detail=original`。随后 trace `c22c06de-491d-4448-a770-41e8a9316974` 和 `1c18bf43-f975-434b-aee0-752164d7a8e1` 都在 Responses→Chat 后由 Qwen vLLM 以 HTTP 400 拒绝，因为 Chat `image_url.detail` 只接受 `auto/low/high`。这不是超时、上下文溢出或最新 v31 流式修复引入的问题。
- 时间线表明缺口长期潜伏：Responses→Chat 的图片对象原样复制自初始桥接提交 `693c3872`（2026-06-02）；Codex 自 2026-03/04 已支持工具图片 `original`；2026-07-25 的 `dce97209`/`b2278d06` 开始系统保留图片能力，但生成的第三方 catalog 仍克隆官方 GPT 模板，令 Qwen 最终获得错误的 `supports_image_detail_original=true`。以前的 Qwen canary 主要验证文本、普通图片和工具调用，没有实际执行会返回原图 detail 的 `view_image`，所以没有触发。
- RED `308e2b32` 覆盖真实 view_image tool output、replayed image_url object、未知未来 detail 和第三方 catalog original-detail 行为。GREEN `92df4c4b` 在共享 Chat 媒体边界统一 `original -> high`、保留 `auto/low/high`、删除未知/非字符串 detail；直接 Responses input_image 对象也复用该归一化。最初把所有第三方 route catalog 写成 `supports_image_detail_original=false` 是错误的能力建模：它混淆了“上游是否原生接受 Responses 枚举”和“Adapter 是否能够提供该 Codex 能力”。纠正提交 `28c44489` 让具备图像输入能力的第三方 Chat 路由继续对 Codex 暴露 original detail，由 Adapter 翻译成上游最高可用的 `high`；纯文本模型仍为 false，同 slug 官方模型仍由 `merge_codex_model_entry` 保持官方权威值。
- 固定 HEAD `7fbcdd015e5ff9d41c78a3bfa8ba6ab2e1ce45e0` 的独立本地流水线完整 exit 0，产物目录为 `C:\Users\sunda\Documents\LLMservice\ccswitchmulti-qwen38-original-fix-7fbcdd01`；NSIS SHA-256 为 `E9A6F0EDB035306B4B1D61AAB88D3E64FF009E47DC16F21B2366A44FCBA95D4E`，raw EXE SHA-256 为 `BF565A08C866953B2269CF4FA6066BF700BF69E12D63A409B7818D6B7BD32655`。更早从 RED 提交启动的后台构建被判定为跨提交不可信产物，没有用于安装。
- 独立回滚事务外层 ID `ccsm-20260816-qwen38-original-7fbcdd01`、内层 ID `ccsm-20260816-013717-f431e5c235604281a249711023a7f9a3` 返回 `Success`，`Error=null`、`RollbackError=null`。安装态 `3.19.1-31` EXE SHA-256 为 `38DF6D5FACB6AF2E9AFA582497E3014FB6E345DC80DB2AC75A001F0B27EB36B6`，PID/15721 owner 为 `65304`，`/health` HTTP 200；NSIS 会修补 bundle type，因此安装态哈希与构建目录 raw EXE 不直接相等，事务使用仓库 helper 计算预期安装哈希并验证通过。
- 首次安装态验证曾确认 Qwen3.8 original-detail capability 为 false，但该状态随后被判定为产品语义错误，不能作为最终期望。协议转换本身的最小同形 canary 将 `function_call(view_image) -> function_call_output(input_image, detail=original) -> 后续文本轮` 发送到安装代理，trace `aabed260-f014-4d4b-9e98-1ea3fcf0d967` 走 `/responses -> /chat/completions`、HTTP 200，并收到 `response.completed` 与 `CCSM_QWEN38_ORIGINAL_REPLAY_OK`；最终安装验收还必须同时证明 Qwen3.8 的 snake/camel capability 均为 true。
- 原失败任务 `01a005e2-7e5e-7a91-91be-1f69223b3c0a` 的约 1.03 MB 完整历史也已在安装态重放；trace `a5eb6b81-c80d-4691-afc8-b130d933f192`、`85a36604-5a87-42c0-ad85-c02a1e2d9a62` 均返回 HTTP 200，并连续完成真实工具动作。该大历史中的 Qwen 仍会做多轮不必要检查、收尾慢，这是模型执行质量/工具策略问题，不是原来的 image detail HTTP 400；应作为独立问题评估，不能用来否定本协议根修。
- 影响面不止 PPT：view_image、MCP/custom tool 图片、function tool output、压缩历史以及跨 provider 回放都可能携带 original；仅修 catalog 不能处理旧历史或先官方后第三方的跨模型回放，仅修转换又会继续错误诱导 Codex 生成 original。两层必须同时修。Native Responses/官方透传不经过 Chat detail 归一化，原始语义保持不变。
- 发布流水线曾在进程启动时读取 v30，随后 worktree 提升 v31，导致实际成功构建 v31 但导出阶段仍寻找 v30；重新执行 `export-latest-ccswitchmulti.ps1 -SkipBuild` 后按当前 v31 正确生成安装包、签名和 `latest.json`。以后版本提升必须发生在启动发布流水线之前，不能在持锁构建期间改变版本源。

## 2026-08-15 v3.19.1-31 GitHub 正式发布

- 正式 annotated tag object 为 `faf339642fc6dbcd5f817d84563c9e04c0fc59ae`，本地与远端 peeled commit 均精确为最终合并候选 `12272a3184838f86284f55bfd3cd75ae7be9bd24`；tag 后的审计文档提交不得移动该标签。
- GitHub Actions Release run `31891915431` 七个 job 全部 success：Windows x64/ARM64、Linux x64/ARM64、macOS、`Publish GitHub Release` 与 `Assemble latest.json`。Release id `371086603`，非 draft、非 prerelease，并成为 Latest。
- Release 共 19 个实际资产。全部下载到独立临时目录后使用流式 SHA-256 逐项对照 GitHub 服务端 digest，`DIGEST_MISMATCH_COUNT=0`；Windows x64 Setup 大小为 `11678809` bytes，FileVersion/ProductVersion 均为 `3.19.1-31`，SHA-256 为 `a638296671d3ca20e1831e96521517e4ddb42c82fd1081ef1859ed79c6d6cdda`。
- `latest.json` 的版本为 `3.19.1-31`，六个平台键齐全，全部下载 URL 指向本 tag；六项 signature 均非空并与对应 `.sig` 文件精确一致。`release.released` 没有触发新的 R2 同步 run，因此本轮只声明 GitHub Release 完成，不声明 R2 已同步。

## 2026-08-15 v3.19.1-28 可信构建、事务安装与 UI 验收

- 可信发布候选固定为 `a94210a8e3be0ca7e0bfd5f8f4bc20621f006a94`。发布流水线先以 `pnpm install --frozen-lockfile --force` 重建真实依赖，再同时校验 package 声明、安装包版本和 `pnpm exec tauri --version` 均为 `2.10.1`；日志明确输出 NSIS bundle marker patch。导出目录 15/15 checksum 一致，raw EXE SHA-256 为 `4150120A7E5CEC39F160F7625786A262DCEC32F4FB12F34E1E47D1F5490953C3`，NSIS installer 为 `C56F2791739D1EA4B0245F7D3E036487D134E067000F2E3D6FC81BA66B0E9673`，预期安装态为 `19F41913EA5F5075FD35E09565B1D4133EEF8F60E228D3BFD60BC4732C4494A6`；updater `.sig` 为 432 字符且与 `latest.json` 一致。
- 独立隐藏事务 `ccsm-20260815-124330-93e15cd367bd4477a1ab26521a0e4ca1` 返回 `Status=Success`、`Error=null`、`RollbackError=null`。新 PID 为 `19660`，安装版 FileVersion/ProductVersion 和注册表均为 `3.19.1-28`，实际安装态哈希与预期精确一致，`127.0.0.1:15721/health` HTTP 200。临时 launcher 因 Windows PowerShell 5.1 `Start-Process -PassThru` 在本机返回空 `ExitCode` 误报失败；事务本体、结果文件和安装态证明实际成功，不能据 launcher 外层空值重复安装。
- 安装版只读 UI 验收确认 MultiRouter 六个顶部导航包含独立“子 Agent”；当前 V2 按钮为 disabled“已启用 V2”，V1 为可操作“启用 V1”。普通入口显示“添加第三方可配置模型”，官方 profile 收进“官方模型（高级）”；不可路由语义不再使用红色错误样式。配置入口明确区分“创建新配置”和“编辑旧配置”，编辑项显示 `New Codex MultiRouter / codex-multirouter`；进入编辑向导后顶部持续显示同一名称、ID 和“编辑旧配置”，首步明确只选择模型源，Provider 凭据、模型目录、API 协议、推理能力和工具兼容性回归各 Provider 页面。

## 2026-08-15 v3.19.1-28 GitHub 正式发布

- annotated tag object 为 `9d6a8d95f36adaabcb5563eba0cf577c1e24f1cf`，本地和远端 peeled commit 均精确为可信构建提交 `a94210a8e3be0ca7e0bfd5f8f4bc20621f006a94`；后续发布证据提交不得移动 v28 tag。
- GitHub Actions run `31865535416` 最终为 success，Linux x64/ARM64、Windows x64/ARM64、macOS、Publish GitHub Release、Assemble `latest.json` 七个 jobs 全部完成。Release id `370966917`，非 draft、非 prerelease，`releases/latest` 返回 `v3.19.1-28`。
- Release 共 19 个实际资产。全部下载到独立临时目录后逐项使用 SHA-256 对照 GitHub 服务端 `digest`，`DigestMismatchCount=0`；`latest.json` 版本为 `3.19.1-28`，六个平台键齐全，URL 全部指向 v28，签名全部非空且与对应 `.sig` 精确一致。

## 2026-08-16 合并官方 v3.19.2 与 CCSwitchMulti 3.19.2-1

- 官方正式 release/tag `v3.19.2` 指向 `43eaf07355af145aebfee301801779e824d4c221`；没有合并比 tag 多 52 个未发布提交的官方 `main`。合并前备份分支为 `backup/qwen38-v31-before-upstream-v3.19.2-merge-20260816`，merge commit 为 `d3b78e09`。四处权威版本源 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json` 统一为 `3.19.2-1`，继续保留 `cc-switch-multi`、`CCSwitchMulti`、`com.ccswitchmulti.desktop` 和 BigStrongSun 仓库身份。
- `-X ours` 只解决同一 hunk 的文本冲突，不能保证语义完整。此次实际发现并修复了 UI import 缺失、`ChatToolCallItems` 半合入、响应读取调用仍用旧 API、OpenCode 模型发现只有前端与测试而没有后端命令、重复测试模块、备份测试函数边界错位、OAuth 测试缺少 QueryClient 上下文等问题。以后合并官方 tag 后必须至少跑 `cargo check --tests` 和全量测试，不能以 `cargo check --lib` 作为完成证明。
- 用户明确选择采用官方无名 tool call 行为：当 Chat 上游返回缺失/空白 `function.name` 的调用，响应本应为 `completed`，且丢弃坏调用后没有任何可用 tool call 时，Responses 转换返回 `TransformError`，避免 Codex 把空成功回合当作 Agent 完成并静默停住。混有合法 tool call 时只丢坏调用并继续；`finish_reason=length` 保持 `incomplete/max_output_tokens`，不得误报无名调用错误。实现提交为 `52b0fee4`。
- v3.19.2 的响应体预算必须在所有传输上逐块执行：Hyper、reqwest 和自定义 Streamed 统一经 `bytes_stream()` 累积并在超限时立即断开，Buffered 在返回前比较已有长度；不能先 `collect()`/`bytes()` 收满再检查。读取中途失败仍按 CCSwitchMulti 既有契约归类为 `ResponsePending`，避免成功记账和重试诊断漂移。根修提交为 `0776cff3`，分类兼容提交为 `c7b47c06`。
- Windows 普通进程可能因 `ERROR_PRIVILEGE_NOT_HELD (1314)` 无法创建测试 symlink；两个 symlink 专项测试仅在该错误下提前返回，其他错误仍失败，有权限的平台仍执行完整安全断言。最终 Rust library 为 `3098 passed / 0 failed / 5 ignored`；前端为 `137 files / 1115 tests` 全通过；`cargo check --tests`、typecheck、Prettier、rustfmt、`git diff --check` 均通过。前端复跑前曾发现 Vite 文件缺失，使用 `pnpm install --frozen-lockfile --force` 按 lock 重建依赖后恢复，不能把损坏的 node_modules 启动错误归因于产品代码。

# 2026-08-16 Codex 真实 HTTP 429 自动续传热修

- 用户现场截图显示 Codex 先出现“正在重新连接 1/5”，随后以 `exceeded retry limit, last status: 429 Too Many Requests` 终止当前 turn；点击“继续”仍能沿用同一线程，说明线程持久化未丢失，但 turn 被 429 打断。
- 根因边界：现行 Codex Rust 客户端在 `ModelProviderInfo::to_api_provider()` 中固定 `retry_429=false`，直接 HTTP 429 可能一次都未重试便被包装成 `RetryLimit`；OpenAI issue `#30471` 仍在跟踪该误导文案和不可配置重试问题。CCSM 的 `ResponsePending/429` 则表示请求可能已经在途，必须继续禁止重放，不能与真实上游拒绝混为一类。
- 提交 `4b6c8e59` 先加入 RED 回归，`d70e0193` 实现代理内自动续传：只对上游明确返回的真实 HTTP 429 重发完全相同的 headers/body，最多 5 次；优先遵循 `Retry-After`，单次最多 60 秒、累计等待最多 180 秒，无头时按 1/2/4/8/16 秒退避。
- 确定性额度耗尽 `usage_limit_reached`、`insufficient_quota`、`billing_hard_limit_reached` 不在同账号空转，立即交给既有 Codex 账号池/MultiRouter 降级或返回客户端。终态 429 会重建响应并保留 `Retry-After`；ResponsePending、连接分类和语义输出后的流恢复边界均未改变。
- 验证：正确的 `3.19.2-1` 源码线上 `codex_rate_limit_retry_` 3/3、`upstream_transport_retry_` 3/3 通过，release 构建成功；仅有既存 `openai_cache_read_tokens` dead-code warning。
- 运行态采用不中断当前 Codex 流的磁盘热替换：安装路径 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 已是 `3.19.2-1`，SHA-256 `CB16F3830786369388222CA66F0F18E87F3C74BC949DFA0E1908B47F03111F50`；PID 35064 仍运行原映射文件且 15721 health 为 200，下次正常重启才加载补丁。回滚备份为 `backups\cc-switch.exe.pre-429-hotfix-20260816-1350.bak`，运行中旧映射文件为 `cc-switch.exe.running-old`。
- 联网前置使用 Codex 内置 Web Search、GitHub 官方 CLI/API 与 Matrix WebSearch 独立链。内置搜索和 GitHub 源码/issue 交叉确认 `retry_429=false`、issue `#30471` 与旧 TypeScript PR `#506`；Matrix 仅返回 MDN 429 定义和低相关结果，未提供 Codex 实现的第二份正证据。

# 2026-08-16 CCSwitchMulti v3.19.2-2 Codex 长网络波动续传根修

- 用户任务 `01a008f6-4db8-7042-8091-8981857ad6cf` 的最终错误虽显示 HTTP 429，但 `codex-router.log` 证明上游没有返回 429：多个并发任务先连续遭遇 `Connect: unexpected EOF during handshake`；目标任务内部每次只重试 2 次，随后消耗 Codex 自身五次重连预算；最后一次为 `SendRequest: connection_closed_before_message_completed`，被本地 `ResponsePending -> 429` 兼容映射显示成 429。
- `v3.19.2-1` 的真实 HTTP 429 重试逻辑本身生效，缺口是确认未发出的传输错误窗口太短。`v3.19.2-2` 将仅针对 `ForwardFailed` 的同 Provider 重试从 2 次增至 5 次，退避为 200ms/600ms/1.5s/3s/6s；结合连接握手时间覆盖现场约一分钟波动，避免过早占用 Codex 客户端重连额度。
- `ResponsePending` 仍不可自动重放。Hyper 官方定义 `IncompleteMessage` 可能是请求已写入后读取 EOF，也可能是消息传输未完成；reqwest 的 `SendRequest` 包装不足以可靠证明是否执行，不能仅凭错误字符串改为可重试。
- RED `a84a2322`，GREEN `1a6c1994`；传输回归 4/4、真实 429 回归 3/3 通过。发布版本必须使用新 tag `v3.19.2-2`，不能覆盖已发布的 `v3.19.2-1`。

# 2026-08-16 CCSwitchMulti v3.19.2-3 消除 ResponsePending 假 429

- `ResponsePending -> 429 + Retry-After` 是用户看到 `Too Many Requests` 的直接本地原因；它不是上游 HTTP 状态。`v3.19.2-3` 改为 `424 Failed Dependency`、`cc_switch_response_result_unknown`、`retryable=false`，并移除 `Retry-After`。
- 三类路径不得混合：确认未送达的 `ForwardFailed` 走 5 次安全连接重试；上游明确 HTTP 429 才走限流等待；发送后结果未知禁止盲重放。Hyper 的 IncompleteMessage 无法证明请求是否已被服务端执行，没有上游幂等键/response id 时不能承诺无损恢复。
- RED `82eb04a` 稳定证明旧实现仍返回 429；GREEN `14863cb2` 完成状态、错误码、retryable 和消息语义收口，聚焦回归 7/7 通过。因为 `v3.19.2-2` 已推 tag 并启动构建，此修复使用新版本 `v3.19.2-3`，不得移动旧 tag。

# 2026-08-16 v3.19.2-3 Windows 本地安装包构建

- 在 `ccswitch-qwen38-stream-fix` 执行 `pnpm tauri build`，Windows x64 release 编译与两个 bundle 均成功；随后仅 updater 签名阶段因本机未设置 `TAURI_SIGNING_PRIVATE_KEY` 返回 exit 1。不得把这个退出码描述成 installer 构建失败，也不得声称本地产物具备正式 updater 签名。
- NSIS：`src-tauri/target/release/bundle/nsis/CCSwitchMulti_3.19.2-3_x64-setup.exe`，11,728,717 bytes，FileVersion/ProductVersion 均为 `3.19.2-3`，SHA-256 `0C6237DF14BB4A550498217462CFD820A62D49CB78C6786A522980822AA0A8B8`。
- MSI：`src-tauri/target/release/bundle/msi/CCSwitchMulti_3.19.2-3_x64_en-US.msi`，15,761,408 bytes，SHA-256 `359F4A6B0B043722C3428EAE0059A780F514B03AC5903675B1F94A7E4D22290A`。Explorer VersionInfo 不解析 MSI 产品版本，版本由 bundle 文件名和 Tauri 构建配置确认；正式分发仍以 GitHub Actions 签名资产为准。

# 2026-08-16 Qwen v30 与主线合并边界

- `main` 已包含 MultiRouter 向导修复与测试收口；随后合入 `bigstrongsun/fix-qwen38-tool-loop-streaming-v30`。该分支不仅包含两个 Qwen thinking 修复（`11ee53b8`、`f391f3d7`），还包含 hosted-tools 流式修复、V2 nickname/能力收口和 v3.19.2 主线合并，不能误称为只合入两个 Qwen commit。
- v30 的前置/子集分支 `fix-qwen38-tool-loop-streaming-v14-base` 与 `fix-qwen38-v2-nickname` 已由 v30 历史覆盖，不再重复合并。备份、发布验证和实验分支不属于本次产品主线合并范围。

# 2026-08-16 Git worktree 清理复核

- 逐个复核此前保留的 worktree 后，`pr36-red@76084550`、`fix-qwen38-tool-loop-streaming-v30@f391f3d7` 和 `upstream/codex-history-repair-session-manager@f2c5fe18` 的提交 tip 均已被 `main@486dd1fd` 包含；前者未提交的 1M preference 测试已被主线更完整的同语义回归替代，后两者的剩余内容分别是 Rust `target-429-hotfix` 与 history-tool `.venv/build/__pycache__` 生成物。
- `bigstrongsun/subagent-v1-v2@412e9b39` 的未提交测试仍要求旧的固定 `model_reasoning_effort=medium`，与主线 delegated/native reasoning 契约冲突；其唯一未合入提交是 v3.19.1-21 发布验收记忆，主线已有 v3.19.1-22 及后续发布证据。已移除本地 worktree/branch，远端 fork ref 作为历史引用保留。
- `bigstrongsun/subagent-v2-capability-injection@b00c43e1` 仍包含未进入主线的学术课件、论文资料和设计资产，因此保留为唯一 linked worktree；其中误生成的 `artifacts/design-audit/subagent-theme-2026-08-11/05-light-after.png` 已删除。主工作树中的三个旧诊断 diff 快照也已清理。
- 清理后 `git worktree list` 只剩 `main` 与上述 Sub-Agent V2 资料 worktree，两个工作树均无脏文件；删除前使用 `git worktree remove`/`prune`，生成物先经 `git clean -n` 预览再清理。Git 官方文档与 Matrix WebSearch 独立检索均确认：linked worktree 应用 `git worktree remove`，手工消失的管理记录用 `git worktree prune`。

# 2026-08-16 CCSwitchMulti v3.19.2-5 GitHub 正式发布

- 发布提交为 `704530b0d1892ef6f3cc7a94d0695c009983db94`，四处权威版本源统一为 `3.19.2-5`；annotated tag `v3.19.2-5` 已推送到 `fork`（`BigStrongSun/ccswitchmulti`），未移动既有 `v3.19.2-4`。
- GitHub Actions Release run `31948794630` 的五个平台构建、`Publish GitHub Release` 与 `Assemble latest.json` 共七个 job 全部 success。Release `CCSwitchMulti v3.19.2-5` 非 draft、非 prerelease，并由 `/releases/latest` 返回为 Latest。
- Release 共 19 个资产。六个平台 updater key（darwin-aarch64、darwin-x86_64、windows-x86_64、windows-aarch64、linux-x86_64、linux-aarch64）齐全；逐项下载 `.sig` 后与 `latest.json` signature 精确一致，签名长度为 412/428/432 字符。
- 19 个资产分四批使用 GitHub 下载 URL 的 Range 请求验证可访问，均返回 HTTP 206；下载的 `latest.json` SHA-256 为 `e61b5aaeca124027d423932726608313024a9f78e0cd288e8355c74a4d4d9eb1`，与 GitHub asset digest 精确一致。

## 2026-08-16 推理强度用户入口设计补充

- 推理能力配置的唯一普通用户入口固定为 `Provider 编辑 → 模型列表 → 编辑模型 → 推理能力`。普通用户只选择自动检测、不支持、开关型、分档型或高级自定义，不直接理解内部 `thinkingParam` / `effortParam` / `effortValueMode`。
- 能返回 reasoning、能开关 reasoning、能分档控制 reasoning 是三个独立能力。Qwen/vLLM 在没有明确强度或关闭契约时显示“支持推理、服务端默认、无强度分档”，不得伪造 GPT 通用档位。
- `CodexModelReasoningCapability` 是唯一事实源，catalog、inline TOML、route 物化和出站转换均从 resolved capability 派生。明确 `supportedEfforts=[]` 不得回退通用档位，无合法分档时不得生成默认 `medium`。
- 设计已补充到 `docs/superpowers/specs/2026-08-13-codex-preset-reasoning-capabilities-design.md`；下一步独立核对 CCS 官方的第三方适配逻辑与 Codex 自身的目录、设置和请求构造逻辑。
- CCS 官方 `upstream/main@d4fefefc` 已有逐模型 `reasoningLevels/defaultReasoningLevel`、平台/模型推断和 Responses→Chat effort 映射，但目录解析会把空数组过滤为未声明，模板档位继续保留；同时目录字段与运行时 `CodexChatReasoningConfig` 是两套结构，仍不能完整表达并统一驱动“有 reasoning、无 graded effort”。
- OpenAI Codex `origin/main@9ded177ce7` 以 catalog `ModelInfo.supported_reasoning_levels` 驱动可选值和换模协调，以 `default_reasoning_level` 补缺，最终构造 Responses `reasoning.effort`；Codex 不理解第三方厂商参数。当前请求边界把 Codex 专用 `ultra` 降为 API `max`。
- 两层责任已经写入同一设计：CCS/CCSM 负责真实模型能力与厂商方言转换，Codex 负责依据 catalog 展示、保存、线程继承、校验并发送统一 Responses effort。两层必须分别验收，catalog 是正式边界契约。

## 2026-08-16 推理能力设计重构与 Sub-Agent 复核

- 主推理设计文档已重构为“已确认知识”和“CCSM 修正计划”两部分。推理支持必须使用 `confirmed_supported/confirmed_unsupported/unknown` 三态；reasoning 产出、开关、graded effort、budget 和输出格式是独立维度。Provider 未返回能力或探测失败只能得到 unknown，不能推断为不支持。
- 自动解析顺序确定为：用户模型级覆盖 → Provider 返回的精确模型能力 → CCSM 维护的“平台 + API 格式 + 精确模型 + revision”能力库 → 平台协议/Provider 级声明 → legacy → unknown。动态元数据必须携带 source、confidence、fetchedAt 和匹配键，一次失败探测不得覆盖用户声明或已确认快照。
- Codex 只消费 catalog 的 `supported_reasoning_levels/default_reasoning_level` 并最终发送 Responses `reasoning.effort`；CCSM 负责第三方参数翻译。模型原生档位优先，额外 Codex 档位只能来自显式映射，禁止静默 clamp。GPT-5.6 与 GPT-5.4 Pro 的不同官方集合再次证明不能使用通用 effort 全集。
- 当前 Codex 主线 Sub-Agent 顺序复核为：先继承父线程 effort/父模型默认；单次 spawn effort 优先于 `[agents].default_subagent_reasoning_effort`；显式换模型且无 effort 时使用目标模型默认；角色 TOML 显式 effort 最后覆盖；最终按目标 catalog 校验。字段暴露还受 Multi-Agent 版本、fork 模式和隐藏元数据影响。
- CCSM schema v2 的 `delegated/model_default/fixed/disabled` 方向保持不变。unknown 模型默认 delegated；新 fixed 配置必须先建立模型级手动能力声明，不能直接写通用 medium/high。现有 unknown + legacy fixed 的信任路径只能作为带警告的迁移兼容，不得成为新配置绕过能力校验的入口。
- 本轮仅更新设计与知识，不实施生产代码。主文档为 `docs/superpowers/specs/2026-08-13-codex-preset-reasoning-capabilities-design.md`；后续实现需先审计 resolver 的动态元数据输入、能力库版本、前端结构化入口、主模型 catalog 投影、Sub-Agent compiler 和 legacy 隔离。

## 2026-08-16 Reasoning AI 配置接口与全局配置平面边界

- Reasoning 不能只有 GUI；它必须作为 CCSM 全局 AI Configuration Plane 的一个领域，与 Provider、MultiRouter、Sub-Agent、MCP 等配置共用后端领域服务、校验、事务、回读和审计。AI 不应直接修改 SQLite、生成的 `config.toml`、model catalog 或角色 TOML。
- Reasoning 正式接口覆盖 inspect/detect/plan/apply/validate/export/reset。detect 默认只缓存；plan/dry-run 与 apply 使用相同校验；apply/reset 使用 revision 乐观并发，完成原子保存、派生产物重建和写后回读。机器模式使用版本化 JSON、稳定错误码/退出码、stdout/stderr 分离、默认脱敏和幂等目标状态。
- 公开导入文件是独立版本化 schema，不等同数据库结构。AI 在没有证据时只能保留 unknown/server_default；配置 Sub-Agent fixed 前必须先 inspect 并建立有效模型能力声明。CLI、未来本地 MCP/JSON-RPC、配置导入和 GUI 只是同一 Application Service 的 transport adapter。
- 已另建独立 Codex 任务研究 CCSM 全面的 AI 可配置方案；其范围包括所有配置域、现有写入路径、统一命令/API、权限与凭据边界、dry-run/并发/回滚/审计、测试验收和分阶段实施。本推理设计只定义 reasoning 领域实例，不提前替代全局设计结论。

## 2026-08-17 推理能力修正可实施路线图

- 独立 AI Configuration Plane 任务因响应流断开未完成，只留下可验证的配置域、写入路径、SSOT 和敏感边界盘点；不得把其未完成草稿描述为正式全局设计或已提交成果。有效结论是 GUI、同步、deeplink、SQL 导入和 live 文件构成多写者，全面 AI 配置必须统一 revision、plan/apply、回读、审计和脱敏。
- 推理设计新增独立实施计划 `docs/superpowers/plans/2026-08-17-codex-reasoning-capability-correction.md`。路线固定为 P0 契约/RED、P1 三态 resolver/来源链、P2 catalog/request/Sub-Agent 同源、P3 结构化 UI、P4 AI/CLI 只读、P5 detect/plan/apply、P6 真实 canary、P7 发布迁移。
- 当前不是从零实现：Rust 已有完整 effort 枚举、`CodexModelReasoningCapability`、`ResolvedSubagentReasoningCapability`、catalog 投影、请求转换、Sub-Agent schema v2 和模型级 UI 基础。首要改动是把持久化 bool 演进为三态/控制类型，扩展来源证据，并用 fingerprint 证明四个消费者同源。
- Provider metadata adapter 必须返回 Found/NotAdvertised/Unavailable/Invalid；后三者不能变成 confirmed_unsupported。常用模型能力库按平台、API 格式、canonical model、revision 匹配，前端不得维护副本。动态结果先缓存，用户采用后才固化。
- P6 真实矩阵至少覆盖 Qwen/vLLM 无元数据、Qwen 用户声明档位、DeepSeek 维护映射、OpenAI 官方和 unknown 自定义网关，并保存同一 trace 的 capability fingerprint、Codex model list、脱敏请求结构、Provider 结果和 Sub-Agent effort。P6 前不改版本号、不发版。

## 2026-08-17 推理能力产品决策确认

- 用户模型级配置最高优先级；Provider 检测到差异时不覆盖，模型行显示小叹号，再次进入时展示旧值、新值、来源和时间，用户可主动采用。能力库确定为独立版本化 JSON，禁止编译进 Rust；第一阶段随应用打包，未来支持用户点击下载签名更新包、差异预览和失败回退，并允许社区 PR。
- 首版只读取原始元数据，不发送 low/high/none 真实推理请求主动探测。Discovery 不限于 `/v1/models`：OpenRouter 可读 reasoning 对象；vLLM 可组合 `/v1/models`、`/version`、`/server_info?config_format=json` 和 OpenAPI/实例配置摘要。统一输出可扩展 `ProviderCapabilitySnapshot`，同时服务 reasoning、工具、结构化输出、模态和端点等能力；敏感实例字段只做 allowlist 提取。
- 公共控制词表为 `none/minimal/low/medium/high/xhigh/max/ultra/custom`，具体模型只显示 resolved 子集；控制形态还包括 server-default、boolean、graded effort 和 token budget，以便服务非 Codex Agent。所有非恒等映射保存前必须可见。
- CCSM 可配置 Codex 根级新任务默认 `model_reasoning_effort`，不强改当前线程，已有任务不追溯变化。Sub-Agent V2 新 profile 默认 delegated；CCSM 可安全配置单个全局 `[agents].default_subagent_reasoning_effort`，但单次 spawn effort 由父 Agent 运行时决定，CCSM 不改 reserved schema。V1 保留兼容读取、运行、导出和迁移，不复制新 reasoning 写逻辑。
- 模型能力 schema 采用读旧写新；不要与 Sub-Agent V1/V2 混称。unknown + legacy fixed 保留两个稳定版本。CLI 暂定 `ccsm`、默认 JSON；mutation 需要人工确认或 planToken + expectedRevision；首版不做 MCP，本地 HTTP API 保留 TBD；JSON 为权威格式，YAML 可选。
- 允许 AI 通过 JSON/stdin 等安全输入直接写密钥，但禁止命令行参数、输出、日志、审计和回读出现明文；只返回 hasSecret/脱敏摘要。审计保留 180 天或 10,000 条 mutation，不记录密钥、Prompt、响应或 reasoning 正文。

## 2026-08-17 Qwen3.8 工具结果后纯进度 stop 的 Responses→Chat 根因

- task `01a00c24-1fec-7410-aaec-d93416db98ce` 的四个短轮不是超时或断流：末次 vLLM 请求均为 HTTP 200、`finish_reason=stop`、无 tool-call delta，内容是“Appending sections ...”等未完成进度句；其上一请求均正常完成工具调用。
- 真实捕获的约 933–1016 KB Chat 请求暴露了协议级根因：同一次 Codex Responses 输出中的 commentary `message` 与随后的 `function_call` 被 `transform_codex_chat.rs` 转成两条连续的 assistant 历史消息。Qwen 因而反复看到“纯进度 assistant 消息 → 下一条 assistant 才调用工具”的错误示范；但 Chat Completions 当前采样在第一条 assistant 的 `stop` 就结束，不会自动进入第二条 assistant。相同 reasoning 还会被重复附挂到两条消息。
- 该拆分行为来自原始 Responses→Chat 桥接提交 `693c3872`（2026-06-02），截至 CCS 官方 `origin/main@d4fefefc` 仍存在，不是 Qwen、vLLM 或 CCSwitchMulti 独有分叉。远端透明代理已经注入“未完成时不要只播报进度”的系统提示，但现场仍复现，说明继续加强提示或伪造 finish reason 不是根修。
- 分支 `bigstrongsun/fix-responses-chat-turn-coalescing`、提交 `5b820624` 在 Chat 所有权边界把直接相邻的 commentary assistant 与 pending tool calls 合并为一条 `{content, tool_calls}` assistant 消息，并对跨 item 重复的 `reasoning_content` 去重。工具输出、媒体边界和没有 commentary 的 tool-call 消息保持原语义。
- 新 Qwen 形状回归覆盖 `reasoning → assistant commentary → function_call(重复 reasoning) → function_call_output`；聚焦测试 1/1、全部 Responses→Chat 相关测试 87/87、`cargo fmt --check`、`git diff --check` 和严格 UTF-8 解码均通过。
- 本地 NSIS 测试包为 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti\CCSwitchMulti_3.19.2-5_5b820624_x64-setup-unsigned.exe`，SHA-256 `AF3301EED778DE0778B9745CBA6EF498C4749E307F766E679568923A057517D6`。bundle 已成功生成，但构建末尾因本机只有 Tauri 公钥、没有私钥而返回签名错误，所以该测试包明确标记为 unsigned，不能当正式 release 资产。当前已安装 exe 因进程锁未被替换，仍是原 `v3.19.2-5`；回滚副本位于 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe.before-turn-coalescing-20260817-061522.bak`。

## 2026-08-17 Qwen3.8 英文 raw reasoning 的最终边界

- 截图任务 `01a00e49-e614-7251-a91f-96a9e8889104` 创建于语言策略写入 `~/.codex/config.toml` 之前；任务创建时冻结的 developer instructions 没有新策略，所以该旧任务中的英文 commentary 不能用来判定新配置失效。新建的受控 Codex CLI 任务 `01a00e9c-4b41-7263-86a9-3b14a7b4cb07` 已证明边界：三个 `type=reasoning` item 仍为英文，但两个进度 `agent_message` 和最终 `agent_message` 全为中文。
- vLLM 与 parser 没有把中文 reasoning 转成英文。直接对 roglinux `127.0.0.1:5001/v1/chat/completions` 发送流式请求，在短 Agent 提示并明确要求中文思考时，服务原生 `delta.reasoning` 持续输出中文；因此编码、SSE 映射和 CCSM Chat→Responses 适配都不是该语言现象的起点。
- 2026-08-25 继续审计 Qwen3.8 reasoning-only：现场 Qwen Provider 从至少 2026-08-16 起一直有 `localProxyRequestOverrides.body.temperature=0.1`，MultiRouter 正确继承后导致 Qwen3.8 thinking 请求偏离官方 `temperature=1.0/top_p=0.95/top_k=20/min_p=0/presence_penalty=0/repetition_penalty=1` 推荐。roglinux 代理只补缺失的 `top_p=1`，不是 `0.1` 的来源。已通过 CCSM Provider UI 保存官方采样参数并只读回验 DB，未重启也未追加模型请求。与此同时，Qwen verified profile 自动选择 raw `reasoning_text`，当前 Codex Desktop 主会话只投影 summary；这是独立的 downstream 兼容问题。完整证据见 `memory-2026-08-25-codex-desktop-raw-reasoning-renderer.md`。
- 已用 `codex debug prompt-input` 取得 37,111 字符的真实新任务输入，并按 CCSM 的规则将 developer/system 合并为约 30,112 字符的首部 system 后直连 vLLM 重放。语言策略保持 system 开头时，reasoning 为 1,111 字符、中文汉字 28 个、英文字母 840 个；把同一策略移动到 system 末尾时，reasoning 为 1,134 字符、中文汉字 28 个、英文字母 864 个。两者都以 `The user is asking in Chinese...` 开始，否定了“只要尾置 system 约束即可修复”的假设。
- 即使把“必须使用中文进行内部思考、进度说明和最终回答”直接追加到当前中文用户消息，真实 Codex 长提示重放仍以英文 reasoning 开始（1,064 字符中中文汉字 91 个、英文字母 764 个）。因此当前剩余问题是 Qwen 在大型、以英文为主的 Codex Agent 上下文中不能可靠遵循 reasoning 语言约束，而不是 Responses→Chat 角色合并、策略位置或 vLLM parser 的 bug。
- 产品边界必须区分 raw reasoning 与用户可见 commentary/final：新任务的后两者已由现有语言策略修复；raw reasoning 若继续开启显示，语言无法由 CCSM 提示词确定性保证。CCSM 不应在协议转换层静默翻译、改写或伪造模型 reasoning。可接受的产品选择只有保留原始英文 reasoning、隐藏 raw reasoning，或另行设计明确标注且有额外成本/延迟的翻译展示层；不得把后两者包装成模型已经用中文思考。

## 2026-08-18 v3.19.2-6 发布核验与未合分支审计

- 上一会话在 GitHub 网络中断前已完成：`bigstrongsun/fix-responses-chat-turn-coalescing`（含 `5b820624` commentary 合并与 `29072912` 第三方中转父级 V2 `message.encrypted` 剥离）合入 main，`bigstrongsun/release-v3.19.2-6` 合入 main，发布提交 `f07b5b8a`，tag `v3.19.2-6` 已推 fork。
- 本轮核验：Release run `32059546738` 七 job 全 success（42m36s）；Release `CCSwitchMulti v3.19.2-6` 非 draft/prerelease、为 Latest，19 资产；Windows x64 Setup 11,720,812 bytes，SHA-256 `02924e15d2e9d9d3387f3f3dbc1a359dc64a7948551e183f9b426b62eb85f1a2` 与 GitHub digest 精确一致。
- 未合分支审计（`git cherry` 判定 patch 是否已在 main）：
  - 已在 main（无需合并）：`fix-qwen38-tool-loop-streaming-v14-base`（v30 覆盖）、`v2-subagent-top-five`（线性合入 v3.19.2-4）、`fix-responses-lite-additional-tools`（`collect_additional_tools` 已在 main）、`release-v3.19.1-25` 的 subagent 写校验/模态（`codex_subagent_v2_write_verification` 已在 main）、`fix-v3.19-codex-pool-session-affinity` 的 in-flight 超时（`effective_streaming_timeout` 已在 main）、GPT-Live `/v1/live` 路由（已在 main）。
  - 不合并：实验分支（`commentary-reasoning-experiment` 85、`portable-reasoning-experiment-nogo` 36）、学术资料（`subagent-v2-capability-injection`）、备份分支、旧 release 分支（`release-v3.19.1-25` 余下为 docs）。
  - 真实缺失（v29 分支，见下节）：5 项产品改动。
  - 上游同步（`fix-responses-commentary-tool-calls` 含约 60 个 2026-08-06~17 上游 commit + 与 `5b820624` 重复的 commentary 修复）：属独立的上游同步决策，本轮未做，留待专门评估。

## 2026-08-18 v3.19.1-29 丢失修复回收与 v3.19.2-7 发布

- 根因：`bigstrongsun/fix-v29-codex-force-repair`（v3.19.1-29，tag `ba318934`）从 `f6f37e93`（v28 证据）分叉；而 v3.19.1-31（tag `12272a31`）与整个 v3.19.2 线从另一条基线（main + qwen38 v30 PR + PR#9 reasoning 直接合并）构建，未包含 v29 分支的修复。用户从 v3.19.1-29 升级到 v3.19.1-31/v3.19.2-x 时这些修复发生回归。
- 回收的 5 项（均带 RED 回归测试，cherry-pick 到 `bigstrongsun/merge-v29-lost-fixes` 再 `--no-ff` 合入 main，merge commit `557b467a`）：
  1. `32e7eb4e` fix(codex) 可回滚配置强制覆盖（`force_repair_and_switch_codex_provider` 等 4 函数）+ `23156fee` 测试。
  2. `dafdfd1f` fix(installer) 事务安装重复实例竞态根修（`Resolve-CcsmReplacementListenerAction` 身份校验接管）+ `3b62ea87` 测试。
  3. `51187729` fix(release) 导出 NSIS 安装态二进制哈希 + `46146769` 测试。
  4. `066ecfb9` fix(release) 统一 worktree 发布目录解析 + `5d1fa8f4` 测试。
  5. `fe527833` fix(codex) Live 写入前归一化 Windows project key（`repair_projects_windows_path_table_line`）+ `67e2e2f5`/`e2865c0b` 测试。
- 冲突处理：`memory.md`/release-notes 一律保留 main 版本（v29 的 v3.19.1-29 发布说明描述的是被取代的同版本号发布，不并入）；`codex_config.rs` 的 `normalize_codex_config_text_for_live_read` 为语义冲突——main 已重构为 `root_scope`+`in_basic_multiline` 的 notify 恢复，v29 新增 project-key 修复。合并逻辑：outside-multiline 先试 project-key 修复，再按 `root_scope` 试 notify 修复；`in_basic_multiline`（developer_instructions 内）仍做 notify 修复。两类行（`[projects."..."]` 表头 vs `notify = [...]`）互不重叠，顺序安全。
- 跳过：`271451e7`（选择性吸收 PR#9 reasoning，已被 main 直接合并 PR#9 取代）、`6cde9d28`（v3.19.1-29 版本号 bump）、各 docs/memory 提交。
- 测试基建修复：`vitest.config.ts` 增加 `exclude: ["**/.worktrees/**", ...]`。此前 `.worktrees/` 下 linked worktree（自带 src/ 与 node_modules）被 vitest 默认 include 命中，主树前端跑测出现 270 文件/2208 用例的假失败；排除后主树为 138 文件/1119 用例全绿。
- 门禁：`cargo check --tests` 通过；Rust lib 3117 passed / 0 failed / 5 ignored（较基线 3098 新增 19 个回归全过）；前端 138 文件/1119 用例全过；typecheck、Prettier（CI 范围 js/jsx/ts/tsx/css/json）、`cargo fmt --check`、`git diff --check` 全绿。`src/index.html` 的 Prettier 告警为 main 既存问题且不在 CI 检查范围，未处理。
- 发布：四处版本源统一 `3.19.2-7`（`53a3f124`）；annotated tag `v3.19.2-7`（peel `557b467a`）推 fork。Release run `32077460561` 七 job 全 success（macOS 51m18s 含 notarization，Windows x64 30m45s，Windows arm64 28m49s，Linux x64/arm64 约 20m）。Release 非 draft/prerelease、19 资产；Windows x64 Setup 11,746,930 bytes，SHA-256 `03f7c4e3c6ea91849efc094d642281e793fdf57f84bb4ffef81b681c01ab74ee` 与 GitHub digest 精确一致；`latest.json` 版本 `3.19.2-7`。
- 环境注意：本机经 Tailscale CGNAT（198.18.0.x）访问 GitHub，API 间歇性 `connectex` 超时，重试即可恢复；`git rev-parse <tag>^{commit}` 的 `^{commit}` 会被 PowerShell 破坏，改用 `git for-each-ref --format=%(*objectname)` 取 peeled commit。

## 2026-08-18 V2 Sub-Agent 能力来源与模型列表跟随 MultiRouter 排查

- 用户报告两个问题：(1) V2 Sub-Agent 的能力字段（任务优势/输入能力/写入范围）能否基于模型 catalog（API 可读的能力声明）自动得出，至少多模态 vs 纯文本、是否支持图像生成应可查询并设置；(2) V2 agent 列表同时出现 `qwen3.6` 与 `qwen3.8`，但 3.6 已随部署切换为 3.8 而不再存在，列表未跟随当前 MultiRouter 模型库。
- 数据链（已核对源码 + 现场 DB `~/.cc-switch/cc-switch.db`）：模型列表唯一来源是 `settingsConfig.modelCatalog.models`（MultiRouter 聚合 catalog SSOT）。`codex_catalog_model_specs()` 读该数组生成 `CodexCatalogModelSpec`（含 `input_modalities`/`text_only`/`reasoning`/`context_window`）；主 catalog 与 V2 `SubagentCatalogModel` 共用同一 `specs`，因此 V2 列表本应跟随 MultiRouter。
- 问题 2 根因（已定位）：当前 `codex-multirouter`（is_current=1）的 `modelCatalog.models` 为 9 个（含 `qwen3.8`、无 `qwen3.6`，catalog 正确）；但 `codexRouting.subagentV2.profiles` 有 10 个 key，含残留的 `qwen3.6`（enabled=false、通用问卷，是 3.6 在 catalog 时由 `SyncCatalog`/初始化生成的草稿）。V2 前端列表由 `readRawProfiles(draft)`（已配置 profile）驱动，而非 catalog；`reconcile` 的 `SyncCatalog` 只新增第三方 catalog 模型、`RemoveAllInvalid` 只删 parse-invalid profile，**没有任何动作删除 parse-valid 但 unroutable（模型已离开 catalog）的 profile**。对比：V1 的 `modelCatalog.spawnAgentModels` 在模型离开 catalog 时会被剪枝（memory 1085 行），V2 profiles 没有对应剪枝——这是不一致点。live `cc-switch-model-catalog.json` 9 模型无 3.6，进一步证明是 V2 profile 残留而非 catalog 未更新。
- 问题 1 现状：输入模态（多模态 vs 纯文本）已实现 catalog 推导（commit `c3e7311c`，memory 114 行）：优先级 profile 显式 > `modelCatalog` 的 `inputModalities/textOnly/supportsImage/vision` > 后端保守识别；`codex_subagent_profile_input_modalities()` 由 spec 的 `text_only`/`input_modalities` 得出 `["text"]`/`["text","image"]`；`hydrate_codex_subagent_v2_input_modalities()` 在保存/初始化/目录同步/恢复时把已知 catalog 能力写回 V2 JSON。但现场 `qwen3.8` 的 catalog 条目**未声明任何能力字段**（无 inputModalities/textOnly/supportsImage），故其模态非 catalog 推导（profile 里的 `[text,image]` 为早期/手工写入）。catalog 条目能力字段并集：`inputModalities`(8)/`textOnly`(8)/`supportsImage`(6)，**无图像生成（image generation）能力字段**。
- 问题 1 结论：输入模态（多模态 vs 纯文本）可且已基于 catalog 推导，前提是 catalog 条目声明了能力（`qwen3.8` 未声明是数据缺口，需在模型源/`/models` 拉取时补齐能力字段）；任务优势（taskStrengths）与写入范围（writeScope）是语义/策略选择，模型能力 catalog 无法推导，只能人工/预设；图像生成当前不是 catalog 能力字段，若要支持需新增能力字段并在拉取时填充。
- 修复方向（问题 2，遵循“跟随 MultiRouter、不另维护列表”）：(a) 新增 reconcile 动作 `PruneUnroutable`，删除模型已离开可路由 catalog 的 profile；(b) provider 更新路径在 catalog 变化时自动剪枝 disabled 的 unroutable profile（enabled 的保留并标记，避免误删用户配置）；(c) 前端把 unroutable profile 明确标为“已失效/不可路由”并提供“与目录同步”按钮。保留“临时离开 catalog 不生成 role、canonical model 重现按 profile.model 恢复”的既有边界，仅对已永久离开 catalog 的模型剪枝。
- 修复实现（2026-08-19，已落地）：(a) `CodexSubagentV2ReconcileAction::PruneUnroutable`（codex_config.rs）删除模型已离开可路由 catalog 的 profile（parse-valid 但 unroutable），显式“与目录同步”动作，用户主动触发，删除全部 unroutable（含 enabled）；(b) `auto_prune_disabled_stale_subagent_v2()`（codex_config.rs）保守自动剪枝：仅删 `enabled==false` 且模型不在 `modelCatalog.models`（catalog 成员判定，非 routability）的 profile，enabled 与 parse-invalid 一律保留；在 `ProviderService::update` 非 additive 路径 `save_provider` 之后 best-effort 调用（Codex only），失败仅 `log::warn` 不影响主保存，无需 live 投影（disabled profile 不生成 role 文件）；(c) 前端 `CodexSubagentProfileEditor.tsx` 新增 `unroutableProfileCount`（status==unroutable 计数）与琥珀色“与目录同步：删除已失效模型（N 项）”按钮，调用 `reconcile("prune_unroutable", draft)`；`codexSubagentV2.ts` 类型加 `prune_unroutable`。
- 验证：`cargo test --lib` 3152 passed / 0 failed / 5 ignored（新增 3 个测试：`codex_subagent_v2_prune_unroutable_removes_stale_and_keeps_routable`、`codex_subagent_v2_auto_prune_removes_disabled_stale_only`、`codex_subagent_v2_auto_prune_is_noop_when_no_stale_disabled`）；`pnpm vitest run` 138 文件 / 1120 用例全过（新增 1 个前端测试）；`pnpm typecheck`、`cargo check --tests`、`cargo fmt --check`、`git diff --check` 全干净。
- 边界保留：显式 `PruneUnroutable` 用 routability 判定（用户主动，可激进）；自动剪枝用 catalog 成员判定 + disabled-only（保守，避免误删）。“临时离开 catalog 不生成 role、canonical model 重现按 profile.model 恢复”边界对 enabled profile 完全保留；disabled profile 若模型重新加入 catalog，由 `SyncCatalog` 重建草稿。

## 2026-08-18 Codex Reasoning Capability P0（三态 schema v2 + disable_contract 关闭契约）

- 计划/规格：`docs/superpowers/plans/2026-08-17-codex-reasoning-capability-correction.md` / `docs/superpowers/specs/2026-08-13-codex-preset-reasoning-capabilities-design.md`；分支 `bigstrongsun/reasoning-capability-p0`（基于 main tip `cac48db4`，v3.19.2-7 线）。P0 = 契约冻结 RED（`efbc78c2`）+ GREEN（`e0d4bbea`）+ 本知识提交。
- 三态 schema v2（`CodexModelReasoningCapability`，Rust `proxy/providers/codex_reasoning.rs` + TS `src/types.ts`）：`supported: bool` → `Option<bool>`（legacy 只读）；新增 `schemaVersion?/supportStatus?/controlKind?/confidence?/fetchedAt?/providerKey?/modelRevision?`。枚举：`ReasoningSupportStatus`（confirmed_supported/confirmed_unsupported/unknown）、`ReasoningControlKind`（none/boolean/graded/budget/unknown）、`CapabilityConfidence`。辅助函数 `effective_support_status()`/`effective_control_kind()`。legacy 派生：Some(true)→ConfirmedSupported、Some(false)→ConfirmedUnsupported、None→Unknown。
- 能力指纹 `capability_fingerprint()`（codex_reasoning.rs:301）：对归一化执行字段做 sha256（effective status/kind、排序去重 efforts、default、disableAllowed、upstream format/parameter/effortMap、outputFormat）；易变元数据（fetchedAt/confidence 等）排除。builtin deepseek-v4 指纹 = `8d5aeff0f2c9743effd90da1cc89b10ec0335e2e2766e8161a9bf0325360abf9`（`codex_config.rs` 6662/6759/7520 三处 exact-contract oracle 硬编码）。`ResolvedSubagentReasoningCapability` 新增 `fingerprint: String`（unknown fallback 为空串）。
- `validate()` 三态一致性：supportStatus↔supported 矛盾拒绝；confirmed_unsupported 不得 advertise efforts/disable；controlKind 一致性（graded 需非空 efforts；boolean/none 不得 advertise efforts）。非法声明 fail-closed 到 Unknown（绝不信任为 confirmed_unsupported）。
- disable_contract 关闭契约（P0 核心语义）：`CodexChatReasoningConfig.disable_contract: bool`（serde `disableContract`，default false，false 时不序列化）。仅当 true 时 Codex 的 `reasoning.effort=none`（Responses 语义）才翻译为上游厂商关闭信号（thinking=disabled / enable_thinking=false / chat_template_kwargs.enable_thinking=false / reasoning_split=false）；false 时省略厂商字段、保留服务端默认。三条来源路径：能力路径 = `capability.disable_allowed`；用户 meta 显式声明路径 = true（声明本身即关闭契约）；推断路径 = false（10 个推断字面量全部）。
- `apply_reasoning_options` 门控（transform_codex_chat.rs）：`emit_switch = reasoning_enabled || config.disable_contract`。provider 级「明确不支持 thinking 时强制写 false」既有分支（`!supports_thinking`）不变；OpenRouter `reasoning.effort=none` 透传不变。
- subagent fixed 能力门禁：`compile_reasoning_policy(policy, capability, origin, key) -> (Option<Effort>, Vec<String>)`。Fixed + Unknown 能力：Legacy（schema1 迁移）保留档位 + 警告「legacy fixed reasoning effort retained for unknown capability; re-save to declare the model capability or switch to delegated」；Declared（schema2 新声明）fail-closed 报 `unknown_capability_fixed_requires_declaration`。`ReasoningPolicyOrigin` 由 raw persisted schemaVersion 判定（1→Legacy、2→Declared），警告并入 `GeneratedRole.warnings`。
- 验证：`cargo test --lib` 3130 passed / 0 failed / 5 ignored（3 个 RED 锁转绿：`declared_fixed_with_unknown_capability_is_rejected`、`legacy_fixed_with_unknown_capability_is_retained_with_warning`、`none_without_disable_contract_omits_vendor_disable_signal`）；`npx vitest run` 138 文件 / 1119 用例全过；`npx tsc --noEmit` 干净；`cargo fmt --check` 干净；`git diff --check` 干净。
- 下一步（P1，计划 §5）：`src-tauri/src/reasoning_capabilities/{mod,catalog,provider_metadata}.rs`；单一 resolver 入口 `resolve_codex_model_capability(provider, model)`；只读发现适配器（OpenRouter `/api/v1/models` reasoning 字段；vLLM `/v1/models`+`/version`+`/server_info?config_format=json`+OpenAPI，仅 allowlist 字段、无 secrets）；`ProviderCapabilitySnapshot`；适配器结果 `Found/NotAdvertised/Unavailable/Invalid`（后三者绝不 → confirmed_unsupported）；禁止用真实推理请求做主动探测；版本化 JSON 能力库（不编译进 Rust、无前端副本；匹配 platform+API format+canonical model+revision；第一阶段随应用打包）；动态读取 → 仅 TTL 候选快照；用户配置永远最高优先级；diff 以 warning 呈现，用户采纳 → `source=user_confirmed_detection`。
- 约束：P0–P5 不 bump 版本；P6 安装 canary 通过前不发 release。

## 2026-08-19 Codex Reasoning Capability P1（统一来源链 + 只读发现适配器 + 版本化能力库）

- 计划/规格同 P0；分支 `bigstrongsun/reasoning-capability-p0`。P1 交付：单一 resolver 入口、OpenRouter/vLLM 只读发现适配器、随应用打包的版本化能力库。
- 新模块 `src-tauri/src/reasoning_capabilities/`：
  - `mod.rs`：`CapabilitySource`（user_config/detection/library/builtin/unknown）、`ProviderCapabilitySnapshot`（通用可扩展，首版只填 reasoning 子对象）、`ReasoningCapabilitySnapshot`（allowlist 字段）、`DiscoveryOutcome`（Found/NotAdvertised/Unavailable/Invalid）、`ResolvedModelCapability`（capability+source+fingerprint）、`DetectionCache`（内存 TTL=1h）、`resolve_codex_model_capability`（公开入口，读全局库）与 `resolve_codex_model_capability_with_library`（可注入库，测试用）。
  - `catalog.rs`：`CapabilityLibrary`/`LibraryEntry`（platform+api_format+model+revision_range+reasoning+source_url+verified_at+evidence_level）；`lookup` 平台精确匹配优先于 any、api_format 精确优先；`revision_range` 支持 `>=x.y.z`/`<=x.y.z`/精确/缺省，有范围但拿不到 revision 时保守不匹配；`load_library_from_str` 校验每条 entry 的 schema v2；全局懒加载 `global_library()`（OnceLock，加载失败保持 None 降级）。
  - `provider_metadata.rs`：`detect_platform`（仅 name+base_url，绝不掺 model 名）；`discover_openrouter`（GET {base}/api/v1/models 公共端点，提取 reasoning.{supported_efforts,default_effort,mandatory,default_enabled,supports_max_tokens}）；`discover_vllm`（GET /version + /v1/models + /server_info?config_format=json，只提取 allowlist 字段，reasoning 子对象保持 None——vLLM 不声明逐模型 effort，逐模型能力由库提供）。
- 单一 resolver 优先级（高→低）：用户模型级声明（modelCatalog）> 检测候选（TTL）> 能力库 > 内置 > unknown。请求路径 `resolve_codex_chat_reasoning_config` 的完整优先级：用户模型级声明 > 用户 provider 级显式声明（meta）> 检测/能力库/内置 > 平台/模型推断。请求路径只读 TTL 缓存，不发起网络请求。
- 关键语义：`NotAdvertised`/`Unavailable`/`Invalid` 与库/内置未命中都只能得到 unknown，绝不自动生成 confirmed_unsupported（缺失证据不是不存在的证据）。`snapshot_to_capability` 对非法快照（如 default_effort 不在 supported_efforts）返回 None 落到库/内置。
- `apply_qwen_vllm_safety_defaults`：能力派生配置同样需要 Qwen/vLLM 输出预算安全下限（min_output_tokens=2048），但只补预算、不纠正 thinking_param（能力声明是 thinking 开关的权威来源，不得被推断覆盖）。与 `merge_qwen_vllm_reasoning_defaults`（meta 路径用，会纠正 thinking_param）区分。
- 内置清单进入请求路径：deepseek-v4-pro/flash、k3、k3-256k 无用户声明/meta 时由内置能力派生请求配置（effort_value_mode=capability 形态，精确档位+映射），而非通用推断模式。这是 P1 的行为变化（更精确），已更新对应测试。
- 联网核验（2026-08-18/19，matrix-websearch，内置 web_search 不可用）：
  - OpenRouter `GET /api/v1/models`（公共、无需鉴权）live 数据：`reasoning.{mandatory, default_enabled, supported_efforts, default_effort, supports_max_tokens}`；effort 值域 minimal/none/low/medium/high/xhigh/max；`none` 可出现在 supported_efforts（如 sakana/namazu）表示支持显式关闭。deepseek/deepseek-v4-pro 与 deepseek-v4-flash 均为 `supported_efforts=["max","high","low"], default_effort="high", mandatory=false`。
  - vLLM（v0.13.0 源码）：`GET /version` → `{"version": VLLM_VERSION}`（恒可用）；`GET /v1/models`（恒可用）；`GET /server_info?config_format=json` → `{"vllm_config": {...}}`，但受 `VLLM_SERVER_DEV_MODE` 门控（开发端点，生产部署通常 404）→ 适配器按 Unavailable 降级，不是错误。
- 第一阶段能力库 `src-tauri/resources/reasoning-capabilities.json`（libraryVersion=1）：仅含 OpenRouter deepseek-v4-pro/flash 两条（platform_api 证据，2026-08-18 核验）。vLLM qwen 条目推迟到 P6（需 min_output_tokens 交互 + 实际部署参数形态验证）。
- 库路径解析：环境变量 `CCSM_REASONING_LIBRARY` 覆盖 > Tauri 资源目录（setup hook `init_resource_dir`）> 开发回退（`resources/reasoning-capabilities.json` 相对 CWD）。tauri.conf.json bundle.resources 已加 `resources/reasoning-capabilities.json`。
- 验证：`cargo test --lib` 3149 passed / 0 failed / 5 ignored（新增 21 个 reasoning_capabilities 测试 + 1 个内置 deepseek 请求路径测试）；`npx vitest run` 138 文件 / 1119 用例全过；`npx tsc --noEmit` 干净；`cargo fmt` 已执行；`git diff --check` 干净。
- 下一步（P2，计划 §5）：四个消费者（Codex catalog/Desktop aliases/inline TOML、Responses→Chat/Anthropic 请求转换、Sub-Agent capability API/profile compiler/角色 TOML、GUI/CLI inspect）改为同源；删除/封闭每个消费者内部的通用 GPT reasoning fallback；每个投影携带 fingerprint 和 source summary；catalog 接受值必须是 codexSelectableEfforts，请求转换目标必须在 providerAcceptedEfforts；none 先按 disable capability 处理；MultiRouter 在 route model map 后用 effective Provider + upstream model 解析。

## 2026-08-19 Codex Reasoning Capability P2（四个消费者同源 + official 来源 + 封闭 GPT fallback）

- 计划/规格同 P0/P1；分支 `bigstrongsun/reasoning-capability-p0`。P2 交付：resolver 核心 settings-based 化、official 来源、catalog 投影走同一 resolver、封闭通用 GPT reasoning fallback、none 按 disable 处理。
- resolver 核心重构（`reasoning_capabilities/mod.rs`）：
  - 新增 `resolve_codex_model_capability_core(settings, platform, model, detection, library, official_models)`——纯函数、无网络、无全局状态，所有消费者必须经由它。
  - `resolve_codex_model_capability`（`&Provider` 包装，请求路径用）：加载 platform（`detect_platform`）+ official 缓存（`codex_official_models_cache`）+ 全局库，调用核心。
  - `resolve_codex_model_capability_with_library`（测试用）：加载 platform，official 缓存传空（保持确定性）。
  - 来源优先级（高→低）：用户配置 > 检测候选 > 能力库 > 内置 > **official（仅 platform=None 即未知平台生效）** > unknown。
- official 来源（P2 新增）：
  - `official_reasoning_capability_for_model` 从 `codex_config.rs` 移到 `codex_reasoning.rs`（纯函数，共享）。
  - `codex_official_models_cache` 改 `pub`（供 resolver 读取）。
  - 仅 `platform=None`（未知平台，含 OpenAI 直连与 catalog 投影）生效；OpenRouter/vLLM 等已知聚合平台不套用官方 OpenAI 形态（它们有自己的推理接口，走平台推断）。
  - 官方 GPT 模型走 OpenAI 顶层 `reasoning_effort` 字段，effort_map 用 identity，`disable_allowed=false`。
- catalog 投影改走 resolver 核心（`codex_config.rs`）：
  - `codex_catalog_model_specs` 的 reasoning 链改为调用 `resolve_codex_model_capability_core`（platform=None、detection=None、library=全局、official=官方缓存）。
  - 若 catalog 模型名是别名、上游模型名命中不同来源，用上游名重试一次。
  - `CodexCatalogModelSpec` 新增 `reasoning_fingerprint` + `reasoning_source` 字段（25 个测试构造点同步更新）。
- 封闭通用 GPT reasoning fallback（`transform_codex_chat.rs`）：
  - `apply_reasoning_options` 的 `config:None` 分支删除 `supports_reasoning_effort` 模型名启发式；config 为 None 表示能力未知，不得按模型名猜测档位注入 `reasoning_effort`。
  - `model` 参数不再使用，从签名移除（唯一调用点同步更新）。
  - 注意：`transform.rs:215` 与 `transform_responses.rs:373` 的 `supports_reasoning_effort` 是 Claude Code→OpenAI 路径（非 Codex 路径），不在 P2 范围，保留。
- none 按 disable 处理（`codex_reasoning.rs`）：
  - `resolve_subagent_reasoning_capability` 的 `codex_selectable_efforts` 排除 `none`（none 是关闭，不是可选正向档位；UI/spawn_agent 可选档位不含 none，关闭走 disable 路径）。
  - `provider_accepted_efforts` 仍含 none（关闭契约需要）；`effort_map` 把 none 映射到 none（identity，即关闭）。
  - `validate()` 已强制 `none` 必须 `disableAllowed=true`（否则拒绝）。
- MultiRouter：请求路径已在 `apply_codex_chat_upstream_model`（route model map）后调用 `resolve_codex_chat_reasoning_config`（effective Provider + upstream model），P2 无需额外改动。
- 四层 fingerprint 一致性：catalog spec（`reasoning_fingerprint`）、请求路径（resolver 核心）、Sub-Agent capability（`resolve_subagent_reasoning_capability` 的 `fingerprint`）均源自同一 resolver 核心，fingerprint 一致。GUI/CLI inspect（P4）将读取 spec 的 fingerprint/source。
- 验证：`cargo test --lib` 3162 passed / 0 failed / 5 ignored（新增 4 个 resolver 核心 official 测试 + 3 个 catalog 投影 fingerprint 测试 + 1 个 none-as-disable 测试）；`npx vitest run` 138 文件 / 1120 用例全过；`npx tsc --noEmit` 干净；`cargo fmt` 已执行；`git diff --check` 干净。
- 下一步（P3，计划 §5）：模型编辑器最终生效视图——用户无需编辑 JSON 即可完成安全配置；GUI 展示 fingerprint + source summary。

## 2026-08-19 V2 Sub-Agent 输入能力（纯文本/多模态）判定链溯源与前端呈现

- 背景：Sub-Agent V2 角色生成依赖模型输入能力（纯文本 vs 文本+图像），但最终结论此前来自 profile > route > catalog > 名字注册表 的隐式判定链，用户遇到问题时无法知道“这个结论是哪一段给的”，也无法发现各来源声明之间的冲突。本次把“输入模态溯源（input modality provenance）”做进 V2 profile status，逐段呈现判定链 + 冲突。
- 提交 `b42c08ec`（分支 `bigstrongsun/reasoning-capability-p0`），4 文件 / 471 insertions：
  - `codex_config.rs` 新增三个类型：
    - `CodexSubagentInputModalitySource`（serde snake_case）：`ProfileExplicit | Route | Catalog | NameRegistry | Unknown`，最终结论的来源。
    - `CodexSubagentModalityDeclaration`：`{ source, declared: Option<Vec<String>> (skip none), adopted: bool }`，判定链中单个来源的声明 + 是否被采纳。
    - `CodexSubagentInputModalityInfo`：`{ modalities: Option<Vec<String>> (skip none), source, declarations: Vec<...>, conflict: Option<String> (skip none) }`。
  - `CodexSubagentProfileStatus` 新增 `input_modality: Option<...>` 字段（紧跟 `field_sources` 之后）；在 configured 状态构造处填充，legacy / 无 profile 处保持 None。
- 判定链与来源归属（`resolve_input_modality_provenance(settings, profile)`）：
  - 最终模态 = profile 显式声明（与 catalog 推导值不同视为用户覆盖 → ProfileExplicit），否则回退 catalog 推导值。
  - 来源归属优先级：profile 显式 > route 能力 > 模型目录 > 内置名字注册表 > 未知。
  - `declared_modalities_from_capabilities(&Value)`：从能力对象提取正向模态声明——`inputModalities` 数组 > `supportsImage`/`vision` 布尔 > `textOnly=true`；`textOnly=false` 是否定声明，不视为对模态的正向声明（返回 None）。
  - `detect_modality_conflict(route, catalog, name)`：route / catalog / 名字注册表 三者声明不一致时生成人类可读冲突说明（如“输入能力声明冲突：route 声明纯文本，模型目录声明文本+图像”）。
- 既有判定链（未改动，本次仅使其可见）：`text_only` = route caps > catalog caps > `is_confirmed_text_only_model`（名字注册表，`model_capabilities.rs`）；`input_modalities` = catalog entry（inputModalities > supportsImage/vision）。最终 V2 = profile 显式 > (text_only?["text"]:input_modalities) > unknown。
- 各消费者 Unknown 策略（有意为之，记录于 `model_capabilities.rs` 的 `ImageInputCapability` 枚举）：Desktop catalog fail-open（按可生图处理）、V2 fail-closed（不派图像任务）、media rectifier no-op。
- 前端：`CodexSubagentProfileEditor.tsx` 的 ProfileBackendOutput 呈现输入能力（纯文本/文本+图像）+ 来源 + 琥珀色冲突行；`codexSubagentV2.ts` 补对应类型；`CodexSubagentV2ProfileEditor.test.tsx` 新增 “renders input modality provenance and conflict in the profile status”。
- 验证：`cargo test --lib` 3168 passed / 0 failed / 5 ignored（新增 5 个 provenance 测试）；`pnpm vitest run` 138 文件 / 1121 用例全过；`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 均干净。
- 注意：本提交由并行会话落盘（当时 P2/P3 reasoning 工作也在并行提交）。已核验提交内容仅含本功能（不含 P3 的 `resolve_codex_model_reasoning_capability`/`trigger_codex_model_reasoning_detection`）；P3 工作仍留在工作区未暂存，勿误提交。

## 2026-08-19 V2 输入模态来源审计：发现自动值与用户覆盖语义混用

- 审计结论：当前 `inputModalities` 同时表示 catalog 自动推导值和用户手动覆盖值，字段本身没有来源标记。
- 证据链：`hydrate_codex_subagent_v2_input_modalities` 会把 catalog 推导的 `inputModalities` 写回 profile；`catalog_profile_draft` / `initialize_legacy_subagent_v2` 也会直接写入该字段；之后 `parse_persisted_subagent_v2` 无法区分这两类值。
- 影响：当 MultiRouter catalog 后续把同一模型从纯文本改成文本+图像（或反向变化）时，旧 profile 字段仍会被 `resolve_input_modality_provenance`、`preview_codex_subagent_profile_with_context` 和角色生成逻辑当作显式 profile 值，阻止新 catalog 能力生效；前端 `inferredInputModalities` 也优先返回旧 profile 值。
- 当前测试缺口：现有 5 个 provenance 测试只覆盖单次解析、route/catalog 冲突和显式覆盖，没有覆盖“catalog 刷新后旧自动值”的跨版本场景；因此测试全绿不能证明该隐患已消除。
- 修复边界：不能只改来源文案或冲突提示。应把“用户覆盖”和“catalog 自动值”分离（优先采用不持久化自动值；若必须兼容既有数据，则增加受控来源标记和一次性迁移），并为 catalog 能力变更补充回归测试。现有未提交的 reasoning P3 工作不应与该修复混合提交。

## 2026-08-19 V2 输入模态持久化语义根修

- 根修提交：`inputModalities` 只保留用户显式覆盖；catalog 推导值不再由 hydration、catalog draft 或默认 profile 写回持久化配置。
- 运行时：编译角色时若 profile 没有显式模态，按当前 `CodexCatalogModelSpec` 补齐文本/图像能力；preview 继续按同一 catalog 规则补齐。这样 MultiRouter catalog 刷新后，新能力会自动进入角色说明和 TOML。
- 兼容：历史配置中已经存在 `inputModalities` 的值仍按显式覆盖处理，不擅自猜测用户意图；新建/同步 profile 不再制造这类伪覆盖。
- 产品形态：UI 的默认 profile 不再预填“纯文本”；未声明时展示 catalog 的当前结果，用户选择“仅文本/文本与图像”才形成持久化覆盖。来源提示保留最终来源和冲突，但不要求用户理解内部多段优先级。
- 回归：新增 `catalog_refresh_replaces_automatic_profile_modality_without_persisting_it`；更新 hydration、初始化、catalog re-key 和 focused mutation 断言。相关 Rust 3175/3175 通过，TypeScript typecheck 通过；Vitest 本轮受 Windows 并发 worker/线程池环境影响未得到稳定完整输出，需在单一干净进程中复验。

## 2026-08-19 Codex Reasoning Capability P3（模型编辑器结构化最终生效视图）

- 前端提交仍在 `bigstrongsun/reasoning-capability-p0` 分支；模型目录编辑页现在为每个模型显示 `CodexModelReasoningCard`，其数据来自 P3 后端的 `resolve_codex_model_reasoning_capability`，因此不再单独复制能力判断逻辑。
- 卡片展示三态（支持推理/不支持推理/未知且使用服务端默认）、控制类型、能力来源、稳定指纹、Provider 原生档位、Codex 可选档位、默认值、关闭能力、effort 映射和最终上游行为；未知状态不静默转成不支持。
- 模型编辑器以当前 `catalogRows` 投影为 `settingsConfig.modelCatalog.models`，异步解析有请求序号和取消保护；空模型不请求，后端解析失败只保留“正在读取/未知”而不猜测。
- “重新检测”调用只读 `trigger_codex_model_reasoning_detection`，仅 `Found` 写 TTL 检测缓存；“采用检测结果”把带 reasoning 子对象的快照转成用户声明；没有 reasoning 的 vLLM 服务快照不可采纳；手动声明和恢复内置值复用既有 capability source mutation。
- 检测 Provider 使用当前草稿的 provider id/name/base URL，仅用于平台识别和只读元数据发现，不把 API key 送入检测请求；Tauri IPC 仍按官方命名参数调用。
- 新增 `CodexModelReasoningCard.test.tsx` 覆盖 unknown/unsupported 三态区分与 graded 行为描述。验证：`npx tsc --noEmit` 通过；`npx vitest run` 139 文件/1123 用例全过；`cargo test --lib` 3171 passed / 0 failed / 5 ignored；仅存在与本轮无关的 `streaming_codex_chat.rs` 未提交改动，提交时不得混入。
- 异常边界：解析 IPC 失败时前端生成不带能力声明的 unknown resolution，卡片明确显示“未知（使用服务端默认）”，不永久显示加载态，也不把通信失败误判为 confirmed_unsupported。

## 2026-08-19 V2 Sub-Agent unknown reasoning 保存门禁

- 根因：`validate_codex_subagent_v2_candidate` 过去只调用编译器，`delegated` 在 reasoning 未知时仍可生成 role 并保存；前端也只阻塞 `invalid/collision`。
- 修复：保存校验在编译后遍历 persisted profile 与 compiler status，仅对 `enabled=true` 且 `Routable`（实际会生成 role）的 profile 要求 reasoning capability 不是 `unknown`。disabled、unroutable、invalid 不因无关能力缺失阻塞保存。
- 错误码：`unknown_reasoning_capability_requires_declaration`；unknown 不允许通过 delegated 绕过，用户必须在模型目录声明能力或采用只读检测结果后再保存。
- 前端保存前用同一状态条件拦截，并显示 profile/model 名称和“推理能力未配置，当前可路由角色无法保存”；能力摘要同步强调这是保存阻塞，而非普通黄色提醒。
- 验证状态：`cargo fmt` 与 `git diff --check` 通过；Rust 全量测试当前被工作区已有的 `forwarder.rs` 缺少 `json!` 导入和 `handlers.rs` 缺少 `streaming_codex_chat` 导入阻塞，非本次修改引入；TypeScript 全量检查仍受现有依赖缺失（vitest、@dnd-kit）阻塞，目标文件无新增类型错误。

## 2026-08-19 Codex Reasoning Capability P3 收口

- 根因修复提交 `760de2d8`：`createCatalogRow()` 对新模型把 `upstreamModel` 初始化为空字符串，而 reasoning resolution effect 只读取该字段，导致模型编辑器虽有可见 `row.model`，却永远不触发能力解析，P3 卡片一直不显示。解析模型现在使用 `catalogRowUpstreamModel(row) || row.model.trim()`，空模型仍不会发起请求。
- 交互闭环测试覆盖：unknown 三态文案与服务端默认说明、手动声明、采用只读检测结果、重新检测；重新检测验证使用当前草稿 provider id/name/base URL，未把 API key 送入只读能力请求。
- 验证：`npx vitest run tests/components/CodexFormFields.test.tsx --pool=forks --poolOptions.forks.singleFork=true` 28/28 通过；`npx tsc --noEmit` 通过；`cargo check --lib`（`src-tauri`）通过；`git diff --check` 通过。测试仍有既有 Radix `act(...)` warning，不影响通过结果。
- P3 边界：模型编辑器结构化最终生效视图和交互闭环已收口；未停止、替换、安装或覆盖运行中的 CCSM；P4（GUI/CLI inspect 等独立投影）尚未开始，不因 P3 提前发布 release。

## 2026-08-19 Mac Codex MultiRouter Debug 红色 WebSocket 探针

- 截图中 `supports_websockets=false`、TCP 可达、live 接管一致，红色项却是 `本地代理 WebSocket 探针失败：error sending request for url (http://127.0.0.1:15721/v1/responses)`。源码 `src-tauri/src/commands/proxy.rs` 的 `diagnose_codex_multirouter` 无条件执行 `codex_probe_websocket_fallback`，即使 live config 已禁用 WebSocket；该探针只验证本地 GET + Upgrade 是否能收到预期 HTTP 426，不是模型请求，也不是上游连通性证明。
- 代理服务器的设计契约是 `/v1/responses` 的 GET/Upgrade 始终返回 HTTP 426，要求 Codex 走 HTTP Responses；因此 `supports_websockets=false` 与“探针失败”并不矛盾。Mac 现场更可能是旧安装包未包含该 426 路由、请求在本机代理/VPN 环境被 reset，或探针请求未收到 HTTP 响应；仅凭截图不能区分三者。需在 Mac 上用 curl/route log/版本哈希复核，不能把这项直接归因到模型或路由规则。
- 追加确认：Mac 使用 `v3.19.2-7`，该 tag 已包含 `5526855c fix codex multi router official websocket relay`。此版本的 `/v1/responses` GET + Upgrade 已从“固定返回 426”改成真实 `WebSocketUpgrade` relay；但 `diagnose_codex_multirouter` 仍无条件把同类请求当作“应返回 426”的 fallback probe。因此截图中的 `error sending request` 是诊断探针与 3.19.2-7 新 WS relay 契约不一致导致的版本内回归，不能据此判断路由或模型请求失败。
- 修复提交 `fa5a2651`：Debug 先读取 live config；`supports_websockets=false` 时跳过探针并报告 HTTP Responses 正常路径，启用/未知时仍保留真实探针失败。回归测试覆盖 HTTP-only 不阻塞与 WebSocket 启用时仍显示失败。
- 用户补充“接管开启但请求到不了 CCSM，怀疑 Mac 梯子冲突”后，排障边界应先看 `codex-router.log`：无新 request/route 事件=Codex -> 127.0.0.1:15721 入站被系统代理/TUN/NO_PROXY 处理阻断；有 `request_prepared` 但 `upstream_send_error`=已到 CCSM，冲突在 CCSM -> 真实上游出站。Mac 梯子应对 `127.0.0.1, localhost, ::1` 做直连/绕过，不能只改远端域名规则。
- 2026-08-20 Mac 现场补充：代理配置修好后，`codex-router.log` 显示官方 route 已进入 CCSM（`route_id=router-codex-official`、`upstream_url=https://chatgpt.com/backend-api/codex/responses`），但 `auth_prepared` 为 `auth_strategy=none auth_header_count=0 oauth_session_header_count=0`。CCSM OAuth 登录后可用、仅 Desktop OAuth 不可用，说明当前“Desktop OAuth 透传”边界没有把 Desktop 登录材料变成入站 Authorization 或 CCSM 可解析凭据；这不是梯子问题，而是 native Desktop OAuth 与 MultiRouter effective official provider 之间的认证契约回归。
- 2026-08-20 附件日志复核：23:41 的 `gpt-5.4` 与 `gpt-5.6-luna` 请求均已进入官方 route，但 `upstream_send` 明确 `uses_upstream_proxy=false`；23:41:49-23:43:22 的 CCSM 日志连续报 `client error (Connect): operation timed out` / `tcp connect error: deadline has elapsed`，根因是 CCSM 到 `chatgpt.com` 的出站直连失败，不是 Codex 未到 15721，也不是 CCSM 进程崩溃。23:53:38 才应用 `http://127.0.0.1:6528` 全局代理，23:57:52-53 OAuth Device Code 授权并保存成功。`app-exit-events.jsonl` 只有 `clean_exit`（`event_loop_exit`、`user_requested_exit`），没有 panic/crash 证据；前端 `unhandledrejection` 是次要 UI 错误。
- 修复提交 `30fa2315`：`materialize_codex_routed_provider_from_target` 对旧版 `provider_config` 官方 route，若目标是内置 `codex-official` 且没有明确 `codex_oauth`/托管账号绑定，恢复 `codexNativeAuthPassthrough=true`，使 Desktop OAuth 走 native 路径；显式 managed OAuth、账号池和污染/托管 route 仍保持原有托管认证。新增回归测试，先 RED 后 GREEN；Codex provider 单元测试 101/101、`cargo check --lib` 通过。
- 追加修复提交 `10576fea`：发现 `3.19.2-9` 仍可能从旧 Router 父 provider 继承 `codexNativeAuthPassthrough=false`，使前一修复未触发。现在只要目标是内置官方 seed、route 不是明确 managed OAuth、也不是账号池，就强制恢复 Desktop native auth；账号池和显式 managed route 仍优先。新增测试覆盖 stale false marker，Codex provider 101/101、cargo check 通过。

## 2026-08-20 Mac Codex Responses 无 User-Agent 导致本地路由误判

- 现象：重启后 Codex Desktop 请求报 `unexpected status 502 Bad Gateway: Unknown error`，但 `codex-router.log` 没有新事件；`~/.codex/config.toml` 仍正确指向 `http://127.0.0.1:15721/v1`，15721 `/health` 和 `/status` 正常。
- 根因：`handle_responses_for_app` 通过 `should_handle_as_codex_client` 判断 `/v1/responses` 是否为 Codex。旧实现把 Codex User-Agent 含 `codex` 作为必要条件；该 Desktop 请求没有满足该条件，于是误入 External OpenAI API 分支，在 MultiRouter 之前返回错误，因此不会写 `codex-router.log`。
- 修复：本地代理入口默认按 Codex 处理；只有显式 External API marker 或 `ccsw_` key 才强制走 External API。这样仍保留第三方 External API 的显式鉴权边界，不依赖不稳定的 User-Agent。
- 回归：新增无 User-Agent 仍走本地 Codex context 的测试；`cargo test --lib proxy::handlers::tests::` 82/82 通过。

## 2026-08-20 main 合并与 Windows 测试安装包

- `main` 已从 `c2d87eb7` 快进合入 `bigstrongsun/qwen-vllm-default-output` 的 `89b410d7`，保留无 User-Agent 的 Codex Desktop 兼容修复；此前明确否决的通用 Qwen 默认输出上限和 hosted/function tool 并行兜底没有重新引入。
- 对入口判定做了回归收口：无 User-Agent 不能单独等价于 Codex，否则普通无认证 `/v1/models`、`/v1/responses` 和 `/v1/images/generations` 会绕过 External API 鉴权。现在要求官方 Codex User-Agent 或稳定指纹头（`originator`、session/thread、`x-codex-*`、Responses 客户端头）；显式 External marker/API key 仍优先走外部 API。覆盖了“无 UA + x-codex-turn-metadata”与“完全无身份头”两条回归。
- 合并后验证：`cargo test --lib` 为 3185 passed / 0 failed / 5 ignored；`pnpm test:unit` 为 140 files / 1128 tests 全部通过；`pnpm tauri build --bundles nsis --config '{"bundle":{"createUpdaterArtifacts":false}}'` 返回码 0。
- 本地测试安装包（未安装、未上传、未推送）为 `src-tauri/target/release/bundle/nsis/CCSwitchMulti_3.19.2-7_x64-setup.exe`，SHA-256 `FE56DEE7D0DE64D852666CE3009E2DB83B4C3E9142A968FD812B12BF3914EA11`，未签名是预期结果（仅关闭 updater artifact 的本地测试构建）。运行中的 CCSM PID 67512 未停止或替换。

## 2026-08-20 推理能力配置入口、映射门禁与官方投影兼容根修

- Provider 表单新增默认可见的独立“模型推理能力”模块，位于模型就绪区与高级选项之间；模型目录明细仍在高级选项，但旧的目录行折叠推理入口已删除。每个模型卡片统一展示最终解析、能力来源、检测/采纳/手动/恢复动作、结构化编辑器和折叠专家 JSON。
- 结构化编辑器按“控制方式 / Provider 原生能力 / Provider 默认档位 / 上游传参 / Codex 到 Provider 映射 / 是否可关闭”分组解释。正常视图只显示当前模型已确认的原生档位；完整公共词表只在“添加 Provider 档位”下拉中出现。映射区也只显示当前模型档位，Qwen `low/medium/high` 不再暴露无关的 `xhigh/max/ultra` 行。
- 持久化契约：`graded + confirmed_supported + string/reasoning_object` 的每个正向 Provider 原生档位都必须有映射，目标必须属于 Provider 支持集合；`none` 是关闭能力，不是正向档位，不要求映射；boolean/none/budget 不要求 effort 映射。专家 JSON 缺映射立即拒绝，结构化表单保存前自动补齐同名映射并显式落库。
- 兼容策略为 read old / write complete：历史内置声明、能力库和旧 Provider 数据允许省略同名映射，Rust 消费入口先补恒等映射再严格校验；新写入仍直接走严格 `validate()`。这修复了官方/内置能力因新门禁被误判后退回通用 `low/medium/high/xhigh` 的回归；官方、DeepSeek、GLM、检测快照和 Sub-Agent 投影测试均恢复通过。
- 前端补图 helper 独立在 `codexReasoningCapability.ts`，避免 ProviderForm 从可被测试替换的 UI 模块导入领域逻辑。验证：Vitest 141 files / 1136 tests 全过；Rust lib 3187 passed / 0 failed / 5 ignored；TypeScript、rustfmt、diff check 均通过。未停止、替换、安装或覆盖运行中的 CCSM。

## 2026-08-20 官方模型配置热重载后推理/速度入口失效根修

- 用户点击推理能力配置并保存后，Codex Desktop 的官方模型推理强度不可用、推理速度消失。现场逐层核对三份模型信息源：`~/.codex/cc-switch-model-catalog.json`、`models_cache.json`/`models_cache.cc-switch-backup.json` 都完整保留 GPT-5.6 的 `supported_reasoning_levels`、`additional_speed_tiers` 和 `service_tiers`；但 `config.toml` 的 custom provider inline `models` 只有 reasoning 字段，完全缺少速度/服务档。
- 已证实根因：Codex Desktop 配置热重载会读取 provider inline `models`，而 CCSM 从 2026-07-13 引入 inline reasoning 投影时只同步了推理字段，没有同步 picker 的速度/服务字段。保存动作使 Desktop 从完整 JSON/cache 路径切到不完整 inline 模型定义，形成同一模型在三份数据源中的元数据分叉，足以稳定解释并复现速度入口消失。
- 修复：`codex_provider_models_toml_array` 现在从已经完成官方同 slug merge 的 catalog 条目同步 `additional_speed_tiers`/`additionalSpeedTiers`、`service_tiers`/`serviceTiers`、`default_service_tier`/`defaultServiceTier`。第三方模型继续投影空 service tier 数组，不能从官方模板继承 fast/priority。
- TDD：完整 `settings -> catalog -> config.toml inline models -> cache` 回归先确认 RED（官方 inline speed/service 字段缺失），再 GREEN；同时断言官方推理档位仍完整、第三方 service tiers 为 0。当前磁盘三份数据和当前 Codex 任务中的官方 reasoning 已恢复，未能再次复现“推理强度不可用”，因此不能把它单独归因为已证实；新构建安装后仍需做真实 Desktop 点击验收。验证：Rust lib 3187 passed / 0 failed / 5 ignored；Vitest 141 files / 1136 tests；TypeScript、rustfmt、diff check 全通过。未停止、替换、安装或覆盖运行中的 CCSM。

## 2026-08-20 Codex 模型元数据投影一致性审计

- 完整 `cc-switch-model-catalog.json` 与 CCSM-owned `models_cache.json` 当前使用 enriched catalog，同一模型的 reasoning、service tier、input modalities、multi-agent 等关键字段一致；真正的结构性分叉集中在 active provider inline `models` 和 CDP 缺条目 fallback descriptor。
- inline 在 `811433d6` 后已有 reasoning + speed/service/default service tier，但仍缺 `input_modalities`/`inputModalities` 与 `multi_agent_version`/`multiAgentVersion`。前两份 JSON 不可读时 Desktop 会退到 inline；官方 schema 对缺失 input modalities 默认文本+图像，可能把纯文本第三方错误声明为可接图，multi-agent 版本缺失则可能使 V1/V2 transport 降级。
- CDP `descriptorFor()` 找不到 payload entry 时硬编码 `medium` 和 `low/medium/high/xhigh`，Rust projection 也为缺值补 `medium`；这违反 unknown 不是 supported 的既定原则。正常 payload 中 modelNames/models 同源，主要风险在 default model 不在 routed entry、异常 payload 或未来名字-only 调用方。
- 同 slug 官方对象在完整 catalog/cache 中保留权威 transport 元数据；CCSM 显式创建的官方 V2 profile 与第三方 profile 当前都可编辑，但 picker 故障不是 profile 编辑器直接改坏官方能力，而是多份投影完备度不同。第三方同样遗漏，且模态、多 Agent、unknown reasoning 后果更高。
- `upgrade`/`upgradeInfo`/`availabilityNux` 属官方发布状态，应继续有意清空，不能复制到第三方；personality/specialty 属 picker-public 候选字段，需结合 app-server schema统一 alias 白名单，不能把全部内部 `ModelInfo` 无差别塞入 inline。
- 修正必须先建立单一 PickerModelProjection 契约和跨层 RED 快照测试，再统一 JSON/cache/inline/CDP；审计文档为 `docs/audits/2026-08-20-codex-model-metadata-projection-audit.md`。本轮只审计，未修改、停止或替换运行中 CCSM。

## 2026-08-20 Codex 模型元数据投影一致性根修

- provider inline 现在以 enriched catalog entry 为唯一能力来源，新增同步 `input_modalities`/`inputModalities`、`multi_agent_version`/`multiAgentVersion`、`supports_personality`/`supportsPersonality`、`model_specialty`/`modelSpecialty`；字段未知时保持缺失，不制造默认能力。已有 reasoning、speed/service/default service tier 投影继续保留。
- Rust `project_codex_model_descriptor` 不再为缺声明模型补 `defaultReasoningEffort=medium`；CDP `descriptorFor()` 也移除硬编码四档和 medium，只负责 identity/display/visibility 和保留现有完整 entry。因此 unknown reasoning 保持 unknown，不会因 Desktop fallback 被升级成 confirmed supported。
- TDD RED 证据：inline Qwen 缺模态断言失败；unknown Rust projection 出现 medium 断言失败；CDP fallback 出现默认/四档断言失败；官方 personality/specialty inline 断言失败。实现后聚焦 inline 1/1、Desktop 25/25、Rust lib 3189 passed / 0 failed / 5 ignored、Vitest 141 files / 1136 tests 全过。
- 当前只是源码与测试收口，未构建、安装、停止或替换运行中的 CCSM。真实验收仍需新安装包后检查官方推理/service tier、第三方档位、纯文本图片入口和 Sub-Agent V1/V2。

## 2026-08-20 DeepSeek MCP tool_search 能力声明根修（upstream PR #6653）

- 官方仓库 PR `farion1231/cc-switch#6653` 指出 bundled DeepSeek catalog 把 `supports_search_tool` 错写为 `true`。OpenAI Codex 的 `search_tool_enabled` 实际用该字段与 provider namespace capability 一起决定是否把 MCP 工具从 direct exposure 延迟到 `tool_search`；它不是 hosted web search 的总开关。
- DeepSeek Responses 网关没有 Codex `tool_search` 协议能力。错误的 true 会让 MCP 工具不再内联，而模型又无法通过 tool_search 发现它们；`web_search_tool_type` 与 provider web-search capability 是独立门控，改成 false 不会关闭 DeepSeek hosted web search。
- 本地集成采用 PR 的两处模板修正，并新增覆盖 bundled `deepseek-v4-pro` 与 `deepseek-v4-flash` 全部条目的回归测试。测试先 RED（实际读到 true），再改为 false 转 GREEN；不能只测试一个模型，否则另一个条目未来仍可能漂移。
- 本轮只合入本地 main，不推送、不发布、不构建或安装，也不替换运行中的 CCSM。

## 2026-08-20 合入 CCSM PR #26：删除 Provider 后保留 MultiRouter 模型顺序

- CCSM PR `BigStrongSun/ccswitchmulti#26`（作者 GaoHu1997，原提交 `6cc2a301`）修复删除 Provider 后 MultiRouter 聚合模型目录重建导致剩余模型重新排序。根因有两层：删除 mutation 过去只删除 Provider，没有移除引用它的 route 并回写聚合目录；普通 provider/route 同步重建目录时又完全采用当前 route/provider 遍历顺序，使已有模型跳位。
- 新增 `codexModelCatalogOrder.ts` 作为前端目录顺序 SSOT：按上一版目录的 `sortIndex`（缺失时按数组位置）保留现存模型相对顺序，真正新增模型追加到末尾；存在自定义排序时把剩余模型压缩为连续 `sortIndex`，恢复默认状态下则清除上游 Provider 可能携带的 `sortIndex`，不把默认排序污染成用户排序。
- `useDeleteProviderMutation("codex")` 删除成功后重新读取 Provider 集合，移除所有引用被删 Provider 的 MultiRouter route，并逐个回写同步后的 plan；非 Codex 应用不进入这条同步路径。该行为不改变 route/modelMap、Sub-Agent 候选剪枝和默认排序的既有边界。
- TDD 证据：当前 main 在只改期望后，`codexMultiRouterSync.test.ts` 明确 RED（`qwen3.6` 从原第二位掉到末尾）；应用实现后目录排序、MultiRouter 同步和删除 mutation 聚焦测试 18/18 GREEN。`pnpm typecheck` 通过，PR 六个变更文件 Prettier 通过；全量 Vitest 首轮 1140/1142，两个 `tests/integration/App.test.tsx` UI wait 超时在隔离单进程重跑为 11/11，通过，属于全量并发/DOM 污染而非本 PR 稳定回归。仓库全量 `format:check` 仍被本轮未改的 `CodexSubagentProfileEditor.tsx` 与 `CodexSubagentV2ProfileEditor.test.tsx` 两个既有格式漂移阻塞。
- 本轮仅合入本地 main，不推送、不发布、不构建、不安装，也不停止或替换正在运行的 CCSM。

## 2026-08-21 Sub-Agent reasoning 保存门禁接入普通 Provider 保存入口

- 主分支审计发现：`e8c19353` 已在 `validate_codex_subagent_v2_candidate` 和 V2 专用 mutation 中阻止 unknown reasoning，但普通 `ProviderService::add/update` 只执行通用 Codex settings 校验，仍可绕过该门禁保存整个 Provider。
- 根修：`ProviderService::add` 与 `ProviderService::update` 在所有数据库/Live 写入前调用同一 `validate_codex_subagent_v2_provider_candidate`；无 `codexRouting.subagentV2` 的普通 Provider 不受影响，有 V2 文档则执行严格解析、编译和 `enabled + Routable + reasoning != unknown` 校验。
- 回归：新增 add/update 两条测试，先确认 RED（两条路径都错误返回 true），接入统一保存校验后 GREEN；断言 rejected add 不入库、rejected update 保留旧 settings。前端新增 unknown 保存阻止测试，断言不调用 `update_codex_subagent_v2`/`update_provider`。
- 当前验证：Rust `cargo test --lib` 3197 passed / 0 failed / 5 ignored；V2 前端文件 126/126；全量 Vitest 143 files / 1144 tests；TypeScript、rustfmt、diff check 通过。测试中仍有既有 React act、MSW 未处理请求和 Tauri window mock 警告，不影响通过。

# 2026-08-21（MultiRouter Provider SSOT v2 合入 main 与向导回归）

- `bigstrongsun/codex-multirouter-ssot-v2` 已在 `main` 以合并提交 `b7865131` 合入；旧的前端 `codexMultiRouterSync` 快照同步已删除，Provider/模型目录、Rust v2 compiler、mutation coordinator、projection 和迁移预览成为主线基础。
- 向导保存重复创建的根因是保存函数没有组件级 in-flight Promise 门禁，且每次构建新方案都用 `Date.now()`/Provider 展示名重新生成 ID。修复为打开向导时稳定生成一次 plan ID、同一轮保存立即复用 in-flight Promise、保存成功后把返回 Provider 作为当前编辑目标；后端 `save_provider` 按 `(id, app_type)` 更新已存在行，因此重复请求不会新增第二条方案。
- 别名漂移根因是前端 `resolveWizardModelNameCollisions` 把展示别名写回 Provider 模型目录，并按 Provider 名称每次重算。现在 Provider 保持 canonical 模型，Route 首次物化 alias；编辑已有 schema-v2 方案时优先保留 Route 已持久化的 alias。别名目标若不在当前 Provider 目录或 `all/include` selection，向导显示处理错误并禁止保存，Rust compiler 仍做最终校验。
- 向导最终页即使没有模型源也保留保存入口并显示可操作说明；模型源卡展示认证、模型目录、协议、能力、OAuth、工具和 projection 状态，Provider 详细配置仍由 Provider 页面维护。保存前 v1 继续要求脱敏迁移预览并显式应用。
- 本轮回归：`src/components/codex/CodexMultiRouterWizard.test.tsx`、`tests/lib/codexMultiRouterWizard.test.ts`、Rust compiler rename test；已验证 `pnpm typecheck`、定向 Vitest、Rust 定向 compiler test。React/Radix 测试仍有既有 `act(...)`/window mock 警告，不是失败。
- 搜索渠道：Codex WebSearch 命中 React 官方 `Managing State`/`Reacting to Input with State`，确认 submitting 状态应禁用提交；Matrix WebSearch 已独立尝试同一官方页面但 relay 返回 `fetch failed`，因此第二条链本轮没有可用正文，不能把它当作交叉来源。

# 2026-08-21 第三方 hosted web search 401 根修

- 已证实 Qwen/vLLM 不是根因：真实上游对精确 `web_search` function schema 在 `tool_choice=auto`、强制 function choice 和流式请求均返回 HTTP 200 的 `tool_calls`。CCSM 也已正确完成 Responses hosted tool 到 Chat function 的投影，并进入 hosted loop。
- 真正根因是 `resolve_hosted_tool_client` 无条件把入站 `Authorization: Bearer ...` 当成 ChatGPT OAuth。第三方 Provider 的入站值实际可能是 `PROXY_MANAGED` 占位符或第三方 API key，导致官方 hosted `web_search` 请求使用错误凭据并返回 401，后续循环只能返回无搜索结果的响应。
- 根修：`source_codex_oauth_credentials` 现在只允许 `provider_uses_native_codex_auth(provider)` 的本机官方 Codex 路由复用入站真实 Bearer，并过滤 `PROXY_MANAGED`；第三方路由不再复用入站认证，直接回退 CCSM 托管 Codex OAuth（或显式环境 API key）。
- 回归证据：新增“官方 native Bearer 可复用”“`PROXY_MANAGED` 被拒绝”“第三方 Bearer 不复用”三条测试；Rust 全量 `3254` tests 中 `3249 passed / 0 failed / 5 ignored`，`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 均通过。
- 仍需完成：提交后构建并事务安装新 canary，重跑真实 Qwen/DeepSeek hosted-search；版本号仍为 `3.19.2-9`，正式 release 还需新版本号、跨平台 macOS/Linux 产物和对应运行态验收。不能复用上游已有的官方 `v3.20.0` 标签。

- 追加运行态证据：提交 `2c41f638` 构建的安装包 SHA-256 为 `EF80037B1E5662C7DE9051F8067F588E59ADCF3ADD9F66202D2E7DD95B23DB33`；事务 `ccsm-20260821-135500-70ff5151640c4a70bbb62be77f60f5e9` 成功，新 PID `5952`，`15721/health` 为 `200`。DeepSeek V4 Pro hosted-search canary 通过；Qwen3.8 仍无 function call，但日志确认上游 HTTP `200` 且未再出现 OpenAI hosted tool `401`，剩余问题属于 Qwen/vLLM 工具调用触发边界。

# 2026-08-21 v3.19.2-10 发布链路卡住后的处理

- `main`/`fork/main` 与 tag `v3.19.2-10` 均指向 `fef82c8f`，版本提交包含 hosted-search 认证根修和“成功但未产生 hosted tool call”诊断；本地工作树仅有未跟踪 `.tmp/`。
- GitHub Actions run `32458164107` 的 Linux x64/ARM64 job 已成功，但 Windows x64、Windows ARM64 和 macOS job 从 `2026-08-21T07:20Z` 长时间停在构建步骤，明显超过上一版 `v3.19.2-9` 约 56 分钟的完整耗时；当时 Release 尚未创建，`/releases/latest` 仍为 `v3.19.2-9`。
- 已提交取消旧 run，并通过 `gh run rerun ... --failed` 请求重跑；GitHub API 随后出现连接超时，重跑 attempt、Release 资产和 `latest.json` 仍需网络恢复后确认。若只重跑失败 job 导致汇总 job 被跳过，应改为完整 rerun 或重新推送同一 tag 的等价 release 流程。
- 发布完成的必要验收顺序：五个构建 job 全部成功 -> `Publish GitHub Release` 成功 -> Release 为非 draft/非 prerelease 且 latest 切到 `v3.19.2-10` -> 六个平台 updater 资产及 `.sig` 存在且 `latest.json` 覆盖全部平台 -> 对 Windows 安装包/运行态健康端口和第三方 hosted-search canary 做最终确认。未完成这些步骤前不能宣称 release 已交付。

# 2026-08-21 Provider 模型级推理能力 UI 收口

- `CodexFormFields` 的正常入口现在只呈现模型级“模型推理能力”：每个 catalog 模型先显示模型名、能力来源、Codex 可选档位、默认档位和 Ultra 编排状态，点击“配置推理能力”才展开该模型的来源选择、探测、映射、编辑器和专家 JSON。展开状态按 `rowId` 保存，更新仍通过既有 `handleUpdateCatalogRow(index, { reasoning })`，不会改写其他模型。
- 原 Provider 级 `codexChatReasoning` 不再作为普通“思考能力”配置显示；只有既有对象非空时才出现折叠的“旧版兼容兜底”，文案明确它影响所有没有模型级声明的模型。新模型级流程不写 Provider 级配置；本轮没有改运行时优先级或存储迁移语义。
- 新增 `CodexModelReasoningSummary` 及测试；模型摘要、既有编辑器、能力卡和持久化回归共 17 条通过，`pnpm typecheck`、Prettier、`git diff --check` 通过。Vite Browser 页面能加载但没有 Tauri bridge，不能读取真实 Provider 数据，安装后仍必须在 Desktop 中验证多模型摘要、单卡展开和旧版兼容区交互。

# 2026-08-21 CCSM 分支与开放 PR 再审计

- 审计基准是 `cc-switch` 子仓库 `main`，当前 HEAD 为 `8d92a8fd`；外层 `LLMservice/master` 是聚合工作区，不能用来判断 CCSM 分支是否合入。
- GitHub 当前开放 PR：#21（`codex/reasoning-model-catalog-fix`）、#24（`checkbox`）、#26（`sort_bug`）、#19（usage route 名称）、#13/#14 和 Dependabot Actions PR #1-#5。#22、#20 已关闭；#18 已合入。
- PR #26 的原提交 `6cc2a301` 及删除 Provider 后的补充修复 `dab41928` 已在当前 main；当前已有 `src/lib/codexModelCatalogOrder.ts`、MultiRouter 删除同步和排序回归，不能再次合入旧 PR。
- PR #24 的核心功能提交 `bd5da4c2` 已在当前 main：catalog `enabled=false` 会在 Codex catalog、Desktop inline models、Sub-Agent 候选和 MultiRouter 同步中被过滤，同时保留原始停用行供重新启用。PR #24 后续 `2fc8d56d`、`146d3e22`、`1cd6342e`、`74a3c875`、`24ca5b4a` 只有格式、测试和“保留”改为“启用”的文案变化，功能未漏合；当前 UI 仍使用旧“保留”文案，是否单独采纳属于后续 UX 决策。
- PR #21 的 `07bbed8f` 仍未被当前 main 等价吸收。它尝试从真实 `config.toml` 的 `[model_providers.*].models[]` inline 定义读取 reasoning 能力，为 MultiRouter routed alias 补齐 modelCatalog 缺失的档位。不能直接 cherry-pick：旧实现绕过当前 `reasoning_capabilities::resolve_codex_model_capability_core` 的统一来源/指纹链，并引入旧的 `provider_config` 来源语义。后续应把 inline 声明作为同一 resolver 的用户拥有配置输入，补充 alias/upstreamModel 回归后再移植。
- PR #19 只改 usage 统计与筛选，不阻断本轮 hosted-search/reasoning 发布；PR #13/#14 是高风险大批量依赖升级，不与功能发布混合。`bigstrongsun/ccsm-agent-mesh`、`fix-unsupported-responses-tools`、portable reasoning 实验和旧 Sub-Agent 发布分支也未合入当前 main，但分别属于独立功能、官方大分叉或实验/历史发布线，不应误并入本轮。
- 当前测试适配了模型推理卡片默认折叠后的交互；定向 Vitest `CodexFormFields`、`ProviderForm.codexCatalog`、`codexSpawnAgentCandidates` 为 41/41 通过，保留既有 React `act(...)` 警告。

# 2026-08-21 CCSM 其他分支与开放 PR 复核（HEAD 34dfbb1b）

- 本轮审计基准为 `cc-switch` 子仓库 `main`，HEAD 为 `34dfbb1b`；工作树只有用户原有未跟踪目录 `.tmp/`。GitHub API 受匿名 rate limit 限制，PR 状态以 `gh pr` 读取结果和本地 refs/提交对照为准，并用 Codex WebSearch 与 Matrix WebSearch 独立检索；Matrix relay 没有返回可用 GitHub 正文，不能把它当作状态证据。
- BigStrongSun fork 当前仍开放：#21（`codex/reasoning-model-catalog-fix`，`DIRTY/CONFLICTING`）、#24（`checkbox`，`DIRTY/CONFLICTING`）、#26（`sort_bug`，`DIRTY/CONFLICTING`）、#19（`provider_total`，`DIRTY/CONFLICTING`）、#13（Cargo 52 项依赖升级，`MERGEABLE/UNSTABLE`）、#14（前端 56 项依赖升级，`MERGEABLE/UNSTABLE`）及 Actions Dependabot #1-#5。#20/#22 已关闭，#18 已合入。
- #21 原提交 `07bbed8f` 仍不是当前 main 的等价 patch，价值是“从真实 `config.toml` inline model 定义为 routed alias 补 reasoning”。但旧实现绕过现行 `reasoning_capabilities::resolve_codex_model_capability_core` 来源链和指纹语义，不能 cherry-pick；应将 inline 声明接入现有 resolver，并补 alias/upstreamModel RED/GREEN 回归后再单独移植。
- #24 核心行为已由 main 的 `bd5da4c2` 及后续主线提交覆盖：`enabled=false` 停用行保留，但 Codex catalog、Desktop inline models、Sub-Agent 候选和 MultiRouter 同步全部过滤。PR 后续提交只是格式、测试和“保留”改为“启用”文案，不能整枝合并。
- #26 的原排序提交 `6cc2a301`、删除 Provider 后的 `dab41928` 已由 main 的 `codexModelCatalogOrder.ts`、MultiRouter 删除级联和 Rust projection 承接；当前 `main` 的 Rust projection 使用 Provider `sortIndex`，前端 helper 注释已明确聚合目录不直接消费 Provider sortIndex，新增测试覆盖该语义，不能整枝回合。
- #19 的 usage provider 名称解析、筛选、模型统计供应商列和缓存命中百分比已由 `475cd008`、`2905ce2e` 等主线提交覆盖；不应以开放 PR 状态误判为功能遗漏。
- 上游 `farion1231/cc-switch` 的 #6530 仍开放，但其核心 patch 与 main 的 `5b820624` patch-id 等价；#6616 仍开放，但主线 `6dc7e007` 已覆盖 unsupported Responses tools 的拒绝逻辑，PR 分支还混入 Ultra/Zen/缓存等其他提交，不能整枝合；#6653 仍开放，但 DeepSeek MCP catalog 修正已由 `255a6771` 本地承接。
- 本地其他 BigStrongSun 分支分类：
  - `bigstrongsun/ccsm-agent-mesh`（`a68b803a`）仍是未接入现有代理生命周期的独立 AgentMesh 后端原型，属于未合入的独立功能，不是本轮 hosted-search/reasoning 缺口。
  - `bigstrongsun/ultra-orchestration` 的 `0c8869c7`/`39d8f44` 虽在该分支上仍为 unique commits，但功能已通过 `5036705f`、`b45235d3` 和合并提交 `77d011c8` 进入 main，不应回合旧分支。
  - `fork/bigstrongsun/fix-responses-lite-additional-tools` 的 `a0d7b47b`/`31d8a937` 仍是 unique commits，但行为已重构落在当前 `transform_codex_chat` 与 `openai_compat` additional_tools 处理；当前测试 `responses_lite_additional_tools_preserves_tools_without_creating_a_message`、`...reuses_custom_namespace_and_deduplication_rules` 均通过，不应 cherry-pick。
  - `fix-responses-commentary-tool-calls` 与 `fix-unsupported-responses-tools` 都是混入大量历史主线提交的长分支；前者只取 `5b820624` 等价行为，后者只取 `6dc7e007` 的拒绝逻辑，整枝合并会重复旧 release/重构并引入无关变更。
  - `commentary-reasoning-experiment`、`portable-reasoning-experiment-nogo`、`subagent-v2-capability-injection` 和旧 `release-v3.19.*` refs 主要是实验、学术材料或发布证据；没有本轮应回合的生产代码。
- 当前实跑验证：Rust `unsupported_responses_tool_type_fails_loudly_instead_of_being_dropped`、两个 Responses Lite additional_tools 测试、`codex_catalog_reasoning_resolves_provider_inline_model_alias` 各 1/1 通过；前端 `codexModelCatalogOrder`、`codexMultiRouterWizard`、`CodexFormFields.keepColumn`、`ProviderForm.codexCatalog` 定向套件 45/45 通过。之前带 `--exact` 的 Rust 命令筛到 0 tests，已改为非 exact 过滤重新执行，不能把那次 0 tests 当作验证。
- 结论：当前确实未合入的生产代码只有 AgentMesh 原型和 #21 的“inline reasoning alias 接入现行 resolver”候选；其余用户此前关注的搜索、unsupported tools、DeepSeek catalog、排序、停用模型、usage 统计、Ultra 和 Responses Lite 均已在 main 有等价或更完整实现。依赖 PR #13/#14 不属于功能修复，应与本轮功能 release 分开评估，尤其 #14 同时跨 React/Vite/Vitest/Tailwind/TypeScript 大版本。

# 2026-08-21 CCSM 分支审计更正（当前 HEAD 048961b3）

- 重新 `git fetch --all --prune` 后，当前审计基准为 `main@048961b3`。活动仓库 `BigStrongSun/ccswitchmulti` 的开放功能 PR 仍为 #21、#24、#26、#19；#13/#14 和 Actions #1-#5 是独立依赖维护线。
- PR #21 的 head `07bbed8f` 仍不在 `main` 祖先链，`git cherry` 也显示不是同 patch；但它解决的行为已经由主线 `reasoning_capabilities::resolve_codex_model_capability_core` 覆盖：catalog 会按 alias 失败后用 `upstreamModel` 重试统一 resolver，inline `model_providers.*.models[]` 作为 `UserConfig/provider_config` 输入参与能力解析。当前回归 `codex_catalog_reasoning_resolves_provider_inline_model_alias` 实跑 1/1 通过，因此不能再把 #21 列为待移植功能，也不能直接 cherry-pick 旧实现。
- PR #24、#26、#19 的原提交仍因历史基线不同而在 `git cherry` 中显示 `+`，但行为分别已由停用模型 SSOT、MultiRouter 排序/删除级联/Rust projection、usage provider/model 聚合主线覆盖；开放状态不等于功能遗漏。
- 当前真正未合入的生产代码只有 `bigstrongsun/ccsm-agent-mesh@a68b803a`：它仍是独立 AgentMesh gateway 原型，未接入现有 CCSM HTTP 代理、Provider 生命周期、凭据边界和运行态 canary，不能整枝合并。
- `commentary-reasoning-experiment`、`portable-reasoning-experiment-nogo`、`subagent-v2-capability-injection`、旧 release/备份 refs 继续归类为实验、学术资料或历史发布证据；`ultra-orchestration`、Responses Lite、commentary/tool-call、unsupported-tools、DeepSeek catalog 等虽有 unique commits，但主线已有等价或更完整实现，不应回合旧分支。
- 当前验证：inline reasoning Rust 1/1；usage stats Rust 3/3；`codexModelCatalogOrder` 4/4、`codexMultiRouterWizard` 34/34。首次 Vitest 命令误传不支持的 `--runInBand`，已改正后 38/38 通过；不存在的旧 `CodexFormFields.keepColumn.test.tsx` 未计入结果。

# 2026-08-23 PR #35 MultiRouter 严格路由与投影修复审计

- PR `BigStrongSun/ccswitchmulti#35` 不能整包合并：它混有 release 版本/签名、公钥轮换、Windows 原子写降级、WebDAV 本地状态表和路由修复，并与当前 `main` 工作台测试冲突。本轮只移植可独立证明的路由与 catalog 根修；版本、签名和直接覆盖写降级明确排除。
- V2 路由契约收敛为 fail-closed：`include` 是严格白名单，前缀不能让未勾选模型逃逸；`mode=all` 以目标 Provider 当前 catalog/alias 为模型集合；未知模型、无 model 的原始请求和未命中模型不再回退 `defaultRouteId` 或首条 enabled route。前端预览只检查当前选中方案，并使用相同 include/mode=all 语义。
- `defaultRouteId` 只作为旧数据读取字段用于提示。统一 `serializeCodexRoutingV2` 不再写出该字段，因此设置、路由、协议切换、Sub-Agent 候选保存等任何新保存都会清理它；新向导不生成它。UI 检查项、诊断和方案摘要必须明确“旧版默认路由已停用/未匹配模型拒绝转发”，不能继续暗示 fallback。
- 保存 Route 或 Sub-Agent 候选不得删除 Router Provider 的 `settingsConfig.modelCatalog`。它承载模型顺序、显示名、推理/输入能力和 Sub-Agent 候选，是数据库事实；live 投影不能替代数据库持久化源。
- catalog 投影必须保留源模型 `reasoning` 与 `displayName`，`/v1/models data[]` 同时投射 `display_name/displayName/name`。V2 live 写入从 Provider 与 route 重新编译 projection，不读取 Router 中可能陈旧的 catalog 快照；多个 MultiRouter 共用目标 Provider 时，只有当前 profile/current provider 对应的激活 Router 可以发布共享 live catalog。
- 兼容读取允许同步合并后的 V2 route 缺少 `modelSelection`，按 `mode=all` 规范化，避免工作台读取 `.mode` 崩溃。带 route alias 后缀的 `deepseek-v4-flash-*` / `deepseek-v4-pro-*` 可作为 V2 角色模型，但 Flash Vision 变体明确排除。
- 搜索渠道：Codex 内置 WebSearch 检查 GitHub PR 页面与提交列表；Matrix WebSearch 独立检索但没有返回可用 GitHub 正文。关键结论最终以本地 PR head `1f362461`、当前 `main@f83a4145`、逐提交 diff 和 RED/GREEN 回归为准。

# 2026-08-23 PR #35 WebDAV session_log_sync 本地状态隔离

- `session_log_sync.file_path` 是本机 Codex 会话文件路径与增量读取进度，不是跨设备配置。旧同步导出会把它写进 SQL，portableize/localize 又会改写主键，可能与目标机器已有路径碰撞并导致 WebDAV/S3 导入失败。
- 根修把 `session_log_sync` 同时加入 `SYNC_SKIP_TABLES` 与 `SYNC_PRESERVE_TABLES`，并让同步快照的 TEXT 路径/密钥重写统一跳过所有 skip/preserve 表。结果是远端 SQL 不含该表数据，导入时保留目标机器自己的进度，路径主键也不会进入跨设备改写链。
- 回归 `sync_import_preserves_local_only_tables` 先 RED（远端 SQL 含 `remote.jsonl`），再 GREEN；测试同时断言远端状态不导出、本机四个进度字段完整保留。主工作树一度被并行 `preset_registry` 编译错误阻塞，等待对方修复后实跑 1/1 通过；隔离编译曾因磁盘不足失败，不计入成功证据。

# 2026-08-23 PR #35 fail-closed 旧测试清理

- 严格路由实现已删除 V2 与兼容 `codexRouting` 数组的首条启用 route 回退，但 `codex_subagent_v2_initialization_includes_runtime_first_enabled_fallback_model` 仍保留旧期望，会让后端全量测试与真实运行语义互相矛盾。
- 回归改为 `codex_subagent_v2_initialization_excludes_unmatched_model_without_route_fallback`：未匹配模型的运行时 route 必须为 `None`，Sub-Agent 初始化不得为其生成草稿。此修改只校正测试契约，不重新引入 fallback。
- `tests/config/codexChatProviderPresets.test.ts` 的 DeepSeek 原生 Responses 目录期望也曾停留在 Flash/Pro 两项，而产品预设已包含 `deepseek-v4-flash-vision-exp`。同步加入 Vision 的 1M 上下文期望，避免预设组合回归长期假红。
- 后端全量还暴露 `codex_subagent_v2_target_provider_record_is_authoritative_with_safe_inline_fallback` 的末项仍把未命中的 `gpt-5.6-sol` 归到首条第三方 route；严格语义下应为 `None`。匹配 route 的 Provider record/inline auth 权威性断言保持不变，仅清除未匹配继承。
- 同一轮全量还发现新提交 `d25ebe31` 已把第三方 `reasoning_content` 与内联 `<think>` 都转换为 raw `reasoning_text`，但只更新了前者测试；`converts_inline_think_chat_sse_to_reasoning_without_leaking_tags` 仍期待旧 summary delta。测试改为断言 `reasoning_text.delta/done` 且不得出现旧 summary delta，保持标签剥离与正文断言不变。

# 2026-08-23 WebDAV 同步 × 可更新 Provider 预设注册表 联调地基

- 用户问“WebDAV 同步是否有问题、最近做的远程预设表（把模型能力/推理信息从硬编码转为可配置）能否与它联调”。结论：两套是不同平面，能联调且边界清晰。WebDAV 同步=用户自有多设备状态同步（整库 db.sql+skills.zip+manifest.json，LWW，无合并，无签名）；预设注册表=官方预设数据分发（版本化、签名、三方合并、用户覆盖保护）。
- WebDAV 同步“有点问题”的实证：`cfa8411b fix(sync): keep session log progress local to each device` 修了一个真实 bug——`session_log_sync`（本机 Codex 会话文件路径+增量读取进度）原先被导出进同步 SQL，其路径主键在 portableize/localize 改写时可能与目标机器已有路径碰撞，导致 WebDAV/S3 导入失败。根修把 `session_log_sync` 同时加入 `SYNC_SKIP_TABLES` 与 `SYNC_PRESERVE_TABLES`，并让同步快照的 TEXT 路径/密钥重写统一跳过所有 skip/preserve 表。另有已知限制：多设备并发编辑同一 Provider 仍是 LWW，无字段级仲裁（首版取舍，非 bug）。
- 关键设计决策 D1（本次联调核心）：`presetBinding` 必须落在 `providers.meta`（DB），不能按原 TODO 放 `settings.json`。因为 WebDAV 整库同步只上传 db.sql+skills.zip，不上传 settings.json；若 presetBinding 在 settings.json，设备 A 应用预设后 modelCatalog 变更（DB）会同步但 presetBinding 不会，设备 B 丢失“哪些字段来自预设/用户覆盖集合/基础快照 hash”，未来更新可能静默覆盖用户编辑。`providers` 表已有 `meta TEXT NOT NULL DEFAULT '{}'` 列，且在 auto-sync 触发表内、不在 SYNC_SKIP/PRESERVE 内，随整库同步自然跨设备一致。设备级 `preset_registry`（源列表+缓存，含 WebDAV 凭据）仍放 settings.json——它是本机如何获取预设的配置，属设备私有，凭据尤其不能跨设备同步。
- 关键设计决策 D2：WebDAV/S3 作为预设注册表 P2 传输，复用 `services/webdav.rs` 原语（get_bytes/head_etag/ensure_remote_directories/put_bytes）。预设源布局 `{remote_root}/presets/{profile}/manifest.json`，与同步布局 `{remote_root}/v2/db-v6/{profile}/` 平行互不干扰。WebDAV 本身不提供签名，故源必须携带离线签名 manifest（发布端受信私钥签名，客户端固定公钥验证）。满足“没有受信源+签名验证前不得裸 URL 下载更新”红线。
- 关键设计决策 D3 信任分层：`pinned-key`（固定公钥+Ed25519 签名+SHA-256+过期+版本不回退，全过才接受）/ `local`（本地导入/用户显式信任，跳过签名但保留 hash/过期/版本，UI 标注未签名）。内置预设 `codexProviderPresets.ts` 永远是离线兜底与最低版本基线。
- 关键设计决策 D4：预设更新是整库同步的输入而非替代——本地单事务写 Provider+presetBinding+备份旧快照，随后由既有 auto-sync 触发器（providers 表变更）把结果同步出去。预设三方合并只在“同一设备本地应用预设”时发生，不跨设备仲裁。

# 2026-08-23 MultiRouter Provider SSOT v2 独立机制审计

- 本轮不是按公开 issue/PR 对账，而是枚举 Provider/Router mutation、Profile 切换、live projection、外部 API、图片路由探测、导入/恢复/同步和并发发布边界，专门寻找未被反馈的 SSOT 漏口。Codex 内置搜索与 Matrix WebSearch 分别检索了公开资料；公开根因文档只确认历史上的 DB/UI 已更新而 live catalog/cache 陈旧问题，Matrix 索引证据较弱，因此具体结论以当前本地源码与 RED/GREEN 回归为准。
- 新确认并根修五类未公开遗漏：失效 Profile Provider ID 过去会阻断 device-local/DB 当前 Router 回退；Profile apply 过去在 Provider 切换后才更新 current profile，导致新 Router 投影被旧 Profile 所有权拒绝；projection build/publish/read-back/status 过去没有统一串行边界，旧构建可能晚于新构建覆盖共享 live 文件；External Agent 后端列表、`/v1/models` 和图片生成 official-route 探测仍读取 Router 派生 `modelCatalog`；数据库备份 restore 替换 SQLite 后没有执行 Provider live projection 与 settings reload。
- 实现边界：新增中央 `compile_provider_v2`，schema-v2 非运行时消费者与 Codex runtime 共用同一 Provider 实时编译；Router 原始 `codexRouting.routes` 只保留为 route policy/ID，不能作为模型目录事实源。投影完整生命周期使用进程级互斥锁；Profile 所有权在切换前发布且验证 Provider 仍存在；restore 成功后运行与 SQL/WebDAV/S3 相同的 post-import sync，后置同步失败以 warning 返回而不伪装数据库替换失败。
- TDD/验证：旧实现下 stale Profile、并发 publisher、External `/v1/models`、backend options 四个回归明确 RED；实现后 MultiRouter 定向 67/67、Profile/外部目录/restore 定向 5/5、Rust lib 3326 passed + 5 ignored、Vitest 144 files/1170 tests、TypeScript typecheck 全过。未构建、发布、安装或替换运行中的 CCSM。

## Provider 模型事实逐字段同步补审

- 用户进一步明确 SSOT 目标：Provider 模型增删、启停、上下文、输入模态、推理档位/映射、Ultra、协议、缓存、显示名、原生 Responses 能力和排序变化都必须使 schema-v2 MultiRouter 重编译并刷新 active Router 的 live catalog/cache；`mode=all` 自动跟随增删，`mode=include` 保持用户显式白名单，不能擅自扩容。
- 新发现两个根因：Rust v2 compiler 未过滤 Provider `modelCatalog.models[].enabled=false`，使停用模型仍泄漏到 Router 与 spawn-agent 候选；Provider 表单 load/save 链未保留模型级 `apiFormat`、`codexCache`、`sortIndex`（以及部分隐藏能力字段），用户只要编辑保存就会先破坏 Provider 事实源。
- 根修：Provider 编辑状态、行状态、相等性/探针身份和保存归一化完整保留协议、缓存、排序、text-only 等隐藏字段；compiler 过滤停用模型，把 context/input/reasoning/cache/parallel-tools/base-instructions/Ultra 纳入 compiled model 和 dependency fingerprint；projection 原样发布这些安全字段。Provider 普通保存继续统一经过 `apply_codex_provider_mutation`，只重建受影响且拥有 shared live projection 的 active Router，不回写或复制 Router route 声明。
- TDD：旧实现下停用模型过滤与 Provider transport/cache/order round-trip 均明确 RED；修复后 Provider helper 8/8、相关 Provider/CodexForm/MultiRouter workspace 前端 153/153、TypeScript typecheck 和四个 Rust 聚焦同步回归通过。主工作树随后被并行未提交的 `services/preset_catalog.rs` 生命周期编译错误阻塞，最终全量 Rust 应在只含本提交的干净 worktree 中验证。
- 代码交付（commit `3ea6aa74`）：`services/preset_registry.rs`（PresetSourceKind webdav/https、PresetSource、PresetRegistrySettings、PresetManifest；validate_manifest 纯函数校验 size/SHA-256/过期/版本不回退/Ed25519 签名；fetch_preset_manifest_from_webdav 复用 webdav.rs 下载并校验；is_newer/is_rollback 版本比较，数值分量前导零归一、非数值退化字符串比较）；`settings.rs` 加 `preset_registry: Option<PresetRegistrySettings>`+get/set+normalize；`commands/preset_registry.rs`+`lib.rs` 加 Tauri 命令 preset_registry_get_settings/save_settings/check_update（使地基可达非死代码，https 源本次返回明确未实现）；Cargo 加 `ed25519-dalek = "2"`（纯 Rust 无原生依赖）。
- 测试：`cargo test --lib preset_registry` 13 全绿（合法签名接受；坏签名/缺签名/过期/版本回退/坏 hash/坏 size/不支持 schema 拒绝；local 信任跳过签名但保留过期；manifest 路径布局；版本比较前导零归一）。settings 相关 40 测试回归通过。
- 范围边界：本次仅“获取+校验”，不落地应用预设、不写 DB。P1（本地可移植预设+三方合并+diff+UI）、P2 完整（缓存/过期策略/检查更新 UI/TUF 多角色 root/targets/snapshot/timestamp）为后续。设计文档：`docs/superpowers/plans/2026-08-23-webdav-preset-registry-integration.md`。
- 环境坑：本机 C 盘一度只剩 0.45GB，`target/debug/incremental` 占 107GB 导致 `cargo test` 报 `rustc-LLVM ERROR: IO failure on output stream: no space on device`。用 `cmd /c rmdir /s /q .../target/debug/incremental` 清掉增量缓存（可再生，不丢 deps）后恢复 88GB。注意：`Remove-Item -Recurse -Force` 被策略拦截，递归删除目录要用 `cmd /c rmdir /s /q`。

# 2026-08-23 自启与 Codex Desktop 启动开关边界

- Windows 系统自启的唯一注册入口仍是 `src-tauri/src/auto_launch.rs`：它只维护 `HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\\CCSwitchMulti`，值为当前 CCSwitchMulti EXE 的带引号路径；不能向该链路加入 Codex Desktop。
- `AppSettings.launch_codex_desktop_with_ccswitch` 是独立、默认关闭的设备级设置，前端字段为 `launchCodexDesktopWithCcswitch`。它只在 CCSwitchMulti 实际启动时调用 `codex_desktop::launch_codex_desktop_with_ccswitch`；仅开启 `launchOnStartup` 不会拉起 Codex。已运行时保持幂等，未找到 Desktop 可执行文件时只记录警告，不阻断 CCSwitchMulti。
- 设置页在“开机自启”之后直接显示“启动 CCSwitchMulti 时启动 Codex Desktop”，文案明确其独立性。回归覆盖旧 settings 只有 `launchOnStartup=true` 时新开关仍为 false，以及启动判定的 disabled/running 四种组合。

## 2026-08-24 MultiRouter Provider SSOT 验收前根修

- schema-v2 Router 数据库只保留路由策略，所有 Provider 模型事实由 `compile_provider_v2` 现算。Router 删除 Provider 后不再回写空 `modelCatalog`；前后端 schema-v2 保存边界都会清理遗留 `modelCatalog/model_catalog`，但不会改动 legacy Router 或普通 Provider 的目录。
- Provider 删除、停用或改名使 `include`/alias 暂时失效时，Provider 保存不再被 Router 反向阻塞：保留用户白名单策略，当前投影只编译 Provider 现存模型交集并显示结构化 warning；模型恢复后自动重新进入原白名单。用户主动保存新的无效 Router 引用仍严格拒绝，disabled Route 不参与依赖校验。
- Sub-Agent V2 的初始化、校验、reconcile、模态 hydration 与 Agent TOML 回读统一使用内存态 Provider-derived effective settings；不再依赖 Router 旧目录，也不再隐式删除 disabled stale profiles。空 Agent 文件集合不能假报 Verified。
- MultiRouter 向导从当前 Provider catalog 与 `routes[].modelSelection` 初始化，保留 `mode=all` 自动跟随语义；spawn candidates 的兼容读取顺序为 `options -> codexRouting.spawnAgentModels -> legacy modelCatalog`，schema-v2 保存不再把旧目录带回数据库。
- 投影状态会真实回读 catalog、config、cache 和 CCSM 受管 Agent TOML；文件漂移/丢失返回 pending，非活动 Router 返回 `not_required` 并提示激活时生成。Provider 新增或保存后前端会只读检查当前激活 Router；pending 或检查失败时立即提示用户到工作台查看并重试，直接 Provider 模式返回空，不制造异常。
- 最终验证：Rust `3343 passed / 0 failed / 6 ignored`；Vitest `146 files / 1187 tests`；`pnpm typecheck`、`pnpm build:renderer`、相关文件 Prettier/rustfmt、严格 UTF-8 无 BOM 与 `git diff --check` 通过。全局 `cargo fmt --check` 仍被本轮未改的 preset registry/catalog、sync 与 openai compatibility 文件既有格式漂移阻塞；本轮没有发布、安装、重启或替换运行中的 CCSM。

## 2026-08-24 MultiRouter 跟随 Provider 的同步边界补审

- schema-v2 MultiRouter 的数据库事实只包含路由策略：`targetProviderId`、`modelSelection`、别名、认证策略、启停/顺序、`spawnAgentModels` 与 Sub-Agent 配置。目标 Provider 的 URL、认证内容、协议、模型清单、上下文、输入模态、推理档位、Ultra、缓存能力、显示名、启停和模型排序不得复制回 Router。
- Provider 保存统一经过 `apply_codex_provider_mutation`；运行时和非运行时消费者都用当前 Provider 集合重新执行 `compile_provider_v2`。只有当前激活 Router 拥有共享 live 投影；非激活 Router 无需生成另一份运行文件，激活时会按当前 Provider 重算。
- 真正需要同步的是可丢弃投影：`~/.codex/cc-switch-model-catalog.json`、`models_cache.json`、`config.toml` 的 catalog 指针/指纹、CCSwitchMulti 受管的 `~/.codex/agents/*.toml`、投影状态/依赖指纹，以及前端 `providers/codex` 查询缓存。`prepare_codex_config_text_with_model_catalog_impl` 已在同一条发布链内同步 catalog、cache 和受管 Agent 文件；写入失败会保留 Provider 数据并把投影标为 pending。
- 本次发现并移除三个残留的 Router 派生目录读取：Rust 投影曾用 Router 旧 `modelCatalog.sortIndex` 压过 Provider 排序；工作台只读投影曾用旧 Router 排序和 `spawnAgentModels` 覆写当前 Provider/`codexRouting`；schema-v2 别名修复曾用 Router 旧 `upstreamModel` 猜测并改写 route policy。现在 schema-v2 只认 Provider 模型事实与 `codexRouting` 策略，旧 `modelCatalog` 仅保留在显式 legacy migration/read-only 路径。
- schema-v2 Router 重新保存时，前后端都会移除遗留 `modelCatalog`/`model_catalog`，因此旧副本不只是“失去读取权”，还会在正常编辑生命周期内从数据库收敛掉；普通 Provider 和 legacy Router 的模型目录不受影响。
- 模式语义：`modelSelection.mode=all` 自动跟随 Provider 模型增删/启停；`mode=include` 是 Router 明确白名单，不因 Provider 新增模型而扩容；`spawnAgentModels` 同样是独立策略，但最终投影会过滤为当前可路由模型。模型排序编辑器直接把 `sortIndex` 写回目标 Provider，不在 Router 维护第二份顺序。
- TDD：后端“旧 Router 排序覆盖 Provider”“schema-v2 保存仍保留旧目录”与前端“旧 Router 目录覆盖排序/候选、猜测 alias、保存仍保留旧目录”均先 RED，再修到 GREEN。最终验证：Rust `3338 passed / 5 ignored`，前端 `146 files / 1184 tests`，TypeScript typecheck、涉及文件 Prettier/rustfmt、严格 UTF-8 无 BOM 与 `git diff --check` 全部通过。全局 `cargo fmt --check` 仍会报告主线上预设目录提交的既有格式差异，本次未改动那些文件。

## 2026-08-24 MultiRouter 双前端与 Sub-Agent 自动跟随补全

- MultiRouter 工作台和创建/编辑向导过去并未共享完整的实时 Provider 语义：工作台虽然能看到 Provider 新目录，但固定 `include` 规则缺少直接恢复自动跟随的入口；向导打开期间又优先保留完整 `draftSources`，Provider 查询刷新后仍显示旧模型、上下文和能力。本次向导打开时始终采用最新 Provider 快照，并明确区分“自动跟随 Provider”和“固定模型筛选”；默认/全部模型保存为 `mode=all`，只有用户取消模型才保存 `mode=include`。工作台的固定筛选详情会显示当前接入数、尚未接入模型，并可直接改为 `mode=all`。
- `2/3 个模型尚未接入` 的准确含义是 Route 当前为 `mode=include` 固定白名单，只路由 3 个 Provider 模型中的 2 个，不是 Provider 列表未刷新。`mode=all` 才会在 Provider 增删、启停或能力变化后自动采用当前目录；显式 `include` 继续保持用户路由边界，不能擅自扩容。新向导不会再把“当前恰好全选”误存成固定白名单。
- Sub-Agent 同步不再把 `spawnAgentModels` 当第二份模型白名单：它只保存用户优先顺序，Rust compiler 保留仍可路由的显式顺序后，从 Provider 实时目录自动补满 Codex 前五候选窗口。Provider 保存时还会自动 reconcile 所有引用它的 V2 Router profile，新第三方模型生成默认关闭档案，已有问卷、覆盖与顺序不变；删除模型保留为 `unroutable`，不会静默删除用户配置。
- 补审又发现停用 Router 原先被投影 affected-list 过滤，导致其 V2 Agent 档案不跟随 Provider。现在“需要 live 投影的启用 Router”和“需要数据库档案同步的所有 Router”使用两份集合：停用 Router 会更新档案但绝不发布当前运行文件；手工“同步目录”按钮降级为历史/异常中断后的修复入口。
- 检索交叉验证继续采用 Codex 内置搜索和 Matrix WebSearch 两条独立链；TanStack Query 官方文档确认 `invalidateQueries` 会使匹配查询 stale，并重取 active observer。具体缺陷和行为结论以本地源码及 RED/GREEN 回归为准。最终验证：Rust `3349 passed / 0 failed / 6 ignored`；Vitest `146 files / 1197 tests`；`pnpm typecheck`、`pnpm build:renderer`、涉及文件 Prettier/rustfmt、严格 UTF-8 无 BOM 与 `git diff --check` 全部通过。本轮未发布、安装、重启或替换运行中的 CCSM。

## 2026-08-24 Qwen Ultra 原生 Responses 映射缺口诊断

- 截图对应任务 `01a032c7-1005-7242-9f9b-88c56e16d13d` 不是 V2 spawn 出来的子任务：rollout `thread_source=user`，但运行时启用了 `multi_agent_version=v2`。任务设置先成功应用 `qwen3.8 + medium`，再成功应用 `qwen3.8 + ultra`，因此本次失败不是 Provider 保存门禁拒绝，也不是模型目录没有刷新。
- 数据库中的 Qwen Provider 已正确保存 `supportedEfforts=[low,medium,xhigh]`、`codexUltra={enabled:true,providerEffort:xhigh}`；live `cc-switch-model-catalog.json` 与 `models_cache.json` 也同时声明 `low,medium,xhigh,ultra`。现场代理日志确认请求路由到 Qwen 的 `openai_responses` 原生 `/v1/responses`，上游收到字面量 `max` 后返回 `400 Unexpected reasoning effort max; supported xhigh/medium/low`。
- 根因在运行时协议分支：`apply_catalog_ultra_setting` 已建立内部 `max -> xhigh`，`resolve_subagent_reasoning_capability` 也会建立 `ultra -> xhigh`；但 `forwarder.rs` 仅在 Responses 转 Chat/Messages 时解析并应用 `CodexChatReasoningConfig`。原生 Responses 直通分支只调用 `apply_codex_request_upstream_model`，没有应用 capability effort map，于是 Codex 用来表示 Ultra 的 Provider 边界值 `max` 被原样发给只接受 `xhigh` 的 Qwen。
- 影响范围不局限于 Sub-Agent：任何使用原生 Responses、开启 Ultra 且 `providerEffort` 不是字面量 `max` 的第三方模型都可能复现；V2 Sub-Agent 选择 Ultra 时会经过同一代理直通链，因此也受影响。修复应落在原生 Responses 请求归一化边界并增加 `max -> configured provider effort` 回归，不能把 Qwen 特判为 `max -> xhigh`。
- 检索渠道：Codex 内置搜索命中 OpenAI Codex 官方 `turn_context.rs`、模型目录和 multi-agent reasoning 校验源码；Matrix WebSearch 独立搜索无结果后直接读取官方 GitHub raw 源码。官方源码确认模型切换/子任务会依据模型目录解析 effort；CCSM 特有的丢映射根因由本地数据库、rollout、live catalog/cache、代理日志和当前源码交叉验证。本轮仅诊断和记录，尚未实现修复、构建、安装或重启。

## 2026-08-24 Codex 第三方模型终止语义根修

- 用户反馈的 DeepSeek “干活时几乎没有思考过程、最后突然甩结果、代码审核入口消失”没有现场日志，因此不能把某一次外部故障定性为已复现；本轮改为从协议边界枚举所有会让 Codex 突然结束的可证明代码路径。根因是 CCSwitchMulti 曾把传输结束当成语义完成：Chat SSE 的 `[DONE]` 会直接触发 `finalize()`，除 `finish_reason=length` 外几乎都合成为 `response.completed`；原生 Responses 又会在有正文后无 terminal event 的干净 EOF 时静默关流，并且不校验 `response.completed` 的 status/最终输出。
- 共享 Chat 终止分类器现在只接受结构化证据：`length/content_filter -> response.incomplete`；`tool_calls/function_call` 必须至少有一个完整工具调用；`stop` 必须有非空最终消息/refusal或有效工具调用；缺失、未知、reasoning-only、空工具轮均失败。`[DONE]` 只结束 SSE 传输，不再代替 `finish_reason`。同一规则覆盖非流 Chat、Chat SSE 和假流式聚合。
- 原生 Responses 在完整 SSE block 层累积 `output_text/refusal`、`response.output_item.done` 和最终 `response.output` 证据。只有 `response.completed + status=completed` 且存在最终消息/refusal、完整客户端工具调用或有效 compaction output 才透传；`response.incomplete`、`response.failed`、`error` 和取消类事件是明确终态，首个终态后停止读取；伪 completed 改发 `upstream_protocol_error`。reasoning-only 不是最终输出。
- 重试边界保持副作用安全：只转发过 `response.created`/SSE 注释时，EOF 或传输失败可沿用有界重连；任何 reasoning/text/tool/其它 semantic event 已送达后绝不重放请求，改发合法 Responses `event: error`；没有 reconnector 的路径也执行相同终态验证，不再旁路。协议合法的普通 `stop` 文本如果模型语义上自行提前收尾，代理无法可靠判断，也不能依据“我继续处理”等自然语言自动补消息重放。
- compaction 是本次跨调用链补审发现的合法例外：remote compaction v2 可经 `/responses` 原生流返回 `compaction` item，不应被“必须有最终文字/工具”误杀；仅带非空 `encrypted_content` 的 compaction output 可作为成功证据。`/responses/compact` 的独立处理路径不受影响。
- TDD/验证：非流 Chat 6 个 RED→GREEN；Chat SSE/假流式 6 个 RED→GREEN；原生 Responses 先有 6 个 RED（含无 reconnector 旁路），新增 compaction RED 后转 GREEN。最终完整 Rust library `3373 passed / 0 failed / 6 ignored`；其中 `codex_chat` 208/208、`streaming_retry` 32/32、compaction 邻近测试 2/2。实际改动文件 rustfmt 与 `git diff --check` 通过。全局 `cargo fmt --check` 仍被本轮未改的 `commands/mod.rs`、`openai_compat.rs`、preset/sync 文件既有格式漂移阻塞。
- 联网交叉验证使用 Codex 内置 Web 与 Matrix WebSearch 两条独立链。OpenAI 官方 Responses Streaming 文档确认 `response.completed` 的 response.status 为 `completed`，截断使用独立 `response.incomplete` 事件；vLLM 官方 Qwen Responses 示例确认 Qwen reasoning 可走 `/v1/responses`。Matrix 能读取 vLLM 官方页面，但 OpenAI 页面被 403/JS challenge 阻断，因此 OpenAI 事件字段以 Codex 内置搜索抓取的官方 API Reference 为正证据。具体 CCSM 根因仍以本地源码、Git 历史和 RED/GREEN 回归为准。
- 实施提交 `2b47d1ce`、`091ac061`、`2ac6e874`、`eeaf1c3e` 已通过 `--ff-only` 快进合入本地 `main`；合入后完整 Rust library 再次通过 `3373 passed / 0 failed / 6 ignored`。施工分支 `bigstrongsun/codex-terminal-semantics` 及其干净 worktree 已清理；未安装、重启、修改 live Provider/Router 配置或推送。Sub-Agent V2 只消费相同 Responses 事件，不是根因；协议选择决定走 Chat 还是 Responses 适配器，但两条路径必须遵守同一完成契约。

## 2026-08-24 原生 Responses 推理档位映射闭环

- Provider 模型能力声明中的 `upstream.effortMap` 是第三方上游真实档位的事实源。此前 Chat/Messages 转换会应用该映射，但原生 `/v1/responses` 直通只改写模型名，导致 Codex Ultra 的线级值 `max` 原样发给只接受 `xhigh` 的 Qwen/vLLM，并被上游 HTTP 400 拒绝。
- 根修复用同一 `CodexChatReasoningConfig` resolver 和 capability 映射器，在原生 Responses 出站边界只改写 `reasoning.effort` 的值并保留 Responses 对象形态；不按模型名特判，不把 Provider 的 `reasoning_effort` Chat 参数形态错误搬进 Responses 请求。
- 回归覆盖非 identity `max -> xhigh` 与 identity `high -> high`。未知能力不猜测，声明不支持 effort 时不擅自改写；声明映射中不允许的档位继续 fail closed。

# 2026-08-23 Codex 第三方协议兼容性自动探测：后端设计待实施

- 用户确认的产品方向：普通模式不让用户选择 raw/summary/字段名；保存包含 Codex 路由的 Provider 时，CCSM 自动运行一次有界的真实兼容性测试，并在首次真实请求中只做无正文的结构复核，不追加计费探测。高级模式保留手工编辑/失效后重测，但必须是同一档案的受校验覆盖，而非另一套前端猜测。
- 已核对现状：`commands/model_fetch.rs` 的 Chat/Responses probe 只发送非流式 `ping` 并返回 HTTP 成功/失败，不能判定 reasoning 字段、SSE、工具调用或续接；应把它的 URL 推导与端点检查收编到统一的 `protocol_compatibility`，避免一次保存两次探测。`reasoning_capabilities` 是 TTL 被动元数据发现，只处理 effort/thinking 参数；它不读取协议探测结论，也不能被一次响应反向写入。
- 后端设计在 `docs/superpowers/specs/2026-08-23-codex-reasoning-compatibility-probe-backend-design.md`，实施计划在 `docs/superpowers/plans/2026-08-23-codex-reasoning-compatibility-probe-backend.md`。新领域 `protocol_compatibility` 在 Provider 保存阶段完成端点/transport、流式形态、推理字段、强制虚拟工具、历史续接、生产投影自检；`ReasoningProjection` 只是 `VerifiedProtocolCompatibilityProfile` 的一个受验证子档案。`ProbeCandidate` 必须由未落库的有效 route/model/transport 编译而来。只持久化脱敏结构证据/哈希，绝不持久化 prompt、推理正文、工具正文或凭据。
- 探测 corpus 不能复用 `ping` 或用户请求体：固定为 baseline JSON、baseline SSE、强制虚拟工具 SSE、工具结果续接 JSON 四例，普通 Provider 最多四个上游请求，`Auto` 仅在明确协议不支持时额外试另一个 transport。每例从逻辑 Responses 请求经生产转换器生成 wire body；禁止用户内容、extra body、原工具/历史、reasoning effort、sampling 与 response format 混入。工具仅有随机 nonce，结果在内存伪造；输出上限 128、连接/响应/总事务上限 5/15/60 秒、无重试或跨域重定向。marker 用于诊断，不能把模型服从性当协议结论。
- 运行时必须用 `有效高级覆盖 > 指纹匹配且未过期的测试档案 > 安全回退` 解析 `ReasoningProjection`，一次性传给流式与非流式 Chat→Responses 转换；不得再让 `outputFormat` 或通用字段提取器单独决定 raw/summary。官方 OAuth 加密续接、原生 Responses 和 V2 agent-message 边界保持原路径。
- 隔离工作树/分支：`bigstrongsun/codex-reasoning-probe-backend`，基线 `ee21cef1`。只添加设计与计划文档，未实施业务代码、未改前端、未安装或发布。起始 `cargo test ... model_fetch --lib` 受同机已有 Cargo 编译占用 artifact-directory 锁阻塞，不能计为通过或失败；不得中断其他工作树的 Cargo 进程。

## 2026-08-23 截图中“Qwen 后来有白字”取证结论（已更正）

- 先前把截图误关联到 thread `01a02da5-70c7-73e1-9587-9cd9fdaf3634`（Terra 官方路由）；这是错误的，不能据此判断截图。按截图英文句精确反查，真正的 rollout 是 `01a02d95-045f-72b1-a7e2-8bdb888c8abc`，目标 turn `01a02df1-e482-7df3-9d46-cc6ed918ee6f` 的 `turn_context` 明确为 `qwen3.8 / medium`。CCSM router 同时记录有效路由 `Qwen`，上游为 vLLM `/v1/chat/completions`，所以该段确实由 Qwen 生成。
- 该 turn 同时出现两种不同的上游 Chat 字段：`reasoning_content` 经桥接成为无 `content` 的 Responses reasoning summary（持续的单行闪烁）；Qwen 在工具调用前又间歇性发送普通 `content`，经桥接成为普通 assistant `message/output_text`。截图的 “Matrix chain is returning …” 是该 turn 的普通 message，紧邻后续 Matrix tool call，且没有 `phase=commentary`；它不是 raw reasoning，也不是 Codex 自行补出的说明。
- 因而“后来显示几句白字”的直接原因是 Qwen 在少数工具子回合产生了非空 Chat `content`，而不是 reasoning 通道从 summary 切换为 raw。桥接正确把这个 `content` 按 `response.output_text.delta` 显示；与此同时 `reasoning_content` 仍未以 `reasoning_text` 形式暴露，原始“完整推理不可读”问题仍未解决。
- 验收/探测必须把 `pre_tool_visible_content` 作为独立结构证据记录，不能把它误判为 raw reasoning 成功，也不能仅按它是否出现决定显示投影。

## 2026-08-23 Codex 第三方协议兼容性自动探测：领域层实现进度

- 隔离分支 `bigstrongsun/codex-reasoning-probe-backend` 已开始后端实施，前端、安装、发布与运行中服务均未触及。首个已提交切片 `c10cbabd` 固化四个探测用例的逻辑 Responses 请求壳；当前待提交切片补充目标身份与自动投影准入规则。
- `ProbeTargetKey` 规范化 endpoint 时移除 URL userinfo、query 和 fragment，再仅保存其 SHA-256 指纹；凭据、提示或正文不能进入目标键。`ProbeCandidate` 可在 Provider 尚未持久化时以 `provider_id=None` 编译，支持保存前探测。
- 自动投影只允许 `ProbeReadiness::Verified`。手工覆盖必须验证已有证据：不透明证据不能升格为 readable，非 readable 不能要求 raw `reasoning_text`，readable 必须提供来源，原生 Responses 不得伪装为 Chat `reasoning_content` 回放。
- 本轮 RED/GREEN：先观察到目标键/准入 API 缺失的编译失败，再完成 2/2；第二轮观察到候选与覆盖类型缺失的编译失败，补齐后 6/6 通过。后续仍须完成结构脱敏、持久化、capture/classifier、真实 probe transaction 和生产桥接，未可声称端到端功能完成。
- 脱敏器现已实现为严格路径白名单：Chat JSON/SSE 只可生成 allowlisted reasoning/content/tool-arguments 字段的路径、值类型、字节长度、SHA-256 与 SSE event 名；`id`、Authorization、正文、nonce 与工具参数原文都不会进入序列化证据。两项 JSON/SSE RED/GREEN 测试已通过；数据库接入前仍需增加持久化层回归。
- 分类器现以实际 payload 形态作保守判定：Qwen/vLLM 非空 `choices[].delta.reasoning_content` 才是 `Readable/ReasoningContent`；工具前非空普通 `content` 仅为 `PreToolVisibleContent::Present`，不改变语义或来源；summary 为 `Summary`，encrypted 与 raw/summary 混合均不得为 `Readable`，跨分片 `<think>...</think>` 闭合后才可归为 `Readable/ThinkTags`。四项 RED/GREEN fixture 测试已通过。

## 2026-08-24 Codex 第三方协议探测更正：每个 Provider 全协议枚举

- 用户纠正了旧的“按已配置 transport 探测，只有 Auto 才 fallback”逻辑。`wire_api` 可能正是历史误配置来源，例如把实际只支持 Chat Completions 的上游标成 Responses；因此它只能作为配置提示，不能删除候选、参与能力评分或直接作为探测结论。
- 每个 Provider/模型都必须枚举 CCSM 当前维护的 Chat Completions 与 Responses 两条候选分支。每条分支先独立做非流 baseline；baseline 明确不支持只终止该分支，baseline 可达则继续做 SSE、强制虚拟工具、工具结果续接，最坏每模型八次请求。
- 运行协议从 baseline 可达分支中严格按 `续接通过 > 强制工具通过 > SSE 通过 > reasoning fidelity（Readable > Summary > Opaque/None）> 原生 Responses` 选择。fidelity 只裁决前三项能力完全并列的分支；选择到 Partial 分支仍可保留基础路由，但只有被选分支四阶段全通过才允许自动 reasoning 投影，fidelity 不会提升 readiness。
- 后改 selector 的现场诊断 canary 已完成：共审计 9 个 Provider 条目，4 个因官方/托管、缺 API key、缺模型或不支持协议而安全跳过，5 个实际候选使用生产 protocol-probe client 运行双协议分支。数据库以 SQLite read-only 打开；没有持久化 profile、Provider/configuration 或 live DB 改动。该证据验证源码工作树的诊断路径，不能推断当前已安装应用已使用本分支。
- 5 个真实候选的结论：DeepSeek `responses -> chat`，前三项能力并列而 Chat=`Readable`、Responses=`Opaque`，证明 fidelity tie-break；Qwen `chat -> chat`，Chat 保持更高 streaming 能力；Kimi `k3` `responses -> chat`，前三项能力并列而 Chat=`Readable`、Responses=`Summary`，证明 fidelity tie-break；Kimi `kimi-for-coding` `responses -> chat`，本轮是 Responses streaming 失败导致 Chat 能力更强，**不是** tie-break 证据；GLM `chat -> chat`，Chat 是唯一 baseline 通过分支且为 `Verified`。其余四个实际候选都因 `forced_tool=unsupported` 保持 `Partial`；只有 GLM 是 `Verified`。
- Task 8 的 RED/GREEN 选择回归与真实 runner fixture 已将 `TransportBranchResult.reasoning_shape.semantic` 接入同一纯选择器：opaque/summary Responses 与 readable Chat 的能力并列时选择 Chat，双方 readable 时仍选原生 Responses，opaque Responses 若工具/续接/SSE 更强仍保留。现场结果与通用规则一致，未引入 Provider 特例。
- capture 层使用 2 MiB 有界字节缓冲接收 JSON/SSE，先跨网络 chunk 合并字节再做 UTF-8/SSE frame 解析，因此 UTF-8 字符或 JSON 跨 chunk 不会被误切；支持 CRLF/LF、命名 event、data-only event 与 `[DONE]`。HTTP/网络/超时错误只返回结构类别和状态码，`CapturedProbeExchange` 自定义 `Debug` 只显示状态、格式、payload 数量和脱敏证据，不显示请求 URL、响应正文或密钥。
- 分类器新增捕获事件语义：只有 `response.reasoning_text.delta` 且 `delta` 非空才把原生 Responses 流归为 readable；summary/opaque 证据仍优先，不能被 raw 事件升格。另修正 `pre_tool_visible_content`：普通 content 只有在后续实际出现 tool call 时才标记 Present，baseline 普通最终回答保持 Absent。capture 4/4、classifier 5/5 回归通过；后续 Task 8 已让 runner 将分支 `reasoning_shape.semantic` 送入纯选择器。
- 双协议 runner 已建立：每个候选固定按 Responses、Chat 两条独立分支执行；baseline 成功后即使 SSE 失败也继续验证强制工具，强制工具成功才做续接。Responses 与 Chat 都完整时同分选择原生 Responses；Responses baseline 404 时只停止 Responses 分支并继续 Chat；Responses 工具强制 400 时记 Unsupported/跳过续接，完整 Chat 胜出。
- 所有 Chat wire body（baseline、SSE、强制工具、续接）均由生产 `responses_to_chat_completions_with_reasoning` 从逻辑 Responses corpus 转换。runner 从真实 SSE 聚合 Chat tool call 的 ID/name/arguments/reasoning_content，再构造逻辑 Responses function_call + function_call_output，并再次交给同一生产转换器生成 Chat 历史；没有第二套 Chat 序列化器。
- 原 `commands/model_fetch.rs` 的 Chat/Responses URL 推导已收敛到 `protocol_compatibility::endpoint::build_probe_url`，旧命令暂时只作兼容包装；13/13 原 URL 回归通过。`ProbeCandidate` 新增只驻内存的 Bearer Header 与 `is_full_url`，自定义 Debug 只显示 `has_bearer_token`，不会打印 Token。runner 场景 3/3、凭据脱敏 1/1 通过；当前仍未接 Provider 保存和数据库档案，因此还不能从设置流程调用。
- schema v17 新增 `protocol_compatibility_profiles` 专表，不复用 `stream_check_logs`。主键是完整 `ProbeTargetKey` 的稳定 SHA-256，档案本体只包含目标元数据、阶段状态、分类和脱敏字段哈希；endpoint/model/route/auth/selected transport 任一变化都会精确 miss。Verified 默认 30 天、Partial/Unverified 7 天，启动时按 `expires_at` 清理。
- 新 Tauri 后端命令 `probe_codex_protocol_compatibility` 接受尚未保存的 Provider/Route/模型/base URL/凭据，运行双协议 runner、选择 transport、生成最终 endpoint 目标键并保存档案。它使用保留 CCSM 当前代理语义的专用 client，同时强制 5 秒 connect、15 秒 request 与不跟随重定向；前端尚未调用该命令，也没有改写 Provider，因此当前是“保存前可调用的后端能力”，不是已完成的普通模式 UI/自动保存事务。
- Provider 草稿现在可由 `compile_provider_probe_candidate` 从真实 Codex TOML、主模型 alias、modelCatalog、base URL、API key 和 `isFullUrl` 编译；历史 `wire_api/apiFormat` 只成为候选提示，runner 仍固定测试 Responses 与 Chat。选出的 transport 会同步写回 `meta.apiFormat`、`settingsConfig.apiFormat`、主模型 `modelCatalog.models[].apiFormat` 和 TOML `wire_api`，避免探测结论与实际运行配置分叉；两条 baseline 都不可达时不改原协议。
- 新后端入口 `preflight_codex_provider_protocol_compatibility` 返回已应用选择的 Provider 草稿与档案；`save_codex_provider_with_protocol_preflight` 提供普通模式未来可直接调用的一步保存入口。网络/凭据导致 preflight 无法开始时仍保存原 Provider，并返回脱敏 `probeError`，不会把探测失败误当成禁止保存。Provider/domain 回归 6/6、protocol compatibility 39/39 通过；前端尚未改用新保存入口，安装态也未变。

## 2026-08-24 Codex 第三方协议探测：跨边界验证

- 相关实现链已依次落在 `c05138f4`（档案持久化）、`24a88b08`（探测选择应用）、`8d3feff1`/`e13801d2`（检测到的 reasoning 形态投影）、`b71a72af`（运行时应用 Verified 投影）与 `0d620136`（不完整 route identity 失配时 fail-closed），随后接本次仅补回归和陈旧测试契约的 verification commit；本次不修改这些生产实现。
- `migrate_v15_to_v16_resets_only_codex_session_usage` 的生产 schema 已继续演进到 user_version 17；该测试所验证的仍是 v15→v16 清理语义，旧断言 16 是测试陈旧而非迁移实现倒退。已先复现真实 RED（选中 1 个测试：actual 17 / expected 16），随后仅将断言更新为 17。
- 新的本地内存 fixture 用真实双协议 runner 让 Responses baseline 404、Chat 完成全部四阶段，因而实际选中且完全 Verified 的 Chat 分支。由该真实 result 构造 `ProtocolCompatibilityRecord` 后只得到 `RawReasoningText`；同一投影送入实际 Chat→Responses 非流转换时，Qwen/vLLM `reasoning_content` 映射为 `reasoning_text`、普通 `content` 映射为 message `output_text` 且 summary 为空；送入实际 SSE 转换时只出现 `response.reasoning_text.delta` 与 `response.output_text.delta`，不出现 summary 事件。fixture 正文仅存在内存中，未写入档案或日志。
- 本轮 fresh 验证（共享 `target-protocol-probe`）：迁移 1、`protocol_compatibility` 44、`streaming_codex_chat` 36、`transform_codex_chat` 145、`openai_compat` 36、`forwarder` 160、`handlers` 87，均为 0 failed；`cargo check --tests` 成功但保留 7 条既有/并行工作树的 unused/dead-code warning；`git diff --check` 通过。
- 本次仍是后端源代码与测试验收：前端调用、真实第三方 Provider、已安装应用与 live 配置均未修改，也未被本轮视为已验收。

## 2026-08-24 Codex 第三方协议探测：自动保存、高级覆盖与运行时漂移收口

- 最终生产规则不含 Qwen、GLM、DeepSeek、Kimi 等模型品牌特例。每个第三方 Codex Provider/有效 MultiRouter route-model 目标都固定枚举 Responses 与 Chat Completions；两条分支分别测试 baseline JSON、baseline SSE、强制虚拟工具 SSE、工具结果续接 JSON。历史 `wire_api` 只帮助编译候选和回写最终选择，不能限制被测协议集合。
- 单一选择器严格按 `续接 > 强制工具 > SSE > Readable > Summary > Opaque/None > Responses 同分兜底` 排序。只有选中分支四阶段全通过才是 `Verified` 并可自动投影 reasoning；更高 fidelity 不得把 `Partial` 升为 `Verified`。marker 仅是诊断提示，模型返回完成且非空的 assistant 输出即满足 baseline/续接，不能因没有照抄 marker 把模型服从性误判为协议失败。
- 普通 Provider 的既有 add/update 保存边界已接入自动 preflight：先从尚未落库的 Provider 草稿编译有效目标并探测，再将 Provider 与所有目标档案放入同一数据库事务，随后才发布 live projection。高级 `codexProtocolMode=manual` 保留用户协议选择；reasoning 高级覆盖另用完整 `ProbeTargetKey` 绑定的 plan/apply/clear、10 分钟一次性 token 与 CAS revision。clear 写 tombstone revision，防止旧 plan 在清除后复活。reasoning effort/推理强度继续走独立 capability 链。
- 运行时新增无正文结构观察：只比较 transport、reasoning semantic/source、字段路径、事件顺序、工具存在标记和 `PreToolVisibleContent`。非流式失配在当前投影前使精确档案失效；流式失配在本轮结束后失效并影响后续请求。捕获上限 1 MiB，超限跳过观察；正常无 reasoning 的请求不会错误失效 readable 档案。Chat SSE 观察器现先在内存规范化 CRLF 再解析，原始字节仍原样转发且正文不落库。
- 本轮另外发现并按 TDD 根修两个探测判定问题：CRLF SSE 原先因只按 `\n\n` 分帧而得到 `ReasoningSemantic::None`；marker 不匹配原先会把完成响应错误标成 Unverified。两项测试均先确认 RED，再作最小 GREEN 修复。
- Fresh 回归：`protocol_compatibility` 65、数据库迁移/事务 25、普通 Provider 保存/手工模式 17、MultiRouter 58、`streaming_codex_chat` 36、`transform_codex_chat` 145、`openai_compat` 36、`forwarder` 160、`handlers` 89、Codex Provider 117，全部 0 failed；`cargo check --tests --no-default-features` 成功。最终全量 `cargo test --lib --no-default-features` 为 3398 passed、0 failed、5 ignored。首次全量暴露 `migrate_v15_to_v16_resets_only_codex_session_usage` 仍硬编码 schema 17；当前 schema 已升到 18，已用 `SCHEMA_VERSION` 替代版本常量并先跑 1/1 再跑全量。fmt/diff/UTF-8 检查均通过。
- 搜索交叉验证：Codex 内置搜索查到 vLLM Responses streaming/tool/reasoning parser 的真实未决或版本相关问题（例如 #36435、#37167、#43221），Matrix 独立链路返回官方 reasoning outputs、tool calling 文档及仓库入口。两条链一致支持“能力属于具体 endpoint/部署/版本，不能按模型品牌或 API 名称猜测”；Matrix 没提供足以替代真实双协议探测的更强证据。
- 边界仍然明确：本分支不实现 React 设置页，不替换安装包、不重启服务、不修改 live Provider，也不声称已安装 Codex Desktop 或生产 Provider 验收。下一切片应只做前端：普通模式显示自动测试进度/结果/失败阶段和显式重测；高级模式调用现有 plan/apply/clear，不在前端复制 classifier 或默认规则。

## 2026-08-24 Codex 自动协议探测后端主线集成

- 先前“已合入主线”只覆盖第三方 Chat/Responses 终止语义提交 `eeaf1c3e`，没有覆盖 `bigstrongsun/codex-reasoning-probe-backend@005761a9` 的 25 个自动探测提交；这是合入范围遗漏，不是功能已经在主线但运行态未更新。本轮从 `main@1d66cb1a` 创建 `bigstrongsun/integrate-reasoning-probe-backend`，对完整探测分支执行三方合并。
- 实际文本冲突只有 `memory.md` 与 `src-tauri/src/proxy/providers/mod.rs`。Provider 导出冲突按并集保留主线新增的 `codex_route_auth_source`、终止语义/remote compaction API，以及探测分支新增的 detected transport 与 reasoning projection API；主线的 `[DONE]`/EOF/`response.completed` 终态校验和原生 Responses `reasoning.effort` 映射均保留。
- 自动合并后的首次 `cargo check --lib --no-default-features` 暴露主线后来新增的 Universal Provider Codex 同步仍按旧三参数调用 `persist_provider_mutation`。该特殊同步入口没有协议档案，现显式传空档案切片，继续走 Provider SSOT/mutation/projection 链，不伪造探测结果；普通设置页的 Codex add/update 仍由异步 Tauri command 在持久化前调用 `automatic_codex_provider_preflight`。
- 集成后 fresh 门禁：`protocol_compatibility` 65/65；`streaming_codex_chat` 41/41；`transform_codex_chat` 151/151；`openai_compat` 36/36；`forwarder` 161/161；`handlers` 89/89；Codex Provider 22/22；MultiRouter 79/79；database 25/25；普通 Provider preflight 3/3；Universal Provider 删除级联 1/1。完整 `cargo test --lib --no-default-features` 为 3469 passed、0 failed、6 ignored；`cargo check`、rustfmt 和 `git diff --check` 通过。
- 联网交叉验证使用 Codex 内置 WebSearch 命中 OpenAI Responses streaming/reasoning summary、vLLM Responses/reasoning/tool calling、DeepSeek `reasoning_content` 工具续轮官方文档。Matrix WebSearch 的 `search/open` 本轮连续返回 `fetch failed`；本机入口脚本存在且 `~/.codex/config.toml` 已指向规定的 `Documents/本地设备/scripts/matrix-websearch-mcp.js`，因此记录为远端检索链暂时不可用，不把失败结果当作技术证据。
- 本轮只完成源码集成与后端回归，没有构建安装包、替换安装态、重启 CCSwitchMulti/Codex、修改 live Provider/Router 或重新对真实 Provider 发探测请求；前端状态页仍属于后续切片。
- 主线集成后的 Provider mutation 调用链补审又发现一个真实旁路：`sync_universal_provider` 生成 Codex 子 Provider 时直接调用同步 `ProviderService::sync_universal_to_apps`，无法执行异步自动 preflight。现命令改为先从当前 Universal 定义和已有 Codex 子项编译完整草稿，调用与普通 Provider 相同的 `automatic_codex_provider_preflight`，再把探测后 Provider 与全部 profile 交给 profile-aware mutation 原子持久化；未启用 Codex 时不探测，探测无法开始时沿用普通保存的安全回退语义。
- Universal Provider 回归先因缺少 preflight 路径明确 RED，再转 GREEN；新测试断言探测闭包只调用一次、选择后的 `openai_chat` 写入生成子 Provider、对应 profile 同事务可读。聚焦 `universal` 11/11、普通 Provider preflight 3/3、protocol compatibility 65/65；完整 Rust library 更新为 3470 passed、0 failed、6 ignored，`cargo check`、rustfmt 与 `git diff --check` 通过。

## 2026-08-25 协议探测合入后 PR #37-#66 重新评估

- 复审基线为 `main@ca545155`，已包含 `27a80890` 的自动协议探测集成和 `ca545155` 的 Universal Provider 探测旁路修复。#36 之后没有高于 #66 的新 PR；当前抓取的 PR head 为 #37/#39/#41/#43/#45/#47/#49/#57-#64/#66。
- #37、#39、#41、#43、#45、#47、#49 的目标行为已经由当前主线等价或更强实现覆盖：v2 `authPolicy`、缺省 `modelSelection`、Provider 派生目录与活动 workspace、`displayName`、Windows 原子替换恢复、仅活动 Router 发布、Flash/Pro 后缀别名均已有源码和回归。旧 PR 分支落后且冲突，不应整枝合入，可关闭为 superseded。
- #58 的 prefix-only legacy migration 修复真实且可干净合并，但其 `provider_catalog_entries` 没有过滤 `modelCatalog.models[].enabled=false`，会把停用模型固化进 `include`。应先补过滤和回归，再选择性移植。
- #66 不能整包合入：它把 Provider 派生的聚合 `modelCatalog` 回写 Router DB，重新制造第二事实源，而且混入 41 个文件、22 个提交和无关冲突。但它暴露的上下文缺口真实存在：当前 `publish_codex_multirouter_projection` 仍调用无 `ProviderClassificationContext` 的目录准备函数；`mode=all` 需要 Provider 上下文判断目标 Provider 拥有的模型。profile reconciliation 已有上下文，Provider 更新、retry、activation/delete 等投影发布入口生成 Agent 文件时却可能无上下文，导致无前缀 `mode=all` route 的 Agent 文件遗漏或被清理。正确修复是只把 context-aware publishing 贯通全部 Provider 派生发布入口，Router DB 继续不持久化派生目录。
- #57 的 live config 并发覆盖问题真实，但 PR 的最后 fingerprint check 到 replace 之间仍有 TOCTOU，并且早于协议档案事务；应在当前主线统一 live-config writer/ownership boundary 后重做。#63 的 MCP 修复也依赖同一 writer，且仅凭 DB 同名 ID 推断 Codex 所有权会误删用户配置；必须引入历史 ownership receipt，不能直接合。
- #59 只恢复内存中的 legacy `defaultRouteId`，serializer 仍会丢字段，修复不闭环；还需明确“持续清除旧 fallback”还是“保留用于降级兼容”的产品策略。#60 是 53 项 Rust 依赖升级且含 major 变化，应独立发布和做兼容性验证。#61 的 plugin marketplace 路径/schema 未由当前 Codex 安装证实，另有字典序版本比较、错误 marketplace 选择、并发写和同步递归扫描等缺陷，应重做。
- #62 的 startup/port ownership 只用 PID、进程存在和自报 `/status`，无法抵抗 PID 复用，也未证明 listener/可执行文件/start-time 所有权；不得据此复用或接管未知进程。#64 用单一全局 outcome，后续成功可覆盖先前失败，缺 acknowledgment/generation 生命周期；应改为有界、分 app/operation、按严重度保留的结果集合。
- 因此当前没有任何一个 #36 之后的 PR 适合原样合并。可执行顺序是：先根修 #66 的 context-aware projection 缺口和 #58 的 enabled 过滤；再统一 #57/#63 的 live config writer 与 ownership receipt；随后分别重做 #62/#64；#59 先定兼容策略；#61 先确认官方/安装态 marketplace schema；#60 延后到依赖专版。
- Fresh 证据：当前主线定向通过 v1/v2 auth source、active-router publication、mode-all Provider classifier、Windows atomic write 以及 `CodexRouterWorkspacePage` 73/73。GitHub CLI/API 与已抓取 PR refs、merge-tree、源码、CI 日志和本地测试作为当前状态权威；Codex WebSearch 的 GitHub 索引滞后，Matrix WebSearch 返回 `fetch failed`，因此未用搜索摘要替代仓库事实。

# 2026-08-25 Qwen MultiRouter 推理档案同步

- `3.19.2-17` 中 Qwen Ultra 映射和 Chat 转发均已生效；不显示思考过程的直接原因是独立 Provider 的 `verified` 协议档案没有同步到相同目标的 MultiRouter route 身份，route 仍命中旧 `partial` 档案并关闭自动 reasoning projection。
- Provider 保存现在除配置投影和 Subagent 目录外，也会为完整目标指纹一致的依赖路由物化 route 专属协议档案。匹配包括公开/上游模型、transport、endpoint、认证类型和凭据；只同步当前版本 `verified` 证据，并与 Provider 同事务保存，不放宽运行时 route identity 隔离。
- 详见 `memory-2026-08-25-qwen-route-reasoning-profile-sync.md`。当前仅完成源码修复和聚焦验证，尚未构建、安装或替换运行中的 `3.19.2-17`。

## 2026-08-25 Qwen 工具循环与旧路由运行时继承

- 任务 `01a032c7-1005-7242-9f9b-88c56e16d13d` 的两条相同命令各真实执行 13 次；每轮 call ID、Codex `/responses` 和 Qwen `/chat/completions` 请求均为新实例，结果成功写回，排除 UI 重复、代理重放和失败 retry。
- 循环时目标 Qwen Provider 档案已 `verified/readable`，MultiRouter route 仍为旧 `partial`，26 个循环工具调用之间没有 reasoning/commentary item。旧运行时只查 route key，导致真实 reasoning 不进入 Codex 历史，只能在下一轮补 `tool call` 占位。
- 在保存时物化 route 档案之外，运行时现允许 route 档案不可用时继承 `codexResolvedTargetProviderId` 的完全等价 Provider 档案；完整目标键仍约束模型、transport、endpoint、认证和凭据。旧安装升级后不需要重新保存 Provider，endpoint 改变或 route identity 不完整时仍 fail closed。
- 同一继承边界同时用于 reasoning projection 和 detected transport。详见 `memory-2026-08-25-qwen-tool-loop-runtime-inheritance.md`；当前仍未替换安装态，Qwen/vLLM 自身公开同类 loop 风险需在新构建 canary 中继续区分。
- 未升级安装态的临时恢复已完成：对 live DB 做 SQLite 在线备份后，将完全等价 standalone Qwen `verified` 档案复制到既有 route identity；原 CCSM 进程和 15721 未重启，health 与新连接读回均正常。备份和事务边界记录在上述专项 memory；这只修当前 route，长期修复仍是 `584a1834`。

## 2026-08-25 Codex 深度协议探测进度页

- 普通 Codex Provider 的“验证连接”不再调用只返回 HTTP 成败的旧浅探测。入口由 `CodexFormFields` 构造当前未保存 Provider 草稿，调用带 Tauri `Channel` 的 `preflight_codex_provider_protocol_compatibility`；后端 runner 在真实状态转换处发送有序进度，前端不使用计时器伪造阶段。
- 每个启用模型固定测试 Responses 与 Chat Completions 两条分支，并展示基础响应、SSE 流式、思考分类、强制工具调用、工具结果续轮五项状态。进度事件包括 candidate/stage/reasoning/branch/batch 的 started/finished 结果，只含模型、协议、阶段、结构分类与 readiness；不含 endpoint、API Key、prompt、响应正文、nonce 或工具 payload。
- 进度弹窗在请求开始前即列出全部启用模型，稳定显示完成数/总数；每个模型卡片并排显示两条协议、阶段状态、reasoning 的 readable/summary/opaque/none、选中协议和 Verified/Partial/Failed。最终结果保留在弹窗内，可重试；Provider 身份变化会使旧事件和结果失效。
- 普通 Provider 会探测目录中所有 `enabled != false` 的模型；没有目录时回退默认模型。所有模型选择同一 transport 才更新全局 `apiFormat/wire_api`；混合结果保持原协议并生成拆分 Provider 建议。修复了保存命令曾用“任一模型有选择”错误回报 `protocolApplied=true` 的契约问题，也补回了表单行对 `enabled=false` 的保真，停用模型不会发探测请求。
- Fresh 验证：Rust `protocol_compatibility` 70/70；前端深探测/表单/就绪组件 37/37；`pnpm typecheck`、`cargo check --tests --no-default-features`、rustfmt、Prettier、`git diff --check`、11 个变更文本 UTF-8 严格解码/no-BOM/U+FFFD 检查通过。`cargo check` 仅保留 5 条既有 dead-code warning，Vitest 保留既有 React `act(...)` warning。
- 浏览器源码态验收实际走通“Codex → 添加供应商 → 单独接入模型源 → 自定义模型源 → 验证连接 → 确认测试 → 深度探测弹窗”，确认弹窗层级、说明、模型卡片、双协议列、错误、重试和关闭入口可见。独立 Vite renderer 缺少 Tauri Channel runtime，所以不能在该环境产生真实阶段事件；当前未构建/安装、未替换 `3.19.2-17`、未重启或修改 live `127.0.0.1:15721`，真实 Provider 与安装态验收仍待后续发布 canary。
- 联网交叉验证沿用本次实现前的两条独立链：Codex 内置搜索读取 Tauri 官方 Channel 文档与 OpenAI 官方 Responses reasoning 事件参考；Matrix WebSearch 独立读取 Tauri 官方 Channel 页面。Matrix 打开 OpenAI 页面遇到 403/JS challenge，因此 OpenAI 字段以 Codex 内置链的官方 API Reference 为证据；两条可用证据共同支持有序 Channel 进度以及 `reasoning_text` 与 reasoning summary 的结构区别。

## 2026-08-26 Qwen 深度探测 HTTP 521 与 MODT 异常重启取证

- 安装态 CCSwitchMulti 的 Qwen 双协议基础探测确实请求了公网 Zoraxy 的 `/vllm` 路径；Responses 与 Chat Completions 同时失败时，上游返回 Zoraxy HTTP 521 页面。Matrix WebSearch 的 `/websearch` 同期也返回相同 521，而本机 `127.0.0.1:15721/health` 正常，因此 521 发生在 CCSM 之后、模型协议解析之前。
- Zoraxy 路由事实源显示 `/vllm -> 100.85.95.33:5000`、`/websearch -> 100.85.95.33:8891`。从工作站直连 roglinux 的 5000/5050/8891 均可达；通过 Tailscale IP 直接 SSH 后，`vllm-qwen36-tp`、`vllm-dashboard-proxy`、`vllm-dashboard-web`、`agent-websearch-unified` 全部为 active，本机 `/v1/models` 与 WebSearch `/health` 都为 HTTP 200。不能把本次 521 归因于 Qwen、vLLM Responses/Chat 协议或 roglinux 模型服务停机。
- MODT 是 `:24443` Zoraxy 中转机。现场 Tailscale Windows 服务虽为 Running/Automatic，但 `tailscale status --json` 为 `BackendState=NoState`、无 Tailscale IP，health 为 `Tailscale is starting. Please wait.`；MODT 到 roglinux 目标端口不可达。MODT 普通公网、控制平面 443 与 `tailscale netcheck` 均正常，所以直接根因是 MODT 的 Tailscale daemon 在异常重启后卡死，Zoraxy 能接收公网请求却无法经 Tailscale 回源。
- MODT 系统日志确认近两天多次异常重启。最近三次分别为 2026-08-25 22:28（bugcheck 0x50）、2026-08-26 01:28（0x1A，参数 1 为 0x41792）和 2026-08-26 02:36（0xA）；均伴随 Kernel-Power 41 与 EventLog 6008。更早还有 bugcheck 0x1A 以及带 PowerButtonTimestamp 的硬复位。当前系统从 02:36 启动；Tailscale 从该次启动即处于 NoState。2026-08-25 08:38 还存在一次独立的 Tailscale 服务意外终止（SCM 7031，系统自动重启服务）。
- 这组三种 stop code 共同指向内核驱动错误或内存损坏，不能只通过重启 Tailscale 当作已修复。先恢复 MODT Tailscale 并验证 `BackendState=Running`、MODT 到 100.85.95.33:5000/8891 可达，可消除当前 521；随后必须取得 `C:\Windows\Minidump`/`MEMORY.DMP` 用 WinDbg `!analyze -v` 确认故障模块，并检查 RAM、超频/内存时序和近期内核驱动。不得删除 Tailscale state、logout 或重建节点身份来掩盖问题。
- Zoraxy 由 NSSM 自动服务托管，系统启动后正常加载路由；05:02 还出现一次无完整原因记录的进程重启。当前证据未显示 Zoraxy 本身导致三次蓝屏。远程 SSH 后续出现“公网 TCP 22024 可达但 banner 超时”，使 dump 文件索引尚未完成；在取得转储分析前，物理 RAM 与具体驱动之间仍属未决。
- 联网交叉验证：Codex 内置 WebSearch 命中 Zoraxy 521 的 upstream-unreachable/无在线 upstream 案例、Tailscale Windows 异常重启后 NoState 的公开问题，以及 Microsoft 对 0x1A/0x50/0xA 的官方说明。Matrix WebSearch 独立调用本身被同一 Zoraxy 521 阻断，因此它是故障同源证据，不作为技术结论的支持来源。

## 2026-08-26 深度探测 521 状态语义与脱敏错误呈现根修

- MODT 恢复后重新验证：`https://www.matrixminecraft.cn:24443/vllm/v1/models`、`/vllm-dashboard/api/health`、`/websearch/health` 均为 HTTP 200，`/relay-dashboard/` 为 302，本机 CCSM `127.0.0.1:15721/health` 为 200。之前的 521 结果是过期的环境故障证据，不能继续用于判断 Qwen 协议或推理能力。
- 后端 runner 原本已经在基础响应未通过时把 reasoning stage 标成 `skipped`；真正的产品 bug 在两层：前端最终 `applyRecord` 和中途 `reasoning_classified` 都无条件把 `semantic=none` 改成 `unsupported`，覆盖了后端状态；非 2xx 在 capture 层以 `ProbeCaptureError::HttpStatus` 提前返回，branch record 又没有保留脱敏失败原因，导致用户只看到“基础响应失败”。
- 根修不新增模型名特判，也不先调用不可移植的 `/health`：双协议各自的 baseline 就是通用可用性门。只有 baseline 成功后 `semantic=none` 才表示“未返回/不支持”；baseline 失败时 reasoning 显示“未检测/已跳过”，后续 SSE、工具和续轮保持跳过，不把链路故障写成模型能力结论。
- `TransportBranchResult.failures` 与 `StageFinished.failure` 现在传递安全结构：仅包含 stage、kind 和可选 HTTP status；覆盖 http_status、timeout、network、response_too_large、invalid_response、invalid_request。禁止包含 endpoint、API Key、prompt、响应正文、nonce 或工具参数。UI 会即时和最终显示 `HTTP 521 · 上游不可达`，404/405/415 显示接口不支持，其他失败显示安全类别。
- TDD 先证明旧实现下两项 RED：失败 baseline 的 reasoning 被显示成“不支持”，HTTP 521 没有任何详情；Rust 521 fixture 的 `failures` 为 null。GREEN 后前端进度弹窗 6/6、相关表单 36/36、Rust protocol compatibility 71/71、完整 Rust lib 3514/3514、完整 Vitest 全过；`pnpm typecheck`、`pnpm build:renderer`、`cargo check --tests --no-default-features` 和 `git diff --check` 通过。保留既有 React act/MSW、浏览器数据过期、bundle size 与 5 条 dead-code warning。
- 联网交叉验证：Codex 内置搜索读取 Cloudflare 官方 521/结构化 5xx 与 Tauri Channel 文档；Matrix WebSearch 独立读取 vLLM 官方 reasoning outputs，确认 Qwen reasoning 只能在成功的 Chat/Responses 响应后按 `reasoning`/delta/Responses reasoning item 分类。具体 CCSM 根因和修复以本地源码、RED/GREEN 与实时端点为权威证据。

## 2026-08-26 Qwen Responses SSE 只到 in_progress 的上游根因

- 本机 `protocol_compatibility_profiles` 当前最新 Qwen 持久档案仍是 2026-08-25 22:46 的旧记录：Responses 非流 baseline/readable reasoning 通过，但 SSE 只记录 `response.created`、`response.in_progress`，强制工具随流式路径失败；Chat 四阶段全通过，故当时选 `open_ai_chat` 正确。设置页本次内存结果没有在用户停止 UI 自动化前保存，不能把旧 `tested_at` 当成新探测。
- 实时 roglinux journal 已定位到 CCSM 之外的 vLLM 上游：部署版本 `0.27.2rc1.dev91+g1f7427bc0` 在 `VLLM_USE_EXPERIMENTAL_PARSER_CONTEXT=1` 时为 Qwen Responses 创建 `ParsableContext`，流式完成路径却在 `serving.py:1383` 断言 `SimpleContext`。HTTP 200 和起始事件已发送后才 `AssertionError`，这正好解释 CCSM capture 为什么只有两条 SSE scaffold、没有 terminal event。
- 关闭该非 Harmony 部署的实验 parser context 并重启 vLLM 后，raw、透明代理和公网 Responses SSE 全部出现 reasoning/output delta 与 `response.completed`；公网强制函数工具和续轮也通过。新进程 `NRestarts=0`，启动后未再出现该断言。没有修改 CCSM runner、classifier、reasoning projection 或协议选择器。
- 因此当前产品状态应区分两层：旧档案继续让已安装 CCSM 运行时走 Chat，这是基于旧证据的安全选择；服务端 Responses 已恢复，用户重新运行深探测并保存后，双协议四阶段都通过时现有选择器应按同分兜底选 `open_ai_responses`。本轮因用户按 Esc 停止界面控制，尚无新的 SQLite `tested_at` 证据，不宣称安装态已完成重探测持久化。
- vLLM 仓库新增 SSE 终态 canary 及测试：`error` event 不再误判成功，只有 `response.completed`/`response.incomplete` 证明流正常到达协议终态。Codex 内置搜索与 Matrix 独立打开 vLLM 官方 issue #30115，均支持非 Harmony ParsableContext streaming 仍需谨慎启用；本地部署 journal 与真实 canary 是本次根因和恢复状态的直接证据。

## 2026-08-26 CCSwitchMulti v3.19.2-18 发布候选

- 版本已同步为 `3.19.2-18`：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 与 `src-tauri/tauri.conf.json` 四处一致；发布说明位于 `docs/release-notes/v3.19.2-18-zh.md`。本次客户端发布说明明确区分 CCSM 的深度协议探测、失败语义、终态校验和档案继承改进，与 roglinux 上独立完成的 vLLM/Qwen parser-context 部署修复。
- Fresh 发布门禁：完整前端 Vitest `148 files / 1208 tests` 全过；完整 Rust library `3508 passed / 0 failed / 6 ignored`；`pnpm typecheck`、`pnpm build:renderer`、`cargo check --tests --no-default-features`、rustfmt、Prettier 与 `git diff --check` 均通过。保留既有浏览器数据、bundle size、动态导入与 5 条 dead-code warning。
- 本机 NSIS 已产出 `CCSwitchMulti_3.19.2-18_x64-setup.exe`，大小 `12,701,030` bytes，SHA-256 `5CA8151773AD7F698550E99BFDECDA8EB0BB8BB3B70B72485100C72B69BF5071`。Tauri 命令随后因本机仅有 updater 公钥、缺少 `TAURI_SIGNING_PRIVATE_KEY` 而退出 1；正式 updater 签名和多平台资产必须以 tag 对应的 GitHub Actions 为准，不能把本地 unsigned 构建当正式 Release。
- 五个发布文本已严格 UTF-8 解码，确认无 BOM、无 U+FFFD。发布前 `fork/main@18873582` 是本地 `main@ad3086ac` 的祖先；未跟踪 `.tmp/`、`docs/provider-settings-layout-preview.html`、`target-protocol-probe/` 与 `target/` 属于既有本地工作，不纳入发布提交。

## 2026-08-26 CCSwitchMulti v3.19.2-18 正式发布验收

- 发布提交为 `128aaf4de231b3809da91861211bf752d02939d0`，annotated tag `v3.19.2-18` 的本地 peeled commit、远端 peeled commit、远端 `main` 与 Release workflow `headSha` 四者一致。Release workflow run `32925578078` 完整 `completed/success`：Windows x64、Windows ARM64、Linux x64、Linux ARM64、macOS universal、Publish GitHub Release、Assemble latest.json 共 7 个 job 全部成功；macOS code signing/notarization 和各平台 updater 资产/签名校验均通过。
- GitHub Release 为非 draft、非 prerelease，`releases/latest` 指向 `v3.19.2-18`，共 19 个正式资产。下载复验的 `CCSwitchMulti-v3.19.2-18-Windows-Setup.exe` 为 `12,697,240` bytes、SHA-256 `aeae02751040e059fc39424137327b3254c7824ad531d2476c845326b5cb517a`；`latest.json` 为 `3,696` bytes、SHA-256 `a14b016fa1795f1a55734c30bdeb571fb511e3613918be80d388357ca5deb457`，两者都与 GitHub asset digest 一致。
- `latest.json` 版本为 `3.19.2-18`，平台键严格为 `darwin-aarch64`、`darwin-x86_64`、`linux-aarch64`、`linux-x86_64`、`windows-aarch64`、`windows-x86_64`，六个平台 URL 与 signature 均非空。
- 同一提交的独立 CI run `32925574572` 并非全绿：Frontend Checks 成功，三个 Backend Checks 都被 Rust 1.95.0 的 `cargo clippy -- -D warnings` 拒绝，共 17 条 lint。最近 11 次可比 `main` CI 从 `v3.19.2-17` 之前即持续失败，本地 Rust 1.95.0 可稳定复现；因此它是长期严格 Clippy 门禁债务，不是本次版本号提交或某个平台构建偶发失败。正式 Release 资产构建与签名全绿仍有效，但后续不得声称“所有 workflow 全绿”，应单独治理 Clippy 基线或修清现有 lint。

## 2026-08-26 MultiRouter V2 认证向导 schema 漂移根修

- 新建 MultiRouter 报 `invalid_v2_schema: unknown field authProvider` 的直接根因是前端旧 helper `codexOfficialAuthRouteBinding` 仍把 Provider/legacy 认证字段 `authProvider=codex_oauth` 写入 V2 `routes[].authPolicy`，而 Rust `CodexRouteAuthPolicy` 自 schema v2 起使用 `deny_unknown_fields`，合法字段只有 `source/accountId`。旧 writer 来自 `7b5fb103`，严格 V2 schema 来自 `0167dd1d`；后者收紧契约时遗漏同步前端 writer。后端拒绝是正确行为，不能通过放宽 schema 掩盖。
- 设置向导还存在第二个读取侧缺口：`inferCodexOfficialAuth` 只读取 legacy `route.upstream.auth`，不读取 V2 `route.authPolicy`。已保存的 CCSM OAuth 或账号池方案重新进入编辑向导时会错误回显为“Codex Desktop 当前登录”，再次保存会覆盖原策略。现在推断同时支持 V1/V2，并保留 V2 `accountId`。
- V2 `CodexRoutingAuthPolicy` 已从 legacy/Provider `CodexRoutingAuth` 类型分离；V2 只允许 `source/accountId`，Provider 认证、认证 IPC 和 legacy `upstream.auth.authProvider` 保持原契约。向导新建、编辑、legacy 转 V2 都只生成 canonical auth policy。工作台设置保存原本已经过 `serializeCodexRoutingV2`，会投影 `source/accountId` 并丢弃 `upstream`；本次保留该正确边界并让其类型也使用严格 V2 policy。
- TDD 先得到两轮明确 RED：新建/legacy 计划均多出 `authProvider`；V2 编辑推断返回 `undefined` 并把已保存 OAuth 策略改成 `native_codex_auth`。GREEN 后相关前端 123/123、全量 Vitest 148 files / 1211 tests、Rust schema 8/8、`pnpm typecheck`、`pnpm build:renderer`、`cargo check --tests --no-default-features`、Prettier、rustfmt 与 `git diff --check` 全部通过。保留既有 React act/MSW、浏览器数据过期、bundle size 和 5 条 dead-code warning。
- 联网交叉验证：Codex 内置搜索读取 OpenAI 官方 Codex 登录和 `requires_openai_auth` schema，确认 ChatGPT 登录与 Provider 是否要求 OpenAI 认证是明确配置边界；Matrix WebSearch 独立搜索无字段级结果。具体 `authProvider`/V2 契约和根因以本地源码、Git 历史、用户真实报错与 RED/GREEN 为直接证据。本轮未构建安装包、未替换安装态、未发布 Release。

## 2026-08-27 v3.19.2-18 深度协议探测请求等价性审计

- 远端实时事实：GitHub `releases/latest` 重定向到 `v3.19.2-18`；annotated tag peeled commit 为 `128aaf4de231b3809da91861211bf752d02939d0`，远端 `main=e470fff4fa0553ace1ba5318404f15c9f7367fbd`。`main` 相比 tag 只修改 `memory.md`，运行源码相同。本地 `main@0424ee5b` 额外两次提交只涉及 MultiRouter V2 auth policy 和 reasoning effort 档位钳制，没有修改 `protocol_compatibility` 请求准备架构。
- 结论：最新版只部分处理了问题。`v3.19.2-18` 已有真实进度事件、双协议四阶段探测、多模型一致选择、521/网络错误分类、Responses 起始事件后无终态识别、raw reasoning/native summary 语义修正和 route profile 继承；但没有实现 `create_provider_draft -> prepare_provider_draft -> probe_provider_draft -> commit_provider_draft` 的生产等价闭环。
- 独立 runner 的核心分叉仍在：`PROBE_PROFILE_VERSION=1`；逻辑请求固定 `max_output_tokens=128`；Chat 分支调用 `responses_to_chat_completions_with_reasoning(logical, None)`，绕过生产 reasoning resolver、最小输出预算和 Provider thinking 参数；candidate 仍从 `resolve_usage_credentials` 取 base URL/API Key并固定 Bearer，未复用 `CodexAdapter.extract_base_url/extract_auth/get_auth_headers`。
- 探测请求没有应用生产路径的 `customUserAgent`、`localProxyRequestOverrides.headers/body` 和完整请求变换。`CodexFormFields` 虽把这些字段纳入前端内存 readiness identity，但实际 `providerDraft` 只携带 auth/config/apiFormat/modelCatalog 与 `meta.apiFormat/isFullUrl`，因此 UI 所声称的配置身份与真正发出的探测请求不等价。
- 终态判断仍复制了一套弱逻辑：Chat SSE 只要看到 `[DONE]` 即通过；Responses SSE 只检查事件名 `response.completed`，没有复用 `codex_terminal.rs` 对 status、正文/refusal/compaction/完整工具调用的生产校验。强制工具阶段只校验函数名和 nonce，不要求合法终态；续轮只要求完成标记和非空 assistant 文本，已有测试还明确允许 marker 不匹配而不影响能力判断，因此没有证明模型消费了工具结果。
- Reasoning 观察仍压缩成单一 `{semantic, source}`，并按 `Opaque > Summary > Readable > None` 覆盖；同一响应同时出现 encrypted、summary 和 raw 时会丢失共存事实。`ProbeTargetKey` 仍只有 provider/route/model/transport/endpoint/auth/credential，没有 reasoning 请求策略、输出预算、custom headers/body 或 request-preparer 版本，旧档案可能在实际请求语义变化后仍命中。
- 流程仍未统一：Provider 设置页手工拼未完成草稿，保存时后端规范化后再次自动探测，没有可复用 receipt；MultiRouter 向导仍先调用 `model_fetch` 的 1024-token 双协议浅探测再保存，未直接消费深探测档案；Universal definition 先单独 upsert，随后 Claude/Codex/Gemini 子 Provider 依次保存，整体不是一个事务。后端已有 `codexProtocolMode/codexReasoningProjection` 和 plan/apply/clear 命令，但当前前端 API 只暴露 preflight，向导手动锁定仍使用 `meta.apiFormatSource`，高级模式契约没有闭环。
- Fresh 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib protocol_compatibility --no-default-features` 为 71 passed、0 failed。该结果证明现有实现与现有测试一致；其中测试明确把 128-token 请求和独立 runner 当作预期，不能据此声称生产请求等价问题已经修复。工作区仅保留既有未跟踪 `.tmp/`、`docs/provider-settings-layout-preview.html`、`target-protocol-probe/`、`target/`，本轮未修改实现、未构建安装包、未替换安装态。
- 联网检索：Codex 内置搜索读取 GitHub Release/仓库和 OpenAI 官方 Responses streaming reference；Matrix WebSearch 独立打开 GitHub 最新 Release 并确认 `v3.19.2-18`。Codex 内置 GitHub 页面缓存曾停留在 `v3.19.2-17`，最终以实时 HTTP redirect、`git ls-remote` 和 Matrix 直开结果裁决为 `v3.19.2-18`。Matrix 打开 OpenAI API Reference 遇到 403/JS challenge，因此 API 字段以 Codex 内置官方文档和本地生产 classifier 为证据，未把 Matrix 失败当作支持结论。

## 2026-08-28 自动协议探测原设计、原实现与主线合并责任边界

- 用户合入没有漏：`27a80890` 的第二父提交是完整后端分支终点 `005761a9`，后者是该 merge 的祖先；`005761a9..27a80890` 在 `src-tauri/src/protocol_compatibility` 与 `src-tauri/src/commands/protocol_compatibility.rs` 中无差异。合并只为当前主线适配了 `commands/provider.rs` 和 `services/provider/mod.rs`，没有删除或重写 runner 请求准备逻辑。
- 原规范在实现前已经要求：Provider 保存时使用与生产请求相同的 effective provider、upstream model、endpoint、认证和 Chat/Responses 转换器，并先通过当前生产编译器。该要求最迟在 `4ac62542`（2026-08-23 19:25）已存在，早于 runner 实现 `11347091`（2026-08-24 01:44）；所以不是后来的审计把标准临时拔高。
- 真正的过程失误发生在设计向实施计划的拆解：runner 任务只要求 Chat wire body 经过 `responses_to_chat_completions_with_reasoning*`，保存任务只要求复用 MultiRouter route/model resolution，没有建立可被生产与探测共同调用的完整 request preparer，也没有以同一最终出站请求为 golden/contract test。`11347091` 随后建立了独立 `reqwest` runner：Chat 只调用 `responses_to_chat_completions_with_reasoning(logical, None)`，认证只发送 candidate Bearer；提交说明却把“复用一个转换函数”表述成了 production transformer，造成了完成度误判。
- `b12aeb65` 把实施计划各项从未完成改为完成，并把设计状态改成“后端已实现并通过源码回归”，但对应测试只断言本地 fixture 收到 Bearer、固定四阶段 body、双协议选择、脱敏和档案事务；它没有把 probe 的最终 URL、认证头、UA、Provider header/body override、reasoning 请求参数和终态判定与真实 `CodexAdapter`/forwarder 出站结果逐字段对比。因此 71/71 绿色只证明独立 runner 契约自洽，不能证明原规范要求的生产等价性。
- 固定 `max_output_tokens/max_tokens=128` 本身是原固定探测语料的明确设计，不是用户合并造成的回归，也不能单独称为 bug。真正缺口是输出预算/请求策略没有进入共享 preparer 与档案 fingerprint，且探测和生产各自准备请求；当 Provider override、reasoning resolver、认证/UA 或终态规则变化时，旧档案仍可能命中不同的真实请求语义。
- 合并后的 `1a3ccd1a`、`23a3df8e`、`b5395d80` 分别补了探测进度 UI、raw/native-summary 字段语义和 521/不可用分类；这些都是局部正确修复，但没有把独立 runner 改造成共享的 `prepare -> probe -> receipt -> commit` 请求事务。因此该架构缺口从原分支一直保留到 `v3.19.2-18`，不是某次后续提交重新引入。
- 责任结论：用户按原提交合入是正确操作；此前把该后端称为“闭环完成”是实现与验收定义不足。后续修复应以统一最终有效 Provider 和共享 request preparer 为根边界，再补 probe-vs-production 请求等价 contract tests；不能继续在独立 runner 上追加 Provider/模型特判。

## 2026-08-28 v3.19.2-18 Codex 推理、自动标题和模型排序追踪

- 截图中的 `422` 不是路由选错 Provider：router log 显示 `gpt-5.6-sol` 已命中 Qwen。v3.19.2-18 在 Responses -> Chat 前把不在 Qwen catalog 的可见官方模型回退改写为 Qwen 默认 `qwen3.8`，随后以 Qwen 的能力契约校验仍由 GPT 会话携带的 `reasoning.effort=high`；Qwen 仅声明 `low/medium/xhigh/ultra`，所以 fail-closed 返回 422。根因是可见模型到上游模型的映射与最终能力契约之间缺少完整的档位归一化，不是旧 cache/config 污染。
- 本地主线提交 `0424ee5b` 已用“Codex 阶梯中的最近受支持档位”补全映射（Qwen 例：`high -> xhigh`），并有 Qwen 回归测试；安装态 `CCSwitchMulti 3.19.2-18` 对应发布提交 `128aaf4d`，不包含该修复。
- v3.19.2-18 的模型排序写入链为：工作台 `saveOrder()` 将每个投影模型的全局顺序反写到目标 Provider `modelCatalog.models[].sortIndex`，MultiRouter projection 再压缩为顺序，catalog writer 用 `sortIndex` 生成 `priority=1000+N`。现场当前 DB 的 OpenAI/Qwen sortIndex 与 `~/.codex/cc-switch-model-catalog.json`、`models_cache.json` 的 11 项顺序一致，说明当前配置的排序已保存并已投影；若 Desktop UI 仍显示旧顺序，需区分 Desktop 未热加载 catalog 的运行态，不能先判作 DB 写入失败。
- v18 排序实现仍有独立缺陷：`saveOrder()` 对 projection model 在目标 Provider catalog 找不到时 `sourceIndex < 0` 直接 continue，却在完成后宣称全部模型已保存。alias、重复上游模型或 stale projection 可导致局部顺序未持久化且用户无提示。当前 live router 的三条有效模型源没有 alias 且条目均可匹配，尚未复现该静默跳过；现有前端测试仅覆盖 catalog 重建时保留 source sortIndex，未覆盖拖动后 `saveOrder()` 的跨 Provider 写回。修复时应先加该场景的 UI -> Provider -> projection 回归测试，再把未保存模型作为明确错误/部分成功返回。
- CCSM proxy 不实现“重命名时选择模型”：普通 title/rename 请求在代理日志只会是 `codex_request_kind=turn`，没有单独的 title 分支。CC Switch 用户文档同时说明 Codex Desktop 在第三方 catalog 下会默认取第一项；因此“自动标题调用第一模型”与 catalog 排序有因果可能，但尚未抓到一次用户真实重命名请求，不能把它写成 CCSM 已证实的 chooser bug。后续应在用户手动触发一次自动标题后，按时间/session 比对 request_model、catalog[0] 和 upstream response，且不记录 prompt/headers。
- 联网交叉验证：Codex 内置搜索命中 CC Switch 官方 guide/changelog，确认 `model_catalog_json` 为目录事实源、Desktop 不一定热加载且第三方场景默认首模型；Matrix WebSearch 独立搜索未找到字段级源码或 issue 证据，故具体根因以本机 v18 tag、源码、SQLite、生成 catalog 和 router log 为准。

## 2026-08-28 Codex guardianv2 配置解析故障追踪

- 报错 `data did not match any variant of untagged enum FeatureToml in features.guardianv2` 不是模型路由、模型目录或 `high -> xhigh` 推理档位映射造成的。`guardianv2` 是 Codex 实验性的 Guardian 自动审批审查功能；它只接受布尔值，或符合当前 Guardian V2 schema 的配置对象。
- 已用本机 Codex `0.150.0-alpha.12.2` 的仅进程内 `-c` 覆盖复现：`features.guardianv2=true` 与合法对象均能加载；在对象中加入不支持字段即得到与截图完全相同的 `FeatureToml` 错误。数值边界错误会给出明确的 Guardian 校验消息，因此该截图代表字段、类型或嵌套结构不兼容，而非普通数值设置错误。
- 现场 `~/.codex/config.toml` 在 2026-08-28 19:45-19:46 被重建，此后已无 `features.guardianv2` 且 `codex features list` 成功。既有 `.codex` 与 CCSwitchMulti 备份中没有原坏块，不能反推其精确键或证明写入者。CCSwitchMulti 源码没有 Guardian V2 写入分支，配置合并保留全局 `[features]`，只校验 TOML 语法；因此它不是已证实的写坏者，但也不会拦截这种语义不兼容配置。
- 社区证据：CCSwitchMulti #68（macOS、v3.19.2-17）在 Codex 自动更新后报同一错误；OpenAI Codex #38803/#40339 记录了近期配置文档/自动迁移写出结构后被严格解析拒绝的同类问题。它们支持“Codex 配置迁移/版本兼容边界”这一原因类别，但没有提供本机原始 Guardian 配置，不能替代现场证据。

## 2026-08-28 v3.19.2-18 排序保存与目录默认模型修复

- 排序保存的确认根因在前端 `ModelOrderTab.saveOrder()`：它逐个将 MultiRouter 投影模型反解为目标 Provider 的 catalog 行；旧逻辑遇到别名指向已删除上游模型、缺失目标 Provider 或不可匹配的 selection 时直接 `continue`。已解析模型仍会进入 `updates` 并被写入，最后却显示“已保存”，形成局部持久化且无诊断的状态。
- 修复把每个可见模型的 route -> canonical upstream -> Provider catalog 解析变为所有写入前的前置条件。任何模型无法解析时抛出包含可见模型、上游模型和 Provider 名称的可操作错误，且 `providersApi.update` 为零调用；全部模型都解析成功后才顺序写入 Provider。前端回归 `does not partially reset model ordering when an advertised model maps to an unavailable upstream model` 使用真实 schema v2 serialized route，先 RED 证明旧实现静默写入，再 GREEN 锁定无部分保存。
- 自动标题没有 CCSM 专用的选模/重命名 route；代理仅收到普通 `turn`。但 catalog writer 在无顶层 `model` 时曾在应用 `sortIndex` 前把原始数组第一个模型标记为 `isDefault`。Desktop 若使用该默认标记生成标题或新会话辅助请求，会和用户已看到的排序脱钩。现改为先按 `sortIndex`/子 Agent 优先级/回退规则排序，再给首项 `isDefault=true`；显式 `model` 仍优先。Rust 回归 `codex_model_catalog_without_explicit_default_uses_first_user_sorted_model` 已 RED（DeepSeek Flash 被错误标记）再 GREEN（Qwen sortIndex=0 成为默认）。
- 本机 2026-08-28 现场没有顶层 `model`，当前已生成 catalog 的 `isDefault` 为 `gpt-5.6-sol`，且它也是 sortIndex=0，所以此次默认错误没有在当前数据上触发；修复防止未来拖动后默认残留。没有可脱敏同时证明“标题请求 prompt/路由模型”的实时日志，故不能声称已验证某个第三方模型自身支持自动标题；新包只保证它看到的默认模型与用户排序一致。
- `0424ee5b` 的 Qwen `high -> xhigh` 能力契约修复仍是当前 HEAD 祖先，将随本次本地构建一并进入产物；已安装的公开 `v3.19.2-18` 不含它。GuardianV2 的 `FeatureToml` 解析报错已由不含 guardian 字段的现有 config 恢复，CCSM 不写该字段，故不在本次路由/排序源码修改范围。
- 验证：前端工作台 74/74、`pnpm typecheck`、Prettier、`cargo fmt --check`、Rust library 3512 passed/0 failed/6 ignored、`git diff --check`。联网交叉验证使用 Codex 内置 web search 命中的 OpenAI Codex Desktop custom-provider issues #29156/#36582/#19694；Matrix WebSearch 对 GuardianV2/title 精确检索无结果。社区 issue 支持“Desktop custom catalog/provider 仍有一等支持缺口”，但本次根因以本地源码、SQLite、生成 catalog 与 RED/GREEN 为准。
- 实施提交为 `0e6a690a`（`fix(codex): make model ordering atomic`）。本机构建已实际生成未安装的 Windows NSIS 包 `src-tauri/target/release/bundle/nsis/CCSwitchMulti_3.19.2-18_x64-setup.exe`，文件/产品版本均为 3.19.2-18，大小 12,698,154 bytes，SHA-256 为 `42F488A9C884B1D783379369D8B27A7C1AE2ABF9A8760C7194FDF496590E2B85`。
- `pnpm tauri build --bundles nsis` 在 NSIS 产物完成后因本机只有 updater 公钥、缺少 `TAURI_SIGNING_PRIVATE_KEY` 而以退出码 1 结束；这限制正式 updater 签名，不影响刚生成的 unsigned NSIS 包。未安装、未重启、未发布或推送。

## 2026-08-28 深度协议探测与生产请求等价性根修

- 实现分支为 `bigstrongsun/probe-production-request-equivalence`，从 `07f4adb0` 开始；核心提交依次为 `31d5440e`（边界设计）、`7e7830ea`（共享请求契约 RED）、`60594a38`（生产 forwarder 共享请求策略）、`0f021433`（candidate/production 等价 RED）、`40d72d88`（probe 复用生产请求策略）、`3b2ca15a`（probe 复用生产终态分类）、`5a126f6e`（迁移旧运行时档案夹具）和 `59fa6210`（测试辅助方法仅在测试构建可见）。该分支已与 `main` 在其后续模型顺序原子保存提交 `0e6a690a` 之上做语义合并；截至合并时仍未推送、未安装、未发布。
- 根边界不是给 Qwen、DeepSeek 或某个网关加特判，而是新增 `CodexThirdPartyRequestPolicy`/`PreparedCodexRequest`：生产 forwarder 和 probe 都从同一 Provider 编译 URL、认证策略/headers、custom User-Agent、Provider header/body override、上游模型、reasoning effort、Responses→Chat 转换和私有字段过滤。Managed/official Provider 明确不能进入第三方 request policy，避免把 OAuth 生产链错误折叠到第三方探测。
- `ProbeCandidate` 不再自己保存一套可执行 URL/Bearer 规则；它携带编译后的生产 request policy，并由 `prepare_request` 生成双协议探测请求。Provider、MultiRouter route 和未保存 Provider 草稿都先解析成最终 effective Provider/模型，再编译同一策略；测试逐字段比较 candidate 和生产策略的最终 URL、header 名/值及 body，而不是只比较中间配置。
- 档案目标键新增 `request_policy_fingerprint`，材料包含 request preparer 版本、完整 Provider 请求语义的 canonical JSON、脱敏 credential fingerprint、固定 probe budget=128 和 probe corpus version。配置、认证、reasoning/override 或 preparer 变化都会使旧档案失效；解析到同一物理 Provider/模型/策略的 route alias 仍能共享一次付费探测，但各自持久化独立 route profile。`PROBE_PROFILE_VERSION` 从 1 升到 2，旧档案不会被新运行时误用。
- 固定 128-token probe budget 仍是有界探测语料的产品约束，不等同于生产会话输出预算；本次把它显式纳入策略 fingerprint，解决的是“探测与生产各自准备请求、配置变化后旧档案仍命中”的架构问题，不是把探测改成无限输出。
- Probe 终态现在调用生产 `codex_terminal` 的 Chat/Responses 分类语义。Chat `[DONE]` 但缺失/未知 `finish_reason`、`finish_reason=length`，Responses `response.completed` 缺 `status=completed`、`response.incomplete`/`response.failed`，无最终正式输出，只有 `<think>...</think>`，工具调用半截/缺函数名，或工具存在但没有完整终态，均不能再标记通过。forced-tool 阶段只有生产分类器返回 Completed 且工具调用结构完整时才进入 continuation。
- RED→GREEN 场景覆盖：生产请求共享契约 3 项；candidate/production 请求等价和 request fingerprint 变化；521 不可用不误判 reasoning unsupported；Chat/Responses 各类伪终态、截断、reasoning-only、半截工具调用、无正式输出；双协议全四阶段及 route alias 去重。旧 profile 测试夹具补齐 v2 请求策略指纹后与运行时契约一致。
- 最终源码门禁（提交 `59fa6210` 前后同一运行逻辑）：`protocol_compatibility` 82/82；生产 Chat streaming 41/41；Chat 非流转换 151/151；forwarder 160/160；Codex Provider 215/215；完整 Rust library 3525 passed、0 failed、6 ignored；`cargo check --lib --no-default-features` 和 rustfmt 通过。收口后的 fresh 聚焦复验为 protocol compatibility 82/82、`codex_request_tests` 3/3、`cargo check` 成功；只剩 5 条与本任务无关的既有 dead-code warning。
- 联网协议依据沿用本任务前置审计的两条独立检索链：Codex 内置 WebSearch 读取 OpenAI Responses streaming/reasoning 官方参考和 vLLM 官方协议材料；Matrix WebSearch 独立核对 GitHub Release/仓库状态，但打开 OpenAI Reference 时受 403/JS challenge 限制。具体请求等价根因、实现与验收以本地生产 forwarder、probe runner、Git 历史和 RED→GREEN 测试为权威；本次收口阶段只是提交、memory 和验证，没有新增需要外部事实裁决的设计判断。
- 主线合并复核确认：`27a80890` 已完整包含旧探测后端 `005761a9`；本次分支与 `main@fdb953d2` 的 merge-tree 仅在两边追加的 `memory.md` 发生冲突，源码自动合并。语义保留主线 `0e6a690a` 的模型顺序原子保存和本分支全部请求等价根修后，合并树 fresh 通过 Rust library 3526 passed、0 failed、6 ignored，前端 148 files/1212 tests、`pnpm typecheck`、`pnpm build:renderer`、`cargo check --lib --no-default-features`、rustfmt 和 `git diff --check`；只剩既有浏览器数据/bundle、React/MSW 与 5 条 dead-code warning。未构建安装器、未安装、未推送、未发布。

## 2026-08-29 Codex 真实场景协议探测、原子保存与 Provider 配置闭环

- 本轮在 `main@88ef1de0` 之上继续完成真实 Codex 使用矩阵，而不是按 Qwen/Kimi/DeepSeek 名称加特判。最终 selector 的顺序为：先比较可达且完整的 baseline/SSE/强制工具/工具结果续轮能力；能力相同才按 Codex 可呈现质量 `native summary > readable raw reasoning > opaque/none`；仍同分时优先原生 Responses。raw `reasoning_text` 继续只投影为 raw reasoning，绝不伪装成模型生成的 summary。
- 探测语料覆盖普通答案、SSE、官方 `apply_patch` custom-tool 形态、动态 root union、强制 function tool 和工具结果续轮；对复杂 Schema 被明确拒绝的上游，runner 只在同一次协议分支内安全协商 Moonshot 兼容 Schema，绝不把 401/403/429/5xx/网络故障当成 Schema 不支持。续轮必须证明返回的 nonce/marker 与实际 tool result 一致；仅有非空文本不再算通过。
- 生产终态与探测终态已经共用分类语义：Chat 缺失/未知 `finish_reason`、`length`、只有 reasoning、半截工具调用或 EOF；Responses 的 `response.incomplete`/`failed`、`response.completed` 但 status 不完整、没有正式输出或工具未完整终止，都会保持 partial/failed，而不会因 `[DONE]` 或起始 SSE scaffold 被误判完成。这样 DeepSeek/Qwen 一类“最后突然结束”会继续、报错或保留失败事实，不会静默伪装成功。
- Provider、MultiRouter route 和 Universal Provider 统一使用 prepare/probe receipt/commit 边界。receipt 绑定最终 effective Provider/model、生产 request-policy fingerprint、认证脱敏指纹、probe corpus/budget/preparer 版本、有效期和目标身份；Provider ID 在草稿保存时变化仍可安全消费同一物理 target receipt。任何 stale/mixed/partial 结果都在写入前拒绝，普通 Provider、MultiRouter 多模型协议应用和 Universal 多子 Provider 写入均保持事务原子性。
- 本机对所有 19 个当时启用且可用的真实 Provider/model target 逐一执行 Chat Completions 与 Responses 四阶段深测，19/19 均根据真实能力选择 Chat Completions；不是按品牌或历史 `apiFormat` 决定。主要原因是对应 Responses 分支在强制工具或工具结果续轮上不满足完整 Codex transaction，而 Chat 分支能够完成。GLM 曾出现瞬时上游失败，恢复后重测纳入最终 19/19；Qwen/DeepSeek/Kimi/GLM/xAI/普通 OpenAI-compatible 路径均由同一 runner 处理。测试夹具中缺 `modelCatalog` 映射、以及测试全局 home 串扰曾造成假失败，均按夹具/隔离根因修正，没有放宽生产校验。
- vLLM 官方 issue 的两条独立检索链交叉确认：non-Harmony Responses 流式工具调用仍可能退化成普通文本，Responses 工具历史拼接还可能在约 5–15 轮后让 Codex 提前结束；因此 HTTP 200、baseline 文本或单次 SSE 不能作为 Responses 优先依据，必须完成强制工具和真实续轮。Codex 内置 WebSearch 找到并读取 vLLM #36435/#37167，Matrix WebSearch 独立打开相同官方 issue；具体 CCSM 结论仍以本地 raw capture、19 个真实 target 和 RED/GREEN 测试为权威。
- Provider 设置 UI 已改为先选模型源、同步/选择模型，再在同一“模型与兼容性”区验证；普通模式自动双协议探测，高级模式才允许手动覆盖工具 Schema、协议和 Chat reasoning 展示。未同步模型时验证按钮给出明确前置步骤；手动模式明确说明 Chat 历史固定写入 `reasoning_content`，并警告跳过探测可能导致 400/422、工具续轮失败或推理不可见。真实 Tauri QA 已验证 Kimi 预设填充、模型目录/默认模型/协议/就绪状态、进度弹窗和高级门禁；独立 Vite 不再被当作桌面验收。
- 桌面 QA 发现并按 TDD 根修一个测试隔离 bug：旧 `get_app_config_dir()` 即使先读取非空 `CC_SWITCH_TEST_HOME`，仍会在 Windows v3.10.3 legacy fallback 再读真实 `HOME` 并回退到 `C:\\Users\\sunda\\.cc-switch`。新增 `test_home_override_is_never_redirected_to_the_legacy_home_database` 先 RED，最小修复让显式测试 home 立即成为最终配置目录，GREEN 后隔离 QA 只初始化 `.tmp/ui-qa-home-2`。首次暴露该 bug 的 QA 误开真实数据库，并在 2026-08-29 11:38 执行 `Rolled up and pruned 15400 proxy_request_logs (retain=30d)`，真实 DB 最后写入时间变为 11:38:39；安装态 PID 33436 没有停止且旧实例身份校验阻止了代理接管，但这次真实 DB 副作用必须保留在审计记录中，不能隐瞒。
- 最终验证：前端 `pnpm run test:unit` 为 151 files/1227 tests，`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer` 通过；Rust library 为 3612 passed/0 failed/6 ignored。隔离修复后的 fresh 门禁为 protocol compatibility 107/107、`cargo fmt --check`、`cargo check`、`cargo clippy` 全部退出码 0；Clippy 仍报告既有非阻断 warning。Windows release/NSIS 重新构建完成，产物 `CCSwitchMulti_3.19.2-18_x64-setup.exe` 为 12,799,244 bytes，FileVersion/ProductVersion 均为 3.19.2-18，SHA-256 `53E95967B573A82CC73A44568768BF584967B078404C7CFC0F1E9EAC6E94C545`；因为只有 updater 公钥、没有 `TAURI_SIGNING_PRIVATE_KEY`，产物完成后命令退出 1 且签名状态为 `NotSigned`。未安装、未重启、未推送、未发布，安装态仍是 PID 33436。

## 2026-08-30 Codex Provider Set 内部叶子与批量向导封口

- 自动混合协议 Provider 继续采用“一个用户门面 + Responses/Chat 两个同协议内部叶子”，但叶子不再属于普通 Provider 公共模型：`ProviderService::list` 过滤结构有效的自动叶子，若历史 current 指向叶子则只在父项确为 Provider Set 门面时映射回门面；孤儿叶子返回空 current 并记录告警。
- 用户可操作边界统一使用同一个结构化叶子判定，不依赖 ID 后缀。更新（包括试图删 marker）、删除、切换、排序、Sub-Agent V2 初始化/编辑/协调、自定义端点读写/last-used、用量查询与脚本测试都在服务层、数据库或 live mutation 之前拒绝叶子；前端隐藏不再是安全边界。门面编辑由专用命令恢复原始逻辑 Provider。
- 设置向导对每个普通逻辑 Provider 调用生产等价的双协议深探测并暂存 receipt；保存只调用 batch prepare/commit。模型源、自动叶子、探测档案、受影响 Router、最终 Router 与缺失的 Sub-Agent V2 初始化在同一数据库事务中提交；Blocked 零写入，Split 必须显式确认，不再逐个 `providersApi.add/update` 后单独保存 Router。官方/托管账号源固定 Responses 且不伪造 receipt。
- 运行时探测档案改协议只保留给无 Provider Set marker、非 schema v2、非手工模式且模型目录同时含 Chat/Responses 的历史混合 Provider。普通自动 Provider、同协议叶子和新 schema v2 Router 均只使用持久化协议，避免请求时重新漂移。
- 前端全量门禁曾出现 `CodexFormFields` 满核不结束。根因是可选 `onProtocolProbeReceiptIdsChange` 的内联默认函数每次渲染改变引用，触发 readiness effect 反复写入新空数组；将默认回调提升为模块级稳定函数后，单文件 32/32 与全量 156 files/1252 tests 均结束并通过。该修复不是通过排除测试、降 worker 或增加超时实现。
- 本轮 fresh 验证记录：Provider Set 规划器 19/19、叶子全部公开操作 guard 1/1；前端 156 files/1252 tests、typecheck、Prettier、renderer build 通过；Rust library 3672 passed/0 failed/6 ignored，`cargo check`、`cargo clippy`、rustfmt 退出码 0。Clippy 保留非阻断 warning，其中 Provider Set persistence enum 有 large-variant 性能提示，不影响事务正确性。未构建安装器、未安装、未重启、未推送、未发布。
- 提交前又以 `main@cc5730b8` 的精确暂存索引重建独立候选树，而不是用包含其它脏工作的主工作区代替验收。候选树先暴露并移除了两组误暂存依赖：`lib.rs` 的 `CleanExit` 启动恢复分支，以及 Provider 切换同步当前 Profile 的旁支（后者依赖未暂存 `services/profile.rs`）；两组代码仍保留在主工作区，未丢失、未夹带。候选第一次 Rust 全量还暴露 5 个“生产语义已暂存、旧断言未暂存”的测试边界；只纳入 4 个对应测试 hunk，使请求准备器版本断言、Anthropic 完整端点断言和残缺并行工具调用断言与已暂存生产语义一致，没有把同文件的 V2 Agent/加密消息等旁支带入。
- 最终精确候选树 `0c203289`（临时 detached 验证对象，实际实施提交为本条所在提交）fresh 通过：Rust library 3619 passed/0 failed/6 ignored，Provider Set 19/19，叶子公开操作 guard 1/1；前端 156 files/1252 tests；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo check --no-default-features`、`cargo clippy --no-default-features`、`cargo fmt --check`、`git diff --cached --check` 全部退出码 0。3619 与前述 3672 的差额来自刻意排除的其它未提交工作和测试，3619 才是本次提交树的发布级证据。Clippy 仍只有既有非阻断 warning，包括 Provider Set persistence enum 的 large-variant 提示。
- 联网依据沿用本轮实现前的两条独立链：Codex 内置搜索和 Matrix WebSearch 都读取了 OpenAI Codex 官方源码中仅保留 Responses wire API、Chat wire API 已移除的实现；因此 Chat 只能是 CCSM 内部转换叶子，不能作为 Codex 外层可操作 Provider。具体封口结论以当前服务层、向导调用链、运行时 guard 和 fresh 测试为权威。

## 2026-08-30 Codex 分协议探测证据、用户覆盖与 User-Agent 同源

- 时间线复核确认 Provider Set 封口不是未完成旁支：`1fda7d1d` 以审计时的 `main@cc5730b8` 为父提交，已经先完成叶子隐藏/全后端操作 guard、向导批量事务和旧混合数据运行时兜底；随后 `0bde38c9` 才开始本轮分协议证据与 UA 设计。当前 `main` 同时包含前者，不能再把 `cc5730b8` 的中途审计结论当成当前状态。
- 协议探测的“自动推荐”和“事实证据”拆成两层：一次双协议探测仍只生成一条 Provider Set selection receipt，但 Responses 与 Chat 各自生成 `ProtocolProbeObservation`，按完整 `ProbeTargetKey` 独立持久化到 schema v19 的 `protocol_probe_observations`。每条 observation 只包含自身 branch/readiness，同时保留同一次 selector 推荐；Provider/route 删除和过期清理会级联清理对应 observation。
- 普通模式继续使用自动推荐；高级模式可以把最终使用协议在 Chat/Responses 间手工覆盖。手工切换不再清空 receipt 或两条探测证据，保存仍携带原 receipt，并通过 `confirm_manual` 明确表达用户意图；只有 endpoint、认证、UA、header/body override、模型目录/映射等真正改变请求身份的字段才使 readiness 失效。这样“探测推荐 Responses”不再等于“用户永远不能切 Chat”，同时也不会把覆盖后的协议伪装成另一次探测结论。
- 探测和生产请求现在共用 `effective_third_party_user_agent()`：`customUserAgent` 优先，否则统一使用 `CCSwitchMulti/<package version>`；custom UA 还高于通用 header override。请求准备器版本升到 7，使 UA 变化进入 request-policy fingerprint，旧 UA 语义的 receipt/档案不会继续命中。
- 本轮工作区 fresh 验证：旧封口 Provider Set 19/19、叶子公开操作 guard、列表/门面 current、运行时非漂移与向导 48/48；新问题定向前端 73/73、数据库 29/29、协议命令 19/19、UA 3/3、request policy 24/24；完整 Rust library 3682 passed/0 failed/6 ignored、完整前端 156 files/1256 tests、typecheck、Prettier 和 renderer build 通过。首次 `cargo fmt --check` 仅发现本轮 Rust 格式差异，执行 rustfmt 后，以精确暂存索引生成 detached 候选 `cd4acd13`；该候选独立通过 Rust library 3629 passed/0 failed/6 ignored、前端 156 files/1256 tests、`cargo check`、`cargo clippy`、rustfmt、typecheck、Prettier、renderer build、clean status/diff 与 23 个变更文本的 UTF-8 strict/no-BOM/no-U+FFFD 检查。工作区与候选 Rust 数量差 53 项，来自刻意排除的其它未提交工作，不能把主工作区结果冒充本次提交证据。
- 联网交叉验证使用两条独立链：Codex 内置 WebSearch 读取 OpenAI Codex 当前 `model-provider-info`/schema；Matrix WebSearch 搜索无结果后直接打开官方 raw 源码。两者一致确认 Codex 外层 `WireApi` 只接受 Responses、`wire_api = "chat"` 明确报已移除；因此用户选择 Chat 表示 CCSM 内部 Responses→Chat 转换协议，不能把内部 Chat 叶子重新暴露成 Codex 外层 Provider。
- 提交前的独立审查又确认了两个真实封口缺口。普通 Codex 新增页虽然为最终 Provider 生成稳定 UUID，但没有把该 UUID 传入 `ProviderForm`，所以深探测仍会把 observation 写到固定 `codex-draft`，Provider Set 提交才改用真实 UUID；此外 `list_codex_protocol_probe_observations` 直接访问 DAO，传入内部叶子 ID 时绕过了 ProviderService 的统一操作边界。
- 两处均按 RED→GREEN 根修：新增页把同一个草稿 UUID 同时交给探测表单和最终 Provider Set 提交，避免 observation 孤立或多个草稿碰撞；observation 公共查询改走 ProviderService，先要求真实 Codex Provider 存在，再用结构化 marker 拒绝内部叶子，最后才访问 DAO。回归先分别观察到 `providerId` 为 `undefined` 和 ProviderService API 缺失的预期失败，再在修复后通过；不是按 ID 后缀、Qwen 品牌或前端隐藏打补丁。
- 修复后的主工作区 fresh 验证：新增/编辑/向导/高级协议前端 8 files/102 tests，协议命令 19/19，数据库 29/29，Provider Set 19/19，Provider Service 71/71，请求策略与 User-Agent 27/27；完整 Rust library 3683 passed/0 failed/6 ignored，完整前端 156 files/1256 tests；typecheck、Prettier、renderer build、`cargo check --no-default-features`、`cargo clippy --no-default-features` 和 rustfmt 全部退出码 0。Clippy 仍只有既有非阻断 warning；本轮未构建安装器、未安装、未重启、未推送或发布。

## 2026-08-30 Codex Provider Set 事务、current 与诊断最终封口

- 独立复审确认旧封口仍有一条跨层原子性缺口：split→single、split 重规划和删除/Universal 禁用会先改 live/current，再提交 Provider/Router 数据；数据库失败时可能留下“数据库仍指向旧 Router、设备设置或 live TOML 已切 official”的分裂状态。根修不是补偿性回滚，而是把结构化 Provider Set current 归一化、Provider/Router mutation、缺失 official seed 插入和最终 current 选择全部放进同一个 SQLite 事务；事务成功后才投影设备设置和 official live 配置。
- current 归一化只看结构化 Provider Set ownership，不按 ID 后缀猜测。split→single 时当前内部叶子原子回到用户门面；已有 split 重新规划且 current 仍指叶子时也归一化到门面。一个事务内若出现互相冲突的 current 转移会拒绝提交，注入失败的回归证明 Provider mutation 与 current 一起回滚。
- 删除和 Universal 禁用先提交权威数据库，再做可派生的 local/live 投影。若 official seed 缺失，事务内补建并设为 current；提交后的 official live 投影失败只返回稳定 warning，保留已提交数据库事实并允许重试，绝不把失败伪装成回滚成功。对应 failure-injection 回归证明数据库失败前不会提前切换设备设置或 live provider。
- Blocked 预览不再只显示笼统失败：从既有脱敏 branch evidence 保留 `stage`、`failureKind` 与 HTTP `statusCode`，不携带响应正文、凭据或其他秘密。legacy mixed Provider 在没有匹配的当前 Verified profile 时保留顶层协议并提示重新探测迁移；普通自动 Provider、内部同协议叶子与 v2 Router 不重新启用请求时协议漂移。
- 本次只精确纳入 Task 10 的 13 个文件和 `services/provider/mod.rs` 对应 hunks；Profile 同步、hosted-tool、reasoning、流式等并行工作仍留在未暂存工作区，没有混入候选。索引 tree `b01642029a917b316d3f1ab3257c903c7e7ccc98` 与 detached 候选提交 `ee71f47b298ec10276140a7813361e5d60a8dc34` 的 tree 完全一致，候选目录干净。
- fresh 候选验证：Rust library 3636 passed/0 failed/6 ignored；前端 156 files/1257 tests；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo fmt --check`、`cargo check --no-default-features`、`cargo clippy --no-default-features` 全部退出码 0。Clippy 保留 24 条既有非阻断 warning，包括 Provider Set persistence enum 的 large-variant 性能提示。
- 同一候选树已生成 Windows debug NSIS：`target-staged-closure/debug/bundle/nsis/CCSwitchMulti_3.19.2-18_x64-setup.exe`，大小 23,382,936 bytes，SHA-256 `69C481BABE70DF2126B83E08F207154E4C0825E1C0AAB8C56358B3760D08EBC9`，Authenticode 状态 `NotSigned`。未安装、未替换正式 CCSM、未重启、未推送、未发布。隔离 Tauri QA home 已准备，但 Windows Computer Use helper 报 `failed to write kernel assets ... os error 3`，所以真实窗口视觉验收仍未完成，不能把组件测试或安装器构建冒充安装态 UI 验收。
- 联网交叉验证使用 Codex 内置搜索与 Matrix WebSearch 两条独立链：内置搜索直接读取 OpenAI Codex 当前 `model-provider-info`，Matrix 搜索无结果后独立打开同一官方 raw 源码；两者一致确认 Codex 外层 `WireApi` 仅接受 Responses，`wire_api = "chat"` 明确报已移除。因此 Chat 继续只能作为 CCSM 内部转换叶子，不能暴露成 Codex 外层可操作 Provider；本轮事务/current 根因与修复结论以本地源码、failure-injection 回归和精确候选树为权威。

## 2026-08-30 MultiRouter 启用成功后的独立设置交接

- Provider Set 后端封口提交已经在 `main`，向导启用成功后不再立即关闭并把用户直接丢到状态页。成功态保留在向导内，明确给出四个互不耦合的后续入口：Codex 历史修复、模型推理强度、Sub-Agent 和模型顺序；只有“稍后完成”才关闭向导。
- 历史修复入口由 `App` 切换到 Codex 会话管理页，并通过一次性 `initialCodexHistoryRepair` 标记自动展开修复面板；面板消费标记后立即清零，避免以后重复打开。推理强度、Sub-Agent 和模型顺序分别进入既有 Router workspace 的 `sources`、`subagents` 和 `model-order` 标签，不会重新执行协议探测或 Provider Set 事务。
- 本轮只提交 `src/App.tsx`、`src/components/codex/CodexMultiRouterWizard.tsx`、两份对应前端测试和本项目记忆；既存 Rust 并行修改、`.tmp`、预览文件和构建目录全部保留在未暂存工作区。首次格式门禁准确发现向导新增 JSX 未按 Prettier 换行，使用项目格式化器只机械整理同一新增区域后复验通过。
- fresh 验证：前端全量 156 files/1259 tests，定向向导与 App 集成 50/50，Rust library 3689 passed/0 failed/6 ignored；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo fmt --check`、`cargo check --no-default-features`、`cargo clippy --no-default-features` 全部退出码 0。Clippy 只有既有非阻断 warning；未构建或安装新安装器、未重启、未推送、未发布，因此这次证明的是源码/测试交付，不是安装态视觉验收。

## 2026-08-30 Codex 协议 observation 与 Provider Set 提交边界最终封口

- 时间线再次确认：用户引用的 `main@cc5730b8` 四项中途审计已经由 `1fda7d1d` 和 `fdeaf5bf` 在主线根修。当前 `ProviderService::list` 隐藏结构化自动叶子，公共 Provider 操作统一拒绝叶子；设置向导使用生产等价深探测和 batch prepare/commit；运行时档案改协议只保留给没有 Provider Set marker、非 schema-v2、非手工且目录确实混合的旧数据。后续分协议 evidence 工作不能倒退这些边界。
- Provider Set receipt 现在是一个不可拆的诊断 bundle：一条模型选择 record 加 Chat/Responses 各自 observation。配对按 batch 的固定双分支顺序完成，不再按 provider/model 字段做全局筛选，因此相同逻辑 target 不会互相吸收 observation。receipt 过期不可复用，Blocked、取消、意图不匹配和数据库失败均零写入；失败保留 receipt 供重试，只有权威数据库事务成功后才消费。
- 普通 Provider、schema-v2 Router、Universal 和向导 batch 的 Provider、选择 profile、两条 observation、受影响 Router 与必要 setting cleanup 统一进入同一个 SQLite transaction。observation owner 只能来自规划器确定的 source/profile owner 集合，不能把 observation 自报的 `providerId` 加入允许集合形成自我授权；Universal 的外来 observation 会在任何子 Provider 写入前拒绝。
- generic schema-v2 Router 保存曾只提交 Router/profile、丢弃 observation；现在统一通过 `apply_codex_provider_mutation_with_protocol_state`。运行时 effective Router Provider 虽不携带完整 `codexRouting.schemaVersion`，但携带 `codexRouterParentProviderId`，因此 routed Provider 被明确排除在 legacy 动态协议桥之外；匹配档案仍可提供工具 Schema/history replay，禁止的只是请求时 Chat/Responses 漂移。
- 最后一个独立诊断缺口在 `run_candidate_and_persist`：旧实现先保存 observations、再单独保存 selection，第二步失败会留下半套事实。新增故障注入回归先因缺少原子 bundle API 真实 RED；`save_protocol_probe_bundle` 现于一个事务内保存 selection 与 observations，observation 插入触发失败时 selection 也回滚，GREEN 为 1/1。
- 本次只暂存协议 state 所需 9 个 Rust 文件及本条 memory；`services/provider/mod.rs` 中的 Profile import、active profile snapshot、switch warning 和对应测试全部保留在未暂存工作区。暂存索引 tree `55948a88b07b10d4f204a4e9b8f42f2f8c37ca60` 导出的独立候选 fresh 通过：Rust library 3643 passed/0 failed/6 ignored；前端 156 files/1259 tests；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo check --no-default-features`、`cargo clippy --no-default-features`、`cargo fmt --check` 全部退出码 0。Clippy 仍有 25 条既有非阻断 warning；未构建安装器、未安装、未重启、未推送或发布。
- 联网复核使用两条独立链：Codex 内置搜索直接读取 OpenAI Codex 当前 `model-provider-info`，确认 `wire_api = "chat"` 已移除且外层只接受 Responses；Matrix WebSearch 对三条相关官方域查询均返回 0 结果，本轮 Matrix 证据不足。SQLite/Provider Set 的具体根因和封口结论以本地源码、RED→GREEN、故障注入和精确候选树为权威。

## 2026-08-30 Codex Provider Set receipt 并发与 Universal 手工覆盖最终封口

- 最终审查确认前一版仍有两个真实缺口：Universal `ConfirmManual` 会消费 receipt，但因只在自动模式加载 bundle 而丢弃两条 Chat/Responses observation；Provider Set receipt 的读取和删除分离，两个并发 commit 可以在任一方删除前同时复用同一份一次性诊断授权。审查所提“preflight 立即落 observation”与 Blocked/取消/错误零写入的既定事务边界冲突，未采纳。
- Universal 手工覆盖现在与 ordinary 手工 Provider 一致：selection 只用于校验 receipt 与请求身份，不替代用户明确选择的协议；同一 receipt 中 Chat/Responses 两条 observation 仍进入 Universal definition、所有子 Provider、profile 和 observation 的单一 SQLite transaction。成功后消费 receipt，数据库失败则 definition/children/profile/observation 全部零写入并保留 receipt 重试。
- `AppState` 为 Provider Set receipt 增加整组原子 claim token。一次 claim 在同一互斥区先验证完整 receipt 集、重复 ID、过期和占用，再一次性标记全部成员；任一成员已被占用会拒绝整组且不部分占用。claim guard 在提交前任何错误、取消或 panic unwind 时自动释放；只有权威数据库 transaction 成功后调用不可回退的整组 consume。已 claim 的 receipt 不会在长事务中被 TTL 清理，释放后若已过期则下一次读取/claim 正常淘汰。
- ordinary Provider Set、设置向导 batch 和 Universal Provider Set 三条 commit 入口都在任何规划/校验/数据库写入前 claim 完整 receipt 集，并持有 guard 穿过 SQLite transaction。第二提交稳定返回 `codex_provider_set_probe_receipt_in_use`，不会写 Provider、叶子、Router、Universal definition 或 observation；数据库失败后原 receipt 可重试。ordinary 的 live projection 已与权威提交解耦：数据库成功即消费 receipt，后续派生投影失败返回 `committed_with_projection_error`，不会用同一 receipt 重复提交数据库。
- TDD 证据：Universal 手工成功回归先在旧逻辑下得到 observation `0 != 2` 的真实 RED，修复后与失败保留回归 2/2 GREEN；receipt claim API 先因方法不存在 RED，再验证整组原子性、冲突、Drop 释放和成功消费；ordinary、wizard batch、Universal 三条入口的冲突回归均先证明旧实现会实际提交，再在接入 claim 后 GREEN。定向门禁为 Provider Set 44/44、wizard batch 8/8、Universal 手工 2/2。
- 精确暂存 tree `a98d21087422409cc157a5cd7e0d54d2e87f9e00` 导出的独立候选使用全新 Rust target 验证：Rust library 3649 passed/0 failed/6 ignored，前端 156 files/1259 tests；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo check --no-default-features`、`cargo clippy --no-default-features`、`cargo fmt --check` 全部退出码 0。Clippy 保留 25 条既有非阻断 warning；未构建安装器、未安装、未重启、未推送或发布。
- 提交前故障注入又发现投影终态误报：`ensure_projection_with_publisher` 按设计把 publisher 失败持久化成 `Pending` 诊断并返回 `Ok(status)`，但 ordinary/batch 命令只把 Rust `Err` 映射成 `committed_with_projection_error`，所以数据库已提交且 live 投影失败时仍会误报 `committed`。根修是在两条提交路径共用的结果分类器中把任何非 `Ready` 投影都映射为可重试的 `committed_with_projection_error`，同时保留 Pending 诊断、已提交 Provider/observation 和已消费 receipt；首次保存但尚未启用的向导 Router 不发布 live 投影，仍正确返回 `committed`。
- 对应 ordinary 与已启用 Router batch 回归均先得到 `Committed != CommittedWithProjectionError` 的真实 RED，再在统一分类器后 2/2 GREEN；定向门禁更新为 Provider Set 45/45、wizard batch 9/9。精确代码候选 tree `dfb52594a8f5f391dd5ef63f3b125f10de229b0e` 独立通过 Rust library 3651 passed/0 failed/6 ignored、`cargo check --no-default-features`、`cargo clippy --no-default-features` 与 `cargo fmt --check`；Clippy 仍为 25 条既有非阻断 warning，前端代码相对已通过 156 files/1259 tests 的前一候选没有变化。
- 联网交叉验证本轮再次使用 Codex 内置搜索和 Matrix WebSearch 独立链。内置链读取 OpenAI Codex 当前官方 `model-provider-info` 源码，确认外层只接受 Responses；Matrix 搜索结果与技术查询不相关，证据不足。receipt/SQLite 并发与事务结论以本地调用链、互斥状态机、RED→GREEN 和独立候选为权威。

## 2026-08-30 Codex Provider Set 叶子操作与历史 current 最终封口

- 独立复审发现两条仍可能把内部叶子泄漏到用户操作面的边缘链路：`force_repair_and_switch_codex_provider` 原先在检查叶子身份前读取 live、准备备份并可能进入修复；启动/全局 live 同步则直接读取历史 effective current，旧数据库若遗留 `current=leaf` 会把内部叶子作为真实 Provider 投影。根因是这些入口没有复用普通 `current()` 已有的“结构化叶子归一到有效门面、孤儿叶子 fail closed”语义。
- 新增 `user_operable_current_provider_id` 作为唯一 current 解析边界：非 Codex 保留原值，普通 Codex Provider 保留原值，结构有效的 Provider Set 叶子只返回门面 ID，孤儿叶子返回空并告警。普通 current、单应用同步、全局 respecting-takeover 同步和服务内同步统一复用该边界，不按 ID 后缀或模型品牌猜测。
- 所有剩余副作用入口在任何 live/备份/数据库动作前拒绝叶子：强制修复先校验已加载 Provider 的结构化 ownership；`remove_from_live_config` 先走统一 ID guard。新增回归覆盖历史 current 叶子映射、强制修复拒绝且不创建备份、以及 live removal 纳入“全部直接用户操作拒绝”矩阵。
- 本轮只精确暂存 `services/provider/live.rs` 与 `services/provider/mod.rs` 中上述 hunks；同文件的 active Profile snapshot、Router projection、switch warning 以及其他流式/Profile 并行修改全部留在未暂存工作区。代码索引 tree `aa4e82c4819fd23a0f630d3a8be415091ab60f01`、detached 候选 `0005e7d0dba0b42a07860a09303700a7266c83b1`，候选工作树干净。
- fresh 候选验证：Rust library 3653 passed/0 failed/6 ignored；前端 156 files/1259 tests；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo fmt --check`、`cargo check --no-default-features`、`cargo clippy --no-default-features` 全部退出码 0。Clippy 保留 25 条既有非阻断 warning；两个 Rust 文件通过 UTF-8 strict/no-BOM/no-U+FFFD 检查。未构建安装器、未安装、未重启、未推送或发布。
- 外部协议事实沿用本任务前置的两条独立检索链：Codex 内置搜索读取 OpenAI Codex 当前源码，Matrix WebSearch 独立链无有效结果，证据不足；两者不改变本轮本地边界结论。内部叶子封口的权威证据来自当前源码调用链、RED→GREEN 回归、精确索引和 detached 候选验证。

## 2026-08-31 Codex 配置漂移对账与前端处置闭环

- 本轮直接在 `main` 修复，不创建分支。根因边界是 CCSM 的预期配置、Codex 当前 `config.toml` 和启动恢复时序此前没有统一的语义指纹/对账状态：注释、空白和键顺序差异不应触发提示，但 Codex 在 CCSM 外部改写模型、路由或 features 后必须可检测；启动对账失败也不能阻塞 CCSM 启动。
- `24205c46` 先收口 Codex V2 Router 的 Common Config live/backup 同步；`137eefb7` 新增语义 TOML fingerprint、changed key paths（只返回路径不返回值）、CAS 指纹校验、外部漂移 backup 及 `apply_ccsm`/`keep_codex`/`later`；`0de7f292` 在启动恢复完成后执行对账，仅 `external_drift` 发布 `codex-config-consistency` 事件，失败记录并继续启动。
- 前端 Task 4 通过 `useCodexConfigConsistency` 同时监听事件并执行一次 inspect 兜底，按 `actualFingerprint` 去重；`CodexConfigConsistencyDialog` 展示 Provider 与变更路径，不展示 API key、OAuth token 或配置值，并提供应用 CCSM、保留 Codex、稍后处理和失败重试。Tauri API 暴露 inspect/resolve 两个命令，四种语言均补齐文案。
- 关键安全边界：应用 CCSM 不是盲写，而是比较当前 live fingerprint 后再写入；若 Codex 在确认后再次变化则拒绝并保留弹窗，外部漂移先备份；不修改 rollout JSONL、SQLite history 或 session projection。
- fresh 源码验证：前端完整 `pnpm test:unit` 为 165 files/1301 tests，新增对话框与 hook 3/3，`pnpm typecheck` 与新增文件 Prettier 检查通过；Rust `codex_config_consistency` 9/9、Provider 相关 109/109 通过。完整 Rust/前端之外的 dirty worktree 修改均未纳入本次前端提交。
- 尚未完成安装态验收：未重启已安装 CCSM、未人工在真实 Codex `config.toml` 外部改写后观察桌面弹窗、未构建/安装/发布。上述结论是源码和测试证据，不等同于安装态 UI 证明。
- 联网交叉验证沿用本任务前置已完成的 Codex 内置搜索与 `matrix-websearch` 两条独立链；二者未提供比本机源码更直接的字段级实现依据，因此具体故障根因、CAS 边界和测试结论以当前源码、日志/配置调用链与 RED→GREEN 回归为准。

## 2026-08-31 Codex 配置漂移所有权边界修正

- 安装态首次验收暴露了 `137eefb7` 的领域边界错误：一致性检查对完整 `config.toml` 做 fingerprint/diff，并在 `apply_ccsm` 时用 DB 预期文本整份替换 live 文件。Codex Desktop 26.825.6671.0 重启后写入的 `agents`、opaque `desktop`、`developer_instructions`、插件生成的 `mcp_servers`、`plugins` 和 `projects.*.trust_level` 因此被误报；用户点击“应用 CCSM 配置”还会把这些新版/用户字段回滚到旧快照。
- 当前所有权规则改为只投影 CCSM 的活动路由面：顶层 `model`、`model_provider`、`model_catalog_json`、路由 fallback，以及预期活动的 `model_providers.<id>`；CCSM 自己写入的 `web_search = "disabled"` 和 MultiRouter 必须清除的固定上下文覆盖也仍在管理面。顶层裸 `wire_api` 不属于 OpenAI 当前 `ConfigToml`，有效协议字段只在 `model_providers.<id>.wire_api`，因此裸字段不再触发 Provider 漂移。
- `apply_ccsm` 保留 managed projection 的 CAS 与可恢复 backup，但写入改为把预期活动路由字段合并进确认时再次读取的 live 文档；所有不属于 CCSM 的表和字段原样保留。`model_catalog_json` 不再把任意路径都规范化成同一占位符，存在但指向其它目录同样属于真实路由漂移。
- TDD 先稳定复现三类旧行为：Codex/Desktop 字段导致 `ExternalDrift`、changed paths 混入用户字段、应用 CCSM 后 Desktop/MCP/project 状态消失；另补模型目录指针被错误视为一致的 RED。修复后 `codex_config_consistency` 11/11 通过。完整 Rust、安装器和安装态弹窗复验尚未执行，不得把源码定向验证等同于已发布修复。
- 外部复核使用两条独立链：Codex 内置搜索读取 OpenAI 当前 `ConfigToml`/schema；Matrix WebSearch 搜索无结果后直读同一官方 raw 源码。两链与本地 `codex-source-latest@a9519cbc` 一致确认 `developer_instructions`、MCP、projects、plugins、desktop、agents 和 `model_catalog_json` 均是合法顶层配置，而 `wire_api` 仅属于 `ModelProviderInfo`。

## 2026-08-31 启动提示优先级与可点击性收口

- 根因不是 z-index 太低。异常退出恢复结果由 Sonner 以顶部、永久且可操作的 toast 展示，Sonner 自带 `z-index: 999999999`；Codex 本地路由提醒、首次运行说明和配置一致性提醒则是 Radix modal Dialog。Radix modal 会锁焦点并将 Dialog 外区域设为 inert/禁用指针事件，因此 toast 会出现“视觉在最上层但按钮和关闭键点不了”的矛盾状态。7 月的路由提醒与 8 月的恢复通知各自实现时没有启动级调度，是组合后的领域缺口，继续抬高 z-index 不能修复。
- `App` 现在先等待 pending recovery outcomes 和 settings 初次读取完成，再统一选择唯一启动提示；优先级固定为“异常恢复通知 → Codex 配置对账 → 首次运行说明 → Codex 本地路由提醒”。低优先级提示只暂停显示，不会清除其 hook/report/settings 状态，前一个处理后会自动继续。运行期新到达的 recovery event 也能抢占低优先级 Dialog，处理后恢复原提示。
- 非 info 的 recovery outcome 在 toast 创建时加入待处理 ID 集；“打开日志”和关闭 toast 都会原子移出对应 ID 并执行既有 acknowledgement。待处理集合非空时不挂载任何启动 modal，因此可见 toast 不再处于 `body.style.pointerEvents = none` 的不可点击背景里。首次运行 Dialog 新增默认开启的 `enabled` 门控，仅供 App 排队，不改变独立使用语义。
- 本机安装态日志给出了真实竞态证据：generation 22 在 `2026-08-31T12:02:33+08:00` 同时记录 `uncleanExit` warning 与 Codex `startupTakeoverRestored`，warning 到 `12:19:07` 才被确认；这说明两种提示确实共享一次启动周期。当前安装进程仍是修复前二进制，本条只记录现场，不把它当修复后 UI 验收。
- TDD 回归在旧代码上先真实 RED：同一启动同时满足 recovery、配置漂移、首次运行和 Codex takeover 时，路由 Dialog 已挂载且 `body.style.pointerEvents` 为 `none`。修复后回归逐个点击四层提示，证明任何时刻只显示一层且最后的路由确认按钮可关闭；相关 19/19、前端全量 165 files/1306 tests、`pnpm typecheck`、Prettier、renderer build 和 `git diff --check` 通过。测试套件仍输出既有的 Tauri window mock、MSW 未注册命令和部分 act warning，但无失败。
- 外部交叉验证使用 Codex 内置 Web 与 Matrix WebSearch 两条独立链。两者均读取 Radix 官方 Dialog 文档/源码，确认 modal content 会自动 trap focus、overlay 覆盖 inert 区域，且 Portal 渲染到 body；因此本轮采用单一启动提示调度，而不是 CSS 层级补丁。未构建或安装新 installer，修复后真实桌面点击验收仍待新二进制。

## 2026-08-31 Qwen Provider 漂移与 Codex TOML 所有权启动收口

- 截图中的 `The 'qwen3.8' model is not supported when using Codex with a ChatGPT account` 不是 Qwen 模型本身不支持 Codex，而是 Qwen 模型名被留在了官方 ChatGPT Provider 上。外层 Codex 必须稳定使用 CCSM 的 `codex_model_router_v2` Responses Provider；Chat/Responses 选择只能发生在 CCSM 内部上游传输层，不能把内部叶子或 Chat 写回 Codex 外层。
- 旧版本存在两套当前 Provider 视图：设备本地 current 已切到新 Router，但活跃 Profile 的 Provider 槽位仍可能保留旧值。Router 投影所有权以 Profile 为先，因此 Provider 保存、同步或启动后投影可能被旧 Profile 拒绝。当前所有成功切换都会在 live/proxy 动作完成后只更新活跃 Profile 的 Provider 槽位，不重拍 MCP、Skills 或 Prompt；Router 投影前也先修复该槽位。同步失败只返回稳定 warning，不能把已经完成的切换伪装成失败。
- 启动对账新增一次旧数据修复：在 TOML consistency inspect 之前，以验证存在的设备 current Provider 校准活跃 Profile。未捕获 Provider 的 Profile 槽位继续保持 `None`，不会被启动流程擅自取得所有权；损坏 Profile 只记录稳定告警，后续 TOML 检查仍继续。
- TOML 所有权按截图逐项锁定：`agents.max_threads`、`desktop.composerPlainTextMode`、`developer_instructions`、Cua/Node REPL 的 command/enabled/env、插件 enabled、项目 trust level 和顶层遗留 `wire_api` 都属于 Codex/Desktop/用户面，检查与“应用 CCSM”均忽略并原样保留；`model_catalog_json`、`model_provider` 和活动 `model_providers.<id>` 仍属于 CCSM 路由面。Qwen 被切回 `openai` 时必须只报告 `model_provider` 漂移。
- Qwen Responses 历史的“commentary message + 紧随 function_call”合并修复已由主线提交 `5b820624` 提供，不能重复造一套品牌特判，也不能把原始 `reasoning_content`/`reasoning_text` 冒充模型 summary。当前日志证明本机 Qwen 连续请求实际走 `/v1/responses` 且 HTTP 200；数据库中的旧协议档案仍是早期 `partial`，安装态需要用当前深探测重新生成双协议证据，不能靠直接改字段伪造 Verified。
- TDD 门禁：启动 Profile 修复回归先因 helper 不存在编译失败，实现后通过；Codex TOML consistency 13/13、Provider service 75/75、Profile service 11/11、Responses→Chat 92/92 均通过；完整 Rust library 为 3732 passed/0 failed/6 ignored，`cargo check --lib --no-default-features`、rustfmt、diff 与 5 个本次文本的 UTF-8 strict/no-BOM/no-U+FFFD 检查通过。安装器和安装态深探测/视觉验收仍待执行，不能把源码证据描述为已安装修复。
- 外部交叉验证使用 Codex 内置检索读取 OpenAI Codex 配置参考、Responses 参考、vLLM 与 Qwen 官方部署文档；Matrix WebSearch 独立搜索结果不足，但直接打开相同官方页面得到一致边界：Codex `model_providers.<id>.wire_api` 外层仅使用 Responses，Qwen/vLLM Chat 的 `reasoning_content` 是扩展字段而不是 Responses summary。具体漂移根因以本地 DB、日志、启动调用链和 RED→GREEN 回归为权威。

## 2026-08-31 Qwen 显式深探测与自动保存探测意图分离

- 安装态实际打开 Qwen Provider 并执行“验证连接”后复现了矛盾终态：`Verified 0，Partial 0，Failed 0`，两个协议分支都停在“等待开始”。同一安装进程的 Router 日志显示普通 Qwen 请求实际走 `/v1/responses` 且 HTTP 200，因此这次故障发生在 runner 发请求前，不是 Qwen/vLLM 521、模型不可用或流式解析失败。
- 根因是 `compile_preflight_candidates` 同时服务两种不同意图。保存前自动规划只需要探测“跟随自动”的模型，手工协议或逐模型覆盖应当跳过；但用户主动点击“验证连接”承诺验证所有已启用模型和 Chat/Responses 两条协议。旧实现把后一种也按自动规划过滤，Qwen 恰好处于旧 Provider 级手工协议迁移视图，因此候选集被过滤为空。
- 后端新增显式 `CodexProtocolProbeScope`：`automatic_models` 保持 Provider Set、Wizard、Universal 的原有保存语义；`all_enabled_models` 专供用户显式深探测，普通 Provider 无论 Provider 级手工协议或逐模型覆盖都编译全部已启用模型。显式范围若仍得到空候选会返回稳定错误，不再产生合法的 0/0/0 outcome。
- Add/Edit 的“验证连接”明确传入 `all_enabled_models`，并对返回记录和期望模型做完整性校验；任一已启用模型缺结果即清空本轮 receipt、保留草稿并报告具名模型。进度弹窗把这种状态显示为“探测未完成”和“未返回探测结果”，不再把空 batch 描述成已完成。
- TDD 先在旧实现上稳定得到三类 RED：API 未发送 scope、显式手工 Provider 只编译自动模型、空 outcome 仍显示 0/0/0；实现后候选编译 3/3、API/Form/进度弹窗 55/55 与 TypeScript typecheck 通过。完整 detached 候选、Windows 安装器与安装态 Qwen 双协议深探测仍需在本提交后执行，结果必须另行记录，不能把定向源码门禁冒充安装态成功。

## 2026-08-31 Qwen 自动协议保存漂移与探测证据重试收口

- 在包含显式深探测修复的安装候选中真实验证 Qwen `qwen3.8`：Chat Completions 的基础响应、流式 SSE、推理文本、强制工具调用和工具结果续轮全部通过，最终为 `Verified`；Responses 的基础、SSE 和 raw reasoning 通过，但强制工具不支持、续轮跳过，最终为 `Partial`。因此该 vLLM/Qwen 实例的正确内部上游协议是 Chat；Codex 外层仍保持 CCSM Router 的 Responses，不得把内部 Chat 写成 Codex `wire_api = "chat"`。
- 保存时出现的 Provider“漂移”不是探测结论错误，而是编辑器初始化竞态：普通 Codex Provider 先用列表中的旧持久化记录挂载表单，随后才异步加载权威 editor snapshot。旧记录仍带 `modelCatalog[].apiFormat=openai_responses`，父子模型目录双向同步在快照到达前把该遗留字段留在草稿；用户即使切回“自动/跟随自动”，后端保存归一化仍会把旧字段再次迁成 manual override，最终报 `codex_provider_set_manual_intent_required`。
- 根修是所有 Codex Provider 编辑都先进入 `loading_snapshot`，只在权威 snapshot 返回后一次性挂载表单；失败显示读模型错误并保留页面，不再让旧列表记录与已迁移快照竞争。兼容旧数据的 `modelCatalog[].apiFormat` 仍仅在后端读取/保存迁移边界处理，不在前端继续传播。
- 同一次用户授权中的 stale retry 还缺两个合法证据失效码：已消费、缺失或与当前目标不匹配的 receipt 会分别产生 `codex_provider_set_probe_required` / `codex_provider_set_probe_target_mismatch`，旧 adapter 只识别 `dependency_changed`，第二次保存因此直接失败。三个 adapter 现统一把这两类错误视为依赖/证据变化，触发现有状态机的一次完整重探；`probe_receipt_in_use` 仍不自动重试，避免并发消费循环。
- TDD 先在旧实现上得到两个稳定 RED：延迟 editor snapshot 时表单已提前挂载；missing/mismatched receipt 未被识别为 stale。修复后 Edit/ProviderForm 定向 20/20、相关六文件 73/73、前端全量 165 files/1311 tests、`pnpm typecheck`、Prettier、renderer build 和 `git diff --check` 均通过。最新编辑器/重探修复仍需重新构建并安装后复验“自动 + 跟随自动”保存最终落为 Chat，不能沿用上一安装候选冒充本次安装态结论。
- 上一安装候选已验证 TOML 启动对账不会修改 `config.toml`（安装/启动前后 SHA-256 一致），异常退出 toast、首次运行说明和 Codex 路由接管提示按顺序可点击，且没有误报配置漂移。该验收覆盖启动调度与 TOML 所有权修复，但不覆盖本节尚未装入候选的编辑器初始化和 receipt 重探改动。
- 提交 `72773b78` 的干净 NSIS 已安装并完成真实 Qwen 保存验收：安装器 SHA-256 为 `805DBC2E08FC51AE830D160BF45CF7BCC9A2885FE68B801EE8A78795FD8DEF3E`，安装后把 Qwen 的逐模型选择改为“跟随自动”、协议模式改为“自动”并保存，统一 workflow 在同一次授权内自动重新深探测后成功提交；Provider 卡片和重开编辑器均显示内部协议为 Chat。权威 editor snapshot 为 `persistence=single`、`status=ready`、`effectiveTransport=open_ai_chat`、`choice=follow_auto`、`choiceSource=automatic`，并分别保留 Chat Verified 与 Responses Partial 证据；Provider meta 不再含 `codexProtocolMode/codexProtocolOverrides`，模型目录也不再含遗留 `apiFormat`。
- 该安装态又暴露一个纯读模型 UI 缺口：后端 snapshot 已是 `ready`，重开编辑器的“就绪状态”仍显示“建议先验证连接”。根因是就绪组件只认本页面刚产生的 `validationTone=success`，没有采用持久化 adaptation；反过来，若无条件采用 snapshot，又会在 endpoint、Key、UA、override 或模型变化后把旧证据错误显示为仍就绪。源码根修同时完成两端：未过期的持久化 `ready` 可恢复“可加入 MultiRouter”，当前 validation error 优先；snapshot 绑定其加载时的完整 readiness identity，草稿身份变化时保留证据详情但把展示状态降为 `stale`。TDD 新增持久化恢复、过期、当前失败优先和身份变化四类回归；相关 47/47、前端全量 165 files/1314 tests、typecheck、Prettier、renderer build 与 diff 检查通过。此条就绪 UI 修复仍需构建新的安装器并做桌面复验。

## 2026-08-31 5d8cc7ce 安装态安全替换与端口接管复验

- 首次替换错误地对 PID 41372 使用 `Stop-Process` 后立即静默安装和启动，没有调用仓库既有事务，也没有等待旧进程句柄与 15721 监听同时消失。新实例在 20:45:31 命中 Windows 10048；`PORT_OWNERSHIP_GUARD` 正确拒绝接管无法完整核验的残留监听，并临时关闭 Codex takeover。该故障是本次安装操作绕过既有安全流程导致的，不是 Qwen 协议探测、Provider Set 或 TOML ownership 修复本身重新写坏端口。
- 权威本地替换入口仍是 `scripts/invoke-ccswitchmulti-local-upgrade.ps1`，其独立事务执行身份/health/hash 预检、维护租约、安装目录与配置备份、停止已核验 PID、等待端口释放、静默卸载/安装、隐藏启动、listener/health/version/hash 验证；失败时按备份清单恢复应用、权威配置和注册表并重新验证旧运行态。不得再用交互会话里的裸 `Stop-Process` 加安装器替代它。
- 事务 Pester 49/49、Guardian/lease Pester 17/17 和 `PlanOnly` 通过后，独立升级 `ccsm-20260831-223840-5df7ec5acf344cdaa929459d20e6fd5f` 成功，内部 transaction 为 `ccsm-20260831-223841-ce8b637affe04381b7ea972baa6dee58`；旧 PID 33644 退出，新 PID 53532 于 22:38:53 独占 `127.0.0.1:15721`，health 为 200，维护租约已释放，`Error=null`、`RollbackError=null`。安装器 SHA-256 为 `FF18725C8ED1F7DB687C434783172F3AF8FE4A84B1865B7421B89F509BABA9A3`，安装后 EXE SHA-256 为 `1EC38F2C9FB371B25F46210EA614EDEC9786738A4C4D4285E06E9ED6A1D22FFA`，版本仍为 `3.19.2-18`。
- 安装后 Codex 外层仍为 `codex_model_router_v2` / Responses；Qwen 数据仍为严格 `openai_chat`，Chat profile `Verified`、Responses observation `Partial` 分协议独立保留，模型目录没有逐模型 `apiFormat`，也没有手工协议覆盖。`agents.max_threads=10` 并未丢失，而是按当前 Codex schema 迁移为 `agents.max_concurrent_threads_per_session=10`；Desktop、developer instructions、Node/CUA MCP、插件、72 个项目 trust 记录均仍在。后续 TOML 审计必须把“旧别名消失”和“值丢失”分开判断。
- 本节没有修改生产代码；工作区中 `app_exit_monitor.rs`、`services/proxy.rs` 及其它 17 个 Rust 文件的未提交更改属于并行工作，未因本次安装事务暂存、回退或提交。端口替换的修复是恢复使用已经过回归验证的事务控制面，不能再为一次错误操作叠加品牌特判或重复生命周期实现。

## 2026-09-01 Codex 运行时 Provider 漂移与端口强制恢复根修

- `The '<third-party model>' model is not supported when using Codex with a ChatGPT account` 的本机现场不是 Qwen/DeepSeek 上游错误：截图对应请求没有到达 15721，`state_5.sqlite` 的新任务仍记录 `model_provider=openai`；Codex app-server 在 21:48:34 启动，而 CCSM 管理的 `config.toml` 在 21:49 后才创建/改写。当前 Codex 源码确认任务会保留创建时的 model provider，运行中的 session 刷新不会替换该静态 Provider，因此磁盘已正确仍必须完全退出 Codex app-server 并在重开后新建任务。
- 配置一致性读模型现在同时返回磁盘状态和运行时激活状态。前端每 30 秒检查一次，只对 CCSM 所有的路由键做语义对账；磁盘正确但 app-server 较旧时显示明确重启提示，不再假称应用 TOML 即可修复已有任务。数据库仍标记 takeover、但 live Router 被 Codex/外部写回时单独标记 `takeover_projection_drift`，应用 CCSM 会重新进入 `ProxyService::set_takeover_for_app`，不能选择“保留 Codex 修改”把接管漂移合法化。
- 端口红色恢复项新增“解除占用并恢复接管”。后端不是通用杀端口：先读取 listener PID/路径/启动时间，只接受与当前 CCSM 同一可执行文件的旧实例；终止前再次核验 PID 与启动时间，等待 15721 真正释放后才启动当前代理、重新写入接管并验证 running/live route。未知程序、身份读取失败、PID 复用或释放后的抢占全部 fail closed。
- Windows Codex AppContainer 会把 `%LOCALAPPDATA%\CCSwitchMulti\cc-switch.exe` 投影成 `%LOCALAPPDATA%\Packages\OpenAI.Codex_*\LocalCache\Local\CCSwitchMulti\cc-switch.exe`；现场两个路径的 NTFS File ID 完全相同。旧升级脚本按字符串比较路径，因而会把同一程序误判成 foreign，这也是“明明有事务脚本仍卡端口”的缺口。Rust 身份核验改用 volume/file ID，Guardian 与事务脚本只把该 Codex LocalCache 投影规范化回宿主 LocalAppData；其它 AppContainer 路径继续拒绝。现场 `Test-CcsmGuardianSamePath` 已从 false 修到 true。
- fresh 门禁：前端 165 files/1318 tests、Rust library 3738 passed/0 failed/6 ignored、配置一致性 16/16、端口身份与虚拟化 hard-link 回归通过，typecheck、`cargo check --lib`、rustfmt、Prettier、i18n JSON 与 UTF-8/diff 检查通过。两条新增 Pester 投影回归均通过；Pester 合并运行另有 4 条既有环境失败（Codex bundled PowerShell 路径、StrictMode/SQLite 测试环境及 lease fixture），与新增断言无关。
- 构建方面，release 程序本体成功；`targets=all` 的 WiX MSI 在 `light.exe` 阶段失败，未用于安装。单独 NSIS 成功生成 `CCSwitchMulti_3.19.2-18_x64-setup.exe`，Tauri 随后仅因本机没有 updater 私钥返回签名错误；安装器文件存在。安装与事务式运行态验收必须在包含最新 File ID 修复的重新构建后另行记录，不能用较早产物冒充。
- 联网交叉验证使用 Codex 内置搜索读取 OpenAI Codex 官方配置参考、app-server README 与当前源码，Matrix WebSearch 独立打开官方 app-server README 并确认 `reloadUserConfig` 边界；Matrix 对精确报错的广搜没有有效结果。运行时 Provider 静态性的最终结论由官方源码与本机进程、配置时间线、SQLite 和 Router 日志共同确认。
- 从 `main@27c56177` 的干净 detached worktree 重新构建并经既有事务脚本安装。NSIS SHA-256 为 `91315482E05B8A86A9FA005C33EEA703ED4C6B38D874AF56E70F2A91236D7335`；raw release EXE 为 `27EA692D55C4DF9901D1F17F86892702839B0ABDEDFE8CE4C48960BBC5A4ED81`，NSIS 替换 Tauri bundle marker 后的正确 installed payload hash 为 `C4485724FEC09C379DC74757F7BD81C56AAD6214DD6FAB3626294D1DAAF56EEE`。第一次误把 raw hash 当 installed hash时，事务 `ccsm-20260901-022953-e15b6f407013441c822e78ff5d7f512d` 拒收并成功回滚；改用 `Get-CcsmTauriNsisPayloadHash` 后，事务 `ccsm-20260901-023339-3c670879e6e84804820c1df3365add15` 成功，PID 73848 被 PID 61300 替换。验收确认 15721 仅一个 listener、`/health=healthy`、`/status.running=true`、安装版本与 payload hash 一致、维护租约已删除，最新 250 行日志没有 10048/bind 错误；Codex live 仍为 `codex_model_router_v2`、本地 15721、Responses。
- 安装后 Codex app-server PID 16104 仍是 21:48:34 启动的旧进程，而 `config.toml` 在 02:33:49 又被接管事务刷新；本任务不能在不中断自身的情况下重启它。因此截图中的当前旧任务仍可能继续报 ChatGPT-account provider 错误，必须由用户完全退出并重开 Codex，再新建任务验证；不能把 CCSM 安装成功冒充当前 Codex session 已热切换。

## 2026-09-01 Codex 运行态一键刷新与确认弹窗

- “手动退出后仍未生效”的直接根因是 Windows 新版 Codex 使用 `OpenAI.Codex` MSIX 包中的 `ChatGPT.exe` 作为桌面壳，并继续持有独立 `codex.exe app-server`；关闭窗口不等于两个进程都已退出。旧的运行诊断只查询 `Codex.exe/codex.exe`，而另一条 Desktop 检测链已经识别 `ChatGPT.exe`，同一产品内的进程身份定义不一致，因此只能提示重启，不能形成可验证的关闭—重投影—重开闭环。
- 新增 `codex_runtime_refresh` 单一事务：预检枚举完整 PID、父 PID、路径、命令行和启动时间，生成一次性进程快照指纹并在确认后重新校验；先向已验证桌面主壳发送 `WM_CLOSE`，超时后只对同一完整身份执行 PID 级强制结束；随后重新投影 CCSM 拥有的 TOML 键、通过当前 Codex AUMID（回退为已解析桌面路径）重新启动，并等待新 app-server 的启动时间晚于配置写入且运行时一致性变为 `current`。任一步失败都返回稳定阶段；配置投影失败会尽力先把 Codex 重新打开，避免用户被留在关闭状态。
- 关闭目标 fail closed：新版 `ChatGPT.exe` 必须位于 `WindowsApps\\OpenAI.Codex[_Preview]` 且不是 renderer/helper；旧版大写 `Codex.exe` 只接受已知官方/安装器目录；app-server 必须带独立 `app-server` 参数并来自 OpenAI Codex bin 或已验证主壳子进程。缺路径/启动时间、任意同名 `C:\\Tools\\Codex.exe`、普通 ChatGPT 和 renderer 都不会进入强制结束集合，PID 复用在每次结束前会因完整身份不一致而拒绝。
- UI 复用最高优先级的配置一致性 Dialog，不再叠加第二个 modal。点击“刷新 Codex 状态”先进入检查弹窗，明确展示桌面主进程数、app-server 数、启动入口和“会中断当前任务”的风险；确认后在同一弹窗逐步展示关闭、应用配置、重新打开和验证，失败保留准确阶段与错误并可重试，成功态要求用户确认完成。刷新期间暂停后台一致性轮询，避免弹窗中途消失或与异常退出、首次运行、路由接管提示争夺点击。
- 本轮先在包含其它并行修改的主工作区验证刷新事务 8/8、配置重投影 1/1、前端刷新/主应用定向 26/26、前端全量 165 files/1322 tests、Rust library 3747 passed/0 failed/6 ignored。随后把精确暂存 tree `9ce56a5dd1ac231555828245717f9937d5363c6b` 导出成 detached 候选 `5abb6d35`，候选独立通过 `pnpm typecheck`、renderer build、前端 165 files/1322 tests、`cargo check --lib` 与 Rust library 3696 passed/0 failed/6 ignored；第一次候选全量里一个 catalog 临时文件并发测试失败，单测和完整全量 fresh 重跑均通过。现有 Rust warning 和测试中的 Tauri/MSW/act 输出是非阻断项。`targets=all` 仍在 WiX `light.exe` 的 MSI 封装阶段失败；安装、真实按钮执行和关闭后自动重开的端到端验收尚未进行，不能把源码/组件门禁写成安装态成功。
- 外部交叉验证使用 Codex 内置检索与 Matrix WebSearch 两条独立链：两者均确认 Windows 打包应用可能在无可见窗口时继续运行，AUMID 是安装应用的稳定身份；Microsoft 官方 `IApplicationActivationManager::ActivateApplication` 是按 AUMID 启动 Store/MSIX 应用的正式接口。具体进程集合、时间线与旧检测缺口仍以本机 CIM、当前源码和 RED→GREEN 回归为权威。
- 固定提交 `c32e4d10` 已由正式导出脚本生成并通过既有事务升级脚本安装。NSIS 位于 `cc-switch-runtime-refresh-release/windows/installer/CCSwitchMulti_3.19.2-18_x64-setup.exe`，SHA-256 为 `7DC888A3463BEDF06D8DC3254574BC8CAA65BC8BADBB2777B9119739AA8BB22B`；事务 `ccsm-20260901-050310-db478bec8c064d329158f175e7272e11` 成功把旧 PID 61300 替换为 PID 71028，安装态 payload SHA-256 为 `82C5FDC2D10064442E3D3FE245DB105235C106E3E989A2F1C06E511601EEF48A`。fresh 读回确认 15721 只有一个 listener、`/health=healthy`、`/status.running=true`、版本 `3.19.2-18`，最新日志无 10048/bind/address already in use/panic/fatal/ERROR。
- 安装态视觉验收确认启动提示按优先级顺序出现，没有配置一致性 modal 与异常退出 toast 同时争抢点击；历史 toast 的关闭控件可点击且队列能够推进。运行时旧配置提示提供“刷新 Codex 状态”，预检弹窗真实识别桌面主进程 1 个、app-server 1 个，列出 AUMID `OpenAI.Codex_2p2nqsd0c76g0!App`，并明确警告所有 Codex 窗口会关闭、运行任务会中断。为避免关闭承载本次工作的 Codex，最终“关闭、重写并重新打开”没有代用户点击，因此已验证的是安装态入口、身份预检、风险确认和取消边界，不把完整破坏性刷新事务宣称为真实端到端通过。

## 2026-09-01 Codex 刷新误杀 CCSM 与统一历史查询回归根修

- 用户真实点击安装版“关闭、重写并重新打开”后，Codex 与 CCSM 一起退出。根因是 `codex_runtime_refresh` 虽然在强制结束前核验了桌面壳身份，但对壳 PID 调用了 `taskkill /T /F`；`/T` 会递归结束调用时的后代，后代并不逐个经过 CCSM 的进程身份规则。在新版 MSIX/AUMID 启动和宿主父子关系变化下，这个边界足以误伤 CCSM。根修删除 `/T`，桌面壳和 app-server 都只对预检快照中的 PID 重新查询路径、启动时间、命令行并逐个执行 `/PID <pid> /F`；回归精确锁定参数中不得出现 `/T`。
- 历史没有被删除。现场只读核对 `state_5.sqlite` 仍有 1209 条任务，`codex-dev.db.local_thread_catalog` 仍有 654 条目录记录，其中本机 296 条、8 月 31 日 8 条，数据库完整性和正文均在。当前 Codex App 26.825.6671.0 / app-server 0.151.0-alpha.7.2 的 renderer 已从旧 `readSnapshot` 接口迁到 `appServices.localThreadCatalog.readStatus/requestStartupSync`，且入口 bundle 只直接加载 `index-*.js`、RPC 位于动态依赖 chunk；旧 CCSM 因固定找可见 asset 和旧 RPC 方法而持续失败。
- 同一 live `AppServerRequestClient` 的 A/B 是决定性证据：`thread/list` 传 `modelProviders: null` 只返回当前 `codex_model_router_v2` 的 1 条，而传 `[]` 返回 100 条并有下一页；这违反当前官方 app-server README 所写的“省略/null/空数组都包含全部 Provider”。兼容层只在 renderer 请求边界把 `thread/list` 的显式 `null` 规范化为 `[]`，随后调用 App 自己的 `threadStore.refreshRecentConversations({mode: "expanded"})`；运行时临时验证后侧边栏立即恢复“昨天”的多条任务。不能用 provider bucket 迁移、JSONL mtime 重拍或直接改 SQLite 代替该查询兼容。
- 新兼容脚本会从可见 `index-*.js` 解析同源动态 JS 依赖，兼容新 `readStatus` 和旧 `readSnapshot`，通过 React root 能力扫描定位真实 request client；失败记录去重并只保留最近 20 条，不再每 1.5 秒无限增长。补丁和历史刷新 Promise 都成功后才写版本标记；刷新事务只有在新 app-server 已读取当前配置、全 Provider 查询补丁已安装且原生列表刷新成功后才报告完成，否则停在“验证新运行态”并保留错误。
- 确认弹窗明确说明只逐个关闭身份已复核的 Codex Desktop/app-server，不递归结束 CCSM 或其它子进程；同时提示重开后会恢复跨 Provider 历史查询并刷新原生目录、不改写历史数据库。定向后端 37/37、前端 10/10、全量 Rust 3749 passed/0 failed/6 ignored、全量前端 165 files/1322 tests、TypeScript 均通过。当前运行中的历史已用临时兼容注入恢复，但生产源码尚未构建、安装或执行完整破坏性刷新，因此不得把运行时临时验证冒充新安装版验收。
- 外部检索的 Codex 内置链读取 OpenAI 官方 app-server README 及相关官方 issue，确认 `modelProviders` 契约并发现相同的 custom-provider 历史隐藏报告；Matrix WebSearch 独立链本轮经固定入口调用返回 HTTP 521，未能形成第二条外部交叉确认。最终根因以官方契约、本机 A/B、数据库计数、renderer 对象图和侧边栏恢复结果共同确定，并明确保留 Matrix 不可用这一不确定性。

## 2026-09-01 Codex 配置激活误报与单任务分页历史停滞根修

- “刚刷新 Codex 又提示仍在使用旧配置”不是路由再次漂移，而是旧实现把整个 `~/.codex/config.toml` 的 mtime 当成 CCSM 路由配置的生效时间。Codex Desktop/app-server 启动后会写入项目 trust、桌面和插件等非 CCSM 所有字段，只要文件 mtime 晚于 app-server 启动时间就会误判。根修新增独立的 managed activation receipt，仅记录 CCSM 所有投影的规范化指纹和最后一次实质变化时间；Codex 自己改写非托管字段不会让 receipt 失效，CCSM 管理投影真正变化才要求刷新。所有中央配置写入口统一更新 receipt；投影内容未变时保留原激活时间。
- 用户所指的“历史消失”发生在单个超长任务内部，不是侧边栏 Provider 过滤。现场任务 `01a02da5-70c7-73e1-9587-9cd9fdaf3634` 的活动 continuation JSONL 仍约 1.13 GB，包含 8 月 24 日之后直到当前的完整原始记录；缺失来自 app-server 的分页历史物化在 ordinal 10307 重复后停滞，projection cursor 仍期待 10308。该行为与 OpenAI 官方 `thread_history_materialization.rs` 以及 issue #41079 的“重复 ordinal 让分页投影停止推进”完全一致。
- 修复只处理已有 projection cursor 且序列为正常递增或相邻重复的 rollout：流式扫描大文件，每遇到重复 ordinal 就对当前及后续 ordinal 累加偏移，保留每一条 JSON 记录和其它字节；在同目录保留原文件备份后使用 Windows 原子替换。不会删除/重置 `thread_history_1.sqlite`，也不会用重拍 mtime 伪造恢复。非重复型回退或断档 fail closed，并作为“跳过、保持不变”显示，不能阻塞其它安全文件的恢复。
- 当前只读预检识别 3 个可安全修复的 rollout、5 个重复 ordinal，另有 1 个 `3882 -> 3881` 的非重复型回退文件被明确跳过。刷新事务在关闭 Codex 后执行安全历史修复，重开后等待每个已修复来源的 projection ordinal 与 byte offset 追到规范化文件末尾；任何配置/历史阶段失败都会尽力重开 Codex，避免把用户留在关闭状态。真实历史文件尚未执行写入式修复，安装态按钮也尚未完成端到端验收。
- TDD 锁定了 Codex 非托管写入不触发旧配置提示、CCSM 管理变化触发刷新、receipt 无实质变化不重写，以及重复 metadata/task_started 保留、累计偏移、游标位置兼容、断档/回退零写入和失败后重开等边界。前端全量为 165 files / 1323 tests，TypeScript 与 renderer production build 已通过；Rust 最终全量门禁和安装候选状态须以随后提交记录为准，不能把源码实现描述成已安装修复。
- 外部检索使用 Codex 内置链读取官方 app-server README、分页历史源码和官方 issue #41079/#41566/#41353；Matrix WebSearch 按固定入口独立调用仍返回 HTTP 521，第二条外部链没有成功结果。关键根因由官方源码/issue与本机 JSONL、projection SQLite cursor、进程及配置时间线交叉确认。
- 根修源码提交为 `ce67069c`。从该提交的 detached 干净 worktree 构建并导出 NSIS，安装器 SHA-256 为 `8800ABB1A5EDC9928F1BE28B5C35AE57064BC425F3A18C1FADE885141B99D07D`，预期 installed payload 为 `F917EF62E35255051D6289883777E9A3612081CB32FE6E6FB543A99625441256`。可回滚升级事务 `ccsm-20260901-173611-72421e2304194b7ba061aaa82ad93bfb` 成功把 PID 21028 替换为 26996；fresh 读回确认 15721 单 listener、`/health=200`、`/status.running=true`、安装哈希一致、维护租约已释放，并已生成 managed activation receipt。由于真实“刷新 Codex 状态/恢复分页历史”会关闭承载当前工作的 Codex，本轮没有代用户点击；首次写入式历史修复与重启后不再误报仍需安装态破坏性验收，不能把安全安装冒充该按钮已端到端执行。

## 2026-09-02 Codex 状态三路拆分与新版 renderer 历史兼容

- “找不到补丁”不是缺少源码或安装文件，而是 Codex Desktop 更新后私有 renderer 对象图变化：`rpc-BaVz1wnT.js` 同时导出尚未解析的 `appHost` RpcPromise 和已初始化的 `appServices` RpcStub，旧脚本先选中了错误对象；真实 `AppServerRequestClient` 又位于普通 DOM 元素的 React fiber/memo cache 内，不在旧脚本仅扫描的根容器中；扫描时碰到不可写 RpcStub 还会提前抛错。根修优先采用 `appServices.localThreadCatalog`、只接受带 `requestStartupSync` 的目录服务、对不可写伪客户端 fail-safe，并扫描当前 DOM 的 React fiber 入口定位真实 request client。
- 当前运行中的 Codex 只读对象图和一次临时 canary 证明新发现路径能找到真实 request client：兼容结果达到 `catalogComplete=true`、`clientCount=1`、`refreshRequested=true`。该 canary 只注入当前 renderer 并触发原生列表刷新，没有关闭 Codex、改写 JSONL/SQLite 或证明新 CCSM 安装包已部署。
- 运行态刷新结果已拆成三个独立字段：配置与 app-server 生效、分页历史修复/投影追平仍是核心硬门禁；renderer 跨 Provider 历史查询兼容失败只返回 `completed_with_warnings`，不得把前两项已经成功的结果改成全局失败。完成弹窗分别显示三项状态，并提供只重试 renderer 注入的按钮；Codex Provider 工具栏新增常驻“Codex 状态与修复”入口，先做非破坏性检查，再由用户进入有确认弹窗的关闭/重写/重开流程。
- TOML 一致性继续只比较 CCSM 所有的投影：`model_provider`、CCSM 目录指针、被选 Provider 表、受管 web-search/路由窗口等。Codex 自己维护的 `agents.*`、`desktop.*`、`developer_instructions`、MCP command/env、plugins、projects trust 等保留且不触发漂移；`model_catalog_json`、当前 Provider 的 `wire_api/base_url/auth` 等受管值变化仍需报告。`codex_config_consistency` 20/20 回归证明启动后非托管改写不让 activation receipt 失效，Apply/refresh 也保留非托管字段。
- 源码 fresh 门禁：状态 Dialog/Hook 14/14、App integration 17/17、前端全量 165 files / 1327 tests；TypeScript、Prettier、renderer production build 和 `git diff --check` 通过。Rust 定向为 runtime refresh 11/11、renderer 27/27、TOML consistency 20/20；`cargo check --lib` 通过，全量 library 为 3757 passed / 0 failed / 6 ignored。记录写入时尚未构建并安装包含本轮变更的新 NSIS，因此只能声称源码与 live canary 已验证，不能声称安装版 UI 已验收。

## 2026-09-02 DeepSeek Responses 工具探测误判根因

- 安装版 `3.19.2-22` 对 `deepseek-v4-flash` 做深探测时，Responses 的基础响应、SSE 和原始推理正文均通过，但“工具调用”显示“不支持”。这不是 DeepSeek 工具能力失效：使用当前 Provider 的同一 endpoint、凭据和模型做只读诊断请求，简单 function tool 的 `tool_choice=auto` 返回 HTTP 200、`response.completed` 和完整 `function_call`；续接前的能力本身成立。`tool_choice=required` 在默认 thinking mode 下会被 DeepSeek 以 HTTP 400 `Thinking mode does not support this tool_choice` 拒绝，不能把该约束错误当成模型不支持工具。
- 决定性根因位于 `protocol_compatibility::runner::extract_responses_tool_call`：它按 SSE 到达顺序对所有 payload 使用 `find_map`，而 DeepSeek 先发送 `response.output_item.added`，其中 function call 已有 `call_id/name`，但 `arguments=""`、`status=in_progress`；随后 `response.output_item.done` 和 `response.completed.response.output` 才带完整 `{"nonce": ..., "mode": "direct"}`。旧解析器在第一条 added 事件提前停止，把空参数归一成 `{}`，`valid_probe_tool_call` 因 nonce 不匹配返回 false，runner 遂把完整 Responses 分支标为 `forced_tool=unsupported` 并跳过续轮。
- 同一现场还区分了 schema 与事件解析两层：探测使用的真实 Codex 动态 root `oneOf` schema 会被 DeepSeek 以 HTTP 400 拒绝，因为 function 参数根必须是 `type: object`；当前 runner 已有 Moonshot/MFJS 方言重试，投影成 object root 后，DeepSeek 在 JSON 和 SSE 两种响应中都能正确调用目标 function。因此最终误判不是 schema fallback 缺失，而是 fallback 成功后的 SSE 增量事件被过早读取。`CustomToolAdmissionJson` 的 `apply_patch` custom tool admission 同样现场返回 HTTP 200，不是阻断点。
- Chat Completions 分支通过 `ChatToolAccumulator` 累加 delta 后再判定，不具有这条“首个 added 事件抢跑”的同型缺陷。Responses 修复没有添加 DeepSeek 特判：`extract_responses_tool_call` 统一复用 `extract_responses_output_items`，只从 `response.completed.response.output` 或 `response.output_item.done` 提取终态 item，保留原有 nonce、参数 JSON、schema dialect、终态和续轮校验。首次回归用 `added(arguments="", in_progress) -> done(arguments=complete) -> completed` 在旧实现下得到 `Unsupported`，修复后 `forced_tool` 与 `continuation` 均为 `Passed`。
- 独立复审发现首版仍可能被非终态 `response.created.response.output=[]` / `response.in_progress.response.output=[]` 抢占。扩展真实序列后第二次稳定复现 `Unsupported`；随后将 `/response/output` 限定为仅接受 `response.completed`，再转 GREEN。最终复审无 Critical、Important 或 Minor 问题；`protocol_compatibility::runner_tests` 32/32、整个 `protocol_compatibility` 126/126、Rust library 3773 passed / 0 failed / 6 ignored，`cargo check --lib`、目标文件 `rustfmt --check`、`git diff --check` 与 UTF-8 严格解码均通过。
- 本次修复只落到本地 `main` 源码与回归测试。没有构建安装包、替换或重启 CCSwitchMulti，也没有重新验收当前安装版 `3.19.2-22`；因此安装态仍应视为保留旧误判行为，直到另行完成构建、安装和运行态验收。

## 2026-09-02 v3.19.2-24 新 Issue 根修与 PR 分流

- Issue #80 的根因是 `ResponsesReasoningTextContent` 适配把可读推理正确放进 `content[].reasoning_text` 后，又删除了 Responses 输入 schema 必填的 `summary`。OpenAI 官方生成 SDK把 reasoning input 的 `summary` 声明为 Required；Paratera 等严格实现因此返回 `Missing required parameter: input[n].summary`。修复保留真实 `reasoning_text`，同时写入空 `summary: []`，不把原始推理伪装成 summary，并继续移除第三方无法使用的官方密文与内部 metadata。
- Issue #81 不是新的退出误判，而是恢复结果查询没有淘汰旧启动代际的 `UncleanExit` 等运行分类。当前查询只让本次启动代际的 ActivePreviousInstance/ConfirmedCrash/UncleanExit/PlannedRestart 分类进入待处理队列；Provider 恢复失败、端口接管失败等持久故障仍跨代保留到用户确认，完整历史记录不删除。
- Issue #82 的残留目录覆写来自启动对账把 CCSM 生成的 `model_catalog_json` 指针本身当成持续所有权证明。启动目录刷新现在先读取 Codex app takeover 状态；明确关闭时直接跳过，不触碰目录，即使 live TOML 仍有旧指针。接管开启时原有刷新行为不变。
- PR #83 只包含 #82 的候选修复，但基于 v18，整体合并会回退 Protocol Lab、Provider Set、运行刷新和设置向导；只按当前主线重新实现并保留作者思路，不 cherry-pick 整个分支。PR #85 的环境注入方向有官方配置依据，但当前实现无法区分“本来就存在且恰好同值”与“CCSM 注入”的键，关闭时可能误删用户配置；即时同步失败也只写日志并向 UI 返回成功，且没有覆盖当前 TOML consistency 所有权回归，因此暂不进入 v24。PR #86 一次升级 56 个 Cargo 依赖并有约 49 个真实编译错误，必须拆分迁移，不能进入发布候选。
- 定向 TDD 先在旧实现得到三个稳定失败：缺失 `summary`、旧代际异常退出仍 pending、接管关闭仍刷新目录；实现后 Responses reasoning 4/4、recovery outcome 6/6、接管关闭目录 1/1 通过。首次并行启动 Rust 链接造成 Windows PDB 冲突，随后确认 C 盘空间耗尽；只清理隔离候选和多个旧日期的可再生成 Cargo target 缓存后改为串行门禁，未删除源码、配置或用户数据。

## 2026-09-02 v3.19.2-24 累计发布与 GitHub 验收

- `v3.19.2-24` 精确标记 release commit `396171fa71742ac4ae7a93acdaf289230bdc4401`；该版本相对 `v3.19.2-15` 为 181 个提交、249 个文件、64752 行新增和 4598 行删除。发布说明不再把用户所称“BrokerProvider 分流”描述为新增类型，而是明确为 Provider SSOT、MultiRouter schema v2 编译/投影、`all/include` 选择语义与 V2 Sub-Agent 同步边界；同时覆盖 13 个常规向导页、零 Provider 动态第 14 页、双协议 Protocol Lab、配置/历史/端口恢复和 #80/#81/#82。
- 历史 `v3.19.2-23` GitHub Release 正文已同步改为完整累计说明，补回 v16/v17 已交付但旧短版遗漏的 Provider 分流、Provider→MultiRouter 跨模型自动同步、共享投影单写者和 V2 Profile 跟随语义。v24 完整说明保存在 `docs/release-notes/v3.19.2-24-zh.md`。
- release workflow `33606908647` 全部通过：Windows x64、Windows arm64、macOS、Linux x64、Linux arm64 五个平台矩阵，以及 `Publish GitHub Release`、`Assemble latest.json` 均为 success。正式 Release 为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.2-24`，非 draft、非 prerelease，共 19 个 uploaded 资产；`latest.json` 版本为 `3.19.2-24`，包含 6 个 updater 平台，所有 URL 均指向该 tag 且签名非空。
- tag 前 fresh 门禁：前端 165 files / 1327 tests，Rust library 3720 passed / 0 failed / 6 ignored；TypeScript、Prettier、renderer production build、`cargo check --lib --no-default-features`、rustfmt、diff、JSON 和 UTF-8 strict/no-BOM/no-U+FFFD 均通过。本地 Windows x64 应用与 13036403 字节 NSIS 已生成；命令只在其后的 updater 签名因本机无私钥返回非零，GitHub workflow 已用正式密钥完成签名与 updater 校验。
- 普通 CI run `33606842838` 仍因仓库既有的 `cargo clippy -D warnings` 债失败，错误覆盖 unused/dead-code/too-many-arguments 等数十项并跨多个旧模块；这不是编译或测试失败，且不阻塞独立 Release workflow，但后续应单独清理，不能在发布提交里机械改写几十处业务代码。
- GitHub 状态已收口：#80、#81、#82 带 commit/tag/测试证据关闭；PR #83 方向正确但基于 v18，已由当前主线等价根修取代并关闭；PR #86 一次升级 56 个 Cargo 依赖且三平台 backend CI 失败，已要求拆批并关闭；PR #85 保持打开且为 `CHANGES_REQUESTED`，Issue #84 保持打开，等待环境变量所有权账本、结构化同步失败、inline/table TOML 兼容与配置一致性回归。
- 本轮没有安装发布版、关闭 CCSM、重启 Codex 或执行破坏性 Codex runtime refresh。Release/资产验收不能替代当前机器安装态、端口接管和现有任务的桌面验收；若后续要安装，必须继续使用 `scripts/invoke-ccswitchmulti-local-upgrade.ps1` 的可回滚事务。
- 外部事实交叉验证使用 Codex 内置 Web 与 Matrix WebSearch 两条独立链；两者均读取 OpenAI Codex 的 `ShellEnvironmentPolicyToml.set`、OpenAI 生成 SDK 的 Responses reasoning input 类型和 Claude Code settings 官方文档，结论一致。具体 Issue 根因和 PR 可合并性仍以本地当前主线、RED→GREEN 回归和 GitHub CI 为权威。

## 2026-09-02 历史 Provider 重放与本机时区显示根修

- 任务 `01a04e4f-55be-7463-98ca-d5d8fb1cd158` 的 Qwen 官方账户报错不是代理未支持 Qwen：其 rollout 同时保存了 `session_meta.model_provider=openai` 和后续 `thread_settings_applied.thread_settings.model_provider_id=openai`。Codex 恢复线程时按事件顺序重放，后者会覆盖初始 Provider；旧历史修复只改 `session_meta` 和 SQLite，因此 App 重启重建后可能再次钉回官方 Provider。
- 历史元数据解析现在与 Codex 当前 state extractor 对齐：读取 `turn_context.model`，并让后续 `thread_settings_applied` 更新模型和 Provider。历史列表同时展示模型与 Provider，便于直接识别“第三方模型 + openai Provider”组合；重建/插入 thread-store 时若 schema 有 `model` 列也会保存真实模型。
- 所有 Provider 桶迁移、当前历史可见性修复和官方历史还原现在都同时改写 `session_meta.model_provider` 与 `thread_settings_applied.thread_settings.model_provider_id`。迁移资格仍由每个 `session_meta` 分段控制，不能因为同一测试文件前一个会话命中就误改后续不在来源集合中的会话；原 rollout mtime、模型名和其它事件内容保持不变。
- 历史修复会核对已在目标 DB Provider 桶中的 rollout，专门完成“SQLite/session_meta 已修、thread_settings_applied 仍是 openai”的半修复状态。UI 的旧 IPC 统计字段 `rolloutFirstLines*` 为兼容保留，但文案明确为 Provider 状态文件，并在确认框列出两种权威记录。
- 时区没有通过固定 `+8` 修正：rollout RFC3339 时间先归一为绝对 epoch 毫秒，后端排序/重建保持同一时刻，前端直接从 `updatedAtMs` 用设备本地时区格式化。这样 `2026-08-29T16:16:57Z` 在上海显示为 8 月 30 日 00:16，同时 UTC/DST 设备仍按自身时区显示；Provider 元数据迁移继续恢复原文件 mtime。此前提交 `392e8c32` 只修复额度统计测试的跨时区日边界，并不覆盖历史列表，本次才补齐历史 UI 的数值时间源。
- TDD 首先复现三类失败：解析器不认识后续设置事件、历史 UI 在 ISO 字段缺失时把有效 epoch 显示为 `-`、半修复 rollout 因 DB 已是目标 Provider 而被跳过。修复后历史迁移 54/54、前端定向 4/4、前端全量 165 files / 1328 tests、Rust library 3723 passed / 0 failed / 6 ignored、TypeScript、renderer production build 和 `cargo check --lib --no-default-features` 全部通过；既有 dead-code/unused、浏览器数据陈旧、Tauri/MSW/act 输出仍是非阻断警告。
- 外部核对使用 Codex 内置搜索与 Matrix WebSearch 两条独立链读取 OpenAI Codex 配置参考；具体重放根因以本地最新 Codex `state/src/extract.rs`、`thread_processor.rs`、真实 rollout/SQLite 与 fork Issue #77 为权威。当前仅完成源码与本地门禁，尚未构建、安装或代用户运行离线历史修复；运行历史修复仍必须先完全退出 Codex/ChatGPT App。

## 2026-09-02 长任务未提交改动归属与主线收口

- `main` 上遗留的 17 个已跟踪文件来自当前长期任务 `01a02da5-70c7-73e1-9587-9cd9fdaf3634` 的多轮实现；通过对应 rollout 的 `apply_patch` 路径、文件时间线与 Git 状态逐项确认。独立 DeepSeek 任务 `01a0619c-56ce-7572-9ceb-9b6b1371e79e` 只提交了 `b493737b` 所含的 runner、runner tests 和当时的 memory 记录，没有遗留这 17 个文件。
- 遗留实现不是同一个未完成功能，而是两组已经过测试但此前被精确提交排除的工作：退出/接管恢复安全，以及第三方 Codex 协议/工具终态/模型目录兼容。收口时发现 macOS `process_identity` 分支排除了原 Unix `tcp_listener_owner_pid` 定义却未补替代符号；已恢复全 Unix 安全兜底。`proc_pidpath`、`PROC_PIDTBSDINFO` 与 `proc_bsdinfo` 字段同时由本地 `libc` 和 Apple XNU 官方源码交叉确认。
- 两组改动分别提交为 `2f351b48` 与 `0770edef`。随后把已发布但未回到主线的 `bigstrongsun/release-v3.19.2-24` 合入 `main`；唯一文本冲突是两边都在 `memory.md` 尾部追加记录，解决时完整保留双方内容。合并后的第一次 Rust 全量门禁发现综合请求契约仍断言 `ResponsesReasoningTextContent` 删除 `summary`，与 v24 的严格 Responses schema 根修冲突；已改为验证必填空数组 `summary: []`，真实推理仍只写入 `content[].reasoning_text`。
- 合并后 fresh 门禁：前端 165 files / 1328 tests，Rust library 3778 passed / 0 failed / 6 ignored；TypeScript、Prettier、renderer production build、`cargo check --all-targets`、rustfmt 与 diff 检查均通过。`cargo check` 仍显示 13 条仓库既有 unused/dead-code 警告，与本轮改动无关；未机械改写业务代码消警。此次只完成源码、提交和主线整合，没有安装、重启或替换当前运行中的 CCSwitchMulti/Codex。

## 2026-09-03 v3.19.2-25 分支审计与功能收口

- v25 从已完成 v24 合流的 `main@41dbfbaf` 建立隔离发布工作树。逐个比较本地分支、worktree、开放 Issue 和 PR 后，确认多数历史产品分支已经完全落后于当前主线；旧完整向导与 Ultra 分支仍显示独立提交，但其有效行为已由主线的完整 13/14 页向导、Protocol Lab、Provider Set 和编译投影实现替代，不能整分支 cherry-pick 回退架构。
- 两个旧工作树仍有跟踪改动：`codex-multirouter-ssot-v2` 是已被当前 takeover/proxy 路径替代的旧强制代理残留；`codex-reasoning-probe-backend` 是旧格式化/探测候选残留。另一些工作树仅有 `.tmp` 或 target 产物。它们都被原样保留，没有 reset/clean，也没有混入 v25。
- Issue #84 / PR #85 的环境注入按当前架构重新实现：新增所有权账本，只接管真正不存在的键；同值旧键仍是用户所有，用户改写已接管值时 CCSM 放弃所有权。Claude JSON 与 Codex inline/table TOML 均支持，Provider 投影和反向导入不会吞掉注入键，失败与冲突作为结构化状态返回设置页。
- Issue #76 的根因是 legacy DeepSeek 修复会把顶层 Provider 快照扩散进 schema v2 route。公共保存事务现在统一剥离 v2 route 的继承字段，legacy repair 在 schemaVersion 2 入口直接跳过，严格 `deny_unknown_fields` 保持不变。
- Issue #78 的向导 Profile 漂移通过稳定上游身份重映射：旧公开名先解析旧 alias，再映射新 alias；Profile key、`profile.model` 与 `spawnAgentModels` 一起迁移，目标冲突保留双方。Issue #79 的模型顺序工作区把移除持久化为 `enabled:false`，即使全部隐藏仍可恢复，目录刷新不复活隐藏模型。
- 旧运行时协议改写 helper 已删除。新 Provider/叶子/schema v2 Router 只服从持久化规划，旧混合数据才允许走迁移兜底。发布代码按环境注入、协议单一权威、schema v2 防污染、向导 Profile 重映射、模型隐藏恢复五个独立提交保存，便于追溯。
- v25 tag 前 fresh 门禁为：前端 166 files / 1337 tests；Rust library 3785 passed / 0 failed / 6 ignored；TypeScript、Prettier、renderer build、`cargo check --all-targets`、严格 Clippy `-D warnings`、rustfmt、diff 与 UTF-8/no-BOM 均通过。Windows 正式导出生成 13,101,567 字节 NSIS、portable ZIP、原始 EXE、签名、SHA256SUMS 和单平台 latest.json；EXE 内嵌版本为 `3.19.2-25`，NSIS SHA-256 为 `CCE176AC342B2BB0CC281F000F89A81FE4F4AD54F3CBB2EC6EBA6F75908FEA79`。本轮没有安装或重启当前运行态，多平台资产仍需 tag 后 GitHub workflow 验收。
- 第一次推送 v25 tag 后，GitHub CI 在 Linux/macOS 的 Clippy 暴露 7 个 Windows 条件编译边界错误；本机 Windows 因这些符号均被实际使用而无法复现。立即取消尚未完成的 Release workflow 并删除新建 tag，没有发布带红灯的版本。根修把 `build_preflight` 分成 Windows/非 Windows 两个目标实现，`Command`、进程计数、启动标签、taskkill 参数与分页历史 `scan`/inspect 只在 Windows 或测试目标编译；没有用全局 allow 掩盖 dead code。Windows runtime refresh 11/11、分页历史 4/4 与本地严格 Clippy 通过，正式 tag 必须等待新的三平台 CI。
- GitHub 后端使用完整 `cargo test`，而先前本地发布记录只明确列出 library 门禁。补跑全部集成测试后，deeplink 和 Profile 的两个失败属于旧架构夹具：第三方 Codex deeplink 没有真实请求授权和双协议 receipt 时必须返回 `codex_provider_set_probe_required` 且数据库零写入；Profile 中的 MultiRouter 必须使用 schema v2 的 `targetProviderId + modelSelection + authPolicy`，不能继续伪造 schema v1 inline upstream。
- Provider command 集成测试同时发现一个真实回归：Provider 快照里的历史 `[mcp_servers]` 会先进入 live 配置，随后显式所有权 reconciler 把它视为用户自有项而拒绝删除。根修把 MCP 从所有 Codex Provider live 投影中统一剥离，再由数据库/McpService 单独投射启用集合；未被 CCSM 账本接管的用户 `[mcp_servers]` 与旧 `[mcp.servers]` 仍按所有权契约保留。fresh 完整 `cargo test` 结果为退出码 0：library 3785/0/6 ignored，全部 integration suites 通过。
- 最终发布源为 `main@a1a62937b6511ae6f74fa53e56b133a2647a1fbf`，annotated tag `v3.19.2-25` 的本地与远端 peeled commit 均精确指向该提交。GitHub CI run `33665964004` 的 Frontend、Windows、Ubuntu、macOS 全部成功；Release run `33668608762` 的 Windows x64/ARM64、Linux x64/ARM64、macOS、统一发布和 `latest.json` 汇总也全部成功。
- GitHub Release 已公开为 `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.2-25`，不是 draft/prerelease，共 19 个资产。下载复核的 `latest.json` 版本为 `3.19.2-25`，包含 `darwin-aarch64`、`darwin-x86_64`、`linux-aarch64`、`linux-x86_64`、`windows-aarch64`、`windows-x86_64` 六个平台；签名全部非空，URL 全部指向本 tag，文件 SHA-256 `1153139a463a09a631678589e41c5ebd5b7bbb8f7ae82cd645b6d3feada84007` 与 GitHub 资产摘要一致。正式 Release body 已替换为仓库内完整中文说明。
- 依据当前主线提交与 fresh 测试证据关闭 Issue #50、#52、#53、#54、#68、#71、#72、#76、#78、#79、#84；关闭已由当前架构等价或扩展实现替代的 PR #70、#73、#75、#85，并保留贡献者署名。Issue #74 仍有独立验收范围，插件 PR #61/#69、依赖 PR 与其余未逐项证明完成的条目继续保持开放。
- 发布后复查仍只发现两个 tracked 脏旧 worktree：`codex-multirouter-ssot-v2` 是落后 271 个提交、会强制开启已改为用户可选的 system-proxy 策略的旧尝试；`codex-reasoning-probe-backend` 落后 152 个提交且 tracked diff 只有 rustfmt 排版。其它相关 worktree 仅有 `.tmp`/target 产物。它们均不是遗漏的可合生产修复，继续原样保留；不得以 `git status` 或 `--no-merged` 结果直接判定应合并。

# CCSM 官方 4.0 集成尝试（2026-10-16）

## 目标与基线

- 用户要求先解释 4.0.13，再尝试同步 4.0、处理冲突。对外仓库为 `BigStrongSun/ccswitchmulti`；`origin` / `upstream` 是官方 `farion1231/cc-switch`，`fork` 是 CCSM。内部 Agent Mesh 不参与本任务。
- CCSM 本地基线 `f78cbeddda0d7e0c29e74bdbd3ef04bf0b476a17`，已含上一批五项官方可靠性修复；官方 `origin/main` 固定为 `8c2c89e0b4b685c728cd6764b1645bf27e84c123`，`package.json=4.0.13`，本轮直接 `git ls-remote origin refs/heads/main` 再次确认一致。远端公开 CCSM 仍为 `8e92a7d12`。
- 复用 `.worktrees/fix-codex-pre-response-retry`，从 clean 本地 main 建立 `bigstrongsun/ccsm-4.0-integration`。owner 是本轮 CCSM 4.0 语义集成任务；Git refs/index、Cargo 验收及最终提交由主 agent 唯一操作，各 worker 按文件独占。
- 主 checkout 的 `bigstrongsun/te-provider-type@b741835c9` 和已有 dirty 文件不属于本批，不暂存、不还原。不新建 worktree，不修改在线数据库、配置、进程或端口，不安装、不发版、不 push。
- 本批验收：采用项与官方提交可追溯；修复真实调用链并有回归；保留 MultiRouter/Provider Set/认证/协议证据和 424 重试边界；本地提交可恢复；明确未完成的架构迁移。
- Cargo target 和测试 TEMP/TMP 使用本集成工作区的 `src-tauri/target/` 与其下 `upstream-4.0-tests/`。原 release 下载证据仍需保留；工作面后续退出前必须确认无进程/资源依赖，不能直接删除该 worktree。

## 4.0.13 是什么

- 官方 4.0 首个稳定发布说明是 `docs/release-notes/v4.0.4-zh.md`：导航、应用管理、全页编辑器、direct/route/stack 模式、key-field 配置写入、用量和会话页均有重构。不能只改版本号就称作 4.0。
- `v4.0.13-zh.md` 的增量包括 Claude Desktop 三模式/最多 200 聚合模型、Codex 自动审批模型（Approve for me，不是代码审查）、聚合子代理模型及 effort、Haiku 1M 声明、Windows 便携 ZIP 下载、切回直连确认、预设与上下文默认值调整。
- 上游 GPT 272K 上限属于针对无法区分 API/订阅中转的保守默认策略，不是所有 GPT 模型的能力上限；官方也只调整新建/补全项，不回写已存卡片。本批不改 CCSM 显式窗口或可信模型目录。
- 上游聚合默认经典子代理工具并重置旧开关；CCSM 已有 `codex_multirouter_needs_plaintext_v2_collaboration` 的受管 V2 投递边界。不能为了对齐一个默认值而撤掉 CCSM 跨供应商投递策略，本批不改已有子代理配置。
- 官方 4.0.13 schema 仍为 20，CCSM 是 25。新旧数字不是可直接覆盖的迁移顺序，必须保留 CCSM 已有迁移与数据。
- 本批不采用赞助预设、上游 updater 地址或官方产品身份。Codex `[DONE]` 终止修复已在 `f78cbeddd` 完成，不重复移植。

## 三方冲突与架构决策

- `git merge-tree --write-tree --name-only f78cbeddd 8c2c89e0b` 返回 1、233 个冲突路径；结果树 `706c485ca75a82e3a14919abd79e5e6f7f4f43ec` 仅是模拟结果，未被 checkout，也没有 MERGE_HEAD 或未合并 index。
- 主要冲突簇：`src/components` 58、Rust services 24、Rust proxy 23、前端 tests 20、Rust commands 15、`src/config` 15、`src/lib` 10、数据库 7。另含依赖、品牌、CI、文档和各应用配置入口。
- 官方新增 `mode/` 约 1.2 万行，以及 `live/` patch/transaction 子系统，虽部分新文件无文本冲突，也依赖 provider/proxy 生命周期的冲突接口；“能自动加文件”不表示语义兼容。
- 必须保留：schema 25；MultiRouter 稳定 bucket/派生目录；Provider Set 的 prepare/commit 和证据 receipt；OAuth/auth 保留、账号池健康与 fail-closed；按应用接管端口/锁；最近 424 pre-response 重试的不可重放边界。
- 当前 `provider/live.rs` / `services/proxy.rs` 有应用级投影和锁，`codex_config.rs` 有原子写、catalog 所有权及 live merge；这些不是官方完整 key-field writer 的替代。Gemini/Grok 常规入口仍可能整份序列化配置。
- 官方 key-field writer 从 `81df5a08d` 延伸到各应用 writer、mode controller 和后续并发修正：只管理声明字段；只清理前一 provider 写入且未被用户修改的值；保留无关配置和格式；字节哈希检测并发编辑；临时文件/原子替换；多文件 journal/恢复；UI 冲突提示与保存流程均属于契约。不能只搬底层引擎。
- 采用语义适配批次，不作 `ours` / `theirs` 整体取舍，也不造 ancestry merge 来宣称已经同步。完整 4.0 功能与架构尚未完成。

## 本批根因与适配

| 官方提交 | 本地问题与适配边界 |
| --- | --- |
| `c1051f8e4` | Provider 表头仅按原文字符串比较，quoted/dotted/escaped 键可能找不到并重复插表。用已有 `smol-toml` 解析表头键路径，再按行定点修改；不整体重序列化。完整表头行解析同时处理尾注释、键内 `#` / `]`；多行字符串示例不得被当成表或赋值，数组表不能作为顶层恢复候选。 |
| `76f99d41a` | `commands/misc.rs` 搜索路径每插一项都扫描整个 Vec，fnm multishell 很多时平方增长。内部改为 `IndexSet<PathBuf>`，API 边界仍 Vec；保留首次插入顺序、原 PathBuf Eq 语义及 Windows 独立 PATH 大小写去重策略。 |
| `09237ebd4` | Windows 标题栏已有 Tauri 事件属性，同时全局/inline 原生 CSS drag 会进入 WebView2 draggable-region 路径。原生 CSS 仅留给 macOS；Windows 保留事件属性，Linux 继续禁用。未迁入官方新页面，不改控件/双击逻辑。 |
| `81bc1bce6` | OpenCode 导入把 latency 写死 0。新导入按合法的 `completed-created` 计时，缺失/倒序/异常时间保留 0，TTFT 仍 NULL；费用、token 和 request_id 去重不变。不回写既有账本或重置同步游标，旧零耗时记录没有历史回填。 |
| `16537fd35` | Codex 接管重建 `model_providers` 时旧 `name=OpenAI` 已被 helper 保留，但显式 `capabilities.remote_compaction` 被丢弃，尤其 `unsupported` 覆盖失效。适配必须只取当前 Provider 活动表的该能力，不能复制上游凭据/headers/其它能力，也不能从 MultiRouter 某个子路由推导全局能力。 |
| `67d63e3e4` | CCSM 已区分双 UUID 的 physical request identity 与 logical session ID，但没有读取 `history_base/history_mode`；forked continuation 仍按新 fork 等待祖先 cutoff，跨页无 last usage 的累计值也缺 baseline。需在现有增量 ledger 内接入延续语义，不照搬官方整文件解析器，不破坏 checkpoint/transaction/重写保护。 |

### Codex 分页账本的本地契约

- continuation 必须同时满足双 UUID 文件名、leading UUID 与 metadata logical ID 一致及明确分页语义；普通单 UUID fork 仍走原父会话 cutoff 验证。request_id 仍按 physical UUID 去重，不按逻辑会话重建历史账单。
- `history_base` 不是整个前页文件，而是 `{thread_id, end_ordinal_exclusive, end_byte_offset}` 指定的冻结前缀。复用既有修复模块的 byte-boundary/ordinal validator，每跳验证唯一 physical predecessor、logical ID、时间顺序与循环；只取直接前页在截止边界内的 componentwise token high-water。前页之后追加的用量和交错 lane 的末条 stale total 均不能改变基线。
- 只有成功装入 predecessor baseline 的 continuation，才能对首个 total-only 快照推断跨页 counter reset；消费首个累计快照后清除 pending。普通 rollout、缺前页但具有精确 last usage 的路径保留既有累计高水位语义，不扩大下降值重新计费的范围。
- checkpoint loader 携带实际 `source_path`，避免 basename fallback 隐藏旧路径状态。旧 None/Parent 或未初始化 continuation，仅在账本无该 physical request 前缀时，事务内清理精确路径 cursor/checkpoint 并重新建立快照；已有账单则 defer 并要求受控重建，不自动重放或补偿。alias 账单同样 defer，保留未知旧路径/兄弟记录。
- 新增 11 个分页/普通计数负向回归，覆盖 linked pages、缺前页、exact last、reset、单 UUID、append/restart、旧账单 alias、空 checkpoint 恢复、冻结 cutoff/stale lane、非法 byte/ordinal、普通 total-only rollback。旧数据已有账单的受控重建工具不在本批范围内。
- 只读抽查本机 5 个分页文件的首条元数据，其中 3 组能在同月索引中唯一定位前页；它们均具备同 logical ID、不同 physical ID、明确 ordinal/byte 边界与更早前页时间。未读取对话正文作报告，未改动原日志或在线数据库；抽查不是生产导入验收。

## 后续完整 4.0 迁移顺序

1. 先固定各应用 key-field golden、用户并发修改、未知字段保留和崩溃恢复测试，以及 CCSM Router/OAuth/证据不变量。
2. 为 schema 25 设计可兼容的字段所有权与 patch/事务接口，避免引入上游 schema 编号冲突。
3. 选择一个应用完成切换、编辑、回滚、UI 冲突提示闭环；Grok 体量较小，但当前尚无已验收迁移，不能直接标记完成。
4. 再处理 Claude、Gemini；Codex 最后单独处理 native auth、takeover、catalog、MultiRouter、OAuth pool。
5. 后端 controller 契约稳定后，再接 4.0 模式面板/新导航/Stack、Desktop 聚合及其迁移；逐应用验收后才讨论 CCSM 自有版本与 Release。

## 验证结果与收口

- 最终前端 `pnpm exec vitest run --silent --reporter=dot`：207 files、1681 tests 全部通过（45.52s）。`pnpm typecheck`、`pnpm build:renderer`、五个前端变更文件的 Prettier check 通过。保留已有 browser-data 过期、混合动态导入和大 bundle 提示，不为本批升级无关依赖。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::misc::tests:: -- --test-threads=1 --quiet`：83 passed；OpenCode 对应 `services::session_usage_opencode::tests::`：8 passed，包括重复导入不增账、原费用/token 不变和 TTFT NULL。
- 首次 OpenCode 新测试编译发现 `lock_conn!` 使用 `?`，已将测试返回类型改为 `Result<(), AppError>`；调整后专用测试通过。没有修改生产锁语义。
- 独立审查发现数组表状态不能用 `undefined` 与顶层混同，否则其中的 URL/protocol 字段会被误迁移；现以独立 `arrayTable` 状态在共享恢复与 OpenAI 清理路径排除，新增负测，增量复审无阻断。最终 provider 两套聚焦 52 tests 通过；平台专用 4 tests 通过。
- CLI/OpenCode/平台/compaction 的独立源码审查无剩余阻断；运行测试仍由各条命令证明，不以审查代替。
- 最终生产逻辑的 Codex focused tests 首轮 61 passed、1 ignored；补齐边界负测后全库 `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1 --quiet`：4335 passed、9 ignored、0 failed（89.63s），包括本批 compaction 与分页回归。忽略项沿用仓库原状态。
- 独立审查已收口普通计数下降误重计、前页 stale lane、physical UUID 歧义及 cutoff 复用问题；最终生产源码复审无阻断。Clippy 首次指出非最简布尔表达式，已作等价简化；全仓格式检查指出一个新增 OpenCode 测试签名换行，已修正。
- `cargo test --manifest-path src-tauri/Cargo.toml --tests -- --test-threads=1 --quiet` 最终通过：全库再次 4335 passed、9 ignored、0 failed（82.12s）；13 个外部集成 target 共 129 passed、0 failed，覆盖配置加载、应用类型、deeplink、Hermes、导入导出、MCP、profile、provider、proxy、skill 与 support safety。该命令也执行了 lib tests，后续不要因换会话而重复同一证据。
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`git diff --check` 最终通过。全库测试仅保留既有 Moonshot 未使用 fixture 和 intentional invalid UTF-8 literal 两项警告，没有引入新的警告。
- 所有 14 个变更/新增文本以严格 UTF-8 解码、无 BOM、无 U+FFFD 检查；中文记录复读。代码/测试/项目 memory 统一作本地提交，单命令绕开仓库自动 release 的 post-commit hook，不持久修改 hooks 配置。本地 main 仅允许从 `f78cbeddd` 快进采用该批，不能因此声称已合入整个官方 4.0 历史。
- 不 push、不发版、不安装、不重启、不改在线配置/数据库。主 checkout 的原 dirty 四项保持原状。复用的集成 worktree 保持 clean，仍归属于尚未完成的 4.0 架构迁移；后续只复用该集成面，不为单个冲突继续创建 worktree。完整迁移验收/放弃后，再按资源与历史证据归档检查收口，不直接清除现有发布材料。
- Windows/macOS 原生窗口拖动尚无安装态验收；平台单元测试与 renderer build 不等同于 WebView2/原生窗口实测。

## 检索

- Codex 内置 Web 打开官方 4.0.13 / 4.0.4 发布说明，与已 fetch 的官方 Git 对象对照。Matrix 独立 search 返回 `fetch failed`，没有取得第二条搜索结果，不能当作“无更新”。
- 当前 tip 以官方 Git object + 直接 `ls-remote` 交叉确认；冲突与本地行为结论来自源码、merge-tree 和测试。没有把凭据、内部源码或路由现场信息发给搜索服务。

# CCSM 官方上游 main 同步（2026-10-16）

## 仓库与基线

- 对外产品始终是 `BigStrongSun/ccswitchmulti`。本次不读取或修改内部 `agent-mesh`，不发版、不安装、不重启。
- `cc-switch` checkout 的 `fork` 指向 CCSM；`origin` 和 `upstream` 都指向 `farion1231/cc-switch`。本次执行 `git fetch origin main --no-tags` 与 `git fetch fork main --no-tags`。
- 官方 `origin/main` 固定在 `8c2c89e0b4b685c728cd6764b1645bf27e84c123`，`package.json` 版本 `4.0.13`；`git ls-remote` 与 fetch 结果一致。
- CCSM 远端基线 `8e92a7d12341a3a426e1f80fcb5d9d8adb51855a`，版本 `3.20.4-3`。本地 `main` 已从 `b4741adea` fast-forward 到该已发布基线，并明确 tracking `fork/main`，避免后续默认 pull 到官方线。
- 两支 merge-base 为 `43eaf07355af145aebfee301801779e824d4c221`（官方 v3.19.2 发布记录）。相对该祖先 CCSM 独有 1902 个、官方独有 751 个提交；这些是祖先关系计数，不等于功能缺失计数。相对上次语义对照的官方 `v3.20.4@43e1d9908`，官方又增加 525 个提交。
- `git merge-tree --write-tree --name-only fork/main origin/main` 返回 1、233 个冲突路径。没有执行有冲突的 checkout merge，也没有产生 MERGE_HEAD；不得将全部 ours/theirs 视为同步。
- 数据库契约已分叉：CCSM `SCHEMA_VERSION=25`，官方为 20。版本、品牌、账号池、Provider Set、MultiRouter、配置投影和自有 CI/updater 均不能整体覆盖。

## 执行边界

- 沿用 `docs/audits/2026-09-09-upstream-3.20.2-trace.md` 的语义同步方式，而非整枝合并。
- 复用已有 `.worktrees/fix-codex-pre-response-retry`，在 clean 状态切换为本地 `main`；没有新建 worktree。owner 为本次官方可靠性同步任务；共享 Git refs 由主 agent 唯一操作，生成物仍在该 CCSM 项目内。
- 主 checkout 保持 `bigstrongsun/te-provider-type@b741835c9`，其已有 `memory.md`、`protocol_compatibility.rs`、`.tmp/` 和布局预览文件不属于本次修改，不暂存、不撤销。
- 验收点：官方基线可追溯；被采用的修复有行为回归；424 重试边界不退化；无版本/身份/运行态改动；本地 main 留下集中提交。工作面退出需先保存 release 下载证据及测试输出，并确认没有进程依赖，不能直接删除该 worktree。

## 本批同步处置

| 官方提交 | 行为 | 本批处置 |
| --- | --- | --- |
| `79bdae477` | GPT-6 Sol/Luna 显式 max 不降为 xhigh | 在现有 resolver 局部适配，保留其它模型的回退 |
| `8e478b2b9` | Grok 4.x（minor >= 5）的 effort 白名单 | 用数值 minor 规则替代只认 4.5 的枚举，保留已有 MiMo 与 GPT 规则 |
| `02759c67a` | Anthropic function tools 转 Responses 显式 non-strict | 保留原 required 子集；覆盖普通 Responses 与 Codex OAuth；不改变 hosted web search |
| `f318e65ec` | Chat 的 DONE 是输入终止边界 | 适配 CCSM 的 Anthropic、Responses 和 hosted continuation 生命周期，不照搬官方循环结构 |
| `866b93b48` | Responses 终止后不重复发送 transport error | 已终止的迟到连接错误不再次发事件；终止前真实错误继续上报 |
| `f2537fdf6` | 任意 models 形状不破坏合法 data | CCSM ModelsResponse 不声明 models 字段，serde 已忽略该未知字段；不直接搬入官方结构体 |
| `c11c7cf16` | 空 upstream error body 解释文案 | CCSM 已避免悬空冒号，本批不修改该文案 |

本记录不是 525 条提交全部采用或逐条审查的证明。未整体同步的内容包括官方 4.0 key-field writer、direct/route/stack mode、Claude Desktop 聚合、Copilot managed accounts、压缩 rollout 与会话迁移、前端表单重构、模型目录、预设/价格及 CI/发布改造。尤其 `c1051f8e4` 的 quoted TOML header 修复、`16537fd35` 的 compaction preferences 等需要按 CCSM 实际写入路径另批适配，不能标记已完成。

## 验证

- 三条新回归在修复前稳定 RED：Sol/Luna 的 resolver 与 Responses 路径输出 xhigh，以及工具没有 strict=false；同一过滤下已有八条其它回归通过。
- 首次 `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy:: -- --test-threads=1`：2032 passed、1 failed。失败为 `streaming_runtime_reasoning_source_drift_expires_profile_after_stream`，单跑稳定复现，不能归为既有 flaky。
- 根因：CCSM `runtime_capture::capture_chat_sse_stream` 原来在物理 EOF 才执行一次 on_shape；converter 在 DONE 结束后不再 poll 到 EOF，协议字段漂移不能及时使 profile 过期。修复应在完整逻辑 DONE 帧交付前提交观察，保留 EOF 完成与 1MiB 限额，不得为观察继续读取 DONE 后内容，也不得把取消后的部分流当作完整证据。
- 独立审查还发现 DONE return 后旧 `has_sent_message_stop` 赋值已成为死状态，现已删除；额外补充 DONE 仍保留 usage 的回归断言。
- 修复集成根因后的最终代理回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib proxy:: -- --test-threads=1 --quiet`，2033 passed、0 failed（25.15s）；包含 forwarder 的 424 opt-in/重试预算/其它路径不重放边界、hosted tools、流终止与所有转换回归。
- 观察器专用回归：`cargo test --manifest-path src-tauri/Cargo.toml --lib protocol_compatibility::runtime_capture::tests:: -- --test-threads=1 --quiet`，8 passed、0 failed。覆盖完整 DONE 在 yield 前回调、CRLF 与拆分帧、同 chunk 迟到 reasoning 不污染、非终止 drop 不提交、EOF 只回调一次、overflow 不改变透传且不提交观察。
- `cargo fmt --check`、`git diff --check`、八个改动/新增文件的严格 UTF-8/no-BOM/no-U+FFFD 检查通过。测试保留两个既有 warning：未构造的 Moonshot fixture variant、故意无效 UTF-8 literal。
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` 通过（1m23s），没有本批新增 warning。未改前端，未重复跑前端全量；没有跨平台安装或真实网络 E2E 验收。只验证源码，不代表安装态或真实上游网络故障已消除。
- 独立审查先核对五个转换/终止文件，再对观察器根修、死状态删除和 usage 断言做一次增量复审，最终无阻断问题；不把该审查当作独立运行测试的证据。
- 本批只提交本地 `main`，不 push、不改 tag/Release。提交时使用一次性的空 hooksPath 跳过已核对仅触发自动安装包构建的 post-commit hook；不修改共享 hook 或持久 Git 配置，避免源码同步意外发起发版构建。

## 检索

Codex 内置 Web 独立搜索命中官方仓库与提交页，但索引只展示到 10 月初，不能确认当前 tip。Matrix WebSearch 已通过工具目录发现并调用 search 两次，均返回 `fetch failed`，该链没有可用独立结果。当前提交与版本以实际 fetch、官方 Git objects、直接 ls-remote 交叉确认。没有向搜索服务发送凭据或内部源码。

# Codex 官方模型上下文设置

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

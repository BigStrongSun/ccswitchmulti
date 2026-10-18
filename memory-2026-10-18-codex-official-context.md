# Codex 官方模型上下文设置

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

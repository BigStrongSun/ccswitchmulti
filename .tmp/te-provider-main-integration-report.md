# TE Provider main 集成报告

## 范围与基线

- 工作区：`C:\\Users\\sunda\\Documents\\LLMservice\\cc-switch\\.worktrees\\te-provider-main-integration`
- 分支：`bigstrongsun/te-provider-main-integration`
- 基线：`7b7aa48a`（当前 `main`）
- 目标提交已按顺序整合：`4cd100ef` → `3deb6796` → `82598734`
- 三次 cherry-pick 均无冲突；未触碰原始 cc-switch 工作树、Token Exchange 仓、主仓或生产机器。
- 本次整合没有产生冲突行为修复或生产代码改写，因此没有新增 RED/GREEN 补丁循环；目标提交自带的回归测试已在整合后重新执行。

## 整合结果

- `10aa7443`：一等 `token_exchange` Provider 类型、类型/能力元数据、静态设置校验与投影、专用预设；TE Provider 强制本机路由。
- `ecd6cb62`：TE Provider 专用绑定设置和模型能力面板；静态设置原样持久化，非法设置不覆盖既有投影；面板不提供 Proxy Key 或 Agent Credential 输入。
- `e15f2d01`：loopback-only 运行态探针；Tauri 注册 `/healthz` 与 `/provider-health` 只读命令；no-proxy；运行时授权字段不通过 HTTP 暴露。

## 验证

| 命令 | 结果 |
| --- | --- |
| `npx tsc --noEmit` | 通过（退出码 0） |
| `npx vitest run src/utils/teProvider.test.ts src/utils/providerCapabilities.test.ts tests/components/TeProviderFields.test.tsx tests/hooks/useOpenclawTeProvider.test.tsx tests/lib/teProviderRuntimeApi.test.ts` | 5 文件、44/44 通过 |
| `cargo test --lib te_provider`（`src-tauri`） | 11/11 通过；包含 loopback URL、未知上游状态、Tauri 命令注册回归 |
| `cargo check`（`src-tauri`） | 通过（退出码 0；仅既有 dead-code 等 warning） |
| `npx vitest run` | 204 文件、1647/1648 通过；1 个既有基线失败 |
| `git diff --check` | 通过 |

### 已知基线失败

`tests/components/AddProviderDialog.test.tsx > AddProviderDialog > 普通 Codex 新增没有 receipt 时按当前协议保存为未验证且不触发深度测试` 失败：`commitCodex` spy 期望调用次数为 1，实际为 0。失败用例只覆盖普通 Codex 新增，不引用 TE Provider 文件；本批未修改该路径，按要求保留为已知基线，不吞掉也不扩大修复范围。

## 安全边界复核

- 静态 `teProvider` 只保存数值 loopback URL、Partner AIC、协议版本、绑定投递方式、探针和模型能力元数据。
- Proxy Key、Agent Credential、task/lease/session/binding 字段仅作为运行时约束或只读状态说明，不进入持久化 Provider 配置。
- Rust 端再次拒绝非数值回环、HTTPS、localhost、私网地址、凭据、query 和 fragment，并显式 `no_proxy()`；错误只回稳定分类，不回显响应正文。

## 搜索与交叉验证

- Codex 内置 Web 搜索：查阅 Tauri 官方调用 Rust 命令文档（`https://v2.tauri.app/develop/calling-rust/`）和 Vitest 官方指南（`https://vitest.dev/guide/index.html`），用于核对 invoke/命令注册与测试入口。
- `matrix-websearch`：使用独立链路搜索同一主题；返回结果主要是泛化或中文聚合页面，未提供比官方文档更强的 CCSwitchMulti/TE 事实证据，因此未用其替代本地源码和测试证据。
- 结论以当前 worktree 的提交、源码、测试输出和 Rust 编译结果为准；外部搜索未改变实现。

## 风险与后续

- 全量前端仍有上述基线失败；本次最终复跑为 204 个文件、1647/1648 个测试通过，不应宣称全量 Vitest 全绿。
- 当前仅提供只读运行态探针，没有一键启动/停止注入器，也没有将运行态写回 Provider 卡片。

## 本次独立复审修复波（TDD RED → GREEN）

### RED（实现前真实结果）

- `npx vitest run src/utils/teProvider.test.ts`：13 tests 中 3 个失败；旧实现对非字符串字段调用 `.trim()` 崩溃，接受 runtime/secret/unknown 字段，且 builder 通过对象展开复制这些字段。
- `cargo test --lib te_provider -- --nocapture`：新增探针/路由测试首先无法编译（缺少 sanitizer 与 `Provider::is_token_exchange` / `requires_local_routing`）；补齐测试编译后 503 语义测试先因 fixture 的错误 Content-Length 得到 `unknown`，修正 fixture 后进入实现 GREEN。
- 既有 `tests/hooks/useOpenclawTeProvider.test.tsx` 先暴露旧断言仍要求把非法 draft 原样写入配置；断言已按 fail-closed 合同更新，验证非法 draft 可编辑但不落盘。

### GREEN（本次修复后）

- 新增/更新前端聚焦：`npx vitest run src/utils/teProvider.test.ts tests/components/OpenClawTeProviderFields.test.tsx tests/hooks/useOpenclawTeProvider.test.tsx`，14/14 通过。
- Rust TE/路由聚焦：`cargo test --lib te_provider -- --nocapture` 14/14 通过；`cargo test --lib token_exchange -- --nocapture` 4/4 通过；覆盖真实本地 HTTP 302（不跟随重定向）、503 degraded 与 transport failure 区分、状态/原因/时间戳清洗、后端 add 持久化拒绝与 canonical descriptor、Claude Desktop local proxy route。
- `npx tsc --noEmit` 通过；`cargo check` 通过（仅既有 dead-code 等 warning）；相关文件 Prettier 与 `git diff --check` 通过。
- 相关 Rust 文件 `rustfmt --edition 2021 --check` 通过；仓库级 `cargo fmt --check` 仍会命中未改动的既有格式差异（`codex_config.rs`、`codex_subagent_profiles.rs`、`services/usage_stats.rs`），未对这些无关文件做格式化。
- 13 个本批变更文本文件严格 UTF-8 检查通过（无 BOM、无 U+FFFD）；启发式敏感信息扫描的私钥、OpenAI key、GitHub token、Bearer token、密码/密钥赋值模式均为 0。
- 前端全量复跑已完成：204 个文件、1647/1648 个测试通过；当前仍保留一条既有 `AddProviderDialog` 普通 Codex baseline failure，不应宣称全绿。

### 根因与修复边界

- Rust probe client 显式使用 `reqwest::redirect::Policy::none()`；任何 HTTP response 都标记 sidecar reachable，503/302 使用受控 `health_degraded_<status>`，连接失败保持 `connect_failed`。
- TE 设置增加运行时类型守卫、静态字段 allowlist、secret/runtime/unknown 字段拒绝或草稿清洗；builder、hook、表单提交、Rust provider mutation 都从零构造 canonical descriptor，不再 spread-copy `teProvider`。
- token_exchange 隐藏 OpenClaw 通用端点、API key、User-Agent、模型编辑器和 JSON editor；submit 与 Rust persistence 均 fail-closed，非法 draft 只留在可编辑 UI 状态。
- Provider 增加 `is_token_exchange` / `requires_local_routing`；Claude Desktop provider mode 默认把 TE 送入本地 proxy 路由，但不把 TE 当成 OAuth 托管账号。
- 本报告保存在 `.tmp/` 下，并与本批代码一并纳入本地提交；代码提交说明末尾按项目规则保留“本次提交由BigStrongsSun完成”。

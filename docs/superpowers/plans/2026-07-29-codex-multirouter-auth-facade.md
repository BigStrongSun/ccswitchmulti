# Codex MultiRouter 动态认证门面实施计划

> 日期：2026-07-29  
> 设计依据：`docs/superpowers/specs/2026-07-29-codex-multirouter-auth-facade-design.md`

## 实施目标

在不改变 `codex_model_router_v2` 任务身份、不修改 Codex `auth.json` 的前提下，让 CCSM 根据 Router 的真实认证所有权自动生成以下两种 live TOML 门面：

- `Native/Mixed`：Codex Desktop 必须发送真实 OAuth；本地 Router 再按 route 决定透传或替换。
- `Fully Managed`：Codex 只发送 `PROXY_MANAGED`；CCSM 必须在出站前替换为 managed OAuth 或第三方凭据。

旧版无法确定认证语义的 Router 不静默迁移，继续保持当前门面，直到用户在 Router 工作台保存明确的 `officialAuth`。

## 任务一：建立后端共享分类器

**修改文件**

- `src-tauri/src/proxy/providers/codex.rs`
- `src-tauri/src/proxy/providers/codex_oauth_auth.rs`

**先写失败测试**

在 `codex.rs` 测试模块加入认证矩阵：

1. 启用的 `native_codex_auth` route => `NativeMixed`。
2. 固定 `managed_codex_oauth` route => `FullyManaged`。
3. `provider_config` route => `FullyManaged`。
4. `account_pool` + 启用 Desktop 池项 => `NativeMixed`。
5. `account_pool` + 仅 managed 池项 => `FullyManaged`。
6. disabled route 不参与分类。
7. 旧 Router 无 `officialAuth` 且无法从 route 唯一推断 => `LegacyPreserved`。

执行失败测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter_auth_facade --lib
```

**最小实现**

新增：

```rust
pub enum CodexMultiRouterAuthFacade {
    NativeMixed,
    FullyManaged,
    LegacyPreserved,
}

pub fn classify_codex_multirouter_auth_facade(
    provider: &Provider,
    pool_policy: Option<&CodexAccountPoolPolicy>,
) -> CodexMultiRouterAuthFacade;
```

分类只读取启用 route、`officialAuth` 和账号池中启用的 `native_codex_auth` 项。`LegacyPreserved` 由投影层根据现有 live provider 表决定，不在分类器里猜测用户意图。

在 `codex_oauth_auth.rs` 增加只读的持久化策略读取函数；它只反序列化 `pool_policy`，不得创建第二个 OAuth manager、不得刷新 token、不得输出 token。

**提交**

```text
feat(codex): classify multirouter auth facades

本次提交由BigStrongsSun完成
```

## 任务二：动态投影 Codex live TOML

**修改文件**

- `src-tauri/src/services/proxy.rs`
- `src-tauri/src/codex_config.rs`（仅在需要复用 provider 表读取/清理助手时修改）

**先写失败测试**

在 `services/proxy.rs` 增加精确 TOML 断言：

1. MultiRouter 两种门面都固定 `model_provider = "codex_model_router_v2"`。
2. 两种门面都固定 `name = "OpenAI"`、`wire_api = "responses"`、`supports_websockets = false`。
3. `NativeMixed` 写 `requires_openai_auth = true`、写 `x-cc-switch-proxy-mode = "router"`，并删除 provider 内及顶层 `experimental_bearer_token`。
4. `FullyManaged` 写 `requires_openai_auth = false` 与 provider 内 `experimental_bearer_token = "PROXY_MANAGED"`，并删除旧 proxy-mode header。
5. `LegacyPreserved` 从现有 `codex_model_router_v2` 表保留认证门面，不保留用户 Router 名或旧 WebSocket 值。
6. 独立 `cc-switch-official` 不受影响。
7. `auth.json` 在两种 MultiRouter 门面下都不被写入占位符或 managed token。

执行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter_takeover_facade --lib
cargo test --manifest-path src-tauri/Cargo.toml official_codex --lib
```

**最小实现**

- 将 `apply_codex_proxy_toml_config_for_provider` 改成消费共享分类结果。
- MultiRouter provider `name` 不再使用 `provider.name`，精确写为 `OpenAI`。
- 只有普通非 Router 自定义 provider 继续使用自己的显示名与 `custom` provider ID。
- `LegacyPreserved` 只保留旧配置中的认证模式，不保留旧名称、URL、协议或 WebSocket 字段。
- 门面发生变化时在日志记录旧/新模式与“需重启 Codex”，不记录凭据。

**提交**

```text
fix(codex): project dynamic multirouter auth facade

本次提交由BigStrongsSun完成
```

## 任务三：收紧转发层凭据所有权与代理标识

**修改文件**

- `src-tauri/src/proxy/forwarder.rs`
- `src-tauri/src/proxy/providers/codex.rs`

**先写失败测试**

加入 forwarder 认证矩阵：

1. local Codex + native route：真实来向 bearer 原样到官方 Codex upstream。
2. local Codex + fixed managed OAuth：删除来向 bearer，注入所选 CCSM OAuth bearer。
3. pool Desktop candidate：透传来向 bearer。
4. pool managed candidate：删除来向 bearer，注入池账号 bearer。
5. provider-config route：删除 Desktop bearer，只注入 provider 自己的认证。
6. External Agent API：即使 route 标记 native，也不得透传 Desktop bearer。
7. `x-cc-switch-proxy-mode` 在所有上游协议前都被删除。
8. `PROXY_MANAGED` 在任何真实 upstream 前仍会硬失败。

执行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_auth_ownership --lib
cargo test --manifest-path src-tauri/Cargo.toml official_codex_rejects_stale_proxy_placeholder_with_restart_hint --lib
```

**最小实现**

- 将 proxy-mode header 加入统一 hop/local-only header 过滤，而不是只在某个协议分支删除。
- 仅 `should_passthrough_codex_official_auth` 返回 true 时保留来向 Authorization。
- managed OAuth、账号池 managed candidate 和 provider-config route 在注入目标认证前统一丢弃来向认证。
- 错误文本区分“Desktop 未登录”和“Fully Managed 占位符未解析”，便于 UI 给出正确操作。

**提交**

```text
fix(codex): isolate multirouter credential ownership

本次提交由BigStrongsSun完成
```

## 任务四：账号池策略变更触发门面重投影

**修改文件**

- `src-tauri/src/commands/codex_oauth.rs`
- `src-tauri/src/services/proxy.rs`
- 相关命令测试

**先写失败测试**

1. 当前 Provider 是 account-pool MultiRouter，启用 Desktop 池项后，保存策略会把 live TOML 重投影为 `NativeMixed`。
2. 禁用 Desktop、仍有 managed 池项后，重投影为 `FullyManaged`。
3. 当前 Provider 不是 account-pool Router 时不重写 Codex live 配置。
4. 重投影失败时策略保存结果必须返回可操作错误，不能假装已完全生效。

**最小实现**

- `set_codex_account_pool_policy` 保存规范化策略后检查当前 Codex Provider。
- 仅当前 Router 明确选择 `account_pool` 且接管启用时，调用已有 provider 重应用路径。
- 返回结构中携带 `facadeChanged` 与 `codexRestartRequired`，但不自动结束 Codex 进程。

**提交**

```text
fix(codex): reproject router facade after pool changes

本次提交由BigStrongsSun完成
```

## 任务五：前端展示门面与重启边界

**修改文件**

- `src/types.ts`
- `src/lib/api/codexOAuth.ts` 或现有 OAuth API 封装
- `src/components/codex/CodexRouterWorkspacePage.tsx`
- `src/components/providers/forms/CodexOAuthSection.tsx`
- 对应 Vitest 文件

**先写失败测试**

1. Desktop route 展示“Desktop / 混合认证”。
2. 固定 managed OAuth 展示“CCSM 托管认证”。
3. 账号池含 Desktop/不含 Desktop 时展示正确门面。
4. 旧歧义 Router 展示“待确认”，保存前不声称已迁移。
5. 修改 Router 官方认证方式或 Desktop 池成员后显示“需要重启 Codex 才能让已有任务采用新认证门面”。
6. 保存失败不显示已生效状态。

执行：

```powershell
pnpm test:unit -- src/components/codex/CodexRouterWorkspacePage.test.ts --run
pnpm test:unit -- src/components/providers/forms/CodexOAuthSection.test.tsx --run
pnpm typecheck
```

**最小实现**

- UI 只展示分类结果，不暴露 `requires_openai_auth` 和占位 token 开关。
- 所有新增文案使用中文。
- 重启提示是明确状态，不自动关闭或重启 Codex。

**提交**

```text
feat(codex): show router auth facade and restart state

本次提交由BigStrongsSun完成
```

## 任务六：端到端验证与项目知识更新

**修改文件**

- `memory.md`
- 必要的端到端测试 fixture，不修改用户真实 `auth.json`

**验证矩阵**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib
pnpm test:unit -- --maxWorkers=1 --minWorkers=1
pnpm typecheck
pnpm lint
```

再使用测试 HOME 或隔离的 Codex 配置目录验证：

- Native/Mixed harness 收到真实测试 bearer，且仅 native upstream 收到。
- Fully Managed harness 收到 CCSM 注入的测试凭据，不收到 `PROXY_MANAGED`。
- Chat、Messages、Anthropic 与 `/responses/compact` 路径继续按 effective route 工作。
- live TOML 门面变化后记录“需要重启 Codex”；不宣称热加载。

更新 `memory.md`，记录分类真值、迁移规则、账号池重投影、验证命令和最终提交号。

**提交**

```text
test(codex): verify dynamic auth facade matrix

本次提交由BigStrongsSun完成
```

## 完成标准

- MultiRouter 不再统一写死 `requires_openai_auth=true + PROXY_MANAGED`。
- `name = "OpenAI"` 在 Desktop OAuth、CCSM managed OAuth 和账号池路径下保持一致。
- Desktop bearer 只可能到达显式 native official upstream。
- 账号池 Desktop 成员变化会重投影当前门面，并明确要求重启 Codex。
- 旧歧义 Router 不被静默改写。
- Rust、前端单测、类型检查和格式检查全部通过。

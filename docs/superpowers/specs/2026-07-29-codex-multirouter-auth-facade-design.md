# Codex MultiRouter 动态认证门面设计

日期：2026-07-29
状态：设计方向已批准，等待实施计划

## 问题

CCSwitchMulti 当前把所有 Codex MultiRouter 都投影成同一个本地代理 provider，并统一写入 `experimental_bearer_token = "PROXY_MANAGED"`。当真实上游凭据由 CCSM 管理时，这种做法是正确的；但它与显式使用 `native_codex_auth` 的 route 冲突：这种 route 要求获得调用方 Codex Desktop 的真实 Authorization，而外层门面却让 Codex 发送占位符。

新设计必须在不改变任务身份、不泄漏凭据的前提下，同时支持四种 route 认证来源：

- Codex Desktop 当前 ChatGPT 登录。
- 指定的 CCSM 托管 ChatGPT OAuth 账号。
- 按顺序调度的 CCSM OAuth 账号池。
- 第三方 provider 自己的 API Key 或认证配置。

## 目标

- Codex 侧 MultiRouter 始终使用稳定 Provider ID：`codex_model_router_v2`。
- provider `name` 只承担能力声明，不承担用户可见的 Router 命名。
- 官方 OAuth 和 CCSM managed OAuth 都保留 OpenAI Codex 能力。
- 所有 MultiRouter 请求使用 HTTP Responses + SSE，不要求实现 Responses WebSocket 上游代理。
- 每次请求的凭据所有者必须显式、可预测。
- 迁移时保留现有 route 绑定，绝不改写 `auth.json`。
- Desktop Authorization 绝不能进入第三方上游请求。

## 非目标

- 本次不实现 Responses-over-WebSocket 转发。
- 本次不把独立 `cc-switch-official` 的任务历史合并进 MultiRouter 历史。
- 不把固定 managed OAuth 账号自动改成 Desktop 当前登录或账号池。
- 不实现 reset credit 兑换，继续由 OpenAI 官方负责。

## 术语

- Provider ID：由 `model_provider` 选择的配置键，也是任务身份。
- Provider `name`：Codex 的能力分类。当前 Codex 使用精确的 `name == "OpenAI"` 判断 OpenAI 专属行为。
- 门面（Facade）：写入 live `config.toml` 的单一 provider 表，使 Codex 把请求发送到 CCSM loopback 代理。
- Route auth source：本次请求真实上游凭据的所有者。

## 核心决策

MultiRouter 永远投影为以下稳定身份：

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
wire_api = "responses"
supports_websockets = false
```

Desktop OAuth 和 CCSM managed OAuth 都使用 `name = "OpenAI"`。`name` 不表示凭据由 OpenAI 或 Codex 持有，而是要求 Codex 保留 OpenAI Responses 能力路径，包括远程压缩和 OpenAI 工具元数据。真正的认证所有权由门面认证模式和选中的 route 决定。

用户可见的 Router 名称只保存在 CCSM 数据库和 UI 中，不再写入 Codex provider 的 `name` 字段。

## 门面模式

### Native/Mixed 门面

当任意启用中的 route 使用 `native_codex_auth`，或者账号池可能选中 Desktop 当前登录账号时，使用此模式：

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = true
wire_api = "responses"
supports_websockets = false
http_headers = { "x-cc-switch-proxy-mode" = "router" }
```

此门面禁止出现 `experimental_bearer_token`。Codex 必须把当前登录的真实 Authorization 发给 CCSM。独立的 proxy-mode header 只用于识别本地 Router 流量，不能占用上游 Authorization，并且必须在出站前删除。

请求行为：

| Route 来源 | 出站凭据行为 |
| --- | --- |
| `native_codex_auth` | 保留 Codex 来向 Authorization 和账号身份。 |
| `managed_codex_oauth` | 删除来向 Authorization，注入指定 CCSM OAuth 账号。 |
| `account_pool` 的 Desktop 候选 | 保留 Codex 来向 Authorization。 |
| `account_pool` 的 managed 候选 | 删除来向 Authorization，注入选中的 CCSM OAuth 账号。 |
| `provider_config` | 删除来向 Authorization，只注入目标 provider 凭据。 |

### Fully Managed 门面

当所有启用中的 route 都不可能使用 Desktop 当前登录认证时，使用此模式：

```toml
model_provider = "codex_model_router_v2"

[model_providers.codex_model_router_v2]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = false
experimental_bearer_token = "PROXY_MANAGED"
wire_api = "responses"
supports_websockets = false
```

Codex 只发送本地占位符。CCSM 必须在所有 managed OAuth 或第三方上游请求前替换该占位符。占位符到达 managed upstream 前仍然必须硬失败。

## 门面分类规则

门面类型根据启用中的 route 和持久化 Router 官方认证策略推导：

1. 任意 `native_codex_auth` route 选择 Native/Mixed。
2. `account_pool` route 在策略启用 Desktop 账号时选择 Native/Mixed，否则选择 Fully Managed。
3. 固定 `managed_codex_oauth` 和 `provider_config` route 不要求 Native/Mixed。
4. 无法确定的旧配置不得静默重分类。用户确认并保存 Router 认证策略之前，保持当前门面行为。

分类器必须是后端共享函数，同时供 config 投影、状态展示、迁移预览和测试调用。前端推断只负责展示，不能成为运行时真值。

## 独立官方接管

原版 CC Switch v3.17.0 的独立官方接管保持不变：

```toml
model_provider = "cc-switch-official"

[model_providers.cc-switch-official]
name = "OpenAI"
base_url = "http://127.0.0.1:<port>/v1"
requires_openai_auth = true
wire_api = "responses"
supports_websockets = false
```

它始终使用 Codex 当前登录，不参与 CCSM 账号调度。

## 能力与协议边界

Codex 侧门面统一声明原生 OpenAI Responses 能力，真实上游协议仍由本次 effective route 决定：

- 官方和 managed ChatGPT OAuth route 使用原生 Responses。
- OpenAI-compatible Chat route 使用现有 Responses-to-Chat 转换。
- Messages 和 Anthropic route 使用现有转换路径。
- `/responses/compact` 必须按 compact 请求自身的 model 重新选 route，并按 effective route 转换。

对于非 OpenAI route，CCSM 必须在出站前删除上游不支持的 OpenAI 专属元数据、工具、认证和指纹 header。Route catalog capability 继续作为图片、工具、reasoning 和模态能力的真值。

## WebSocket 边界

`wire_api = "responses"` 描述应用协议，`supports_websockets = false` 表示使用 HTTP POST + SSE 运输。MultiRouter 保持禁用 WebSocket，因为每个 HTTP 请求都包含完整 model，可以独立路由；持久 socket 则需要维护逐 frame 的模型、账号、重试和 failover 状态。

自定义 MultiRouter 门面不需要通过 `426` 回退。`426` 只适用于保留内置 `openai` 并使用 `openai_base_url` 的方案。

## 安全边界

- 绝不把 `PROXY_MANAGED` 或 CCSM managed OAuth token 写入 Codex `auth.json`。
- proxy-mode header 绝不能发往上游。
- Desktop Authorization 绝不能发往 `provider_config` 或 managed-account route。
- External Agent API 请求不能借用 Desktop 当前登录凭据。
- 日志和诊断只记录 auth strategy 和 account ID，绝不记录 bearer 值。
- 只有本地 Codex 来源且 effective route 显式选择 native 时，才允许透传 native Authorization。

## 迁移

- `codex_model_router_v2` 继续作为任务 Provider ID。
- 保留每条 route 的准确 auth source 和 managed account ID。
- 显式包含 native route 的 Router 重建为 Native/Mixed，并从门面删除占位符。
- 只包含 managed route 的 Router 保留占位符并重建为 Fully Managed。
- 无法确定的旧 Router 显示迁移选择，保存前不改变 live 行为。
- 门面类型变化后明确标记必须重启 Codex，不宣称现有 session 能热加载认证。
- 迁移绝不插入、删除或替换 Codex 登录记录。

## UI

Router 工作台提供一项官方认证策略：

- Desktop 当前登录。
- 固定 CCSM OAuth 账号。
- OAuth 账号池。

UI 只读展示生成的门面类型：`Desktop/Mixed` 或 `Fully managed`。不把 `name`、`requires_openai_auth`、`experimental_bearer_token` 暴露成独立开关，避免用户组合出无效状态。

修改策略或在账号池中启用 Desktop 账号后，保存成功即展示需要重启 Codex。

## 错误处理

- Native/Mixed 没有可用来向 Codex Authorization 时，返回可操作的 Desktop 登录错误。
- Fully Managed 无法解析占位符时，在发起网络请求前返回内部认证解析错误。
- 固定 managed 账号缺失时，返回该账号需要重新登录的错误，不得回退到 Desktop。
- Desktop 池条目不可用时，只有存在其他显式启用的 managed 候选才允许跳过，否则返回 Desktop 登录错误。
- 除非 Router 显式启用对应 fallback，否则认证失败不得把官方账号静默切换到第三方 route。

## 验证要求

实现验收必须包含：

1. 两种门面的 config 投影测试，并断言精确 `name = "OpenAI"`。
2. Codex 认证 harness，证明 Native/Mixed 发送调用方真实 bearer，Fully Managed 发送占位符。
3. 覆盖 native、固定 managed OAuth、pool-native、pool-managed、provider-config 的端到端 forwarder 测试。
4. 凭据泄漏测试，证明 Desktop Authorization 不能到达第三方或 External Agent API 上游。
5. 覆盖官方、Chat、Messages、Anthropic route 的 HTTP/SSE 和 `/responses/compact` 路由测试。
6. 覆盖显式 native、固定账号、含/不含 Desktop 的账号池、旧版歧义 Router 的迁移测试。
7. 覆盖策略持久化、门面状态、账号顺序、保留额度、重启提示的前端测试。
8. 重建 CCSM 后，使用实际 Codex 0.146 系列发起真实请求；代理日志必须证明 route、auth strategy、transport 和上游状态，且不能记录凭据。

## 放弃的方案

- 拆成两个 Router Provider ID：会拆分任务历史并重复 catalog 和 UI 状态，因此放弃。
- 所有模式统一使用内置 `openai + openai_base_url`：内置 provider 不能安全覆盖 managed placeholder，也不能直接关闭 WebSocket，因此放弃。
- 永远使用 `PROXY_MANAGED`：无法向 native route 传递 Desktop 当前登录 Authorization，因此放弃。
- 永远使用 Desktop Authorization：Fully Managed 用户必须能在没有 Codex Desktop 登录时使用，且第三方 route 不能收到该凭据，因此放弃。

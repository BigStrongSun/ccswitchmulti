<!-- guide-page: 00-overview.png | 入门准备 -->

# CCSwitchMulti Codex 多路由使用说明

> 适用版本：CCSwitchMulti v3.16.3 及以上。本文面向已经安装 CCSwitchMulti、并希望在 Codex Desktop 里同时使用官方 GPT、DeepSeek、GLM、本地模型等多个模型源的用户。

## 先理解这条链路

Codex Desktop 仍然只看到一个“当前模型提供方”，但这个提供方会被 CCSwitchMulti 接管成一个本地 MultiRouter。你在 Codex 里选择不同模型时，请求先进入 CCSwitchMulti 本地路由，再由路由规则转发到 OpenAI 官方 OAuth、DeepSeek、GLM、OpenAI-compatible 中转站或本地模型。

这里不是把 Codex Desktop 内置 WebSocket 做传统反代。官方 GPT / Codex 侧使用 CCSwitchMulti 托管的 OAuth 授权请求；第三方模型源通常走 OpenAI Chat Completions 或兼容接口，再由 CCSwitchMulti 转换成 Codex 需要的 Responses 形态。

## 准备工作：你需要什么

- ✅ 已安装并能启动的 Codex Desktop App。
- ✅ 已安装 CCSwitchMulti。
- 🔐 一个能登录 Codex Desktop 的 ChatGPT / Codex 账号。
- 🧩 你要接入的额外模型源，例如 DeepSeek、GLM、OpenRouter、硅基流动、Ollama / vLLM / LM Studio 等本地服务。
- 🔑 每个额外模型源的 Base URL、API Key，以及可用模型名。

> 注意：使用 ChatGPT / Codex OAuth 访问官方 Codex 后端可能涉及服务条款、账号风控和长期可用性风险。不要分享 `~/.codex/auth.json`、OAuth token、refresh token 或真实上游 API Key。

<!-- guide-page: 01-flow.png | 总流程速览 -->

## 总流程速览

1. 🔐 在 Codex Desktop App 里先完成官方登录。
2. 🔗 在 CCSwitchMulti 的 `设置 → 认证` 里完成 ChatGPT / Codex OAuth 授权。
3. ➕ 在 Codex 面板添加 DeepSeek、GLM 或本地模型等“模型源”。
4. 🧭 对额外模型源开启 `需要本地路由映射`，填写 Base URL / API Key，获取模型列表并配置上下文窗口。
5. 🧱 进入 `Codex 多模型路由` 工作台，创建多路路由方案。
6. 🚦 添加并启用路由规则，配置模型范围、前缀、可见别名和认证策略引用。
7. 🤖 确认 V4 Pro / Flash 已进入最终可路由模型目录；CCSwitchMulti 会自动注册对应 custom roles。
8. 💾 保存并选中该多路由方案；只有需要手工覆盖 `spawn_agent.model` 展示顺序时才展开高级设置。
9. ▶️ 在 `设置 → 路由` 打开路由总开关和 Codex 路由，启动 MultiRouter。
10. 🔄 完全退出并重启 Codex Desktop，检查模型列表和请求连通性。
11. 🕘 如果历史记录暂时消失，进入会话管理的 `Codex 历史修复`，预览修复并确认写入，再次重启 Codex。

完成后，你应该同时验证两件事：Codex 模型列表能看到 MultiRouter 汇总的候选模型；不写模型名的委派提示能让 Codex 按任务选择 `deepseek-flash` 或 `deepseek-pro` custom role。

<!-- guide-page: 02-step-1.png | 1. 先登录 Codex Desktop -->

## 1. 先登录 Codex Desktop

先启动 Codex Desktop App，用你的 ChatGPT / Codex 账号完成官方登录。这个步骤的目的不是让所有请求都走官方模型，而是让 Codex Desktop 和 CCSwitchMulti 都有一个健康的官方 OAuth 登录基础。

登录完成后，Codex 会把官方登录材料保存到本机 `~/.codex/auth.json`。后续不要手动复制、编辑或分享这个文件。

<!-- guide-page: 03-step-2.png | 2. 在 CCSwitchMulti 里完成 OAuth 授权 -->

## 2. 在 CCSwitchMulti 里完成 OAuth 授权

打开 CCSwitchMulti，进入：

```text
设置 → 认证
```

找到 ChatGPT / Codex OAuth 认证区域，点击 `使用 ChatGPT 登录`，按页面提示完成授权。授权完成后，CCSwitchMulti 会托管这个 OAuth 登录，用于 MultiRouter 里的官方 GPT / Codex 路由。

![设置认证页中的 ChatGPT / Codex OAuth 授权入口](../images/codex-multirouter/01-settings-auth-oauth.png)

如果授权状态显示未认证、会话过期或无法刷新，请先在这里重新登录，再继续配置多路由。

<!-- guide-page: 04-step-3.png | 3. 添加额外模型源 -->

## 3. 添加额外模型源

回到 Codex 面板，点击右上角加号添加模型源。常见选择包括：

![Codex 面板右上角添加供应商入口](../images/codex-multirouter/02-add-provider-entry.png)

- DeepSeek
- GLM / 智谱
- Kimi / MiniMax / 硅基流动 / OpenRouter 等中转或兼容服务
- 本地部署的 Ollama / vLLM / LM Studio / OpenAI-compatible 服务

添加时至少填写：

- `API 请求地址` 或 `Base URL`
- `API Key`
- 默认模型或模型映射

![供应商配置页中填写 API 请求地址、Key 并开启本地路由映射](../images/codex-multirouter/03-configure-provider-local-routing.png)

<!-- guide-page: 05-step-4.png | 4. 开启本地路由映射并获取模型列表 -->

## 4. 开启本地路由映射并获取模型列表

如果额外模型源本身不支持 OpenAI `/v1/responses`，而只支持 `/v1/chat/completions` 或类似 Chat 格式，建议直接开启：

```text
需要本地路由映射
```

这一步很关键。Codex Desktop 和 Codex CLI 强绑定 Responses 风格请求，大多数第三方模型并不原生支持完整 Responses 行为。开启本地路由映射后，CCSwitchMulti 会在本地把 Codex 的 Responses 请求转换给第三方接口，再把返回结果转换回 Codex 能读的响应。

继续向下展开 `高级选项`，找到 `模型映射`：

1. 点击 `获取模型列表`。
2. 等待 CCSwitchMulti 从该模型源拉取可用模型。
3. 检查每个模型的显示名。
4. 为模型填写或确认 `上下文窗口`。
5. 保存模型源。

![模型映射区域中获取模型列表并填写上下文窗口](../images/codex-multirouter/04-fetch-models-context-window.png)

如果 `获取模型列表` 失败，先检查 Base URL、API Key、网络和该服务是否真的暴露 `/models` 或兼容模型列表接口。模型列表失败不会自动证明模型不可用，但会影响 MultiRouter 候选模型和子 Agent 候选排序。

<!-- guide-page: 06-step-5.png | 5. 创建 Codex 多模型路由 -->

## 5. 创建 Codex 多模型路由

在 CCSwitchMulti 右上角找到路径 / 路由形状的入口，打开：

```text
Codex 多模型路由
```

进入工作台后，点击 `创建多路路由`。这里创建的不是普通上游模型源，而是一套 Codex MultiRouter 方案。它会引用你刚才配置好的 OpenAI 官方 OAuth、DeepSeek、GLM、本地模型等模型源。

![右上角 Codex 多模型路由入口](../images/codex-multirouter/05-multirouter-entry.png)

目前建议为每套方案起一个清晰名称，例如：

- `Codex GPT + DeepSeek + GLM`
- `Codex OpenAI + Local vLLM`
- `Daily MultiRouter`

![MultiRouter 工作台中的创建多路路由按钮](../images/codex-multirouter/06-create-multirouter.png)

<!-- guide-page: 07-step-6.png | 6. 添加并启用路由规则 -->

## 6. 添加并启用路由规则

在工作台进入 `路由规则` 页，点击 `编辑匹配规则` 或添加规则。选择你希望加入这套 MultiRouter 的模型源，并确认它们处于启用状态。

![路由规则页中添加和启用模型源](../images/codex-multirouter/07-configure-route-rules.png)

每条 schema v2 规则至少要确认：

- 路由名称：给用户看的名称，例如 `DeepSeek`、`GLM`、`Local vLLM`。
- 模型选择：`全部模型` 会自动接收目标 Provider 后续新增模型；`仅选中的 canonical 模型` 不会扩大用户选择。
- 匹配前缀：用于没有精确可见名时的前缀匹配；精确匹配始终优先。
- 可见别名：只在确实需要不同显示名时填写 `visible=canonical`，目标必须是该 Provider 的 canonical 模型 ID。
- 认证策略引用：选择 Provider 配置认证、Codex Desktop 当前登录、托管 Codex OAuth 或账号池；这里只保存引用 ID，不保存 Token。

Base URL、API Key、默认协议、模型级协议覆盖、上下文窗口、输入模态、推理和缓存能力都在目标 Provider 的“模型与兼容性”中维护。Route 编辑器不会复制这些字段；修改 Provider 后，下一次请求直接使用新值，无需删除或重建 Route。

如果打开的是旧 schema v1 方案，CCSwitchMulti 会先显示迁移预览。确认新建 Provider、引用变化、删除的冗余字段、冲突和警告后再应用迁移；预览和日志不会显示 API Key/Token。迁移完成前旧方案可查看，但不能直接编辑或启用。

规则配置完成后，保存路由方案。

![路由规则配置完成后保存方案](../images/codex-multirouter/08-save-route-rules.png)

<!-- guide-page: 08-step-7.png | 7. 子 Agent 自动角色与高级覆盖 -->

## 7. 子 Agent 自动角色与高级覆盖

只要 `deepseek-v4-flash` 和 `deepseek-v4-pro` 位于最终可路由模型目录，CCSwitchMulti 就会分别注册 `deepseek-flash` 和 `deepseek-pro` custom role。父 Codex 决定委派后，会根据角色描述自动选择：长上下文扫描和轻量验证优先 Flash，复杂调试、跨模块推理和高风险实现优先 Pro。用户不需要在主向导里选择子 Agent 模型。

`路由规则` 页仍保留默认折叠的 `高级：子 Agent 模型覆盖`。它只控制 Codex V2 直接 `spawn_agent.model` 参数描述里的前 5 个模型顺序，不控制 custom role 是否注册，也不是 Pro/Flash 自动选型的主路径。

建议做法：

只有明确需要 direct model override 时才展开这里：

1. 选择最多 5 个希望显示在 `spawn_agent.model` 描述中的模型。
2. 拖动调整它们的展示顺序。
3. 点击 `保存排序`。

![子 Agent 候选模型排序和保存排序](../images/codex-multirouter/09-subagent-model-order.png)

保存后，MultiRouter 会把这个顺序作为 `codexRouting.spawnAgentModels` policy 保存；compiler 再按当前可路由 catalog 过滤并投影。无论是否设置这份高级排序，managed custom roles 都从完整可路由目录生成。Codex Desktop 仍然需要重启后才会刷新模型和角色。

<!-- guide-page: 09-step-8.png | 8. 选中 MultiRouter 并启动路由 -->

## 8. 选中 MultiRouter 并启动路由

回到 CCSwitchMulti 主界面，选中刚才创建的 MultiRouter 方案。

再进入：

```text
设置 → 路由
```

依次打开：

1. 路由总开关。
2. Codex 路由开关。

然后启动本地路由服务。默认情况下，Codex takeover 端口通常是：

```text
127.0.0.1:15721
```

![设置页中同时开启路由总开关和 Codex 路由](../images/codex-multirouter/10-enable-routing-settings.png)

不要把这个端口和对外 OpenAI-compatible API 端口混淆。Codex Desktop takeover 使用的是本地 Codex 路由端口；外部 agent API 可能使用另一个端口。

<!-- guide-page: 10-step-9.png | 9. 使用状态 / Debug 检查链路 -->

## 9. 使用状态 / Debug 检查链路

启动后，回到 `Codex 多模型路由` 工作台，进入 `状态` 页，运行 Debug 检查。

![MultiRouter 状态页中的 Debug 检查入口](../images/codex-multirouter/11-debug-entry.png)

重点看这些结果：

- 本地代理是否运行。
- Codex live config 是否指向 CCSwitchMulti 本地路由。
- 当前 MultiRouter 方案是否被选中。
- 启用路由数量是否正确。
- 模型目录是否包含你配置的模型。
- dependency fingerprint 是否为最新、投影是否处于 ready；pending 表示 live 文件发布失败但数据库真值仍保留，可以重试投影。
- 诊断里的最终 Provider、canonical/upstream model、协议、认证所有者和能力来源是否符合预期；诊断不会回传凭据。
- 最近日志里是否出现 `route_resolved`、`request_prepared`、`upstream_send`、`upstream_status`。

如果 Debug 里显示端口可达但没有近期路由事件，请先在 Codex Desktop 里发送一条测试消息，再回来看日志。没有日志通常说明 Codex 还没有真正走到 MultiRouter，而不是第三方上游一定坏了。

<!-- guide-page: 11-step-10.png | 10. 完全退出并重启 Codex Desktop -->

## 10. 完全退出并重启 Codex Desktop

完成 MultiRouter 配置后，必须完全退出 Codex Desktop App，再重新启动。

重启是必要步骤，因为 Codex Desktop 通常在启动时读取：

- `~/.codex/config.toml`
- `model_catalog_json`
- `models_cache.json`
- Desktop 内部模型候选快照

只在 CCSwitchMulti 里保存配置，不一定会让正在运行的 Codex Desktop 热刷新模型菜单。

重启后检查：

1. 模型候选列表里是否出现你新增的 DeepSeek、GLM、本地模型等。
2. 官方 GPT 路由是否仍可用。
3. 给 Codex 发送一条简单测试消息。
4. 回到 CCSwitchMulti 状态页确认请求命中了正确路由。

如果配置正确，此时 Codex 的模型候选列表里应该能看到你在 MultiRouter 里设置的所有备选模型。验证子 Agent 时不要写模型名：分别要求子 Agent 做长上下文扫描和复杂修复设计，检查父 Codex 是否选择了对应的 `deepseek-flash` / `deepseek-pro` role。

![Codex Desktop 中显示 MultiRouter 提供的模型候选列表](../images/codex-multirouter/13-codex-model-picker-validation.png)

如果模型候选仍然只有少数 OpenAI 模型，优先确认所有 Codex Desktop / app-server 进程已经完全退出；必要时从 CCSwitchMulti 重新启动或解锁 Codex Desktop，再检查模型列表。

<!-- guide-page: 12-step-11.png | 11. 修复 Codex 历史记录显示 -->

## 11. 修复 Codex 历史记录显示

切到 MultiRouter 后，Codex 历史记录可能看起来被清空。这通常不是对话文件丢失，而是历史记录的 provider bucket 从官方或旧路由桶切到了新的 MultiRouter 桶。

进入右上角倒数第二个时钟 / 会话管理入口，打开：

```text
会话管理 → 历史修复
```

在 `Codex 历史修复` 面板里：

![会话管理中的 Codex 历史修复面板](../images/codex-multirouter/12-13-history-repair-panel.png)

1. 点击 `加载历史`，让工具读取 active SQLite 历史。
2. 如果要把当前页结果全部纳入修复，使用全选当前加载页。
3. 其他选项不确定时保持默认。
4. 点击 `预览修复`。
5. 确认预览结果、目标 provider、active DB 和计数正常。
6. 点击 `确认写入`。

写入完成后，再次完全退出并重启 Codex Desktop。历史记录应该重新出现在 Codex 的会话列表中。

更多历史机制和数据安全说明可参考 [统一 Codex 会话历史：功能介绍与使用攻略](./codex-unified-session-history-guide-zh.md)。

<!-- guide-page: 13-faq.png | 常见问题 -->

## 常见问题

### 为什么额外模型源建议开启“需要本地路由映射”？

因为 Codex 请求是 Responses 风格，而大量第三方模型源只稳定支持 Chat Completions。不开启本地路由映射时，Codex 可能直接请求第三方 `/responses`，常见结果是 404、400、流式格式不兼容或工具调用解析失败。

### 为什么保存模型映射后还要重启 Codex？

Codex Desktop 的模型菜单和 custom roles 不是每次都热加载。修改模型映射、上下文窗口、managed roles 或高级 direct override 顺序后，完整退出并重启 Codex 最稳。

### 为什么高级子 Agent 模型覆盖只有前 5 个？

这是 Codex 在 `spawn_agent.model` 工具描述中展示 direct model override 的固定窗口。它不限制 custom agent role 数量；V4 Pro / Flash 自动选型使用完整模型目录生成的 roles，因此不会因 Pro 位于第 6 位而消失。

### 为什么历史记录看起来没了？

通常是历史 provider bucket 改变导致 Codex 当前列表过滤不到旧会话。先用 `Codex 历史修复` 做 dry-run，再确认写入。这个流程会在写入前备份，不会删除你的 `.jsonl` 会话正文。

### 如何判断请求真的走到了 MultiRouter？

不要只看 Codex 左下角账号或模型显示。以 CCSwitchMulti 状态页、Debug 结果和 `codex-router.log` 为准。出现 `route_resolved` 和对应上游状态，才说明请求进入了 MultiRouter。

### 修改 Provider 协议后，为什么不需要重新建 Route？

schema v2 Route 只引用 `targetProviderId` 和 canonical 模型，不保存协议快照。每次请求都会从数据库读取最新 Provider/模型条目，再由 compiler 生成 effective Provider。把 Qwen 从 Chat 改为 Responses（或反向修改）后直接发送测试请求，并检查日志中的 `request_prepared effective_endpoint` 和转换标记即可；只有 Codex 模型菜单缓存需要时才重启 Desktop。

<!-- guide-page: 14-related-docs.png | 相关文档 -->

## 相关文档

- [Codex 本地模型路由指南](./codex-deepseek-routing-guide-zh.md)
- [使用第三方 API 时保留 Codex 远程操作和官方插件](./codex-official-auth-preservation-guide-zh.md)
- [统一 Codex 会话历史：功能介绍与使用攻略](./codex-unified-session-history-guide-zh.md)
- [添加供应商](../user-manual/zh/2-providers/2.1-add.md)
- [本地路由](../user-manual/zh/4-proxy/4.2-routing.md)

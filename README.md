<div align="center">

# CCSwitchMulti

### 面向 Codex 的多模型路由与 Provider 管理工具

[![Version](https://img.shields.io/github/v/release/BigStrongSun/ccswitchmulti?color=blue&label=version)](https://github.com/BigStrongSun/ccswitchmulti/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/BigStrongSun/ccswitchmulti/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/BigStrongSun/ccswitchmulti/total)](https://github.com/BigStrongSun/ccswitchmulti/releases/latest)

English | [中文](README_ZH.md) | [日本語](README_JA.md) | [Deutsch](README_DE.md) | [Changelog](CHANGELOG.md)

</div>

<div align="center">

<img src="assets/xiaohongshu-discussion-qr.png" alt="小红书讨论群二维码" width="180" />

**求助和反馈**：可以提交 GitHub Issue，也可以扫码加入小红书讨论群一起讨论。（二维码有效期至 2026-07-20）

</div>

## CCSwitchMulti 分支说明

CCSwitchMulti 是面向 Codex 多模型工作流维护的桌面工具。它提供 Provider 数据库、本地代理、MCP/Skills 同步、会话管理、云同步和 Tauri 跨平台结构，同时加入 Codex MultiRouter 工作流，让多个模型来源可以合并到同一个 Codex Provider 后面使用。

使用 `BigStrongSun/ccswitchmulti` 发布版本时，请先阅读本节，因为这里记录的是 CCSwitchMulti 的核心能力、实现边界和使用注意事项。

### Codex 多路由配置说明书

如果你是第一次配置 Codex MultiRouter，请先看这份中文说明书：

**[CCSwitchMulti Codex 多路由使用说明](docs/guides/codex-multirouter-guide-zh.md)**

它按实际操作顺序覆盖 Codex Desktop 登录、CCSwitchMulti OAuth 授权、添加 DeepSeek / GLM / 本地模型源、开启 `需要本地路由映射`、获取模型列表、配置上下文窗口、创建多模型路由、设置子 Agent 前 5 个候选模型、启动 Codex 路由、Debug 检查、重启 Codex Desktop，以及历史记录修复。

### 适合谁使用

这个分支特别适合已经有 ChatGPT Pro、Plus 或 Team 订阅，并且希望把 GPT 系列最新、最强模型作为主 Agent 来做规划、决策和质量把关的用户。你可以让主 Agent 继续使用官方 GPT/Codex 能力，同时把大量可拆分的执行任务路由到自己的廉价 API、本地部署模型，或 DeepSeek V4、Qwen 等国产/开源模型上，从而降低 Codex 官方额度消耗。

典型用法是：主线程使用 GPT-5.5 / GPT-5.4 负责复杂判断、任务拆解和最终审查；子 Agent、批量执行、简单修复、日志分析、重复验证等工作交给 DeepSeek V4 Flash、Qwen、本地 vLLM 或其他 OpenAI-compatible API。按我们的实际测试，这种“强主 Agent + 低成本执行模型”的组合在不少 Codex 工作流里可以至少节约一半官方额度，具体节省比例取决于你的任务结构、路由规则和上游价格。

### 功能截图

#### Provider 列表中的 MultiRouter

![CCSwitchMulti Provider 列表](assets/screenshots/ccswitchmulti/provider-list.png)

`OpenAI Multi-Model Router` 会作为一个 Codex Provider 出现在列表中。它不是普通单一上游，而是一个本地路由入口：Codex 只连到 CCSwitchMulti，本地代理再按模型把请求分发到 OpenAI、Qwen、DeepSeek 或其他上游。

#### Codex 多模型路由工作台

![Codex 多模型路由状态页](assets/screenshots/ccswitchmulti/multirouter-status.png)

多模型路由工作台会展示路由入口、本地监听、Codex 接管、启用规则和最近转发状态。这里用于判断 Codex 请求是否真的进入 MultiRouter，而不是只看模型菜单是否出现。

![Codex 多模型路由规则](assets/screenshots/ccswitchmulti/multirouter-routes.png)

路由规则页可以把同一个 Codex 入口拆成多个上游规则：例如 `gpt-*` 走官方 OpenAI/Codex，`qwen3.6` 走本地或远端 vLLM，`deepseek-*` 走 DeepSeek API。规则启用后，Codex 侧只需要按模型名选择即可。

#### Codex Desktop 中的模型选择

![Codex Desktop 模型选择器](assets/screenshots/ccswitchmulti/codex-model-picker.png)

接管成功后，Codex Desktop 的模型选择器可以同时看到 GPT-5.5、GPT-5.4、GPT-5.4 Mini、Codex Spark、Qwen3.6 Local、DeepSeek V4 Flash、DeepSeek V4 Pro 等候选模型。主 Agent 可以用官方 GPT，子任务可以切到更便宜的模型。

#### 使用统计与成本观测

![CCSwitchMulti 使用统计](assets/screenshots/ccswitchmulti/usage-statistics.png)

统计页可以按模型查看请求数、token 和成本。截图中的工作流同时使用了 GPT-5.5、DeepSeek V4 Flash、Qwen3.6、GPT-5.4 Mini、GPT-5.4 和 Codex Spark，便于评估哪些任务适合迁移到低成本模型。

### 本分支额外提供的能力

- **Codex MultiRouter Provider**：提供一个通常名为 `OpenAI Multi-Model Router` 的 Codex Provider，可在同一个 Codex 模型选择器里展示并路由官方 OpenAI/Codex、Codex Spark、Qwen、DeepSeek 等模型来源。
- **模型目录投影**：在 CC Switch Provider 配置中维护路由模型目录，并写出 Codex 可读取的 `model_catalog_json`、`cc-switch-model-catalog.json` 和 CC Switch 接管的 `models_cache.json`，让 Codex 能发现合并后的候选模型。
- **按模型分流**：通过 `settings_config.codexRouting` 保存路由规则；Rust 本地代理会读取每次请求里的 `model`，选择匹配的上游，注入对应凭据，并在需要时把 OpenAI Responses 请求桥接到 Chat Completions 后端。
- **稳定的 Codex 运行桶**：MultiRouter 使用 `codex_model_router_v2` 作为运行时 provider bucket，而不是 Codex 内置 `openai` 或易漂移的通用 custom bucket，从而避免重新触发官方 OpenAI WebSocket 语义，并减少 Codex 历史记录分桶混乱。
- **Codex Desktop 模型菜单解锁**：包含运行时诊断和基于 CDP 的 renderer 注入，用于处理 Codex Desktop 里 Statsig 模型白名单导致本地/路由模型被隐藏的问题。
- **Codex 历史显示修复**：提供独立的历史修复工作区，可先 dry-run，再修复 provider bucket、session index、project hints、user-event 标记和当前 Desktop sqlite 位置等问题。
- **外部 OpenAI-compatible API sidecar**：提供单独的本地 OpenAI-compatible API 表面，给第三方客户端使用；它和 Codex takeover 端口不是同一路。

### 实现方式

Codex MultiRouter 不是简单地把 Codex 切到某一个第三方 Provider。CCSwitchMulti 会为 Codex 启用 app-level takeover，启动本地 Codex 代理端口，把 Codex live config 写成指向本地的 Responses-compatible Provider，并把真实上游、模型目录和路由计划保存在 CC Switch 数据库里。

关键实现点包括：

- Codex live config 中的 MultiRouter 运行桶是 `model_provider = "codex_model_router_v2"`。
- Codex config 顶层写入 `model_catalog_json = "cc-switch-model-catalog.json"`，同时在用户 Codex 配置目录下生成 catalog/cache 文件。
- `settings_config.modelCatalog` 是 CC Switch 侧维护可见模型的事实来源。
- `settings_config.codexRouting` 是 CC Switch 侧维护模型到上游路由规则的事实来源。
- 本地 router provider 写入 `supports_websockets = false`，让 Codex 走 HTTP Responses 路径，避免回到内置 OpenAI WebSocket 行为。
- Desktop 集成保留 `requires_openai_auth = true`，这样 ChatGPT OAuth 账号和额度状态仍可在 Codex Desktop 中显示，但实际请求仍由本地 MultiRouter 接管。

### 使用注意

- 需要 CCSwitchMulti 能力时，请使用 [BigStrongSun/ccswitchmulti](https://github.com/BigStrongSun/ccswitchmulti/releases) 的发布版本。
- Codex 使用 `OpenAI Multi-Model Router` 时必须保持 CCSwitchMulti 运行，因为 Codex 请求会经过本地 takeover 代理。
- 修改 router 模型目录、路由规则或 takeover 状态后，需要完整退出并重新打开 Codex Desktop；已经运行的 Codex app-server 可能继续持有旧的模型管理器缓存。
- 如果诊断显示 catalog 已完整，但 Codex Desktop 模型菜单仍只显示官方模型，请通过 CCSwitchMulti 的模型菜单解锁流程启动 Codex，让 renderer 带 remote debugging 端口运行并接受运行时补丁。
- CCSwitchMulti 不会修改 Codex Desktop 磁盘上的 `app.asar`；模型菜单解锁是针对当前 Desktop 会话的运行时 renderer 注入。
- 不要把 router TOML、`model_catalog_json` 或 `127.0.0.1:<port>` 写进共享的 Codex common config。这些是 Provider takeover 私有字段，应由 CCSwitchMulti 写入。
- 不要让 MultiRouter 走 Codex 内置 `openai` Provider 或 `openai_base_url`。那条路径可能重新启用官方 OpenAI/WebSocket 语义，破坏路由和 fallback 边界。
- Qwen、DeepSeek 等非 OpenAI 路由仍依赖对应上游 endpoint、API key 和网络可用性。模型出现在菜单里只说明 catalog 可见，不代表请求一定成功。
- Codex takeover 端口和外部 OpenAI-compatible API sidecar 是两套不同入口；不要用 sidecar 的健康检查来判断 Codex MultiRouter 是否已经接管成功。

### 构建与发布说明

- 当前分支的包名/产品名是 `ccswitchmulti` / `CCSwitchMulti`。
- Windows 发布导出使用 `pnpm release:export`；本地打包在没有签名私钥时会显式关闭 updater artifact 签名。
- 免安装版仍使用系统默认用户数据和配置目录，因此除非明确要共享状态，否则不要同时运行多个安装版或便携版实例。
- macOS 产物需要 macOS 构建、签名和 notarization 环境；Windows/WSL 构建不会产出已签名公证的 macOS 包。

# Codex `version` 请求头与新模型拒绝（2026-09-30）

- 用户提供的同账号、同 token、同模型和正文对照：额外带 `version: 0.158.0-alpha.2.1` 得 HTTP 400“ 不支持该模型 ”，不带则得 HTTP 200。此附件是排查线索，不等于本机完整运行态验收。
- 当前运行中的 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 文件版本为 `3.20.2-22`，不是刚发布但未安装的 `3.20.4-1`。因此不能把当前运行失败归因于新版已安装。没有改动运行进程、凭据或用户配置。
- 根因链：旧提交 `fb2adf271` 为追求原生请求等价，在官方 Codex 转发末端从可信 User-Agent 提取构建号并合成独立 `version`；`forwarder.rs` 的普通 JSON、raw 与 WebSocket 路径都会调用该函数。官方 Codex `default_client.rs::default_headers()` 的一手源码默认只放 `originator` 和 `User-Agent`，并不默认放 `version`。同一个值作为 UA 内信息与独立请求头可触发不同上游模型准入，不能把它们视为等价。
- 第三方路径的另一侧边界：`codex_request.rs::apply_provider_header_policy` 会替换客户端 UA，却曾继续透传客户端 `version`；Codex raw/WebSocket 构造器也会透传该头。修复策略是分清 header 归属：官方路由移除独立 `version`，不再合成；第三方普通路径和 raw/WebSocket 默认剥离 Codex 入站版本指纹，Provider 明确配置的覆盖值仍可设置。Codex→Anthropic Messages 转换另有指纹过滤表，也须过滤独立 `version`，同时保留 `anthropic-version`。
- 三条新回归分别对官方 alpha UA、不受控第三方请求、raw 透传先 RED 后 GREEN；另有第三方显式 header override 的保留用例。代码仅在源码与本地构建候选生效，尚未替换安装态。
- 原版 v3.20.4 的 MiniMax Code 是单独的受管应用集成，主提交 `06082e189` 改动 81 文件、3305 行，涉及配置/MCP/Skills/会话/用量/UI 与 schema。原版升级线 v18→v19，CCSM 当前 schema 为 v25；不能直接 cherry-pick 或套用原版迁移。此前 v3.20.4-1 的目标是选择性同步代理和认证修复，不是完整跟进所有新功能。
- 检索：Codex 内置 Web 搜索命中原版官方 v3.20.4 发布说明及 OpenAI Codex 官方 `default_client.rs`；Matrix WebSearch 独立检索但结果不相关或官方 GitHub 打开失败。关键技术结论依据本地 CCSM/上游 tag 源码、官方 Codex 源码和用户提供的成对请求观察；上游为何按该 header 拒绝特定模型，尚无公开权威说明。

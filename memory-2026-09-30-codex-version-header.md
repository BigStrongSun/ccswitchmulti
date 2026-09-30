# Codex `version` 请求头与新模型拒绝（2026-09-30）

## 本地候选验收结果

- 修复提交 `acfacae4a08b31b9ce56df982eff3bdb646a72b3`，作者与提交者均为孙大壮；串行 Rust 测试 4296 通过、9 忽略，`pnpm typecheck`、`cargo fmt --check`、`git diff --check` 与中文文件 UTF-8 严格检查通过。
- 本地发布流水线于 2026-09-30 13:14 +08:00 成功退出，生成 `release/v3.20.4-2-local` 下 Windows NSIS 安装包、portable 包、raw exe、签名与 `latest.json`。元数据指向上述提交及版本 `3.20.4-2`，`SHA256SUMS.txt` 所列 16 项逐一重算均匹配。这个目录是本地候选，不代表 GitHub 已发布或正在运行的桌面程序已安装升级。
- 本轮没有可用的真实 Sol 账号端到端准入验证；需要安装该候选并在用户现有代理配置下再次比较请求头及上游响应，才能确认现场问题消除。

## 正式发布（2026-09-30）

- 用户授权发布后，提交 `9af4cb480ebc7e84cebe600ceb51c8bfa9b3b127` 更新正式版说明（不改程序代码），并将 `main` 与 annotated tag `v3.20.4-2` 推送到 `BigStrongSun/ccswitchmulti`。标签指向该提交；本地候选程序来自其前一个文档提交之前的 `acfacae4`，程序源码和版本号在两者间未变。正式 GitHub Actions 自标签重新跨平台构建。
- Release run `36676386857` 的五个平台构建、Publish GitHub Release、Assemble latest.json 共七个 job 全部 success。正式 Release `https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.20.4-2` 非 draft、非 prerelease，19 个公开资产下载到项目内忽略目录 `release/v3.20.4-2-remote-check` 后逐项核对大小和 SHA-256 digest，19/19 匹配。`latest.json` 版本为 `3.20.4-2`，六个平台 URL 与下载的 `.sig` 内容逐项一致。
- 这证明正式资产发布与完整性，不证明本机已安装升级，也不证明用户账号下 Sol/Astra 等模型已经完成安装态端到端验收。本轮未重启正在运行的 CCSM 或 Codex。

- 用户提供的同账号、同 token、同模型和正文对照：额外带 `version: 0.158.0-alpha.2.1` 得 HTTP 400“ 不支持该模型 ”，不带则得 HTTP 200。此附件是排查线索，不等于本机完整运行态验收。
- 当前运行中的 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 文件版本为 `3.20.2-22`，不是刚发布但未安装的 `3.20.4-1`。因此不能把当前运行失败归因于新版已安装。没有改动运行进程、凭据或用户配置。
- 根因链：旧提交 `fb2adf271` 为追求原生请求等价，在官方 Codex 转发末端从可信 User-Agent 提取构建号并合成独立 `version`；`forwarder.rs` 的普通 JSON、raw 与 WebSocket 路径都会调用该函数。官方 Codex `default_client.rs::default_headers()` 的一手源码默认只放 `originator` 和 `User-Agent`，并不默认放 `version`。同一个值作为 UA 内信息与独立请求头可触发不同上游模型准入，不能把它们视为等价。
- 第三方路径的另一侧边界：`codex_request.rs::apply_provider_header_policy` 会替换客户端 UA，却曾继续透传客户端 `version`；Codex raw/WebSocket 构造器也会透传该头。修复策略是分清 header 归属：官方路由移除独立 `version`，不再合成；第三方普通路径和 raw/WebSocket 默认剥离 Codex 入站版本指纹，Provider 明确配置的覆盖值仍可设置。Codex→Anthropic Messages 转换另有指纹过滤表，也须过滤独立 `version`，同时保留 `anthropic-version`。
- 三条新回归分别对官方 alpha UA、不受控第三方请求、raw 透传先 RED 后 GREEN；另有第三方显式 header override 的保留用例。代码仅在源码与本地构建候选生效，尚未替换安装态。
- 原版 v3.20.4 的 MiniMax Code 是单独的受管应用集成，主提交 `06082e189` 改动 81 文件、3305 行，涉及配置/MCP/Skills/会话/用量/UI 与 schema。原版升级线 v18→v19，CCSM 当前 schema 为 v25；不能直接 cherry-pick 或套用原版迁移。此前 v3.20.4-1 的目标是选择性同步代理和认证修复，不是完整跟进所有新功能。
- 检索：Codex 内置 Web 搜索命中原版官方 v3.20.4 发布说明及 OpenAI Codex 官方 `default_client.rs`；Matrix WebSearch 独立检索但结果不相关或官方 GitHub 打开失败。关键技术结论依据本地 CCSM/上游 tag 源码、官方 Codex 源码和用户提供的成对请求观察；上游为何按该 header 拒绝特定模型，尚无公开权威说明。

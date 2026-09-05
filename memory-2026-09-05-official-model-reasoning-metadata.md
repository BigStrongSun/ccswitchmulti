# 2026-09-05 新官方模型 reasoning 元数据动态同步根修

## 现象与根因

- 新官方模型 `gpt-6-astra` 仅接受 `low`、`medium`、`high`、`xhigh`、`max`，旧任务携带 `reasoning.effort=none` 时，上游返回 HTTP 400。
- Codex OAuth `/backend-api/codex/models` 已返回完整的 `supported_reasoning_levels` 与默认档位，但 CCSwitchMulti 的 `FetchedModel` 只保留模型名、窗口和输入模态，前端刷新时又用通用模板补能力，导致 `max` 丢失且官方请求没有可用的迁移映射。
- 本机 Codex 0.147.0 bundled 目录尚无 Astra；CCSM 在线刷新生成的空能力行不能替代服务端官方元数据。

## 根修边界

- `FetchedModel` 跨 Rust/Tauri/TypeScript 保留可选 `reasoning`，Codex OAuth 解析直接复用 `official_reasoning_capability_for_model`，刷新现有或新增模型时一并写入目录。
- 不维护 Astra 或 GPT 型号白名单；任何官方 OAuth 模型只要返回 reasoning 元数据，都会按同一链路动态同步。
- 官方模型未声明 `none` 时，把旧 Codex 档位 `none`、`minimal` 映射到规范顺序中的最低受支持档，不能依赖服务端数组顺序。Astra 因此映射到 `low`；服务端原生五档仍完整保留。
- 进入 Codex Router 工作区时沿用既有官方 OAuth 自动刷新事务，无需手工补模型配置。

## 源码验证

- RED 阶段分别复现 OAuth parser 丢 reasoning、`none` 无映射、前端刷新丢 reasoning。
- GREEN：OAuth parser 9/9、reasoning 25/25、Router Workspace 79/79、TypeScript typecheck、rustfmt 与 `git diff --check` 通过。
- 安装态、在线目录与真实 SSE 请求证据应在构建和可回滚安装后补记；源码通过不能冒充已安装生效。

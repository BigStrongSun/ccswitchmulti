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
- Rust 全库验证为 3869 passed、0 failed、6 ignored；安装事务 Pester 为 52/52。

## 构建与安装态验证

- 默认 Tauri release 构建已生成可运行 EXE；本机 WiX `light.exe` 在 MSI 打包阶段失败。改走 NSIS 专用发布链后成功生成 `CCSwitchMulti_3.19.2-29_x64-setup.exe`，不能把 MSI 失败混同为应用构建失败。
- 分支私有 NSIS SHA-256：`D6A84CFE67A222D35A148662F1B08ECF492DB0F7628903B600A7D86C7D7C380F`；安装后 EXE SHA-256：`2F4EED91AD2530AADB5D9BE90D3E0768F548A253208E3738DBC88E68AB75CF75`。
- 安装与 UI 验收期间的可回滚事务：`ccsm-20260906-023323-3b982874a7a342299f4380754bfb4032`、`ccsm-20260906-023835-f32e457ed604469ca50cf6fa54ebcb81`。
- UI/CDP 验收后，又用同一分支私有 NSIS 执行普通启动事务 `ccsm-20260906-024925-fb6465d724984af39dbd8cbe3d73325f`：新 CCSM PID 为 59972，`127.0.0.1:15721/health` 返回 200，临时 CDP 端口 9338 已无监听，安装后 EXE 哈希保持不变；原有 Codex PID 2344、58288 均未结束。
- 已安装版工作台“管理路由规则”显示 `OpenAI Official / 8 个模型 / 已读取并更新 8 个模型`。随后只读检查 `~/.cc-switch/cc-switch.db`，`codex-official` 中 Astra 为 `low/medium/high/xhigh/max`、默认 `medium`、`disableAllowed=false`、`none→low`、`minimal→low`、`source=official`、`confidence=authoritative`；普通重启后仍完整保留。

## 真实请求证据与客户端边界

- 原始失败 trace `fb3b5556-41ac-4b61-8e6a-ace86177bdcb` 明确返回 `Unsupported value: 'none'`，与截图中的上游 HTTP 400 一致。
- 首次 canary 使用的旧 Codex CLI 身份为 0.147.0，Astra 会先被“需要新版 Codex”门禁拒绝；这是 Codex 客户端版本门禁，不是 CCSM 目录同步失败。最终验收时，当前运行路径下的 `codex.exe --version` 已返回 0.153.4，当前机器已跨过该门槛。
- 以官方 0.153.4 客户端身份发送相同 `reasoning.effort=none` 后，trace `bbacf2a2-c588-48b2-996a-3e1d54be82dc` 返回 HTTP 200、终端 SSE `response.completed` 和输出 `OK`。请求仍由客户端提交 `none`，结合已安装代码路径与数据库中的 `none→low`，证明 CCSM 在转发前完成了兼容迁移。
- OpenAI 官方 GitHub Release 在验收时显示 0.153.4 为稳定版，并包含 Astra picker/default 相关修复；版本会继续变化，后续判断“当前版本”必须重新查询官方发布页。

## 发布目录竞态

- 仓库旁共享目录 `C:\Users\sunda\Documents\LLMservice\最新版ccswitchmulti` 在本分支产物生成后被另一个 `main@770968b5` post-commit 管线覆盖，当前内容不是本分支候选，不能用于复装或发布。
- 本次候选必须从分支 worktree 的私有路径取用：`src-tauri\target\release\bundle\nsis\CCSwitchMulti_3.19.2-29_x64-setup.exe`。修复保持在 `bigstrongsun/fix-gpt6-astra-metadata`，未合并主线、未推送远端。

# 2026-08-21 当前收口审计：源码门禁已合入，第三方搜索仍缺真实运行态证据

- `main` 当前 HEAD 为 `24509724`，普通 Codex Provider 的 `add`/`update` 已统一调用 `validate_codex_subagent_v2_provider_candidate`，因此带有 `codexRouting.subagentV2` 的 Provider 不会绕过 unknown reasoning 保存门禁。
- 前端 V2 子 Agent 保存会重新读取权威状态，并对 enabled + routable + unknown reasoning 阻止保存；普通 Provider 表单仍由统一后端保存校验兜底。当前聚焦回归：Vitest 4 个文件 196/196；Rust unknown reasoning 6/6；hosted streaming auto 1/1。
- hosted tool loop 的默认策略已是：流式 `tool_choice=auto` 默认接管，显式关闭 `hostedTools.streamingAuto.enabled` 才关闭；真实第三方请求需要在日志中出现第三方 route、`responses_to_chat=true`、hosted loop/搜索调用及续接证据。
- 2026-08-21 运行态 `127.0.0.1:15721` healthy，最近请求均为 `codex-multirouter::route::router-codex-official`，没有第三方上游或 `web_search` hosted loop 证据。因此当前不能宣称“第三方模型已真实调用官方搜索”验收完成。
- 当前源码版本元数据仍是 `3.19.2-9`；`v3.20.0` 是另一条 upstream/main 历史线，当前 `main` 不是其祖先，不能把该 tag 当作本分支 release。
- 剩余工作按顺序：切换到真实第三方 route/model；发起带 hosted `web_search` 的真实 Codex 请求；检查 route/log/响应续接；必要时修复并补回归；随后再构建安装 canary、验证全新 Codex catalog 和 release 元数据。

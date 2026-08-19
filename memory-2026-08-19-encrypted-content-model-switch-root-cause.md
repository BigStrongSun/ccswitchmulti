# 2026-08-19 Codex 跨 Provider 密文回放根因

## 现象

同一 Codex 会话从 DeepSeek Responses 切换到 GPT-5.6 Luna（ChatGPT Responses）时，官方端返回 `encrypted content could not be verified` / `invalid_encrypted_content`。

## 根因

`encrypted_content` 是 Responses reasoning/compaction/agent-message 的 provider-bound opaque payload，不是“第三方 API 主动加密用户文本”。Codex runtime 会把历史 item 持久化并在下一轮原样回放；第三方 Responses 端可能回显同形字段，或接收到 Codex 生成的 OpenAI 专用 agent payload。切回 OpenAI 时，官方端无法用对应 provider/key 解密，于是拒绝请求。

本机 `codex-router.log` 已确认 DeepSeek 与 ChatGPT 路由均能返回 200，错误发生在历史输入回放边界，而非网络或路由故障。

## 修复

提交 `9a7ebafd` 在 `src-tauri/src/proxy/providers/openai_compat.rs` 的 Codex OAuth 输入规范化阶段隔离 foreign encrypted replay items：第三方密文转为 summary-only reasoning；不可读 compaction 投影为普通 user message；可疑 agent-message 密文恢复为可读文本。保留足够长且 `gAAAAA...` 的官方 Codex opaque payload。

验证：normalizer 13 passed，`transform_codex_chat` 140 passed，全量 Rust lib 3180 passed，`cargo fmt --check` 通过。

## 运行态

已安装 `CCSwitchMulti 3.19.2-7`，安装包为 `src-tauri/target/release/bundle/nsis/CCSwitchMulti_3.19.2-7_x64-setup.exe`；安装器退出码 0，运行中的 `cc-switch.exe` 版本为 3.19.2-7。安装后路由日志未出现新的 `invalid_encrypted_content`。仍需用 Codex Desktop 完全重启后实测 DeepSeek → GPT-5.6 Luna 的同会话切换。

## 后续架构注意

当前保留官方密文使用外观启发式；更稳妥的长期方案是给历史 item 持久化 provider/ownership metadata，在 provider 边界按所有权清理，而不是仅依赖密文前缀。

## 为什么现在才暴露

这不是 8 月 6 日 replay-ID 修复的直接回归。旧修复覆盖了 plain reasoning 的 `rs_*` ID、DeepSeek reasoning status 和 `web_search_call` ID，但当时保留了 `encrypted_content`，默认它来自 OpenAI 同源历史。

本次日志显示：DeepSeek 在 20:12--20:13 连续返回 200；切回 GPT-5.6 Luna 后，20:14:06 开始自动 `compaction`（`compaction_reason=comp_hash_changed`、`compaction_transport=responses_compact`），同一个会话连续收到 400。也就是说，新的 Codex runtime/历史 compaction 分支把此前隐藏在会话中的 opaque item 真正送进了官方 compaction 请求，才把旧盲点放大成可见错误。

同期上游 Codex 0.147/0.148 的 Multi-Agent V2 与第三方 Responses 兼容性也发生变化：OpenAI 专用 `agent_message`/encrypted payload 会进入非 OpenAI provider。故“以前没有”更准确地说是该组合路径尚未触发，并非密文协议从未存在；CCSM 旧逻辑只覆盖了先前已观察到的字段变体。

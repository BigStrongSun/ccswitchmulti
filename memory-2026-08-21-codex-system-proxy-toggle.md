# 2026-08-21 Codex 系统代理规则改为用户可选

- `respect_system_proxy` 是 Codex 进程级网络策略，不应作为 OAuth 凭据逻辑无条件写入。它可能影响 OAuth、MCP、WebSocket 和其他 Codex 后台请求。
- 代理面板新增“Codex 使用系统代理规则”开关，默认关闭。用户遇到 Codex 把 `127.0.0.1:15721` 送进梯子时再打开。
- 该偏好存入通用 `settings` 键 `codex_respect_system_proxy`，通过 `GlobalProxyConfig` 读写；Codex takeover 投影按该偏好决定是否写入/移除 `[features].respect_system_proxy`。
- CCSM 的上游代理（例如用户某台 Mac 的 `127.0.0.1:6528`）独立于该开关，不受影响；关闭 Codex takeover 时仍由 live backup 恢复用户原始配置。

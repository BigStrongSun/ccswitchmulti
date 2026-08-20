# 2026-08-21 Codex 接管期间回环代理链路根修

- 现场根因已由 Mac A/B 证实：正确链路是 Codex → `127.0.0.1:15721` CCSM → CCSM 出站 `127.0.0.1:6528`（人间 ZRJ）→ 上游；失败链路是 Codex 自己先连 `6528`，再让梯子代理访问 `15721`，返回空体 502，且 CCSM router 日志不增长。
- Codex `features.respect_system_proxy=false` 时 reqwest 默认代理解析会错误截获 loopback；启用为 `true` 后 macOS CFNetwork 对 `127.0.0.1` 返回 DIRECT，CCSM 才能正常接收并继续经 `6528` 出站。6528 作为 CCSM 上游代理配置本身正确，不应删除。
- 根修位置：Codex takeover 的统一 TOML 投影。`codex_config::ensure_codex_respect_system_proxy` 保留已有 `[features]` 字段并强制 `respect_system_proxy=true`；官方 route 和第三方/MultiRouter route 都调用。接管关闭仍从 live backup 恢复，因此不会覆盖用户原始值。
- TDD：先新增断言确认字段缺失并 RED，再实现；第三方投影 7/7、官方投影 1/1 通过。真机仍需安装包含该提交的构建后验收连接关系和 router 日志。

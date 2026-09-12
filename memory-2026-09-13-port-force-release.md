# 2026-09-13 代理端口被旧 CCSM/AppContainer 进程占用的强制释放根修

## 症状

- 启动/接管时提示“代理端口被身份不明的程序占用，CCSwitchMulti 已拒绝接管”，按钮“解除占用并恢复接管”点击后仍无法释放端口。
- 监听 15721 的进程是 `C:\Users\sunda\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\CCSwitchMulti\cc-switch.exe`，它与直接安装路径 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` 是同一文件的 AppContainer hardlink 投影。
- 旧进程版本可能是 3.20.2-5，而新进程/安装包是 3.20.2-6/3.20.2-7；`same_executable()` 比较文件身份时因版本不同而不相等，强制恢复被分类为 ForeignOwner 并拒绝终止。

## 修复

- 新增 `proxy_status_matches_previous_ccswitch_instance`：在拒绝/强制恢复前读取监听端口的 `/status`，验证 `app=ccswitchmulti`、同一 major 版本、PID/start ticks、executable identity、config scope、runtime API 存在、`listener_role=takeover`、`running=true`。
- `classify_forced_port_recovery_target` 增加 `verified_previous_ccswitch_listener`：即使当前安装 exe 与旧监听 exe 文件身份不同，只要 `/status` 证明是同一 CCSM 接管实例，就按 `VerifiedPreviousInstance` 处理并安全终止。
- 当前进程自己持有端口时，先调用 `self.stop()` 停止自身代理服务器，再执行 `set_takeover_for_app` 重新绑定；此前直接恢复接管会把自己的监听误判为占用。
- 仍然只允许已验证的 CCSM 实例；无法通过 `/status` 证明的 foreign owner 继续 fail closed，不扩大为通用杀端口。

## 验证

- 新增/更新 `previous_ccswitch_listener_status_allows_cross_version_force_release`、`forced_port_recovery_accepts_only_the_same_verified_executable` 回归。
- `services::proxy` 98/98、`paginated_history` 24/24、`cargo check --all-targets`、rustfmt 通过。
- 全量串行发现一个与本改动无关的既有失败 `database::dao::usage_rollup::tests::test_rollup_merges_with_existing`（10 existing + 3 new 断言得到 10）；该测试在改动前也需进一步单独调查，不能把它当作本修复的回归或掩盖 CI 证据。

## 运行态恢复边界

- 当前运行态仍是旧 3.20.2-5，监听 15721 的 PID 为 31188（AppContainer hardlink，hash `6E4C451B...2AC6`）。
- 新版本安装前需要一次受控手工恢复：验证 PID/路径/哈希后停止旧监听，再安装并启动新版本；不能依赖旧版本自身运行尚未包含的修复代码。

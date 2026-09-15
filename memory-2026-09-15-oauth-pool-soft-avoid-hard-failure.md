# 2026-09-15 OpenAI Official「503 无可用 Provider」：账号池软避让被当成硬失败

## 现象

用户截图（09:33）：Codex 显示「正在重新连接 5/10」，
`unexpected status 503 Service Unavailable: CC Switch local proxy failed while handling
Codex endpoint /responses. Provider: OpenAI Official; model: gpt-5.6-sol; cause: 无可用 Provider`。
同时 DeepSeek 线路一切正常。

## 证据链（本机真实数据，全部只读）

1. `proxy_request_logs`：官方线路（`codex-multirouter::route::router-codex-official`）
   自 09:19:13 起 **240 次请求 100% 是 503**，延迟 3–6ms（少数 100ms–2s 是额度探测）；
   `router-98e3bdc6…`（DeepSeek）同期 200 正常。
2. `codex-router.log`：官方线路最后一次 `upstream_send` 是 **09:19:12**；此后没有任何
   `route_resolved` / `request_prepared` / `upstream_send`，即请求根本没离开 CCSM。
3. 同一日志里 09:17:59–09:19:12 全部是
   `upstream_send_error … error_sending_request: client error (Connect): unexpected EOF
   during handshake`（访问 chatgpt.com 的 TLS 握手失败）。
   按账号计数：`4044e7ae-…` 在 09:17:59–09:18:35 连续 **9 次**；
   `native_codex_auth` 在 09:19:10–09:19:12 连续 **6 次**。
4. 事后网络验证：`chatgpt.com:443` TCP 可达、HTTPS 有响应 → 09:19 之后不是上游不可用。

## 根因

- 账号池把**网络层瞬时失败**按“每个账号各自的连续失败”累计
  （`CodexPoolAttemptOutcome::Transient`）；阈值 3、升级梯度
  `[30s, 120s, 600s, 1800s]`。9 次 / 6 次连续失败都取到最高档
  → 两个账号各自 **软避让 30 分钟**（≈09:48:35 / 09:49:12）。
- `ordered_pool_entries` 把软避让当硬过滤 → 返回空 → `expand_codex_account_pool`
  （`forwarder.rs`）对账号池路由展开出 **0 个候选** → `forwarder` 直接
  `ProxyError::NoAvailableProvider` → 3ms 的 503，且**从不访问上游**。
- 因为没有请求，就永远不会有成功结果去清除软避让：网络恢复后仍要等满 30 分钟。
  错误文案只有“无可用 Provider”，日志里也没有任何一行线索（`cc-switch.log`
  在该窗口完全没有账号池记录）。

## 根修（v3.20.2-16，提交 4861a204）

- `CodexPoolRuntimeState::account_avoid_remaining_ms`：只对「代际匹配且不需要重新
  登录」的账号返回软避让/冷却剩余毫秒；凭据失效、额度低于保留值仍返回 `None`。
- `ordered_pool_entries`：严格过滤为空时，按“最早恢复”回退放行（软避让 = 排序偏好），
  并 WARN 记录最早恢复时间与候选数。fail-closed 边界不变：
  `reauth_required`、额度低于 reserve、代际不匹配都不参与回退。
- `expand_codex_account_pool`：账号池没有任何候选时记录
  `[CodexOAuthPool] [POOL-001]`，让“请求没发出去”第一次有日志可查。
- `get_error_message(NoAvailableProvider)` 补上“没有可尝试的上游候选：账号池全部
  不可用或全部被熔断拒绝”。

## 验证

- 新增回归 `all_soft_avoided_accounts_still_probe_earliest_recovery`：三个账号
  （native + 两个托管账号）各累计 3 次 `Transient` 后，`ordered_pool_entries`
  仍必须返回全部候选；旧实现返回空。
- `cargo test --lib proxy::` 1968 passed / 0 failed；`cargo fmt` 通过。
- 未做：没有代用户触发真实 Codex 请求验证线上恢复（重启 CCSM 本身就会清空内存态
  软避让；且用户当时未再发请求）。

## 安装（v3.20.2-16）

- 干净 worktree `ccswitchmulti-build-v3.20.2-16` @ `60bd05e0` 构建，
  安装包 `CCSwitchMulti_3.20.2-16_x64-setup.exe` SHA-256
  `672FAEE72797C1FCECC3C1DCE657E851F46043FE33A0CCB0AB0C9D05EF75ADCC`，
  声明安装态 payload `6BC4428A14445A41CCABDC64A1AA339829E5B50D0F72CC6B542D608357790B10`。
- 事务 `ccsm-20260915-101253-3c56e10e7b7f43a890e6108f662cc031`：3.20.2-15 → **3.20.2-16**，
  PID 55260 → 58556；安装态 hash 与声明一致，`/health` 200、`/status` `running=true`
  `listener_role=takeover`、内置守护 64640 `--ccsm-supervise 58556`、run marker 版本 3.20.2-16。
- 安装态二进制含新字符串 `POOL-001`、`没有可尝试的上游候选`、`账号池全部处于软避让`。
- 安装态真实链路验证（3.20.2-16，11:01:50）：用 Codex 形状的最小合成请求
  （`User-Agent: codex_cli_rs/…`、`originator: codex_cli_rs`、`session_id`，
  `POST /v1/responses`，`model=gpt-5.6-terra`）打通官方线路，`codex-router.log` 记录：
  `route_resolved → provider=…::account::4044e7ae-…` →
  `auth_prepared auth_strategy=CodexOAuth` →
  `upstream_send upstream_url=https://chatgpt.com/backend-api/codex/responses` →
  `upstream_status status=400 (624ms)`；`proxy_request_logs` 同一条为
  `status=400 lat=629 provider=…::account::4044e7ae-…`。
  对照故障形态（3–6ms、完全没有 `upstream_send`）：账号池现在能给出候选并把请求真正
  发到上游，拿到的是上游真实响应而不是本地的即时 503。
  该 400 是上游对合成请求的回应（`The 'gpt-5.6-terra' model requires a newer version
  of Codex`），原因是合成请求缺少真实 Codex 客户端的版本指纹，不是 CCSM 缺陷；
  同一天 09:16–09:17 的真实 Codex 请求在该线路上是 200。

## 教训

- “避让/冷却”这类**降级偏好**在任何情况下都不能让候选集合变成空集合：
  候选为空意味着请求不会发出去，也就永远无法通过成功结果自愈。
- 任何“即时失败且不访问上游”的分支都必须留下一条能定位的日志，
  否则现场只剩一个 3ms 的 503。

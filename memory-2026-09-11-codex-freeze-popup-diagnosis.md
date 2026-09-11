# 2026-09-11 Codex「运行一半突然暂停 + 弹窗」三线程诊断（01a089ba / 01a084a0 / 01a088fb）

## 一句话结论
三个独立原因叠加：(A) 14:30 打开/resume 01a06c7e 触发官方 Codex 运行时（app-server 子进程）在 live_writer 重放时踩到子线程 rollout 的重复 ordinal 24390，三连 0xC0000005 访问违例崩溃 + fatal_error_broadcasted → UI 致命弹窗 + 全部线程冻结至 14:59 重启（弹窗主嫌疑）；(B) relay www.matrixminecraft.cn:24443 全天不稳（4 种错误签名、多线程同秒爆发，16:30 仍持续），客户端 10 次指数退避重试把单次失败放大成 4-25 分钟的「冻结感」（暂停主因）；(C) deepseek-flash（Moonshot MFJS）422：源码修复 d1c231fe 已提交，但 16:15 安装后 16:21 又出现同签名 422，行为证据表明安装态运行时仍不含有效修复。

## 层A：app-server 崩溃（弹窗主嫌疑）
- 崩溃签名（每次崩溃前 "Most recent error" 恒定）：failed to project durable rollout for 01a06c7e-1457-7972-bb25-0c4119df5351: thread-store internal error: thread history projection for 01a06d19-b2f8-75f1-900b-4d7723aac557 expected ordinal 24391, got 24390（target=codex_thread_store::local::live_writer）。
- 三连崩（实例 66612，日志 codex-desktop-217bac0e-...-66612-t0-i1-054921-0.log 行 4909/5174/5708）：14:30:08 / 14:31:42 / 14:35:18（UTC 06:30/06:31/06:35），closeCode=3221225786（0xC0000005），随后 fatal_error_broadcasted。14:29:43 用户在 UI 打开 01a06c7e（IAB_LIFECYCLE ownerRoutePath=/local/01a06c7e + thread_stream_view_activity_changed active=true），25 秒后崩。
- 根数据（全量扫描证实）：C:\Users\sunda\.codex\sessions\2026\09\04\rollout-2026-09-04T23-46-35-01a06c7e-1457-7972-bb25-0c4119df5351_01a06d19-b2f8-75f1-900b-4d7723aac557.jsonl，369.3MB / 57903 行 / ordinal 1777..59677，含重复 ordinal 24390（×2）与 30686（×2）。投影指针停在 (byte 186909932, ordinal 22275)，thread_items 仅投影 6019 条（max 22271）；指针到缺陷尾部之间 2115 条记录，重放按序命中重复 24390 → 确定性 3/3 崩溃。
- 父线程 01a06c7e 自己的文件（13.9MB，1783 行，0..1782）无重复且已完全投影（指针 next=1783）；15:10 重启后 15:10:23 thread/read 01a06c7e 成功（73ms）→ 读路径安全；但只要触发尾部重放（resume/续写）还会再崩。指针至今未推进。
- 新发现（本次对 4 个 rollout 全量扫描）：同类重复 ordinal 脏数据还存在于用户其它线程：
  - 01a084a0（141.9MB，20284 行，ordinal 0..20281）：重复 2129（在指针之前，live append 带过）+ 重复 19063（在未投影尾部，指针 next=2130 之后）→ 下次重启/resume 重放尾部时高风险触发同型崩溃。
  - 01a088fb（80.8MB，5076 行，ordinal 707..5781）：重复 3745，指针（byte 59490879, next=3746）正压在第 2 个 3745 上。15:12:54 / 15:12:56 两次 thread/revert 失败 -32603「expected ordinal 3746, got 3745」（revert 路径优雅处理未崩溃），但尾部同样卡死。
  - 01a089ba（108.7MB，19986 行，0..19985）：无重复 → 干净，暂停纯由 relay 造成。
- 性质：官方 Codex 桌面运行时缺陷（live_writer 重放路径遇 ordinal internal error 时进程 AV 而非优雅处理；revert 路径则优雅返回 -32603，同错误不同路径不同后果）+ 客户端已知「重复 ordinal 分配」脏数据。09-09/09-10 memory 既有决定：交由官方；可选缓解=归档隔离脏线程（需用户拍板）。

## 层B：relay 不稳（「运行一半暂停」主因，截至 16:30 仍在持续）
- relay：https://www.matrixminecraft.cn:24443（用户自有 Envoy relay）。4 种错误签名（router log upstream_send_error + CCSM 台账）：① tcp connect error 10060（约 137s 超时）→ 502；② unexpected EOF during handshake（约 117s）→ 502；③ connection closed before message completed（SendRequest 中途）→ CCSM 424 ResponsePending（设计上进重试循环会重复发送，故不进重试，非 bug）；④ 503：上午为 Envoy upstream connect error or disconnect/reset before headers, reset reason: connection termination；16:30 出现 CCSM 自产「无可用 Provider」（所有上游尝试失败）。
- 今日统计（cc-switch.db proxy_request_logs，截至 16:30）：01a084a0 19×424 + 69×502 + 6×503（last 16:30:09）；01a088fb 2×424 + 19×502（last 16:30:07）；01a089ba 6×424 + 27×502（last 16:30:07）；新会话 01a08f92 16:27-16:30 也有 424/502/503。同秒爆发：15:25:34（7 请求）、15:42:44（3 会话 4×424）、16:27:48、16:30:07-09（4 线程同秒）→ 共享上游路径。16:30 实时 TCP 探测 17ms 通 → 间歇性。
- 客户端重试算术（codex-source-rust-v0.137.0 源码核实）：424/502 → CodexErr::UnexpectedStatus（api_bridge.rs）→ is_retryable=true（protocol/src/error.rs:173）；stream_max_retries=10，backoff=200ms×2^(n-1)×jitter（最大约 102s，core/src/util.rs:85），单次尝试最长约 137s → 单失败周期 4-5 分钟，10 次重试累计最长约 25 分钟；第 2 次重试起 UI 弹 Reconnecting... n/10 toast。这就是「暂停这么久」的来源。
- 修复在 relay 侧（用户自有基础设施，超出本次诊断范围）。

## 层C：MFJS 422（01a088fb 15:11；01a08f4f 16:21 复发）
- 01a088fb 15:11:21 切 deepseek-flash → 15:11:32-15:12:21 9×422；16:15 新构建安装后，新会话 01a08f4f 16:21:06-16:21:10 又 5×422，签名完全相同：tool automation_update schema at $.tools[11].tools[0].parameters.anyOf[1].oneOf is incompatible with Moonshot MFJS: oneOf can only be represented as MFJS anyOf when every branch is provably disjoint。
- 源码修复：d1c231fe（09-11 15:37，分支 bigstrongsun/fix-codex-official-catalog-provenance，worktree C:\Users\sunda\Documents\LLMservice\cc-switch）：根 union 预处理阶段递归展开纯 union 分支，保留原 MFJS 编译/投影/strict 语义；TDD 同路径 RED→GREEN，library 4091 passed / 0 failed。详见 memory-2026-09-11-nested-oneof-mfjs-projection.md（同会话已提交）。
- 安装时间线：16:05 构建完成（dist/assets 16:05:36、codex-history-repairer.exe 16:05:16）→ ccsm.exe 16:13:22 → cc-switch.exe 16:15:00（41.8MB，SHA-256 44480F863204AD80ED2DE6A98C97FF61F7E5E93A7FD7FE6D6210BF4CE04A13D8）→ uninstall.exe 16:16:14 → 进程 54872 16:16:15（唯一 15721 监听，无旧进程残留）。版本字符串仍 3.20.2-3（未 bump）。
- 结论（行为证据）：16:21 同签名 422 = 安装态运行时不含有效修复（或安装事务实际回滚了旧二进制；exe mtime 无法区分两者）。未核对构建产物哈希 vs 安装 exe。deepseek-flash 在当前运行时会继续 422，需另一会话重验 build→install 链后再用真实请求验证。

## 层D：config.toml 解析错误（已自愈）
- 14:22-15:10：config.toml:163:1: invalid type: integer 10, expected struct AgentRoleToml（MODT 主机同型 :124），每次 config reload / experimentalFeature/enablement/set 返回 -32603。21308 日志最后出现 15:10:34；15:12:17 CCSM managed activation 重写 config.toml 后 tomllib 解析 OK、无 agent_role 键。无损坏版本备份。

## 进程/时间线
- Codex 应用（ChatGPT.exe 26.903.9818.0）：8552（09-10 01:40→09-11 13:48:35，尾部干净无崩溃）→ 66612（13:49:21→14:47:04，3 次 app-server 子进程崩溃）→ 39116（14:59:34→15:04:54，用户重启）→ 21308（15:10:16→当前，稳定；此前「任务日志 0 字节 45 分钟」异常已解除，日志持续写入至 16:23+）。
- CCSM：31572（14:59:03 用户请求重启）→ 54872（16:16:15，新构建安装后）。

## 关键路径
- 应用日志：%LOCALAPPDATA%\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\Codex\Logs\2026\09\11\（66612-t0 崩溃行 4909/5174/5708；21308-t0 revert 行 936/965）
- 线程历史 DB：C:\Users\sunda\.codex\thread_history_1.sqlite（thread_history_projection_state：thread_id/next_rollout_byte_offset/next_rollout_ordinal；thread_items 用 rollout_ordinal 列）
- 脏 rollout：C:\Users\sunda\.codex\sessions\2026\09\04\（01a06c7e 子线程 369MB、01a084a0 142MB、01a088fb 81MB、01a089ba 109MB 干净）
- CCSM 台账：C:\Users\sunda\.cc-switch\cc-switch.db proxy_request_logs（sqlite3 只读 file:...?mode=ro；列 request_id/provider_id/model/status_code/latency_ms/error_message/session_id/created_at epoch 本地时区）
- Router log：C:\Users\sunda\.cc-switch\logs\codex-router.log（key=value；rg status=424 只命中旧行，今日 424 以 DB 台账为准）
- 客户端重试逻辑：C:\Users\sunda\Documents\LLMservice\codex-source-rust-v0.137.0\codex-rs（protocol/src/error.rs:173、core/src/util.rs:85、codex-api/src/api_bridge.rs）
- CCSM 424 路径：src-tauri/src/proxy/hyper_client.rs（SendRequest/EOF→ResponsePending）、proxy/error_mapper.rs:32、proxy/forwarder.rs:7013

## 不确定性
- 弹窗文字未见到（附件未传入对话）：最可能=14:30 崩溃后的致命错误对话框；候选=Reconnecting toast、15:12 revert 错误提示。
- 8552→66612 实例切换原因未验证（旧实例日志尾部干净）。
- relay 侧 Envoy 配置未查（用户基础设施，范围外）。
- 客户端重试逻辑按 v0.137.0 源码验证，安装应用 26.903.9818.0 非同源码树（版本接近，低风险）。
- 16:15 安装的二进制是否为 16:05 构建产物未定（构建产物哈希未核对）；但 16:21 行为证明修复未生效。
- 09-10 09:53 一次同签名崩溃与 WARN 间隔 10 分钟（同签名推断因果）。

## 建议
1. 官方修复前：避免打开/resume 01a06c7e；01a084a0、01a088fb 同为高风险（重启/resume 可能触发同型崩溃）；如需归档隔离三个脏线程需用户拍板。
2. relay Envoy 自查（上游超时/reset/熔断/TLS 握手），用户自有基础设施。
3. MFJS 修复：重验 build→install 链（构建产物 vs 安装 exe 哈希），重装后用 deepseek-flash 真实请求验证 422 是否消失。
4. 官方崩溃缺陷：按 09-09/09-10 既有决定交官方。

## 第二轮修正与闭环（2026-09-11 ~17:40，官方模型「中途暂停不弹错」根因定稿）

### 修正（对第一轮结论）
1. **官方上游更正**：official 路由真实上游是 `https://chatgpt.com/backend-api/codex/responses`（17:00-17:25 窗口 request_prepared 316/316 命中；该窗口 6 条 upstream_send_error 全部 provider=router-codex-official::account::native_codex_auth）。`www.matrixminecraft.cn:24443` 只是 Qwen provider 端点，official 流量**不经过**该 relay。第一轮「relay 不稳是暂停主因」对官方模型**不成立**。
2. **排除 at capacity**：今日台账（12,572 条）+ 全部 router log 中 `529 / at capacity / capacity / overload` **0 命中**。今日官方模型停止均非 capacity 错误。
3. **真实根因 = TW 出口链路不稳**：OpenClash TUN → TW/TPE 节点（codexEgressTimezone=TW/TPE，egress 2407:cdc0）到 chatgpt.com 间歇性黑洞/reset。

### 故障 4 形态（③④ 为第二轮新增签名）
| 形态 | 签名 | CCSM 映射 | 时长 |
|---|---|---|---|
| ① SYN 黑洞 | tcp_connect_error os_error_10060 | 502 | ~137.5s |
| ② TLS 中断/悬挂 | unexpected_EOF_during_handshake | 502 | ~120.6-122s |
| ③ 快速 reset | connection_closed_before_message_completed (SendRequest) | 424 ResponsePending | 0.3-5s |
| ④ SSE 流中途断 | SSE response.failed: upstream_error; HTTP success does not imply stream success | 502 ForwardFailed | 200 头正常打开后悬挂 5.3-157s 再断 |

### 今日故障波次（全部 official 路由，全部恢复，0 turn 失败）
- 16:25:45-16:30:15：~4.5min 断网（10060/EOF），全数在重试预算内恢复。
- 17:00:17-17:07:16：波次，424×4 + 502(SSE)×10 交错（17:00:17 / 17:02:43×2 / 17:03:40×4 / 17:05:08×4 / 17:07:16）。
- 17:13:48：三会话同秒 424（01a088fb 4231ms、01a084a0 2167ms）+ 502（01a089ba 10567ms）。
- 17:32:42-17:33:06：502×8（三会话+01a08f92，流悬挂 137-157s 后断）→ 17:34:51 起全数 200 恢复；17:38:54 仍正常。
- 17:00-17:25 验证：3 会话 316 请求终态 = 310×upstream_status 全 200 + 6×send_error 全部恢复；每个错误窗口后均紧跟 200（最慢 24s，最快 0.8s）。

### 「停止不弹错」机制闭环（源码级）
1. CCSM 映射正常：502/424 都真实生成并写入 DB 台账（error_mapper.rs:19 ForwardFailed→502；ResponsePending→424）；CCSM 层没有吞错。
2. 客户端 transport 层（config.toml `request_max_retries=2`、`retry_5xx=true`，共 3 次尝试）：5xx（含 502，含 SSE 中途断）静默重试，**无任何用户通知**；单次可悬挂 120-157s → 用户体感「几分钟静默假死、零错误」。
3. 客户端 turn 层（`stream_max_retries=10`）：424（非 5xx，transport 不重试）进此层；**release 构建隐藏第 1 次 WebSocket 重试通知**（`core/src/responses_retry.rs`：`report_error = retry_count > 1 || cfg!(debug_assertions) || !responses_websocket_enabled`）→ "Reconnecting... 1/10" 不显示；第 2 次起才显示，且 StreamError 是流内 inline 通知（`session/mod.rs:3187 notify_stream_error` → EventMsg::StreamError），不是错误弹窗。
4. **rollout 不持久化 StreamError/Warning**（`rollout/src/policy.rs should_persist_event_msg` 对两者返回 false）→ rollout 里查不到 "Reconnecting" 是设计使然，不能作为「从未通知」的证据。t0 Electron 日志（21308 实例）实测 0 "Reconnecting"（不区分大小写的命中全是启动 websocket-reconnect 生命周期字段或模型输出文本）、0 "Falling back from WebSockets"（未发生 WS→HTTPS 降级）。
5. 结论：今日 4 波故障全部在**第一次重试即恢复** → 通知要么落在隐藏区（WS 首重试）、要么根本没生成（transport 静默重试内恢复）→ 用户可见 = 纯暂停 + 零错误。若 transport 3 次或 turn 10 次重试耗尽，错误必现（RetryLimit/turn 失败）——今日未到达该边界。
- 01a088fb rollout（105MB 全量）：0 个 stream_error/warning 事件类型（与第 4 点一致，设计性缺失）。
- 账号 failover 观察：失败后 CCSM 从 4044e7ae（CodexOAuth）切到 native_codex_auth（auth_strategy=none，客户端自带鉴权）——multirouter 账号池行为。

### 「at capacity 修复没完善」核实
- 运行中的 CCSM（进程 54872，16:16:15 启动；exe LastWrite 16:15:00，43,817,984B，MFJS 分支 16:05 构建）**确实不含** capacity auto-retry：功能提交 9e831c57（09-11 00:43）只在 main/capacity 分支，16:15 从 MFJS 分支重装把功能丢掉了（exe 内 rg capacity_retry 无命中）。
- 但该功能丢失与今日官方模型停止**无关**（今日根本没有 capacity 错误）。若将来真遇 529/at capacity，运行态将无自动恢复（旧行为）。
- 如需保留 capacity 功能：把 9e831c57 合入 MFJS 分支后重新构建安装（需用户拍板，不擅自重装/切分支）。

### 遗留小疑点
- reqwest 主池 connect_timeout=30s（http_client.rs:220-228），但观测 137.5s 才报 tcp_connect_error——待查 16:05 构建时超时值（git log http_client.rs）或 TUN 层未应用超时。
- DB 台账计数（257）与 router log（316）不完全对齐（子代理 session_id 归因/台账覆盖口径），两源对故障形态与恢复结论一致，不影响结论。

### 建议方向（未实施，待用户拍板）
1. 根因在出口链路：建议更换 OpenClash TW/TPE 节点，或做「official 路由直连 vs 换出口」对照，定位坏节点。
2. CCSM 可加 egress 探针提前告警（chatgpt.com 可达性/延迟异常时通知，而不是等 turn 卡住）。
3. capacity auto-retry：如需保留，合入 MFJS 分支后重新构建安装。
4. 官方客户端的静默重试是设计行为（WS 首重试隐藏）；若希望可见性更高，只能等官方版本变化——调低 stream_max_retries 只会让失败更快冒泡成错误，不建议。

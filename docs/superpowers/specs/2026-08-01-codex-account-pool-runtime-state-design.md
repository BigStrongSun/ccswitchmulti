# Codex OAuth 账号池运行态与失败分类设计

日期：2026-08-01
状态：已批准，实施中

## 问题

3.19 的 `reservePercent` 与 session affinity 冲突已经修复：已绑定任务跌破保留额度后仍能继续，429 仍会解除绑定并切换账号。但源码审计证明当前账号池仍由四张互不协调的内存表组成：session binding、固定冷却、剩余额度和额度检查时间。它们没有统一生命周期、容量限制、凭据代际或失败分类。

这会留下以下可复现风险：

- 删除、禁用、重新登录或隔离账号后，旧 binding、冷却和额度状态仍可残留。
- `normalized_pool_policy` 只检查账号 key 是否存在，已持久化为 `invalidated_at` 的账号仍可能进入候选。
- 账号池 candidate 的 401/403 被视为 official 认证失败并立即终止，不会隔离坏账号或尝试池内备用账号。
- 只有 429 会更新账号池状态；connect、timeout、5xx 不形成账号级连续失败和 soft-avoid。
- session binding 没有 TTL、容量限制或凭据代际校验，长时间运行后可能无界增长并引用旧凭据身份。

## 本阶段目标

- 用一个原子账号池运行态替代四张独立内存表。
- 为 session affinity 增加 24 小时空闲 TTL、2048 项上限、LRU 淘汰和凭据代际校验。
- 在删除、清空、禁用、策略移除、认证失效、同账号重新登录和 Desktop 身份变化时统一清理相关运行态。
- 任何候选生成路径都不返回 `invalidated_at`、运行时待重新认证、硬冷却或 soft-avoid 的账号。
- 明确区分 success、credential、quota、transient 和 neutral/caller 五类结果。
- 账号池 candidate 收到 401/403 或本地取 token 失败时，标记运行时待重新认证、解除 affinity，并允许同一显式账号池中的备用账号继续接管。
- connect、timeout、流空闲超时和 5xx 在五分钟窗口内累计；达到三次后 soft-avoid，并按 30 秒、2 分钟、10 分钟、30 分钟升级。
- 保持现有固定优先级与 `reservePercent` 行为，不改变已发布配置的默认调度语义。

## 非目标

以下工作分别形成后续设计和提交，不混入本阶段：

- 解析 `Retry-After`、额度 reset、模型额度作用域、冷却 generation 和单探测租约。
- 持续检查完整 SSE terminal，并在中途 `response.failed`、断流或流内 429 时反馈账号健康。
- 分离路由级重试预算与同一路由账号候选预算。
- 增加 `quota`、`round-robin`、`fill-first` 策略或改变默认 `priority` 策略。
- 修改公开版本号、创建 tag、上传 GitHub Release 或自动安装到用户机器。

## 方案比较

### 方案 A：独立的 Rust 账号池运行态模块（采用）

新增 `src-tauri/src/proxy/providers/codex_oauth_pool.rs`，集中定义状态、状态迁移和纯分类接口。`CodexOAuthManager` 只持有一个 `Arc<tokio::sync::Mutex<CodexPoolRuntimeState>>`，认证持久化仍由 `codex_oauth_auth.rs` 负责，forwarder 只把真实结果提交给状态机。

优点是账号状态更新可以原子完成，纯状态机容易用确定时间做单元测试，也避免继续膨胀已经很大的 OAuth 文件。代价是需要一次受控迁移，把现有四张内存表收拢到新模块。

### 方案 B：继续扩展四张独立 HashMap（放弃）

改动行数最少，但一次删除需要跨四把锁，无法保证 purge 与并发候选读取之间的一致性；加入 reauth、transient 和 credential generation 后锁顺序与竞态会继续增加。这只能缓解症状，不能修复状态所有权问题。

### 方案 C：直接复用全局 Provider 熔断器或整体移植 opencodex（放弃）

全局熔断器以 Provider 为健康单位，而账号池 candidate 是请求时临时展开的同一路由账号，401/403、额度、凭据代际和 affinity 都不是普通 Provider 故障。整体移植 opencodex 又会把 TypeScript 配置、默认策略和主账号模型带入 Rust 控制面，改变 CCSM 的产品契约。可借鉴状态机不变量，但不复制其数据模型。

## 架构与文件职责

### `codex_oauth_pool.rs`

负责：

- `CodexPoolRuntimeState` 的唯一内存真值。
- affinity 绑定、读取、触碰、过期清理和 LRU 淘汰。
- account runtime purge、全量 purge 和策略 reconciliation。
- quota 快照、固定硬冷却、运行时 reauth、连续失败与 soft-avoid。
- `CodexPoolAttemptOutcome` 状态迁移。
- 只接受账号 ID、凭据代际、时间戳和分类结果，不接触 bearer、refresh token、Tauri state 或网络客户端。

### `codex_oauth_auth.rs`

负责：

- OAuth 凭据与账号持久化。
- 为 managed account 维护非敏感 `credential_generation: u64`。旧存储缺失字段时按 `0` 读取；新账号从 `1` 开始；同账号重新登录递增；普通 access/refresh token 自动刷新不改变代际。
- 为 Desktop 当前登录计算进程内凭据代际：只把 Authorization 的 SHA-256 摘要保存在内存，不序列化、不展示、不记录日志；摘要变化时清理 native 账号运行态并提升代际。
- 在账号删除、清空、invalid_grant 隔离、重新登录和策略变更时调用统一 purge/reconcile。
- 候选生成前提供当前账号的凭据代际，并排除不可用账号。

### `forwarder.rs`

负责：

- 判断 provider 是否为账号池临时候选。
- 将成功或 `ProxyError` 映射为 `CodexPoolAttemptOutcome`。
- 对账号池 candidate 单独处理 401/403：记录 credential outcome 后允许下一个候选；直接选择的独立 official route 仍保持不可重试。
- 继续沿用当前 Provider 健康度与用户显式 fallback；账号级状态不能写入持久 Provider 健康记录。

## 数据模型

```rust
pub(crate) struct CodexPoolRuntimeState {
    bindings: HashMap<String, PoolSessionBinding>,
    accounts: HashMap<String, PoolAccountRuntime>,
}

struct PoolSessionBinding {
    account_id: String,
    credential_generation: u64,
    last_used_at_ms: i64,
}

struct PoolAccountRuntime {
    credential_generation: u64,
    remaining_percent: Option<f64>,
    quota_checked_at_ms: Option<i64>,
    reauth_required: bool,
    cooldown_until_ms: Option<i64>,
    consecutive_failures: u32,
    last_failure_at_ms: Option<i64>,
    soft_avoid_until_ms: Option<i64>,
}

pub(crate) enum CodexPoolAttemptOutcome {
    Success,
    Credential { status: Option<u16> },
    Quota { status: u16 },
    Transient { status: Option<u16> },
    Neutral,
}
```

本阶段的 hard cooldown 继续使用 60 秒固定值，但写入统一状态机；下一阶段再让 `Quota` 携带 `Retry-After`、reset、scope、generation 和 probe lease。

## 凭据代际

managed account 的 `credential_generation` 表示“用户身份凭据被重新建立”，不是普通 token 刷新次数：

- 首次登录：`1`。
- 同一 account ID 重新完成登录：旧值加一，并 purge 该账号运行态。
- access token 到期刷新、refresh token 正常轮换：代际不变，避免健康任务无意义换号。
- 旧 v1 文件没有字段：读取为 `0`；下一次重新登录后进入 `1`。

Desktop native 账号不写入 CCSM 凭据文件。forwarder 从本机 Codex 来向 Authorization 计算 SHA-256 摘要，manager 只保留摘要和单调递增的进程内代际；摘要改变时 purge native 运行态。摘要绝不进入日志、错误消息、前端或磁盘。

binding 每次复用前必须同时满足：未超过 24 小时空闲 TTL、账号仍在启用策略中、账号可用、未处于 reauth/cooldown/soft-avoid、binding 代际等于当前凭据代际。任一条件失败都删除 binding，再按正常优先级选择。

## 状态迁移

| 结果 | 账号运行态 | affinity | 当前请求 |
| --- | --- | --- | --- |
| `Success` | 清零 transient 计数和 soft-avoid；清除运行时 reauth | 建立或刷新当前 session binding | 返回成功 |
| `Credential` | `reauth_required=true`；清除 quota/cooldown/transient | 删除该账号全部 binding | 账号池 candidate 继续下一账号；独立 official 仍终止 |
| `Quota`（402/429） | 固定硬冷却 60 秒；清 transient | 删除该账号全部 binding | 继续下一候选 |
| `Transient` | 五分钟窗口内累计失败；达到三次后 soft-avoid，后续按 30s/2m/10m/30m 升级 | 达阈值后删除该账号全部 binding | 维持现有 retryable 行为 |
| `Neutral` | 不改变账号健康 | 不改变 | 按现有 caller/config 分类处理 |

`Success` 只代表本阶段现有 forwarder 已确认的成功边界。它不会被描述成完整请求成功；SSE 最终语义状态将在后续阶段迁移到 terminal recorder。

## 生命周期钩子

- `remove_account(id)`：账号删除成功后 purge `id`。
- `clear_auth()`：清空全部账号池运行态。
- `mark_account_invalid_after_refresh_failure(id)`：持久化 invalidated 状态后 purge `id`，再保留运行时 reauth 标记用于当前进程诊断。
- `add_account_internal(id, ...)`：读取旧 generation，写入递增 generation，purge 旧状态，再保存新账号。
- `set_account_pool_policy(policy)`：保存规范化策略后，对所有禁用或移除条目执行 reconcile；重新启用不会恢复旧状态。
- `load_from_disk_sync()`：只同步持久账号；发现账号被外部删除、变为 invalidated 或 generation 变化时 reconcile 运行态。
- Desktop bearer 摘要变化：只 purge `__native_codex__`，不影响 managed accounts。

所有 purge 必须通过一个状态机方法完成，禁止调用方分别操作 binding、cooldown、quota 或 health map。

## 候选选择

候选顺序继续完全服从持久化 policy：

1. 如果 session binding 有效，绑定账号排第一，并继续允许其低于 `reservePercent`。
2. 未绑定账号按 policy 原顺序排列。
3. 新 session 排除 `remaining_percent <= reservePercent` 的账号。
4. 所有 session 都排除 disabled、missing、`invalidated_at`、runtime reauth、hard cooldown 和 soft-avoid 账号。
5. 429/402、credential 或达到阈值的 transient 会解除绑定；下一次选择从剩余候选开始。

TTL 与 LRU 只约束 affinity，不删除账号健康和 quota 数据。账号健康由成功、显式 purge 或相应计时器恢复。

## 原子性与竞态

- 运行态所有字段由单一 `tokio::sync::Mutex` 保护；任何候选读取、结果写入和 purge 都在一次临界区内完成。
- 不在持有运行态锁时执行网络、磁盘、Tauri command 或 OAuth 刷新。
- 账号持久化与运行态更新的顺序是：先使账号凭据状态不可选，再 purge，最后落盘；失败恢复不得让旧 binding 重新变为可选。
- 同账号重新登录时 generation 是最终防线：即使旧请求晚到，它携带的旧 generation 也不能重新绑定新凭据身份。
- 晚到的 transient/credential 结果只能修改同一 generation；代际不匹配的结果直接忽略。

## 错误与安全边界

- 401/403 只对显式账号池 candidate 触发池内接管；独立 official route 不自动切到第三方 provider。
- `invalid_grant` 继续是唯一会持久化 `invalidated_at` 的 OAuth 证据；普通上游 401/403 只写运行时 reauth，不删除凭据。
- 400、405、406、413、414、415、422、501 和客户端取消不污染账号健康。
- 日志只记录 account ID、outcome 类别、状态码和冷却/soft-avoid 截止时间；不记录 token、摘要或请求正文。
- External Agent API 继续不得借用 Desktop Authorization；本设计不改变认证所有权边界。

## 自动测试与验收

每个行为严格先红后绿，并至少包含：

1. affinity 超过 24 小时后失效，2049 个 binding 淘汰最久未使用项。
2. managed account generation 变化后旧 binding 失效；普通 token 刷新不改变 generation。
3. 删除、clear、invalid_grant、重新登录、策略禁用/移除分别清理该账号运行态。
4. `invalidated_at` 账号从未进入 `ordered_pool_entries`。
5. 账号池 A 收到 401、403 或本地 `AuthError` 后被标记 reauth、清除 binding，并尝试 B。
6. 直接 official route 的相同认证错误仍不可重试。
7. 402/429 冷却并切换；已绑定 session 也必须离开冷却账号。
8. connect、timeout、stream idle 和 5xx 在第三次失败后 soft-avoid；五分钟窗口外重新从一次计数；成功清零 transient 状态。
9. caller 错误和客户端取消不改变账号池运行态。
10. 晚到的旧 generation 结果不能清理或污染新 generation 状态。
11. 现有 `reservePercent`、候选顺序、Desktop/managed 去重、External API 认证边界回归保持通过。

阶段完成前至少运行：

- 新模块和 OAuth manager 定向 Rust 测试。
- forwarder 账号池与认证分类定向 Rust 测试。
- `cargo fmt --check`。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`；若仓库既有 warning 阻塞，必须记录精确 warning 与基线，不得静默忽略。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`。
- `git diff --check`。

## 兼容与迁移

- `codex_oauth_auth.json` 仍保持 version 1；新增 generation 字段使用 serde default，旧文件可直接读取。
- 不改变现有账号池配置 JSON、前端排序、启用开关和 `reservePercent`。
- 运行态不持久化，升级或重启后从干净状态开始；这是有意的安全降级。
- 本阶段不自动发布 3.19。只有后续完整验证、真实 Codex 请求验证和用户明确发布指令完成后，才进入版本与 Release 流程。

## 后续阶段顺序

1. 自适应冷却：`Retry-After`、reset-derived、scope、generation、单探测租约和手动清除。
2. 流式最终结果：SSE terminal、断流、流内 402/429、media 二次成功反馈。
3. 重试预算：路由预算与账号候选预算分层、大池公平性。
4. 策略层：保留 `priority` 默认值，按显式选择增加 `quota`、`round-robin`、`fill-first`。

# Codex 配置同步与启动对账设计

日期：2026-08-30  
状态：已获用户批准，待实现

## 背景与目标

CCSwitchMulti 同时维护 SQLite 中的 Codex Provider/Common Config，以及 Codex 的运行时 `config.toml`。当前 schema-v2 MultiRouter 在 Common Config 更新时只刷新 projection，导致数据库、live 配置和接管备份出现分裂。另一个风险是 Codex Desktop/CLI 可能在 CCSM 之外改写 `config.toml`，而 CCSM 下次启动没有明确告诉用户，也不应静默覆盖。

本次变更目标：

1. 让所有 Codex Router 同步路径都从同一个有效配置构造流程写入 live 或 backup。
2. 在启动恢复流程完成后，对当前 Codex 配置做一次无敏感值的语义对账。
3. 发现外部修改时由用户明确选择应用 CCSM 配置、保留 Codex 修改或稍后处理。
4. 保持 Codex 历史数据库、rollout 文件和会话投影不被此功能改写。

## 非目标

- 不尝试修复 Codex 自身的 duplicate ordinal/history projection bug；该问题属于 Codex durable history 投影层。
- 不把代理接管时 CCSM 有意生成的 placeholder/proxy 配置判定为外部漂移。
- 不在对账响应或日志中返回 API key、OAuth token、Cookie 或完整配置文本。
- 不把用户选择“保留 Codex 修改”解释为自动将所有 live 内容写回每个 Provider；该选择只确认当前 live 为用户接受的基线。

## 方案选择

### 方案 A（采用）：后端对账服务 + 启动事件 + 前端确认

后端在启动恢复、Common Config 初始化、代理状态恢复完成后执行一次检查，发出 Tauri 事件；前端同时保留一次查询兜底。对账和写回均由后端完成，前端只传递用户决定。优点是拥有统一的数据库/文件锁边界，可避免前端竞态和静默覆盖。

### 方案 B：前端启动后轮询 `config.toml`

实现简单，但会与启动恢复、代理接管和其他 live writer 竞争，容易在中间状态弹出误报；不采用。

### 方案 C：启动时自动以 DB 覆盖 live

能消除分裂但会直接覆盖用户在 Codex 中的合法修改，违反数据安全要求；不采用。

## 后端设计

### 1. 统一 Codex Router 同步

在 `services/provider` 中抽取一个 Router 有效配置路径：

1. 读取当前 Provider 和 Common Config，调用 `build_effective_settings_with_common_config`。
2. 若为 schema-v2 Router，编译并叠加 projection-owned 字段（模型目录、projection fingerprint）。
3. 非接管态写入真实 `config.toml`；接管态更新 restore backup，并由现有 proxy-active writer 维护 live projection。
4. 保持现有 Codex 原子写锁和“写前/写后重读”保护；任何并发修改都重新基于最新快照构造，超过重试上限只返回可重试错误。

`sync_current_provider_for_app`、`sync_current_provider_for_app_respecting_takeover` 和 `set_common_config_snippet` 最终都调用该路径，避免 V2 分支提前 `return` 而跳过 Common Config。

### 2. 对账数据模型

新增后端对账结果（camelCase 序列化）：

- `state`: `consistent`、`external_drift`、`not_applicable`、`unavailable`。
- `providerId`：当前可操作 Provider（无当前 Provider 时为空）。
- `expectedFingerprint`、`actualFingerprint`：对规范化 TOML/owned projection 输入做 SHA-256。
- `changedKeys`：最多返回受影响的 TOML 键路径；只返回路径，不返回值。
- `reason`：例如 `proxy_takeover_active`、`live_config_missing`、`invalid_toml`。

期望配置由当前 Provider + Common Config + V2 projection 生成；实际配置来自 `~/.codex/config.toml`。比较使用 TOML 语义值，忽略空白、注释和键顺序；CCSM 生成的模型目录绝对路径按“配置目录内同名受管文件”归一化。比较前过滤动态/非 CCSM-owned 字段，避免 Codex 自己更新运行元数据造成误报。对账失败不写文件，只记录日志并返回 `unavailable`。

### 3. 用户决定命令

新增一个带显式 action 的 Tauri command（具体命名在实现计划中固定）：

- `apply_ccsm`：再次读取并校验当前 live 指纹，先创建带时间戳的 drift backup，再使用统一 Router 有效配置路径原子写回；指纹不匹配时拒绝覆盖并要求重新对账。
- `keep_codex`：不改 live/DB，只把当前实际指纹写入 CCSM 的设置表作为已确认基线。
- `later`：不改 live/DB，也不写确认基线；下次启动继续提示。

设置表只保存 provider id、expected/actual fingerprint、处理动作和时间，不保存配置正文。所有写回都复用现有 Codex 写锁，drift backup 写入 Codex 配置目录并限制为最近一份，避免无限增长。

### 4. 启动时机与事件

在 `lib.rs` 的启动异步任务中，顺序保持为：异常恢复 → 清理泄漏 → Common Config 初始化 → 代理状态恢复 → 对账。对账完成后发出 `codex-config-consistency` 事件。若事件发送失败只记日志，不阻塞 CCSM 启动。

前端启动后先注册事件监听，再调用一次查询 command 作为事件竞态兜底。对同一 `actualFingerprint` 只展示一次弹窗；silent startup 也执行检查，但弹窗由现有窗口唤醒/显示策略处理，不改变代理或会话历史。

## 前端设计

新增独立 `CodexConfigConsistencyDialog` 组件，复用现有 `Dialog`/`Button` 样式和 i18n：

- 标题明确说明“Codex 配置与 CCSM 不一致”。
- 展示 provider、配置路径、变更键路径和“不会显示密钥”的提示。
- 主操作为“应用 CCSM 配置”；次操作为“保留 Codex 修改”；关闭/取消对应“稍后处理”。
- 应用失败（文件在确认后再次变化、TOML 无法解析或写入失败）保留弹窗并显示可重试错误，绝不回退到静默覆盖。

## 错误与并发策略

- 代理接管、无当前 Provider、Codex 文件不存在：返回 `not_applicable` 或 `unavailable`，不弹漂移确认。
- TOML 解析失败：显示“无法安全对账”，提供打开配置目录/日志的入口，不覆盖原文件。
- 用户确认期间 live 被 Codex 改写：后端指纹 compare-and-swap 失败，重新返回最新结果。
- DB 设置写入失败：不宣称已确认；保留当前文件并记录日志。

## 测试策略

### Rust

- V2 Router Common Config 合并后，live/backup 均包含最新字段的回归测试。
- TOML 注释/空白/键序变化不产生漂移；值或 owned 键变化产生 `external_drift`，且只返回键路径。
- 接管态、缺失文件、非法 TOML 的状态测试。
- `apply_ccsm` 的 compare-and-swap、drift backup、原子写失败保护。
- `keep_codex`/`later` 的设置表行为和重复指纹去重。

### 前端

- 事件和查询同时到达时只显示一个弹窗。
- 三个动作调用正确的 action；应用失败时弹窗不关闭。
- 变更键路径和敏感值脱敏文案渲染测试。

## 验收边界

源码测试和本地构建只能证明实现正确；仍需在安装态实际重启 CCSM、让 Codex 改写一个非敏感字段、观察弹窗和三种决策后的文件/设置结果。该安装态验收不由本设计文档自动宣称完成。

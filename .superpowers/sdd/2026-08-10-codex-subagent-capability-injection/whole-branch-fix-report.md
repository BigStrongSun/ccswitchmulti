# Sub-Agent V2 capability injection 整分支修复报告

日期：2026-08-11

分支：`bigstrongsun/subagent-v2-capability-injection`

范围：新建 MultiRouter 方案的 V2 初始化所有权、初始化结果采纳、V2 选型边界文案，以及 preview 对 canonical alias collision 的 fail-closed 行为。

## 1. 联网检索与交叉验证

修改前分别执行了两条相互独立的检索链：Codex 内置 Web 与 `matrix-websearch`。两条链都优先读取官方一手资料，结论一致，没有发现来源冲突。

- OpenAI Subagents：<https://developers.openai.com/codex/subagents/>
  - `default`、`worker`、`explorer` 是内置角色；自定义角色通过 role description 为 Codex 提供选择信息。
  - role description 是语义选择指导，不是保证某个 Flash/Pro profile 必然命中的确定性路由规则。
- OpenAI Configuration Reference：<https://developers.openai.com/codex/config-reference/>
  - 自定义 Agent 配置和模型/provider/reasoning 字段属于 Codex 配置边界；前端不应复制后端 profile compiler。
- React `State as a Snapshot`：<https://react.dev/learn/state-as-a-snapshot>
  - 异步事件处理器保留触发该次 render 的状态快照，因此初始化 command 返回的新 Provider 必须被显式采纳，不能继续使用调用前的本地草稿。
- Tauri `Calling Rust from the Frontend`：<https://v2.tauri.app/develop/calling-rust/>
  - `invoke` 通过 Promise 返回 Rust command 的序列化结果；前端应以 backend initializer 返回的完整 Provider 作为持久化后的权威对象。

交叉验证结论：新方案的 schema-v1 profile 初始化应由 Rust backend 拥有；前端只负责先持久化带稳定 ID、但不含 `subagentV2` 的 Provider，再调用 initializer 并采纳其返回值。V2 UI 必须说明 best-effort 语义选择和内置角色仍可被选择。

## 2. RED 证据

RED 提交：`cd195beda47795f6ac0c288b4c3a000c43d60c30`

旧实现被以下回归 oracle 精确击穿：

1. Wizard 在仅有 Qwen 可用的新建场景中，第一次 `add_provider` 仍携带前端硬编码的 DeepSeek Flash/Pro profile。
2. Workspace 在 official-only 新建场景中同样写入不存在于目录的 Flash/Pro phantom profiles，没有采纳 backend initializer 返回的 official profile。
3. Wizard 与 Workspace 没有向用户说明 role selection 是 best-effort，也没有说明 `default`、`worker`、`explorer` 仍可被选择。
4. Rust preview 在 canonical map key 与 alias sibling 指向同一 canonical model 时，预先删除 sibling，绕过 compiler collision 检查并错误返回 TOML；而 status 已报告两个 collision，真实 materialization 也没有生成文件。

RED 测试同时固定了安全边界：公共 status/preview error 不得泄漏原始 alias sentinel。

## 3. GREEN 实现

### 3.1 新建 Provider 的 backend-owned 初始化

Wizard 与 Workspace 统一使用以下生命周期：

1. create helper 先生成稳定、唯一的 Provider ID；
2. `add_provider` 持久化不含 `settingsConfig.codexRouting.subagentV2` 的 Provider；
3. 使用稳定 ID 调用 `initialize_codex_subagent_v2`；
4. 仅采纳 backend 返回的完整 Provider，并写入 query cache 与对应的本地选中/保存状态。

`providersApi.add` 只返回成功状态，不伪造返回 Provider。初始化失败时，不执行 query-cache 采纳、不更新 `savedPlan`/`optimisticRoutingPlan`/selected ID，也不显示成功 toast；因此 UI 不会把失败伪装成已初始化成功。后端可能保留已成功创建但尚未初始化的方案，这是真实持久化状态，后续可由显式初始化流程恢复。

未在前端增加 profile catalog、schema normalizer 或 compiler。已有方案普通保存也没有被改成隐式初始化。

### 3.2 Preview 保留完整 draft 并 fail closed

`preview_codex_subagent_profile_with_context` 现在只对精确 canonical map key 执行 `profiles.insert(...)`。它不再按 profile 内的 canonical model 主动清理 alias sibling，也不重排其它 profile。

这样 status、preview 与真实 materialization 都将同一个完整 draft 交给唯一 Rust compiler：

- canonical alias sibling 仍存在时，两个 identity 都报告 collision；
- materialization 不生成任何冲突 role 文件；
- preview 返回受控 non-generation error，而不是绕过冲突返回 TOML；
- 原始 malformed/alias key 继续保持脱敏。

### 3.3 用户可见边界

Wizard 与 Workspace 均明确说明：

- Codex 在符合条件的内置与自定义角色之间做 best-effort 语义选择；
- 能力问卷与 role description 只提供指导；
- 不保证选择 Flash 或 Pro；
- 内置 `default`、`worker`、`explorer` 仍可被选择；
- Flash/Pro 是当前 backend preset，不是 V2 全部候选。

## 4. 修改范围

生产文件：

- `src/components/codex/CodexMultiRouterWizard.tsx`
- `src/components/codex/CodexRouterWorkspacePage.tsx`
- `src-tauri/src/codex_config.rs`

测试文件：

- `tests/components/CodexMultiRouterWizard.test.tsx`
- `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`
- Rust oracle 位于 `src-tauri/src/codex_config.rs` 的 tests module。

没有修改 V1 direct override、provider transport、credential、reserved tool schema、role TOML compiler、现有方案普通保存语义或安装/发布流程。

## 5. 新鲜 GREEN 验证

### Frontend

- `pnpm exec vitest run tests/components/CodexMultiRouterWizard.test.tsx src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts --exclude ".worktrees/**"`
  - 3 files passed
  - 176 tests passed
- `pnpm typecheck`
  - exit 0
- scoped Prettier check（两个生产 TSX 与两个测试 TSX）
  - all matched files use Prettier code style

### Rust

- `codex_subagent_v2_preview`
  - 4 passed，包括 full-draft duplicate role、非末位 profile allocation order、canonical alias collision fail-closed/redaction。
- `codex_subagent_profile_status`
  - 6 passed。
- `managed_agent_files`
  - 4 passed。
- `codex_subagent_v2_real_sync`
  - 2 passed。
- `codex_subagent_v2_backend_initialization_and_catalog_sync_own_canonical_drafts`
  - 1 passed。
- `codex_subagent_sync_`
  - 2 passed，覆盖用户 TOML ownership 边界。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`
  - exit 0。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
  - exit 0。

### Diff hygiene

- `git diff --check`
  - exit 0。

唯一已知非阻塞 warning：Rust `openai_cache_read_tokens` 当前未使用；前端提示 `baseline-browser-mapping` 数据超过两个月。两者均为既有警告，不是本次行为回归。

## 6. 剩余不确定性

- 本轮是源码与组件/单元测试验收，没有构建安装包、替换本机安装版或执行真实 Codex child canary；因此不把源码 GREEN 扩大表述为安装态/发布态验收。
- Codex 的语义角色选择会随任务描述、eligible roles 与上游版本变化。官方资料和既有本机 canary 都支持“best-effort”边界，但不能承诺模糊任务稳定命中第三方 Flash/Pro。
- create 已成功而 initializer 失败时，数据库会保留一个未初始化 Provider。当前 UI 不会伪装成功；若要自动回滚该 Provider，需要另行设计原子 create+initialize backend command，不能由前端用补偿删除猜测事务结果。

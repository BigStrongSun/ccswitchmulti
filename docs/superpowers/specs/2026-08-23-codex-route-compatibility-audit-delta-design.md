# Codex 路由兼容性审计增补设计（第 7、8 项）

日期：2026-08-23

状态：已批准（2026-08-23）

本文件是 `2026-08-23-codex-ssot-config-preservation-design.md` 的独立增补，只覆盖审计报告第 7、8 项。世豪已于 2026-08-23 明确批准第 7、8 项增补规格，并允许边界独立时并发；本阶段的全部产品代码和测试修改由 Luna 统一负责。256 的本次规格任务只维护本规格、配套计划和配套账本，不修改主体实现报告或产品代码。

## 1. 输入与当前事实

输入文件：

- `docs/diagnostics/sdd-prefix-migration-preview-failclosed-explore-2026-08-23.md`
- `THREAD-AUDIT-01a02dd1-2026-08-23.md`
- `docs/superpowers/specs/2026-08-23-codex-ssot-config-preservation-design.md`
- `docs/superpowers/plans/2026-08-23-codex-ssot-config-preservation.md`

只读核对时点：

- 当前 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`
- 当前 HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`
- 当前 `origin/main`：`9b0fd548301b2734772c155d8475deb285352bba`
- 当前 worktree 有 30 个未提交状态条目；Luna 正在实现已批准主体方案。本增补规格阶段不修改产品代码。

第 7 项事实：

- `src-tauri/src/codex_multirouter/migration.rs` 的 `legacy_route_canonical_models()` 只读取旧路由的 `match.models`。
- 旧路由只有 `match.prefixes` 时，迁移构造出的 V2 `modelSelection` 是空 `include`。
- `preview_codex_multirouter_migration()` 随后调用 `compile_v2()`；`validate_v2()` 用 `include_models_empty` 拒绝空 include，所以问题发生在迁移预览阶段，不是迁移完成后才失效。
- 直接把这种路由改成 `mode=all` 会扩大到目标 Provider 当前 catalog 的全部模型，违反“旧前缀只接住前缀模型”的边界。

第 8 项事实：

- 当前 fork 的 V2 后端已经 fail-closed：`resolve_codex_v2_routed_provider()`、`resolve_codex_v2_raw_passthrough_provider()` 和 `resolve_codex_route_from_settings()` 都不再使用 `defaultRouteId` 兜底。
- 当前 fork 的前端仍在三处表达旧语义：`handlePreviewRoute()` 说“会走默认路由”，方案设置说“没有精确命中时才使用默认路由”，测试发布页的发布检查要求“已设置默认路由”。
- `origin/main` 的后端仍保留默认路由兜底，前端也保留同一套旧文案；因此“后端 fail-closed、前端仍承诺默认路由”是当前 fork 的分裂，不是 `origin/main` 当前已有的前后端矛盾。
- Task 16 已经把 include 反选门控收敛到 `routeCanMatchVisibleCatalogModel()`；本增补不重新设计 include 匹配，只处理未命中后的结果表达、存量 `defaultRouteId` 字段和发布边界。

## 2. 目标

1. 旧 schema 中只有 `match.prefixes` 的路由可以迁移到 schema v2，并且迁移后的可路由范围只覆盖迁移预览时当前 catalog 中被前缀命中的模型，不能扩大到全部 Provider 模型。
2. 迁移预览必须把“前缀已冻结为 include 白名单，未来新增模型不会自动加入”明确展示给用户，并给出刷新目录后重新保存的规则。
3. 目标 catalog 为空、catalog 无匹配、alias/modelMap/canonical 映射、跨设备旧数据、迁移幂等和重复预览都必须有确定行为。
4. 当前 fork 的预览、方案设置、发布检查和诊断展示必须与 fail-closed 后端一致。
5. 上游 PR 不能在 `origin/main` 尚未采用 fail-closed 后端时孤立提交“未命中会拒绝”的文案。

## 3. 非目标

- 不把 prefix-only 旧路由迁移成 `mode=all`。
- 不新增动态 prefix schema；动态捕获未来模型属于后续 C 层架构任务。
- 不改变 V2 include 的反选安全语义：被 include 排除的模型不能通过 prefix 复活。
- 不改变已批准 MCP 对账、启动恢复、插件登记或 Phase A/B 契约。
- 不在本增补规格阶段修改产品代码、原规格、原计划或原恢复账本。
- 不为 `origin/main` 单独提交只改文案的 PR。

## 4. 第 7 项方案比较

| 方案 | 规则 | 兼容性 | 未来新增模型 | 安全边界 | 实施范围 | 结论 |
|---|---|---|---|---|---|---|
| 甲：继续拒绝并引导手选 | prefix-only 迁移仍失败，提示用户手动选择模型 | 最保守；不会误扩大 | 不自动加入 | 最安全 | 只改错误文案和引导 | 不推荐；旧配置不能自动升级，兼容性最差 |
| 乙：按当前 catalog + prefix 展开为 include | 预览时把当前 catalog 中命中 prefix 的模型写入 `include` | 兼容现有 V2 schema、compiler、runtime、UI 序列化和跨设备数据 | 不自动加入；必须给警告和重新保存规则 | include 仍是精确白名单，反选安全不变 | 主要改 `migration.rs`，加迁移警告和编辑器提示 | 推荐 |
| 丙：新增或扩展动态 prefix 选择语义 | 新增 `mode=prefix` 或让 `all + prefixes` 在运行时动态匹配 | 最接近旧 prefix 的长期语义 | catalog 刷新后可自动加入 | 需要重新定义 include 反选、未知模型能力、认证来源和 fail-closed 边界 | schema、compiler、runtime、projection、UI、迁移和发布检查都要改 | 推迟到 C 层；本轮改动和上游审查风险过高 |

推荐方案乙。核心理由是：本次缺陷是旧配置迁移兼容性，不是新增路由匹配模式。方案乙能把旧 prefix-only 配置安全带入现有 V2 精确白名单模型，同时不让 include 反选模型通过 prefix 复活。它的已知限制是未来新增模型不自动加入，因此警告和重新保存规则是必须交付的一部分。

## 5. 第 7 项推荐契约：当前 catalog + prefix 展开 include

### 5.1 适用输入

一条旧路由满足以下条件时进入本契约：

- `match.models` 缺失、为空，或全部条目 trim 后为空；
- `match.prefixes` 至少有一个 trim 后非空的条目；
- 路由有明确的 `targetProviderId` 或现有逻辑能推断官方目标；
- 迁移仍处于预览阶段，尚未写入数据库。

同时存在 `match.models` 和 `match.prefixes` 的旧路由也适用：显式 models 继续保留，prefix 只追加当前 catalog 中额外命中的模型。

### 5.2 展开算法

迁移预览构造 V2 route 时执行：

1. 读取并规范化 `match.prefixes`：trim、去空、按 ASCII 小写比较。
2. 继续使用现有 `legacy_route_canonical_models()` 处理显式 `match.models`，包括 `upstream.modelMap` 的 visible 到 canonical 映射。
3. 读取目标 Provider 当前 `modelCatalog.models`。每个 catalog 条目的权威选中名是可见 `model` 字段；`upstreamModel`/`upstream_model` 只作为等价识别和编译输入，不能单独把不可见模型扩大进路由。
4. 对 catalog 中每个条目，如果可见 `model` 以任一 prefix 开头，就把该可见 `model` 加入 include。
5. 对 `upstream.modelMap` 中每个别名，如果别名 key 以任一 prefix 开头，并且 alias target 能通过 `model`、`upstreamModel` 或 `upstream_model` 对应到当前 catalog 条目，就把该 catalog 条目的可见 `model` 加入 include。
6. 合并显式 models 和 prefix 命中结果，去重后构造 `CodexModelSelection::Include { models }`。
7. 保留 `matchPrefixes`，但它只作为迁移来源和兼容信息；在当前 fork 的 V2 runtime 中，最终路由边界由编译后的 include catalog 决定。

禁止行为：

- 不得因为 prefix-only 就改用 `CodexModelSelection::All`。
- 不得把目标 catalog 中未命中 prefix 的模型加入 include。
- 不得因为 alias target 存在就把 alias target 以外的同 Provider 模型加入 include。
- 不得让未来请求绕过 compiled model catalog 直接按 prefix 进入 route。

### 5.3 catalog 为空和无匹配

定义两个确定错误码：

- `prefix_selection_catalog_empty`：prefix-only 路由的目标 Provider 没有可用 `modelCatalog.models`，无法从当前 catalog 展开。
- `prefix_selection_no_matches`：目标 Provider catalog 存在可用模型，但没有任何可见模型或有效 alias 命中 prefix。

这两个错误只在 prefix-only 且没有任何显式 models 可用时阻断迁移。错误消息必须包含 route id、目标 Provider id 和 prefix 列表，但不能包含 API Key、base URL 之外的认证材料或完整上游配置。

如果同一路由有显式 models，但 prefix 当前没有命中任何额外 catalog 模型，迁移可以继续，并追加 `prefix_selection_no_current_matches` 警告；显式 models 仍然生效。

### 5.4 警告和未来模型规则

只要迁移使用了 prefix 展开，预览必须返回稳定警告码 `prefix_selection_frozen`。`CodexMultiRouterMigrationPreview.warnings` 仍保持 `Vec<String>` 接口，字符串以机器可读码开头，后接中文说明，例如：

```text
prefix_selection_frozen: route `qwen-route` 的 prefix 规则已按当前 catalog 展开为 2 个精确模型；未来新增模型不会自动加入。刷新模型目录后，请在路由规则中重新勾选并保存。
```

如果显式 models 有效但 prefix 当前没有额外命中，使用：

```text
prefix_selection_no_current_matches: route `qwen-route` 的 prefix 当前没有命中额外 catalog 模型；本次只保留显式模型，未来新增模型不会自动加入。
```

迁移完成后的路由编辑器在 `modelSelection.mode === "include"` 且 `matchPrefixes` 非空时显示同一规则：prefix 已经在迁移时冻结为 include；未来 catalog 刷新后不会自动加入，用户需要重新勾选并保存。

### 5.5 alias、modelMap、canonical 与 include 反选安全

- `modelMap` 的 key 是旧请求可见名，value 是目标 canonical/upstream 名；迁移继续沿用这个方向。
- prefix 命中 alias key 时，include 写入当前 catalog 中对应条目的可见 `model`，alias 本身保留在 `aliases` 中。
- alias target 不在当前 catalog 或不在 include 结果中时，继续使用现有 `alias_target_not_selected` / `alias_target_missing` 失败，不能用 prefix 绕过。
- include 中的模型是精确白名单。当前 fork 的运行时和 Task 16 的前端共享匹配都必须保持：include 模式下，prefix 只能解释迁移来源，不能把已反选模型重新放行。
- `mode=all` 原有语义不变；本增补不修改 compiler 对 `mode=all` 收集当前 catalog 全部模型的行为。

### 5.6 预览、应用、幂等与回滚

- prefix 展开发生在 `preview_codex_multirouter_migration()` 构造候选 V2 计划时。
- 预览必须先通过 `compile_v2()`，再签发 plan token；任何 `include_models_empty` 都不能作为 prefix-only 迁移的最终用户错误。
- apply 使用预览时保存的迁移快照，不在 apply 时重新扫描 catalog。这样预览内容和实际写入内容一致，重复 apply 可以继续返回 `already_applied=true`。
- 如果用户在预览后刷新目标 catalog，新增模型不会进入该 token；用户需要重新生成预览，或在迁移后的路由编辑器中重新勾选并保存。
- 回滚代码时，已经应用的 V2 include 路由仍是合法 schema，不需要数据库降级；本增补不新增自动退回旧 schema 的功能。

## 6. 第 8 项契约：fail-closed 语义一致化

### 6.1 当前 fork 的可观察行为

当前 fork 必须统一表达：

- 请求模型命中已编译可见模型时，预览显示将命中对应 route。
- 请求模型未命中时，预览显示“没有命中任何启用规则；当前 V2 运行时会拒绝该请求，不会使用默认路由。”
- 即使配置中存在 `defaultRouteId`，预览也不能承诺会走默认路由。
- 方案设置不得把 `defaultRouteId` 描述成运行时兜底。
- 发布检查不得把“已设置默认路由”作为通过条件；检查项必须改为“未命中请求将被拒绝”或同等 fail-closed 表达。

### 6.2 `defaultRouteId` 字段裁定

裁定：保留 `defaultRouteId` 作为可选数据和诊断字段，但当前 fork 不再把它作为运行时路由输入，也不再提供“默认路由兜底”的编辑入口。

具体规则：

- Rust `CodexRoutingConfigV2.default_route_id` 和 TypeScript `CodexRoutingConfigV2.defaultRouteId` 保持可选字段，继续读取旧数据和跨设备同步数据。
- 已有 `defaultRouteId` 在保存方案时默认保留，不因用户打开设置页就被静默删除；无效 route id 仍可按现有清理规则移除。
- 新建 MultiRouter 计划不再主动写入 `defaultRouteId`。
- 设置面板不再显示可编辑的“默认路由”下拉框。若旧数据已有该字段，只显示只读兼容说明：“旧版默认路由字段已保留；当前版本未命中请求会被拒绝，不会使用该字段兜底。”
- 诊断接口继续返回 `routePlan.defaultRouteId`，前端标签改为“旧版默认路由字段”，并可追加 `default_route_legacy_ignored` 警告；route 列表中的“默认”徽标改为“旧版字段”或不再显示。

这样保留旧数据可读性和回滚兼容性，同时避免用户误以为该字段仍控制请求走向。

### 6.3 诊断与错误状态

- 未命中运行时仍使用现有 resolver `None` / unroutable 结果，不新增新的请求错误码。
- 当诊断发现 `defaultRouteId` 存在时，追加稳定警告码：

```text
default_route_legacy_ignored: 当前版本未命中请求会被拒绝；defaultRouteId 仅作为旧版兼容字段保留。
```

- 该警告只包含 route id 或字段存在事实，不包含认证、base URL 或模型目录敏感内容。

### 6.4 当前 fork 与 `origin/main` 的发布边界

- 当前 fork 已包含 fail-closed 后端提交 `a567b44e`，所以 fork 集成分支可以把本增补的 UI/诊断一致化作为后端 fail-closed 之后的独立提交。
- `origin/main` 当前仍使用 `defaultRouteId` 兜底。上游贡献必须采用以下两种顺序之一：
  1. 新上游 PR 同时包含后端 fail-closed 变更、预览/设置/发布检查一致化和测试；或
  2. 等后端 fail-closed PR 先合入上游，再提交文案和诊断一致化 PR。
- 禁止在 `origin/main` 尚未 fail-closed 时提交“未命中会拒绝”的孤立 UI PR。
- 本增补不引入运行时特性开关来表达两种语义；如果未来上游要求同时支持两种模式，必须另开规格并定义明确的 schema/capability 字段。

## 7. 可观察验收标准

第 7 项：

- prefix-only 旧路由迁移预览成功，生成 `include` 且只包含当前 catalog 中 prefix 命中的模型。
- catalog 有多个模型但只有部分命中 prefix 时，未命中模型不进入 include。
- catalog 为空时返回 `prefix_selection_catalog_empty`，不是 `include_models_empty`。
- catalog 无匹配时返回 `prefix_selection_no_matches`，不是 `include_models_empty`。
- alias/modelMap/canonical 用例能编译，alias target 不会被 prefix 绕过。
- 迁移后更新目标 catalog，新增模型不会自动进入既有 include。
- apply 重复执行保持幂等；预览 token 过期或 revision 变化仍按现有规则拒绝。
- 迁移警告和路由编辑器提示都说明“未来模型不自动加入，刷新后重新保存”。

第 8 项：

- 带 `defaultRouteId` 的方案输入未命中模型时，预览不出现“会走默认路由”，并明确显示“请求会被拒绝 / 不会使用默认路由”。
- 设置面板不出现“没有精确命中时才使用默认路由”，旧字段只读展示为兼容字段。
- 发布检查不要求已设置默认路由，并明确 fail-closed。
- 诊断中 `defaultRouteId` 不再被标成运行时默认路由；存在旧字段时出现 `default_route_legacy_ignored` 警告。
- 既有后端 fail-closed 回归测试继续通过。

## 8. 风险与缓解

- 风险：prefix 展开冻结当前 catalog，用户以为未来模型会自动加入。缓解：预览警告和路由编辑器持久提示必须同时存在。
- 风险：目标 catalog 是旧快照，迁移结果不覆盖用户心里的“未来模型”。缓解：错误和警告都要求先刷新目录；apply 不静默重扫。
- 风险：alias key 与 catalog visible 名不一致。缓解：同时检查 alias key 和 catalog visible 名，但 include 只写 catalog 的可见 `model`。
- 风险：隐藏默认路由编辑入口后，旧字段仍在数据中。缓解：字段保留为只读兼容信息，并在诊断中明确“不参与当前路由”。
- 风险：上游审查者只收到文案改动而后端语义未变。缓解：上游 PR 必须和后端 fail-closed 同组，或等待后端先合入。

## 9. 生产者、消费者与冲突裁定

- `migration.rs` 生产迁移后的 V2 plan，消费者是 `compile_v2()`、plan token、apply 流程和前端迁移预览。裁定：prefix 展开只发生在预览构造阶段 — 原因是预览内容必须与 apply 内容一致 — 判断错误的代价是用户看到的预览和实际写入不同。
- Provider `modelCatalog.models` 生产当前可选模型，消费者是迁移展开、compiler、路由编辑器和目录投影。裁定：include 只使用 catalog 可见 `model` — 原因是运行时暴露的是可见模型 — 判断错误的代价是把用户看不到或旧请求不会匹配的上游名扩大进路由。
- `modelMap`/`aliases` 生产旧请求名到 canonical 的映射，消费者是迁移、compiler 和运行时 alias。裁定：prefix 可以命中 alias key，但 alias target 必须仍受 include 校验 — 原因是 alias 不是绕过白名单的通道 — 判断错误的代价是反选模型通过别名复活。
- `routeCanMatchVisibleCatalogModel()` 是 Task 16 的前端共享 include 门控，消费者是目录卡片、预览和日志归属。裁定：第 8 项只复用该助手，不修改其匹配规则 — 原因是避免两个并发任务重复定义匹配语义 — 判断错误的代价是 UI 与后端再次漂移。
- `defaultRouteId` 的生产者是旧配置、迁移输入和旧版设置页；消费者是 schema、诊断和旧版 runtime。裁定：当前 fork 保留字段但移除运行时兜底含义 — 原因是旧数据和跨设备同步仍可能携带它 — 判断错误的代价是删除字段会让旧配置丢失信息，继续展示为默认路由会误导用户。
- 上游发布边界由当前 fork 后端 fail-closed 与 `origin/main` 默认兜底差异共同决定。裁定：上游 UI 一致化必须和后端 fail-closed 同组 — 原因是文案必须描述真实运行时 — 判断错误的代价是上游用户看到与实际请求行为相反的提示。

## 10. 交付与提交边界

- 后端 prefix 迁移修复是一个独立边界：只包含 `migration.rs`、迁移测试和必要的迁移警告展示。
- fail-closed 一致化是另一个独立边界：只包含预览、设置、发布检查、诊断警告、wizard 默认字段停止写入和对应测试。
- 两个边界都依赖当前工作树中已批准的 Task 16 include-gate 共享 matcher；如果 Task 16 仍在修改 `CodexRouterWorkspacePage.tsx`，本增补的前端任务必须排在 Task 16 之后，或由同一位 Luna 实现者顺序完成。
- 上游交付至少拆成两个新 Issue/PR：prefix-only 迁移兼容；fail-closed 后端与 UX 一致化。第二个 PR 必须包含后端 fail-closed 或等待其先合入。

## 11. 批准与下一阶段

本规格已获世豪批准（2026-08-23）。Luna 可以按配套计划修改产品代码和测试，并统一承担所有实现边界内的执行顺序；边界独立只用于提交、验证和上游 PR 拆分，不表示 256 会参与产品代码实现。256 的第二次任务负责规格符合性复核、交叉测试和回归测试。

# 计划：Codex 路由兼容性审计增补（第 7、8 项）

日期：2026-08-23

状态：已批准（2026-08-23）；Luna 统一负责产品代码和测试实现

规格：`docs/superpowers/specs/2026-08-23-codex-route-compatibility-audit-delta-design.md`

账本：`docs/superpowers/plans/2026-08-23-codex-route-compatibility-audit-delta-ledger.md`

目标：修复审计第 7 项 prefix-only 旧路由无法迁移的问题，并让当前 fork 的第 8 项预览、设置说明、发布检查和诊断与 fail-closed 后端一致。实现必须以兼容性为核心：旧 schema 可迁移，新 schema 语义不被破坏，当前 catalog 不被扩大，未来模型规则可观察，include 反选安全不回退，上游 PR 不表达与 `origin/main` 后端相反的行为。

## Global Constraints

- 世豪已于 2026-08-23 明确批准第 7、8 项增补规格，并允许边界独立时并发；本计划的全部产品代码和测试修改由 Luna 统一负责，256 的本次规格任务只改三份增补文档。
- 每个行为变化先写 RED 测试，观察当前失败，再实现 GREEN。
- 不访问或修改真实 `~/.codex`、真实 `~/.agents`，测试只使用 `Database::memory()`、临时目录和现有前端测试替身。
- 不把 prefix-only 迁移成 `mode=all`；不得扩大到目标 Provider 的全部模型。
- 不新增动态 prefix schema；动态 prefix 属于后续 C 层架构任务。
- 不改变 Task 16 共享 matcher 的 include 反选规则；本计划只复用 `routeCanMatchVisibleCatalogModel()`。
- 不更新历史 PR 分支；上游交付必须使用新 Issue、新分支和新 PR。
- 本计划阶段只允许创建三份增补文档；以下产品文件归属在世豪批准后才生效。

## Producer / Consumer / Conflict Scan

- `src-tauri/src/codex_multirouter/migration.rs` 生产迁移预览、plan token 和 V2 route；消费者是 `compile_v2()`、apply 流程、数据库 provider 保存和前端迁移弹窗。
- `src-tauri/src/codex_multirouter/schema.rs` 生产 `include_models_empty`、`alias_target_not_selected` 和 `default_route_missing` 校验；消费者是 compiler 和迁移预览。
- `src-tauri/src/codex_multirouter/compiler.rs` 生产 compiled model catalog；消费者是 V2 runtime、投影和诊断。
- `src-tauri/src/proxy/providers/codex.rs` 生产当前 fork 的 fail-closed 运行时结果；消费者是代理请求、raw passthrough、后端测试和前端可观察行为。
- `src/components/codex/CodexRouterWorkspacePage.tsx` 生产迁移警告展示、路由编辑器提示、匹配预览、方案设置、发布检查和诊断标签；消费者是世豪的 Codex 工作台操作。
- `src/lib/codexMultiRouterWizard.ts` 生产新建 MultiRouter plan；消费者是工作台初始配置和 schema-v2 序列化。
- `src-tauri/src/commands/proxy.rs` 生产 `CodexRoutePlanDiagnostics.default_route_id` 和诊断 warning；消费者是状态页和发布前检查。

冲突裁定：

- 裁定：prefix 展开只使用预览时目标 catalog 的可见 `model` 与有效 alias key — 原因是旧 runtime 按请求可见模型匹配 prefix — 判断错误的代价是把 upstream-only 名称或全部 Provider 模型误放进路由。
- 裁定：prefix-only 且无法展开时返回 `prefix_selection_catalog_empty` 或 `prefix_selection_no_matches` — 原因是空 include 是结果不是用户可理解原因 — 判断错误的代价是用户仍看到 `include_models_empty`，不知道需要刷新目录或改 prefix。
- 裁定：prefix 展开结果冻结为 include，并通过 `prefix_selection_frozen` 警告告知重新保存规则 — 原因是现有 V2 schema 的安全边界是精确白名单 — 判断错误的代价是未来新增模型不会自动加入却被用户误以为已覆盖。
- 裁定：alias 可以参与 prefix 迁移，但 alias target 必须继续受 include/catalog 校验 — 原因是 alias 是映射不是安全豁免 — 判断错误的代价是反选模型通过别名或 prefix 复活。
- 裁定：`defaultRouteId` 保留为可选数据和诊断字段，隐藏其“默认路由兜底”编辑语义 — 原因是旧数据和跨设备同步仍可能携带该字段 — 判断错误的代价是删字段造成数据损失，保留旧文案造成路由误判。
- 裁定：当前 fork 的 fail-closed 文案可以和既有后端提交集成；上游必须同组提交后端 fail-closed 与 UI/诊断一致化，或等待后端先合入 — 原因是 `origin/main` 仍会默认路由兜底 — 判断错误的代价是上游 UI 文案与实际请求行为相反。
- 裁定：前端 fail-closed 一致化排在 Task 16 的共享 matcher 之后，或由同一位 Luna 实现者顺序完成 — 原因是两者都触碰 `CodexRouterWorkspacePage.tsx` — 判断错误的代价是并发覆盖 include-gate 修复。

## File Ownership and Execution Boundary

角色边界：

- Luna 统一负责本计划内所有产品代码、测试代码、联调和自测。
- 256 当前只负责 `docs/superpowers/specs/2026-08-23-codex-route-compatibility-audit-delta-design.md`、本计划和对应 ledger；不修改主体实现报告或产品代码。
- 下面的“后端迁移边界”和“前端与诊断一致化边界”用于控制 diff、提交和上游 PR 范围，不表示由不同实现者并发写入。

后端迁移边界：

- `src-tauri/src/codex_multirouter/migration.rs`

前端与诊断一致化边界：

- `src/components/codex/CodexRouterWorkspacePage.tsx`
- `src/components/codex/CodexRouterWorkspacePage.test.ts`
- `src/lib/codexMultiRouterWizard.ts`
- `src-tauri/src/commands/proxy.rs`

不得在本增补边界内修改：

- `src-tauri/src/codex_multirouter/schema.rs`
- `src-tauri/src/codex_multirouter/compiler.rs`
- `src-tauri/src/proxy/providers/codex.rs` 的 fail-closed 行为
- 已批准主体方案中的 MCP、启动恢复、插件登记文件

## Task R1: Add RED migration tests for prefix-only legacy routes

**Files:**

- Modify tests only in `src-tauri/src/codex_multirouter/migration.rs`

**Interfaces:**

- Consume `Database::memory()`, `preview_codex_multirouter_migration()`, `apply_codex_multirouter_migration()`, and the existing `target()` / `legacy_router()` / `legacy_route()` test helpers.
- Produce assertions for expanded include models, stable warnings, explicit error codes, alias preservation, future catalog behavior, and apply idempotency.

**Steps:**

- [ ] Add `prefix_only_legacy_route_migration_expands_current_catalog_into_include`: target catalog has `qwen3.8`, `qwen3.9`, and `deepseek-v4`; legacy route has `match.models=[]` and `match.prefixes=["qwen"]`; expect preview success, `modelSelection.mode == "include"`, include contains only the two Qwen models, and warnings contain `prefix_selection_frozen`.
- [ ] Add `prefix_only_legacy_route_migration_does_not_expand_to_unmatched_catalog_models`: use the same multi-model catalog and assert `deepseek-v4` is absent from include and from the compiled `model_catalog`.
- [ ] Add `prefix_only_legacy_route_migration_rejects_empty_target_catalog`: target catalog has no usable models; expect `prefix_selection_catalog_empty`, not `include_models_empty`.
- [ ] Add `prefix_only_legacy_route_migration_rejects_prefix_without_current_match`: catalog is non-empty but no visible model or alias matches; expect `prefix_selection_no_matches`, not `include_models_empty`.
- [ ] Add `prefix_only_legacy_route_migration_preserves_alias_and_canonical_mapping`: catalog entry has visible `model="flagship"` and `upstreamModel="qwen3.8"`, legacy `modelMap` has `qwen-old -> flagship`, prefix is `qwen-`; expect include contains `flagship`, aliases preserve `qwen-old`, compile succeeds, and no alias target error occurs.
- [ ] Add `mixed_models_and_prefixes_keep_explicit_selection_and_warn_when_prefix_has_no_extra_match`: explicit `deepseek-v4` remains selected, prefix `qwen-` adds only current Qwen matches; when no extra match exists, expect `prefix_selection_no_current_matches`.
- [ ] Add `prefix_expanded_include_does_not_auto_include_future_catalog_models`: apply a prefix migration, then add `qwen3.10` to the target catalog and compile again; assert `qwen3.10` is not in the compiled catalog until the route is re-saved.
- [ ] Add `prefix_only_legacy_route_migration_apply_is_idempotent`: apply once, apply the same token again, and assert `already_applied=true` plus unchanged include models.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml prefix_only_legacy_route -- --nocapture`; expected RED is `include_models_empty` from the current migration preview.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml prefix_expanded_include -- --nocapture`; expected RED follows from the same current empty include.
- [ ] Record command, exit status, and key assertion in the delta ledger.

## Task R2: Implement current-catalog prefix expansion in migration preview

**Files:**

- Modify `src-tauri/src/codex_multirouter/migration.rs`

**Interfaces:**

- Add a private helper that reads normalized legacy prefixes.
- Add a private helper that expands prefixes against the target Provider catalog and legacy `modelMap`.
- Produce `CodexModelSelection::Include { models }` with explicit models plus prefix-expanded current catalog models.
- Produce stable warning strings and explicit `AppError::InvalidInput` codes defined by the approved delta spec.

**Steps:**

- [ ] Add `legacy_route_match_prefixes(route: &Value) -> Vec<String>` with trim, empty removal, and deterministic ordering.
- [ ] Add a catalog entry helper that reads visible `model` plus `upstreamModel`/`upstream_model` identity for alias resolution.
- [ ] Add `legacy_prefix_selected_models(route, upstream, target) -> BTreeSet<String>`; match prefixes case-insensitively against catalog visible `model` and `modelMap` alias keys; write only catalog visible `model` names into the result.
- [ ] Change `build_migration()` around the current `model_selection` construction so explicit canonical models union prefix-expanded models; never select `All` solely because a legacy route had prefixes.
- [ ] Return `prefix_selection_catalog_empty` for prefix-only routes with no usable target catalog entries.
- [ ] Return `prefix_selection_no_matches` for prefix-only routes with a usable catalog but no current match.
- [ ] Add `prefix_selection_frozen` whenever prefix expansion is used; add `prefix_selection_no_current_matches` for mixed routes whose prefixes add no extra current model.
- [ ] Keep `match_prefixes` on the V2 route for migration provenance and editor messaging.
- [ ] Do not modify `schema.rs`, `compiler.rs`, or V2 runtime matching in this task.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml prefix_only_legacy_route -- --nocapture`; verify GREEN.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml prefix_expanded_include -- --nocapture`; verify GREEN.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml mixed_models_and_prefixes -- --nocapture`; verify GREEN.

## Task R3: Show the frozen-prefix resave rule after migration

**Files:**

- Modify `src/components/codex/CodexRouterWorkspacePage.tsx`
- Modify `src/components/codex/CodexRouterWorkspacePage.test.ts`

**Interfaces:**

- Reuse the existing migration dialog warning list for `prefix_selection_frozen` and `prefix_selection_no_current_matches`.
- Add a small route-editor notice when `modelSelection.mode === "include"` and `matchPrefixes` is non-empty.

**Steps:**

- [ ] Add a RED component test named `shows prefix-expanded include routes need resave after catalog refresh`; render a migrated include route with `matchPrefixes=["qwen-"]`, assert the editor explains that future catalog models do not join automatically and must be reselected and saved.
- [ ] Implement the notice in the route editor near the include model selector; keep the existing warning dialog rendering unchanged so backend warning text remains visible.
- [ ] Run `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "prefix-expanded include routes need resave"`; verify GREEN.
- [ ] Sequence this task after the in-flight Task 16 edits to the same files, or keep it in the same Luna implementation sequence.

## Task R4: Add RED fail-closed UI and diagnostics tests

**Files:**

- Modify tests only in `src/components/codex/CodexRouterWorkspacePage.test.ts`
- Modify tests only in `src-tauri/src/commands/proxy.rs`

**Interfaces:**

- Consume `CodexRouterWorkspacePage` with `initialTab: "test"` for preview and publish-check assertions.
- Consume the existing settings panel through the “重命名/设置” action for legacy-field assertions.
- Consume `codex_route_plan_diagnostics()` or a narrow extracted helper for the new warning.

**Steps:**

- [ ] Add `does not promise a default route when the preview model is unmatched`: build a V2 plan with `defaultRouteId`, render `initialTab: "test"`, type `gpt-unmatched-xyz`, click `预览命中`, assert `/会走默认路由/` is absent and `/不会使用默认路由|请求会被拒绝/` is present.
- [ ] Add `settings panel presents defaultRouteId as a legacy field instead of runtime fallback`: open the settings panel for a plan with `defaultRouteId`, assert `/没有精确命中.*使用默认路由/` is absent and `/旧版默认路由字段|不参与当前路由|不会使用该字段兜底/` is present.
- [ ] Add `publish checklist does not require defaultRouteId and states fail-closed`: render the test tab, assert `已设置默认路由` is absent and `未命中请求将被拒绝` is present.
- [ ] Add a backend diagnostics test named `default_route_legacy_ignored_warning_is_secret_free`: plan contains `defaultRouteId`, diagnostics warnings contain `default_route_legacy_ignored`, and the warning does not contain auth values or base URLs.
- [ ] Run `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "does not promise a default route"`; expected RED is the current `会走默认路由` output.
- [ ] Run `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "legacy field instead of runtime fallback"`; expected RED is the current settings explanation.
- [ ] Run `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "publish checklist does not require defaultRouteId"`; expected RED is the current `已设置默认路由` check.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml default_route_legacy_ignored -- --nocapture`; expected RED is that the warning code does not exist.

## Task R5: Align current-fork UX and diagnostics with fail-closed routing

**Files:**

- Modify `src/components/codex/CodexRouterWorkspacePage.tsx`
- Modify `src/lib/codexMultiRouterWizard.ts`
- Modify `src-tauri/src/commands/proxy.rs`
- Modify `src/components/codex/CodexRouterWorkspacePage.test.ts` only if test helpers require final assertion adjustments

**Interfaces:**

- Keep `defaultRouteId` / `default_route_id` as optional data fields.
- Keep `CodexRoutePlanDiagnostics.default_route_id` serialized for compatibility.
- Produce UI text that always describes current-fork unmatched requests as rejected.

**Steps:**

- [ ] Change `handlePreviewRoute()` unmatched output to: `没有命中任何启用规则；当前 V2 运行时会拒绝该请求，不会使用默认路由。`
- [ ] Remove the settings panel’s editable default-route select. When an existing `defaultRouteId` is present, show a read-only compatibility notice that names the legacy field and states it is not used for current fail-closed routing.
- [ ] Preserve an existing valid `defaultRouteId` during ordinary settings saves; continue allowing existing invalid-id cleanup. Do not add a new default-route writer.
- [ ] Stop writing `defaultRouteId: routes[0]?.id` for newly created wizard plans in `src/lib/codexMultiRouterWizard.ts`; preserve the field when reading or serializing old plans.
- [ ] Replace the publish-check item `ok={Boolean(selectedRouting?.defaultRouteId)} label="已设置默认路由"` with a fail-closed check whose label is `未命中请求将被拒绝` and whose status does not depend on `defaultRouteId`.
- [ ] Relabel diagnostics display from `默认路由` to `旧版默认路由字段`; route badges that currently say `默认` must say `旧版字段` or be omitted.
- [ ] Add the `default_route_legacy_ignored` diagnostics warning when a legacy field exists; keep the warning secret-free.
- [ ] Reuse `routeCanMatchVisibleCatalogModel()` unchanged for matched-route detection; do not alter Task 16 include-gate behavior in this task.
- [ ] Run every Task R4 command and verify GREEN.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml test_codex_route_unmatched_model_no_longer_uses_default_route_fallback -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml v2_runtime_resolves_only_compiled_visible_models_and_fails_closed_otherwise -- --nocapture`.

## Task R6: Delta verification gate

**Files:**

- No new product files; verification and ledger update only

**Steps:**

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter::migration -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter::compiler -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter::schema -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml test_codex_route_unmatched_model_no_longer_uses_default_route_fallback -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml v2_runtime_resolves_only_compiled_visible_models_and_fails_closed_otherwise -- --nocapture`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml default_route_legacy_ignored -- --nocapture`.
- [ ] Run `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts`.
- [ ] Run `pnpm test:unit`.
- [ ] Run `pnpm typecheck`.
- [ ] Run `pnpm format:check`.
- [ ] Run `cargo fmt --check`.
- [ ] Run `git diff --check`.
- [ ] Record every command, exit status, key output, RED/GREEN transition, and any pre-existing failure in the delta ledger.

## Task R7: Commit and upstream boundary check

**Files:**

- No product files; Git inspection and delivery planning only

**Steps:**

- [ ] Confirm the prefix migration diff contains only `migration.rs`, migration tests, and the approved route-editor notice if Task R3 is included.
- [ ] Confirm the fail-closed UX diff contains only the frontend files, wizard change, diagnostics warning, and their tests.
- [ ] Keep both boundaries as separate commits in the current worktree only after the main agent explicitly authorizes commits.
- [ ] For fork integration, place the fail-closed UX commit after the existing fail-closed backend commit `a567b44e`.
- [ ] For upstream delivery, create a new Issue and PR for prefix-only migration compatibility.
- [ ] For upstream fail-closed consistency, either include the backend fail-closed change and UX/diagnostic changes in the same new PR from then-current `origin/main`, or wait until a backend fail-closed PR has merged; never submit the UI-only wording change while `origin/main` still falls back to `defaultRouteId`.
- [ ] Fetch `origin` before constructing either upstream branch and record the new base in the delta ledger.

## Handoff Brief For Luna

- Read the approved delta spec and this plan; do not infer a different contract from the old UI text.
- Start with Task R1 and observe RED before editing migration production code.
- Never convert prefix-only routes to `mode=all`.
- Prefix expansion uses current target catalog visible `model` names and valid alias keys only; future catalog models do not join automatically.
- Keep Task 16 include-gate matcher unchanged; the preview task only changes the unmatched-result branch.
- Preserve `defaultRouteId` as data, stop presenting it as runtime fallback, and stop writing it for new plans.
- If Task 16 is still editing `CodexRouterWorkspacePage.tsx`, complete or integrate that work before Tasks R3-R5.
- Report status as `DONE`, `DONE_WITH_CONCERNS`, `NEEDS_CONTEXT`, or `BLOCKED`, with exact commands and outputs.

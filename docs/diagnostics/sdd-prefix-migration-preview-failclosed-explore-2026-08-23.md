# SDD Flash 只读探索：第 7、8 项审计复核

> 状态：`DONE_WITH_CONCERNS`  
> 探索时间：2026-08-23 23:15（Asia/Shanghai）  
> 当前 worktree：`D:\CCSwitchMulti\.worktrees\codex-startup-config-preservation`  
> 当前 HEAD：`6923a99693ef38f8fbc25ff5042b58c0679eaa73`  
> 基线对照：`origin/main` = `9b0fd548301b2734772c155d8475deb285352bba`  

## 一、结论先行

第 7 项和第 8 项在当前 worktree 中都仍然存在。第 7 项的正确现状不是“迁移后数据库里出现了空的 include 名单”，而是 **迁移预览阶段就会拒绝这条旧路由**，错误码是 `include_models_empty`。这个拒绝是当前代码的既有行为，不是已经修复。

第 8 项是当前分支特有的前后端语义不一致：

- 后端已经在 `a567b44e` 中改成未命中即失败关闭，不再使用 `defaultRouteId` 兜底，相关后端测试也存在且覆盖这个行为。
- 前端“测试发布”页的 `handlePreviewRoute` 仍然在未命中时显示“会走默认路由”。
- 当前原始基线 `origin/main` 中后端仍保留默认路由兜底，因此“后端 fail-closed、前端文案仍说走默认路由”的分裂发生在当前分支，不在 `origin/main`。

马博士没有替 256 定契约。第 7 项的最终方案需要在“保留旧前缀语义”“支持未来新增模型”“迁移发生后不扩大路由范围”三者之间取舍，报告只提供事实和代价。

## 二、工作树与并发状态

工作树与 HEAD 的关系：

```text
HEAD            6923a99693ef38f8fbc25ff5042b58c0679eaa73
origin/main     9b0fd548301b2734772c155d8475deb285352bba
```

采样记录：

- 23:10:49：工作树有 30 个状态条目，其中 26 个已跟踪修改、4 个未跟踪新文件。
- 23:15:59：工作树仍为 26 个已跟踪修改、4 个未跟踪新文件。
- `migration.rs`、`compiler.rs`、`schema.rs` 与 HEAD 一致；与 `origin/main` 相比，`migration.rs` 只有认证来源读取的一处并发改动，没有触及 prefix 迁移逻辑。
- `CodexRouterWorkspacePage.tsx` 和测试文件正在被 Luna 并发修改，本报告没有把这些并发改动视为 flash 的产出，也没有修改或回滚它们。
- 本报告没有访问真实 `~/.codex`，没有联网，没有写任何产品代码、规格、计划或账本。

## 三、第 7 项：只有 prefixes、没有 match.models 的旧路由

### 3.1 确认仍然存在

位置和调用链：

1. `src-tauri/src/codex_multirouter/migration.rs`
   - `legacy_route_canonical_models()` 只读取 `route.pointer("/match/models")`，约第 555-576 行。
   - 只有 `match.prefixes` 时，函数返回空集合。
   - `build_migration()` 用空集合生成 `Include { models: [] }`，约第 426-432 行。
   - `match_prefixes` 只从 `/match/prefixes` 复制到 V2 route，约第 443-453 行。
   - `preview_codex_multirouter_migration()` 在约第 151 行调用 `compile_v2()`。

2. `src-tauri/src/codex_multirouter/schema.rs`
   - `validate_v2()` 在约第 184-197 行把空的 include 集合判为 `include_models_empty`。
   - `compile_v2()` 因此无法通过，迁移预览直接报错。

3. `src-tauri/src/codex_multirouter/compiler.rs`
   - `collect_candidates()` 在约第 226-312 行只按 `modelSelection` 选择目标 Provider 的模型。
   - `mode=all` 时 `selected=None`，会收集目标 Provider catalog 的当前全部模型。
   - `mode=include` 时只收集 include 列表中的模型，prefix 字段不参与候选筛选。
   - 即使迁移成功，`resolve_codex_v2_routed_provider()` 也只匹配 `compiled.model_catalog` 中的精确模型名，当前代码不再使用 `matchPrefixes` 兜底。

结论：当前一条只有 prefixes、没有 models 的旧路由，无法完成从 v1 到 v2 的迁移；它会停在 `include_models_empty`。如果未来实现选择“扩展 Include”，前缀匹配到的模型会在迁移时被写入 include 名单；若 “prefix-only 的旧语义”希望自动捕获未来新增模型，当前编译器和运行时都不支持，必须改设计。

### 3.2 modelSelection 语义事实

- `mode=all`：编译时读取目标 Provider 当前 catalog，`collect_candidates()` 会收集当前目录中的全部模型。若 Provider catalog 后续更新，V2 请求时重新编译，理论上会包含新增模型；但 **当前 `mode=all` 不会用 `match_prefixes` 过滤目录**，所以它会扩大到目标 Provider 的全部模型，而不是“只接住 prefix 匹配的模型”。
- `mode=include`：只包含迁移时确定的模型名单。当前 Provider catalog 未来新增模型不会自动加入，除非重新保存规则；这是 include 的精确白名单语义，也是第 7 项“兼容未来模型”的主要代价。
- prefix 与 aliases/modelMap/canonical model：迁移函数用 `upstream.modelMap` 生成 `aliases`，`legacy_route_canonical_models` 会优先用 modelMap 把 visible 模型映射为 canonical；prefix 本身不会参与 canonical 模型推导。V2 compile 会校验 alias target 必须存在于 selected/found 集合中（`compiler.rs` 约 296-310 行），因此 **prefix-only 路由加上 alias 也不能绕过空 include 校验**。
- 目标目录为空时：`canonical_models` 为空、`target_catalog` 为空，仍然走 `Include { models: [] }`，同样报 `include_models_empty`。
- 目录无匹配模型时：`include` 为空或 alias target 缺失，编译失败；对用户的实际表现是迁移无法完成，而不是产生一个静默失效的空路由。

### 3.3 给 256 裁定的方案与代价

马博士只列事实，不替 256 选方案。

1. **保持拒绝，明确提示用户手动选择模型。**
   - 优点：不改 schema，不扩大路由，旧数据不会被悄悄改坏。
   - 代价：旧 prefix-only 路由无法自动升级，用户必须先补充模型；兼容性最保守，但体验是额外操作。

2. **迁移时按当前 Provider catalog + prefix 展开为 include。**
   - 优点：迁移后的模型范围最接近旧配置“当前”行为；不改变 V2 运行时契约。
   - 代价：只覆盖迁移时的目录，未来新增模型不会进入 include；Provider catalog 为空时无法展开；需要处理 alias/canonical 映射，否则可能出现 alias target missing。

3. **为 `mode=all` 增加“prefix 过滤”语义，或新增“all + prefixes”选择模式。**
   - 优点：能同时保留旧 prefix 语义和未来新增模型，兼容性最完整。
   - 代价：需要改 schema、compiler、V2 runtime、前端投影和测试；如果只改 compiler 而运行时仍按精确模型匹配，用户仍会看到“显示可路由但实际失败”；如果运行时直接放行 prefix，则需要重新权衡 include 反选模型的安全边界。这是改动最大、风险最高的选项。

4. **保留 prefix，但只允许 `mode=all` 的 route 走 prefix；include route 继续 fail-closed。**
   - 这是第 3 项的部分方案。优点：兼容未来模型且不破坏 include 反选安全；代价：需要让 V2 运行时在 `mode=all` 下重新考虑 prefix 匹配，编译后的模型能力、别名和认证信息可能缺失，须配套测试。

### 3.4 现有测试缺口

现状：`migration.rs` 测试模块只有 5 个迁移测试，辅助函数 `target()`、`legacy_router()`、`legacy_route()` 约在第 776-822 行。所有现有测试都使用 `match.models`，没有 prefix-only 用例。

建议 Luna 首先补：

```rust
#[test]
fn prefix_only_legacy_route_migration_does_not_emit_empty_include() { ... }
```

它应构造：

```json
{
  "id": "prefix-router",
  "enabled": true,
  "targetProviderId": "qwen",
  "match": { "models": [], "prefixes": ["qwen-"] }
}
```

当前预期 RED：`preview_codex_multirouter_migration(&db, "router", &revision)` 返回 `Err`，错误包含 `include_models_empty`。修复后的契约由 256 先裁定，测试再断言迁移成功、`modelSelection` 不是空 include，以及目标模型在 `compile_v2` 的 `model_catalog` 中。

建议命令：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml prefix_only_legacy_route_migration_does_not_emit_empty_include -- --nocapture
```

## 四、第 8 项：预览未命中仍提示默认路由

### 4.1 真实 UI 入口和触发步骤

入口：Codex 工作台 → 顶部“测试发布”Tab → “匹配预览”面板。

触发步骤：

1. 选择一个启用的 V2 MultiRouter 方案，并在方案设置中设置一个 `defaultRouteId`。
2. 点击“测试发布”。
3. 在模型输入框输入一个未命中任何启用规则的模型，例如 `gpt-unmatched-xyz`。
4. 点击按钮“预览命中”。

当前实际输出：面板会显示

```text
没有精确命中，会走默认路由 <defaultRouteId 对应名称>。
```

对应代码位置：

- `src/components/codex/CodexRouterWorkspacePage.tsx`
  - `handlePreviewRoute()` 约第 3417-3453 行。
  - 兜底文案第 3450-3452 行。
  - 方案设置页还有“没有精确命中 model 时才会使用默认路由”的说明，约第 5183-5184 行。
  - 发布检查还把“已设置默认路由”当作必须通过的检查项，约第 7952-7953 行。

### 4.2 后端当前确实 fail-closed

当前 worktree（与 HEAD 一致）代码：

- `src-tauri/src/proxy/providers/codex.rs`
  - `resolve_codex_v2_routed_provider()` 约第 584-601 行：只从已编译模型目录精确匹配；未命中直接返回 `None`，注释明确写“无匹配一律 fail-closed”，不使用 prefix/default。
  - `resolve_codex_v2_raw_passthrough_provider()` 约第 643-663 行：相同语义。
  - `resolve_codex_route_from_settings()` 约第 1397-1424 行：路由未命中后直接返回 `None`，不再读取 `defaultRouteId`。

相关后端回归测试：

- `src-tauri/src/proxy/providers/codex.rs`
  - `test_codex_route_unmatched_model_no_longer_uses_default_route_fallback()`，约第 4534 行。
  - `v2_runtime_resolves_only_compiled_visible_models_and_fails_closed_otherwise()`，约第 6861 行。

`defaultRouteId` 在运行时已没有兜底作用，但仍保留为数据字段和展示字段：

- `src-tauri/src/codex_multirouter/schema.rs` 约第 80-81 行。
- `src-tauri/src/commands/proxy.rs` 约第 1286-1307 行（route plan diagnostics）。
- 前端 `defaultRouteId` 序列化和设置表单仍在多处出现。

### 4.3 当前分支与 origin/main 的差异

`origin/main`（`9b0fd548`）的后端代码仍保留“默认路由 + 首个 enabled route”兜底，前端也是同一句旧文案；因此那不是“前后端不一致”，只是旧行为。

当前分支引入 fail-closed 的提交是：

```text
a567b44e fix(codex): include-gate prefix matches and fail closed on unmatched models
```

该提交删除了 V2 runtime 的 prefix/default/official fallback，并改写了相关后端测试，但没有同步修改 `handlePreviewRoute()` 的默认路由文案，也没有补这个 UI 的前端测试。这就是第 8 项在当前分支存在的直接原因。

### 4.4 正确可观察结果建议

预览是纯前端的本地匹配模拟，不应该在未命中时承诺“会走默认路由”。正确文案应说明：

```text
没有命中任何启用规则；按当前 V2 运行时规则，该请求会被拒绝，不会走默认路由。
```

如果产品仍希望保留“默认路由”作为配置项，应明确改为“默认展示/建议路由”之类的非运行时语义，或者彻底删除该设置，避免再次误导。这个决定需要由 256 纳入规格。

### 4.5 现有测试缺口

当前前端测试没有覆盖“未命中后预览文案”：

- `src/components/codex/CodexRouterWorkspacePage.test.ts` 中有“测试发布”Tab 存在性测试，约第 2164-2187 行。
- 没有任何测试点击“预览命中”并断言未命中文案。

建议 Luna 补：

```ts
it("does not promise a default route when the preview model is unmatched", async () => { ... })
```

步骤：构造一个带有 `defaultRouteId` 的 V2 计划，输入未命中模型，点击“预览命中”，断言：

- 页面不出现 `/会走默认路由/`。
- 页面出现“请求会被拒绝”或“不会走默认路由”的文案。

当前预期 RED：现有实现会显示“会走默认路由”，断言会失败；修复后断言通过。

建议命令：

```powershell
pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts -t "does not promise a default route when the preview model is unmatched"
```

## 五、验证证据及限制

马博士没有完成 Rust 实跑回归。尝试命令：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_route_unmatched_model_no_longer_uses_default_route_fallback -- --nocapture
```

结果：编译阶段失败，底层错误为

```text
failed to mmap file ... libcc_switch_lib.rlib: 页面文件太小，无法完成操作。 (os error 1455)
```

这是 Windows 本机内存/页面文件限制导致的工具链失败，不是代码断言失败。马博士没有把该命令解释成“测试通过”或“测试失败”，只作为验证限制记录。

前端测试未在当前 worktree 实跑：该 worktree 未安装 `node_modules`，马博士没有为它安装依赖，也没有访问真实用户配置。审计报告提到的“66 个工作台测试、5 个迁移测试通过”是原报告的历史证据，不能作为本次 worktree 的新证据。

## 六、交接给 256 的决策问题

1. 第 7 项采用哪种契约？
   - 只保持“当前模型”准确：展开 include。
   - 保持“未来模型”兼容：给 All 增加 prefix 语义或新增选择模式。
   - 拒绝自动迁移：要求用户手动处理 prefix-only 路由。
2. 若引入新选择模式，是否允许 V2 runtime 对 `mode=all + prefix` 放行当前 Provider catalog 之外的请求模型？若允许，需要额外定义能力、别名、认证和 `canonical/upstream` 模型在无 compiled model 条目的行为。
3. 第 8 项是只改“测试发布”预估文案，还是同时处理“设置页默认路由说明”和“发布检查默认路由勾选”？这影响用户对默认路由语义的整体理解。

## 七、下一步建议

1. 256 先裁定第 7 项和第 8 项的兼容性契约，并补充/修订规格与验收标准。
2. Luna 先补两条可失败测试：`prefix_only_legacy_route_migration_does_not_emit_empty_include` 和前端 `does not promise a default route when the preview model is unmatched`，再实现修复。
3. Luna 实现后由 256 做规格符合性复核和交叉回归，重点覆盖：
   - prefix-only 旧路由迁移前后行为；
   - alias/modelMap/canonical 映射；
   - Provider catalog 为空、目录新增模型；
   - include 反选模型不能通过 prefix 复活；
   - 未命中模型必须 fail-closed；
   - 前端预览与后端对外可观察行为一致。

最终状态：`DONE_WITH_CONCERNS`。第 7、8 项均存在，且现有测试不能证明修复；后续进入规格阶段前必须先由 256 完成契约裁定。

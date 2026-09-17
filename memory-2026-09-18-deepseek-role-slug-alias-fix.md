# 2026-09-18 DeepSeek 角色 slug 别名统一修复（“目录中缺失”bug）

## 问题与结论

- 用户报障：V2 Agent 配置页 `deepseek-flash` 角色覆盖徽章显示“目录中缺失”，
  但目录里 `deepseek-flash` 已启用/已选（live 请求 HTTP 200 可用）；pro 正常。
- 结论：是 bug。产品内部 canonical slug（`deepseek-v4-flash`/`deepseek-v4-pro`，
  用于 V2 默认 profile、catalog 模板、preset）与 DeepSeek 现行 API slug
  （`deepseek-flash`/`deepseek-pro`）不一致，各角色分类点硬编码旧 slug。
- 修复提交：`7fb839db`（main，本地未推送）。

## 根因链（catalog 用新 slug 时的各层症状）

1. UI 徽章 `CodexRouterWorkspacePage` 只认 `deepseek-v4-flash*` → 误报“目录中缺失”。
2. V2 编译器 `codex_subagent_profiles.rs` 精确匹配 catalog → 旧 slug profile
   Unroutable，role 文件不生成（agents 目录里只有 deepseek-v4-pro.toml）。
3. V2 自动生成 `catalog_profile_draft` 按 identity 取默认 preset → `deepseek-flash`
   落成 disabled 通用 stub（用户 DB 现状），而非富 flash preset。
4. V1 role helper（role name/description/effort/guidance）contains 旧 slug →
   新 slug 拿不到 Flash/Pro 角色语义。
5. text-only guard 精确匹配缺别名 → 新 slug 不强制文本模态，Codex 会注入
   DeepSeek 不支持的 hosted image_generation 工具。
6. reconcile PruneUnroutable/Recover 按旧口径 → alias profile 可能被误删。

## 修复方案（单一事实来源）

- Rust 共享 helper（`codex_subagent_profiles.rs`，pub）：
  - `deepseek_role_identity_for_model`：flash 族 = `deepseek-flash`/`deepseek-v4-flash`
    及 `-` 前缀版本（排除含 `vision`）→ canonical `deepseek-v4-flash`；pro 同理。
  - `deepseek_role_models_match`：精确忽略大小写 或 同角色族等价。
- 所有 catalog↔profile 匹配点统一走该口径：V2 编译匹配、role 模型钉到 catalog
  实际 slug（别名匹配时）、`catalog_profile_draft` preset 查找、两处 `preferred`、
  `routable_by_identity`（prune/recover 别名键）、compile/preview 的
  input_modalities 水合与 spec/role 查找、`route_classifications`/
  `reasoning_capabilities` 查询（map 插入别名键）、V1 四个 helper、
  text-only guard 加 `deepseekflash`/`deepseekpro` 精确项。
- TS 共享 helper `src/utils/deepseekRoleModels.ts`（与 Rust 同口径）：
  徽章、`CODEX_SPAWN_AGENT_PRIORITY_MODELS` “重点”名单、
  `modelNameLooksTextOnly`（顺手修掉 08-22 修复在 TS 侧漏掉的
  `compactTail.startsWith("deepseekv4")` 前缀过匹配——旧逻辑会把
  `deepseek-v4-flash-vision-exp` 误判为纯文本）。

## 行为边界（有意为之）

- `deepseek-v4-flash-vision-exp` 不再被 V1 role helper 的 contains 命中为
  flash 文本角色（走通用 slug 路径，独立角色名）；与 08-22 既定口径一致。
- 旧测试 `codex_subagent_v2_catalog_alias_change_preserves_profile_and_marks_it_unroutable`
  的 fixture `deepseek-flash-alias` 在新口径下属于 flash 族（前缀规则，
  与既有 TS 测试 `DeepSeek-V4-Flash-DeepSeek` 可路由的口径一致），已改名为
  `..._catalog_renamed_to_unrelated_model_stays_unroutable` 并改用
  `deepseek-v5-flash`（真正不属于角色族）保留“改名→Unroutable”意图。
- V2 默认 profile 键、provider preset slug、profile 冲突身份（collision
  identity）均未改：alias 双 profile 同存时各自可路由，保持既有行为。
- 用户既有配置不会被自动“升级”：已存在的 disabled stub profile 保持原样；
  启用开关保存后即可生成角色文件（修复后编译匹配与徽章均正确）。

## 验证

- `cargo test --lib codex_subagent_profiles::` 97 passed；`codex_config::` 232 passed；
  `codex_multirouter::` 104 passed；`deepseek` 过滤 45 passed。
- `pnpm vitest run` CodexRouterWorkspacePage.test.ts + deepseekRoleModels.test.ts：
  92 passed。`pnpm typecheck` 通过。
- 6 文件 UTF-8 严格解码，无 BOM/U+FFFD。

## 运行态边界（source/test 证据 ≠ 安装运行态证据）

- 本机安装仍是 v3.20.2-18（安装二进制不含本修复）；修复要生效需构建安装新版本。
- 用户 live catalog（`C:\Users\sunda\.codex\cc-switch-model-catalog.json`）
  flash 条目 slug 就是 `deepseek-flash`（用户态 entry，reasoning source=user）；
  live 前 5 窗口在保存后需重启 Codex Desktop/app-server 才按
  spawnAgentModels 重排（产品页面本身有此提示，非 bug）。
- 用户 `deepseek-flash` profile 当前 enabled=false（DB 实证）；pro 为 enabled=true
  且 agents 目录已有 deepseek-v4-pro.toml。

## 后续（如需）

- 构建新版本并安装后，UI 徽章应立即显示“可路由”；启用 flash profile 保存后
  `~/.codex/agents/` 应出现 flash 角色 toml（model 钉到 deepseek-flash）。
- 若 DeepSeek 将来发布 `deepseek-flash-*` 日期版本，现有前缀口径已覆盖；
  发布 `deepseek-pro-*` 视觉型号时需重审 vision 排除规则。

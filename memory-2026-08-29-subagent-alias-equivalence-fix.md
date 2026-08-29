# 2026-08-29 子 Agent 自动字段为空根因与别名等价修复（Issue #74）

## 现象与取证

- Codex MultiRouter → 子 Agent 页面，`glm-5.3-flash-4faa657f-1e92-467e-b3b0-d446aeb27b9a`、`deepseek-v4-flash-deepseek`、`deepseek-v4-pro-deepseek`、`k3` 四个 profile 的高级字段自动值（角色名/描述/开发者指令/昵称）全空；名字与投影目录一致的 profile 正常。
- live DB `~/.cc-switch/cc-switch.db` 与 2026-08-21/24/26/28 备份、08-29 13:01 glm-normalize 前备份逐份比对，还原了每个 profile 的播种时刻与失效时刻；live `~/.codex/config.toml` 投影目录中现存 `model = "glm-5.3-flash-4faa657f-…", upstreamModel = "glm-5.3-flash"`。

## 根因（三层叠加）

1. 后端投影编译器把"路由别名 key"改写为可见模型名（`compiler.rs` `compile_v2` explicit_aliases 逻辑），碰撞时再追加 provider 名后缀；基元律动路由的 UUID 别名使投影/live config 的模型名带后缀，原名退到 `upstreamModel`。别名最初如何写入无法完全锁定（8/24 备份中 DeepSeek 路由已有同构别名；GLM 别名出现在 8/28 18:15 之后），但当前代码下它会自我维持。
2. 子 Agent profile 按播种时刻的后端投影名落库；SyncCatalog 只增不改不删，投影改名后旧名永久残留。
3. 子 Agent 页 preview/statuses 用前端 `buildModelCatalogForRoutes` 的目录（按目标 provider 目录原名建表，不感知别名）覆盖 `modelCatalog`；编译匹配是 `eq_ignore_ascii_case` 精确等值。两套投影一旦分叉 → Unroutable → preview 报 `Profile model is not routable` → 前端 `overrides.X ?? preview?.X ?? ""` 兜底为空。

## 修复

- `codex_subagent_profiles.rs`：`CatalogModel` 新增 `equivalent_models`（upstream 身份 + 指向该模型的路由别名 key）；编译匹配先精确、后等价。
- `codex_config.rs`：新增 `codex_subagent_catalog_equivalent_names`；preview 的 spec 查找与 input modalities 补水加等价回退；`SyncCatalog` 增加**改名迁移**——解析成功的 profile 的 model 已不在当前可路由目录、但经 upstream/别名等价映射到目录中现有模型时，重键迁移（保留问卷/覆盖/启用状态），目标身份已被占用或无任何关联时不迁移（交给显式 PruneUnroutable / RecoverAllInvalidFromCatalog）。
- 沿用 2026-08-21 路由侧 visible/upstream 双身份索引的既有约定，属同域缺口补齐；未改前端。

## 验证

- 新增/改造回归：`route_alias_key_becomes_projected_visible_model`（后端改名事实）、`preview_accepts_alias_keyed_profile_across_frontend_and_backend_catalogs`（两套目录都能预览）、`preview_still_fails_when_no_alias_maps_the_profile_name`（失败关闭）、`catalog_sync_migrates_stale_alias_keyed_profile_to_current_projection_name`、`catalog_sync_migrates_stale_upstream_identity_profile_to_current_projection_name`、`catalog_sync_leaves_profiles_without_an_equivalent_catalog_model_untouched`。
- `cargo test --lib` 全量 3532 通过（含并行会话的 tool-wire 9 项）。
- 未构建/未替换安装态、未改动 live DB 与 live config；数据侧止血（删 UUID 别名 + 重存 Provider + 目录同步）留待用户执行。

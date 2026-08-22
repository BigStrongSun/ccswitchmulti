# 2026-08-23 可更新 Provider 预设审计

- 当前 Codex 预设是 `src/config/codexProviderPresets.ts` 中的编译期数据；不存在远程 manifest、
  下载、签名/哈希校验、版本检查、导入/导出或预设更新三方合并。
- 已有 Provider 的真实 SSOT 是 SQLite `providers.settings_config`；模型目录位于
  `modelCatalog.models[]`，所以用户对单个 Provider 的输入能力编辑已可持久化，但它不是可分发的预设。
- 2026-08-23 新增后端回归：显式 `inputModalities=[text,image]`、`supportsImage=true`、
  `textOnly=false` 会覆盖模型名文本兜底，并被投影到最终 Codex catalog。前端模型目录回归 31 tests
  已通过。
- 后续工作已登记在 `docs/superpowers/plans/2026-08-23-remote-provider-preset-registry-todo.md`：
  先做本地可移植预设与三方合并，再做签名远程源。没有受信源和签名验证前不得添加裸 URL 下载更新。

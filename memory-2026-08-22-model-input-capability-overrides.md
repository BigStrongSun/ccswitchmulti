# 2026-08-22 模型输入能力 UI 覆盖

## 产品规则

- 每个 Codex Provider 的 `settings_config.modelCatalog.models[]` 是用户配置的持久化来源。
- `inputModalities`、`supportsImage` 和 `textOnly` 必须作为一组写入：避免目录显示多模态而
  后端仍因为残留 `textOnly=true` 将模型降为文本。
- CCSM 内置 Provider preset 是初始值和可恢复基线，不是运行时锁。用户在 Provider 表单中选择
  输入能力后，覆盖只属于当前 Provider，不需要等待新 release，也不修改全局内置 preset。

## UI

- `CodexFormFields` 的每个“模型目录明细”行都有“输入能力”分段控件：`文本与图像` / `仅文本`。
- 有内置声明的模型额外显示“恢复 CCSM 预设”；该动作重新写入预设对应的三字段。
- 该控件与推理能力来源独立：推理仍有 automatic/builtin/manual 三层；输入能力现在是用户直接
  可保存的 Provider 级声明。

## 验证

- `pnpm typecheck` 通过。
- `pnpm exec vitest run tests/components/CodexFormFields.test.tsx --reporter=dot --maxWorkers=1 --minWorkers=1`
  通过：31 tests。新增回归覆盖视觉预设切换为仅文本并恢复预设。

# 2026-09-23 Codex Provider 保存校验 fail-closed

- 根因：Provider 保存时 `normalizeCodexCatalogModelsForSave` 对 Ultra 缺少供应商强度抛中文异常；外层 `catch` 仅用英文 `message.includes("reasoning")` 区分校验错误，因此 Ultra 异常落入 fallback，重新使用旧 `values.settingsConfig` 并继续提交，导致 API Key 等当前草稿改动看似保存但实际丢失。
- 修复：目录/推理能力校验抛带 `model` 字段的 `CodexCatalogValidationError`；保存时按错误类型显示并 return。Codex 设置构造 catch 现在一律 fail-closed，不再回退旧 settingsConfig。Ultra 配置无效时在当前模型目录区显示“本次所有修改均未保存”，高亮并滚动定位对应模型；目录编辑后会重新校验全量目录，仍无效则持续提示并更新定位，有效才清除。普通解析错误和后端提交 reject 也有持久反馈，错误文案不包含 API Key。草稿保持当前组件状态，不会自动猜测 Provider effort。
- 验证：`ProviderForm.reasoning.test.ts` 12/12（结构化 Ultra 与其它推理错误）、`CodexFormSaveFeedback.test.tsx` 2/2、`pnpm exec tsc --noEmit --pretty false`、Prettier 检查、`git diff --check` 通过。尚未做完整 Provider 页面 E2E、安装或发布验收；未触及后端或运行态。

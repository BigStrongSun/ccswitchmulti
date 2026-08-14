# Codex Provider 与 MultiRouter 配置体验实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Codex Provider 配置收敛为“接入并验证一个健康模型源”，并把 MultiRouter 十步引导收敛为四步、删除历史修复收尾。

**Architecture:** 保留既有 Provider 数据结构、`codexRouting` 兼容链和 MultiRouter 工作台，不修改后端路由语义。前端以可测试的模型源就绪组件承载同步模型、连接验证和状态摘要；原高级区只保留手动覆盖与专家请求配置；向导状态机把原十个可见步骤映射为四个用户任务阶段，自动过程保留在阶段内部执行。

**Tech Stack:** React 18、TypeScript、React Hook Form、TanStack Query、Vitest、Testing Library、Tauri 2。

## Global Constraints

- 普通 Provider 页不得恢复旧 `codexRouting` 编辑入口；历史 schema、normalize/save 和 Rust resolver 保持兼容。
- 维护预设通过稳定 `codexPresetId` 识别，协议、模型能力和 `/model` 投影继续由 CCSwitchMulti 维护。
- 自定义 Provider 可以手动覆盖协议和模型能力；catalog opt-out 保持为高级最后一项。
- 未知模型不推断 GPT reasoning 档位，不改变现有 capability resolver。
- 自动协议探测会发送真实请求，执行前必须保留费用/流量说明与确认。
- MultiRouter 完成后停留在状态工作台，不自动进入历史修复或迁移。
- 不修改或暂存 `codex_config_diff.txt`、`wizard_diff.txt`、`workspacepage_diff.txt`。
- 每个 RED、GREEN/重构和文档阶段都创建独立本地 Git 提交，提交说明最后一段必须是“本次提交由BigStrongsSun完成”。

---

### Task 1: Provider 模型源就绪主流程

**Files:**

- Create: `src/components/providers/forms/CodexProviderReadinessSection.tsx`
- Create: `tests/components/CodexProviderReadinessSection.test.tsx`
- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`

**Interfaces:**

- Consumes: `catalogModels`、`apiFormat`、维护预设标记、同步模型回调、协议探测回调和探测状态。
- Produces: `CodexProviderReadinessSection`，展示模型目录、默认模型、协议结论、统一“验证连接”动作和可加入 MultiRouter 的就绪摘要。

- [ ] **Step 1: 写 Provider 主流程失败测试**

  在 `tests/components/CodexProviderReadinessSection.test.tsx` 使用真实组件断言：
  - “同步模型”“验证连接”“模型与兼容性”“就绪状态”默认可见；
  - 无模型时状态为“需要同步模型”；
  - 已有模型且维护预设时显示“由 CCSwitchMulti 维护”和“可加入 MultiRouter”；
  - 自定义 Provider 显示自动探测说明，维护预设不要求用户选择 Chat/Responses；
  - 探测错误使用 `role=alert`，正常状态使用 `role=status`。

- [ ] **Step 2: 运行测试确认 RED**

  Run: `pnpm exec vitest run tests/components/CodexProviderReadinessSection.test.tsx --exclude '.worktrees/**'`

  Expected: FAIL，因为 `CodexProviderReadinessSection` 尚不存在。

- [ ] **Step 3: 实现主流程组件并接入 Provider**

  新组件使用现有 Card/Button/Badge 设计系统，不复制网络逻辑；`CodexFormFields` 把现有模型同步按钮和协议探测状态/确认回调传入组件。模型同步、默认模型摘要、协议结论与连接验证从高级折叠移到常显主流程；高级区不再重复渲染这些控件。

- [ ] **Step 4: 运行定向测试确认 GREEN**

  Run: `pnpm exec vitest run tests/components/CodexProviderReadinessSection.test.tsx tests/components/ProviderForm.codexPreset.test.tsx src/components/providers/forms/ProviderForm.reasoning.test.ts --exclude '.worktrees/**'`

  Expected: PASS，且维护预设能力只读、自定义能力覆盖保存行为不变。

- [ ] **Step 5: 提交 Provider 主流程**

  Stage only Task 1 files and commit with RED/GREEN evidence in the body.

### Task 2: 专家配置与全局 Codex 设置边界

**Files:**

- Create: `src/components/settings/CodexGlobalConfigSettings.tsx`
- Create: `tests/components/CodexGlobalConfigSettings.test.tsx`
- Modify: `src/components/settings/SettingsPage.tsx`
- Modify: `src/components/providers/forms/CodexConfigEditor.tsx`
- Modify: `src/components/providers/forms/CodexConfigSections.tsx`
- Modify: `tests/components/CommonConfigModalBehavior.test.tsx`

**Interfaces:**

- Consumes: `configApi.getCommonConfigSnippet('codex')`、`configApi.setCommonConfigSnippet('codex', value)`、既有 TOML Goal mode 工具。
- Produces: 设置页高级区的“Codex 全局配置”面板；Provider 页默认折叠的“专家配置”容器。

- [ ] **Step 1: 写职责边界失败测试**

  断言 Provider 的 `CodexConfigEditor` 初始只显示“专家配置”，不直接暴露 `auth.json`、`config.toml`、Goal mode、应用通用配置或编辑通用配置；展开后仍可编辑 raw auth/config 和远程压缩。断言 `CodexGlobalConfigSettings` 能加载/保存公共 TOML，并在同一处切换 Goal mode。

- [ ] **Step 2: 运行测试确认 RED**

  Run: `pnpm exec vitest run tests/components/CodexGlobalConfigSettings.test.tsx tests/components/CommonConfigModalBehavior.test.tsx --exclude '.worktrees/**'`

  Expected: FAIL，因为全局面板不存在，Provider raw 配置仍默认展开。

- [ ] **Step 3: 实现职责迁移**

  `CodexConfigEditor` 增加默认折叠的专家容器，只保留 raw auth/config 与 Provider 级远程压缩；删除 Provider `CodexConfigSection` 内 Goal mode、公共配置开关和公共配置编辑按钮。`CodexGlobalConfigSettings` 直接维护统一 Codex TOML 片段，并复用 `isCodexGoalModeEnabled`/`setCodexGoalMode`，挂到设置页高级区最后。

- [ ] **Step 4: 运行测试确认 GREEN**

  Run: `pnpm exec vitest run tests/components/CodexGlobalConfigSettings.test.tsx tests/components/CommonConfigModalBehavior.test.tsx tests/hooks/useCommonConfigSave.test.tsx --exclude '.worktrees/**'`

  Expected: PASS，公共配置读写和原 Provider 保存流程均无回归。

- [ ] **Step 5: 提交专家/全局设置边界**

  Stage only Task 2 files and commit with RED/GREEN evidence in the body.

### Task 3: MultiRouter 四步向导

**Files:**

- Modify: `src/components/codex/CodexMultiRouterWizard.tsx`
- Modify: `src/components/codex/CodexMultiRouterWizard.test.tsx`
- Modify: `tests/components/CodexMultiRouterWizard.test.tsx`
- Modify: `tests/lib/codexMultiRouterWizard.test.ts`

**Interfaces:**

- Consumes: 既有 provider 选择、模型同步、碰撞别名、模型选择、路由生成、保存和启用函数。
- Produces: 四个可见阶段 `sources`、`prepare`、`review`、`activate`；内部继续复用现有纯函数和网络动作。

- [ ] **Step 1: 写四步向导失败测试**

  断言可见步骤严格为“选择模型源”“自动准备与验证”“选择模型并预览路由”“启用并验证”；不再出现“理解 MultiRouter”“命名方案”“获取模型列表”“处理重名模型”“整理模型”“保存并发布”独立步骤。断言方案名在预览阶段出现，模型同步/协议探测/碰撞摘要在准备阶段出现，最终 CTA 保存并启用后打开状态页。

- [ ] **Step 2: 运行测试确认 RED**

  Run: `pnpm exec vitest run src/components/codex/CodexMultiRouterWizard.test.tsx tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts --exclude '.worktrees/**'`

  Expected: FAIL，现有向导仍渲染十个步骤。

- [ ] **Step 3: 收敛状态机与 JSX**

  把介绍文案放在页头；`prepare` 顺序执行 provider 核对、模型同步、协议验证和碰撞处理并只展示异常；`review` 同页承载名称、模型选择和路由预览；`activate` 执行保存与启用。保留错误 issue、取消/返回和重试语义。

- [ ] **Step 4: 运行定向测试确认 GREEN**

  Run: `pnpm exec vitest run src/components/codex/CodexMultiRouterWizard.test.tsx tests/components/CodexMultiRouterWizard.test.tsx tests/lib/codexMultiRouterWizard.test.ts src/components/codex/CodexRouterWorkspacePage.test.ts --exclude '.worktrees/**'`

  Expected: PASS，工作台路由和状态逻辑不变。

- [ ] **Step 5: 提交四步向导**

  Stage only Task 3 files and commit with RED/GREEN evidence in the body.

### Task 4: 删除 MultiRouter 历史修复收尾

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx`
- Modify: `src/components/codex/CodexMultiRouterWizard.tsx`
- Modify: `tests/integration/App.test.tsx`
- Modify: `src/components/codex/CodexRouterWorkspacePage.test.ts`

**Interfaces:**

- Consumes: MultiRouter 运行态成功信号。
- Produces: 成功后保持状态工作台并显示完成提示；不再切换到 Sessions/历史修复。

- [ ] **Step 1: 写历史收尾失败测试**

  在 App/Workspace 测试中模拟运行态成功，断言不调用历史修复导航、不出现“下一步请修复历史记录可见性”，并显示“MultiRouter 已通过真实请求验证”。

- [ ] **Step 2: 运行测试确认 RED**

  Run: `pnpm exec vitest run tests/integration/App.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts --exclude '.worktrees/**'`

  Expected: FAIL，当前 App 会跳转 Sessions 并打开历史修复。

- [ ] **Step 3: 移除自动跳转**

  删除 MultiRouter 专属 post-setup history ref、成功导航回调和向导历史文案；状态页保留运行态成功通知，但不触发历史工具。独立的 Sessions 历史修复工具仍保留，避免删除已有手动能力。

- [ ] **Step 4: 运行测试确认 GREEN**

  Run: `pnpm exec vitest run tests/integration/App.test.tsx src/components/codex/CodexRouterWorkspacePage.test.ts tests/components/SessionManagerPage.test.tsx --exclude '.worktrees/**'`

  Expected: PASS，MultiRouter 不再自动迁移，手动 Sessions 功能仍可用。

- [ ] **Step 5: 提交历史收尾移除**

  Stage only Task 4 files and commit with RED/GREEN evidence in the body.

### Task 5: 完整验证、真实 UI 验收与记忆

**Files:**

- Modify: `memory.md`
- Create: `docs/audits/2026-08-14-provider-configuration-ux/10-provider-main-flow-implemented.png`
- Create: `docs/audits/2026-08-14-provider-configuration-ux/11-provider-expert-config-implemented.png`
- Create: `docs/audits/2026-08-14-provider-configuration-ux/12-multirouter-four-step-implemented.png`

**Interfaces:**

- Consumes: 完成后的前端、测试与当前分支。
- Produces: 可复核截图、全量验证结果和长期项目记忆。

- [ ] **Step 1: 执行静态与定向门禁**

  Run: `pnpm typecheck`

  Run: `pnpm exec prettier --check <本轮所有变更的 ts/tsx 文件>`

  Run: `git diff --check`

- [ ] **Step 2: 执行完整前端测试**

  Run: `pnpm test:unit -- --exclude '.worktrees/**'`

  如果再次出现已知 App 并发污染，必须使用完全相同提交隔离重跑 `tests/integration/App.test.tsx` 并分别报告全量与隔离结果，不能隐去失败。

- [ ] **Step 3: 启动本地 UI 并真实验收**

  使用现有 Vite/Tauri 开发入口，在实际 Provider 表单和 MultiRouter 向导验证主流程可见、专家折叠、四个步骤、键盘焦点和错误状态；保存三张新截图。

- [ ] **Step 4: 更新项目记忆**

  在 `memory.md` 记录最终职责边界、文件入口、测试结果、已知警告和后续不得恢复的旧行为。

- [ ] **Step 5: 提交验收与记忆**

  Stage only Task 5 files and commit with exact validation evidence in the body.

# Codex V2 Sub-Agent Technical Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一份有官方与源码证据、图解完整、可独立阅读的 Codex V2 Sub-Agent 与 CCSM 第三方模型适配中文报告，并把同源内容制作为 HTML 技术课件和完善后的 V23 Release Note。

**Architecture:** Markdown 报告作为权威内容源，按 V1 基线、V2 原生机制、第三方障碍、CCSM 控制面/数据面、角色案例和踩坑复盘组织。HTML 使用 html-ppt `document-deck` 模板和 `engineering-whiteprint` 主题重组同一证据链；Release Note 只保留发布摘要并链接两份完整材料。

**Tech Stack:** Markdown、Mermaid、HTML/CSS/SVG、html-ppt runtime/exploration、PowerShell、GitHub CLI。

## Global Constraints

- 只解释已经由 OpenAI 官方文档/源码、官方 issue、CCSM 当前源码/测试或发布证据支持的事实。
- 每段源码示例必须是最小真实机制片段，并逐行附中文职责注释；版权来源与路径必须标明。
- Markdown 和 HTML 必须包含结构图、时序图、角色图、状态机或状态流水线，而不是只写文字。
- HTML 是独立阅读主文档，正文不能依赖 presenter notes；不可通过缩小字号掩盖裁切。
- 不修改 V1/V2 运行代码、数据库或路由行为。
- 每一阶段独立提交，提交说明结尾必须为 `本次提交由BigStrongsSun完成`。

---

### Task 1: 建立证据索引与引用账本

**Files:**
- Create: `docs/references/codex-v2-subagent-evidence-index.md`

**Interfaces:**
- Consumes: OpenAI Subagents/Config 文档、Codex `multi_agents_v2`/role/config 源码、官方 issue、CCSM `codex_subagent_profiles.rs`、`codex_config.rs`、`codex_multi_agent.rs`、`forwarder.rs`。
- Produces: 后续 Markdown/HTML 共用的事实、链接、源码路径、提交与验证边界索引。

- [ ] 逐项记录 V1、V2、role、reserved schema、encrypted message、agent_message、Provider materialization 与 V23 配置修复证据。
- [ ] 标注每项为“官方机制 / 官方问题复现 / CCSM 实现 / 本地运行验证 / 未验证边界”。
- [ ] 检查所有外链可打开，所有本地源码路径存在。
- [ ] 提交证据索引。

### Task 2: 编写 Markdown 权威技术报告

**Files:**
- Create: `docs/codex-v2-subagent-third-party-adaptation-zh.md`

**Interfaces:**
- Consumes: Task 1 证据索引与已批准设计规格。
- Produces: HTML 课件的权威文字、图和代码内容源。

- [ ] 写完 Sub-Agent 定位、V1 结构/时序和 V2 动机。
- [ ] 写完 Codex V2 六层结构、完整时序、角色图和能力描述语义。
- [ ] 用四层故障模型解释第三方 V2 失败，给出失败时序和请求剖面。
- [ ] 写完 CCSM 控制面/数据面、混合路由时序和新角色完整状态流水线。
- [ ] 加入真实 Rust/TOML/JSON 最小代码案例，每行中文注释并解释整体副作用。
- [ ] 写完踩坑矩阵、V23 复盘、边界与引用。
- [ ] 运行链接、标题、Mermaid、占位符和 diff 检查并提交。

### Task 3: 重写 V23 Release Note 并更新 GitHub Release

**Files:**
- Modify: `docs/release-notes/v3.19.1-23-zh.md`

**Interfaces:**
- Consumes: Task 2 报告。
- Produces: 精炼发布正文及技术报告链接。

- [ ] 增补 V2 role/catalog 投影上下文、V23 两个根因及技术报告入口。
- [ ] 保留升级说明、验证边界和 workflow 自动下载区语义。
- [ ] 提交、推送文档；用 `gh release edit` 更新正文并复查正式 Release 和资产未变。

### Task 4: 搭建 HTML document-deck

**Files:**
- Create: `docs/presentations/codex-v2-subagent-report/index.html`
- Create: `docs/presentations/codex-v2-subagent-report/style.css`
- Create: `docs/presentations/codex-v2-subagent-report/README.md`
- Copy: html-ppt shared assets required for offline repository viewing under `docs/presentations/codex-v2-subagent-report/assets/`

**Interfaces:**
- Consumes: Task 2 Markdown 内容源。
- Produces: 24–30 页阅读型 HTML 技术课件。

- [ ] 从 `document-deck` 模板复制结构与运行时，不从空白页面编写。
- [ ] 实现封面、阅读地图、V1/V2、原生 V2、第三方失败、CCSM 适配、角色案例、踩坑和引用页。
- [ ] 使用内联 SVG/CSS 制作结构图、时序图、角色图和状态流水线。
- [ ] 加入术语 hover/focus/pin 交互、overview、主题切换和键盘导航。
- [ ] 源码页按“代码—逐行机制说明—整体说明”三层布局完成。

### Task 5: 浏览器渲染与视觉修订

**Files:**
- Create: `artifacts/codex-v2-subagent-report/*.png`
- Modify: Task 4 HTML/CSS as required by evidence.

**Interfaces:**
- Consumes: Task 4 deck。
- Produces: 实际渲染证据和无裁切的最终课件。

- [ ] 用本地 HTTP 服务和浏览器打开课件，逐页检查桌面视口。
- [ ] 截取封面、V1、V2 六层结构、第三方失败、CCSM 数据面、新角色和踩坑页。
- [ ] 检查小窗口、overview、主题切换、术语交互和 hash 深链。
- [ ] 修复所有裁切、重叠、过密、不可读代码或交互问题。
- [ ] 提交 HTML、资源和选定截图证据；不提交临时/错误截图。

### Task 6: 完成审计、项目记忆与远端交付

**Files:**
- Modify: `memory.md`

**Interfaces:**
- Consumes: 全部任务产物与验证输出。
- Produces: 可追溯的最终交付记录。

- [ ] 按设计规格逐项核对章节、图形、代码、引用、Release 和渲染要求。
- [ ] 运行 HTML 链接/结构检查、`git diff --check`、工作树与提交审计。
- [ ] 在 `memory.md` 记录报告结构、证据边界、文件路径、Release 更新与视觉验收。
- [ ] 单独提交 memory，推送当前分支，复查 GitHub 链接可访问。

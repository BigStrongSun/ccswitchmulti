# Codex V2 Sub-Agent Plain-Language Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Codex V2 Sub-Agent 32 页 HTML 技术课件逐页增加独立、准确、浅显的中文“大白话”解释，同时保留可对应源码的英文专业术语。

**Architecture:** `deck.js` 保存与页面顺序一一对应的 32 条浅显解释，并在运行时为每个 `.deck > .slide` 注入统一的 `.plain-language` 说明框。`style.css` 负责工程白图纸主题下的浅橙色视觉和小窗口自适应；`index.html` 保持原有专业内容与源码锚点不变。

**Tech Stack:** HTML、CSS、JavaScript、html-ppt document-deck runtime、应用内浏览器。

## Global Constraints

- 32 个页面必须各有一条独立解释，不能使用同一句模板文本重复填充。
- 每条解释用一至两句话回答“本页实际在讲什么、为什么重要”。
- 专业字段、函数名、协议名和源码原文保留英文；首次出现时优先给出中文直译。
- 大白话说明是页面正文的一部分，不放入 presenter notes，也不依赖术语弹窗。
- 不缩小现有代码字号；内容不足一屏时使用页面纵向滚动。
- 桌面 1280×720 与小窗口 900×650 都要实际渲染验证。
- 每阶段单独提交，提交说明结尾为 `本次提交由BigStrongsSun完成`。

---

### Task 1: 增加逐页浅显解释数据与组件

**Files:**
- Modify: `docs/presentations/codex-v2-subagent-report/deck.js`
- Modify: `docs/presentations/codex-v2-subagent-report/style.css`
- Modify: `docs/presentations/codex-v2-subagent-report/README.md`

**Interfaces:**
- Consumes: `.deck > .slide` 的固定 32 页顺序。
- Produces: 每页一个 `.plain-language` 元素，含 `.plain-label` 与页面专属正文。

- [ ] 在 `deck.js` 定义 32 条不重复的中文解释。
- [ ] 启动时校验解释数量与页面数量完全一致；不一致则抛出明确错误，避免静默漏页。
- [ ] 为每页标题之后插入解释框；封面插在副标题之后，普通页面插在 `h2` 之后。
- [ ] 在 `style.css` 增加浅橙色说明框、标签和响应式样式。
- [ ] 在 README 记录“大白话解释层”的定位。
- [ ] 运行静态检查并提交。

### Task 2: 真实浏览器验收与视觉修订

**Files:**
- Modify: Task 1 files when渲染证据要求修订。
- Modify: `artifacts/codex-v2-subagent-report/*.png`
- Modify: `memory.md`

**Interfaces:**
- Consumes: 注入后的 32 页课件。
- Produces: 无漏页、无不可接受裁切、可在小窗口滚动的大白话版本和新截图证据。

- [ ] 在应用内浏览器加载课件，确认 32/32 页各有且只有一个 `.plain-language`。
- [ ] 验证 32 条文案互不重复，页面顺序与主题一致。
- [ ] 在 1280×720 逐页检查横向溢出、元素越界与控制台错误。
- [ ] 在 900×650 检查代码页、时序页和表格页，确认字号未缩小且能纵向滚动。
- [ ] 等待页面过渡稳定后重新生成七张关键页截图。
- [ ] 更新 `memory.md`，记录术语中文化策略和验收结果。
- [ ] 运行最终审计、提交并推送当前分支。

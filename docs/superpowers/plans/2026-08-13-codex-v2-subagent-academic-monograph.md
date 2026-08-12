# Codex V2 Sub-Agent 学术专著课件实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付一套由不少于 25 篇 2025–2026 年论文支撑、可独立阅读、约 68 页的 Codex V1/V2 Sub-Agent 与 CCSM 第三方模型适配中文学术材料。

**Architecture:** 以 `papers.json` 作为论文元数据唯一结构化来源，稳定论文编号贯穿注释书目、权威报告、证据索引与 HTML 课件。Codex/CCSM 结论与论文研究结论分层取证，HTML 继续使用现有静态 `index.html + deck.js + style.css` 体系，并新增自动化内容、引用、样式与浏览器验收。

**Tech Stack:** Markdown、JSON、Node.js ESM、静态 HTML/CSS/JavaScript、Playwright/Chromium、PowerShell、Git。

## Global Constraints

- 核心论文不少于 25 篇，计数论文只能来自 2025 或 2026 年，且 2026 年数量必须多于 2025 年。
- 正式发表、预印本、在审与撤稿状态必须按 ACL Anthology、DOI、arXiv 或 OpenReview 原始页面准确标注。
- 每篇核心论文必须记录研究问题、方法、实验范围、可支持结论、不可外推边界、材料映射、PDF 状态与 SHA-256。
- 同时记录 Codex 内置搜索与 Matrix WebSearch 两条独立检索链；Matrix 召回不足时如实记录，不能把无关结果当证据。
- HTML 页数必须在 66–70 页；目标为设计规格定义的 68 页。
- 每页必须包含唯一的“这页用大白话说”，章节导览与章节总结必须完整。
- 1280×720 与 900×650 不允许横向溢出；代码字号不得低于 12.5px；正文表面必须不透明。
- 不声称 CCSM 重写 Codex orchestrator、解密 OpenAI ciphertext、提供确定性 Role 路由，或用配置存在代替真实 Child 运行证据。
- 不提交用户已有的 `artifacts/design-audit/subagent-theme-2026-08-11/05-light-after.png`。
- 每个修改/调试阶段单独提交，提交正文最后一行必须是 `本次提交由BigStrongsSun完成`。

---

### Task 1: 论文数据契约与失败测试

**Files:**
- Create: `scripts/validate-subagent-academic-materials.mjs`
- Create: `docs/references/subagent-multiagent-2025-2026-papers.json`

**Interfaces:**
- Produces: `loadPapers(path): Paper[]`、`validatePapers(papers): ValidationIssue[]`；后续书目、报告和 HTML 都消费稳定的 `P01`–`Pxx`。

- [ ] **Step 1: 写验证器的失败断言**

验证器必须检查：总数 ≥25、年份仅 2025/2026、2026>2025、ID 唯一、必需字段非空、正式论文有 venue/DOI、每篇至少一个报告章节和课件页映射、下载成功时必须有本地路径和 64 位 SHA-256。

- [ ] **Step 2: 用空数据运行并确认失败**

Run: `node scripts/validate-subagent-academic-materials.mjs --papers docs/references/subagent-multiagent-2025-2026-papers.json`

Expected: 非零退出，并逐项报告数量、年份与字段错误。

- [ ] **Step 3: 定义 JSON schema-v1 数据**

顶层包含 `schema_version`、`generated_at`、`search_chains`、`papers`；每篇论文包含规格第 3.3 节全部字段，结论字段使用完整中文句子，不保存任何凭据或 Cookie。

- [ ] **Step 4: 提交数据契约阶段**

Run: `git add scripts/validate-subagent-academic-materials.mjs docs/references/subagent-multiagent-2025-2026-papers.json && git commit -m "test(research): 建立Sub-Agent论文数据验收契约" -m "本次提交由BigStrongsSun完成"`

### Task 2: 权威元数据、注释书目与 PDF 归档

**Files:**
- Modify: `docs/references/subagent-multiagent-2025-2026-papers.json`
- Create: `docs/references/subagent-multiagent-2025-2026-annotated-bibliography.md`
- Create: `docs/references/papers/subagent-multiagent-2025-2026/README.md`
- Create: `docs/references/papers/subagent-multiagent-2025-2026/*.pdf`

**Interfaces:**
- Consumes: Task 1 数据契约。
- Produces: 至少 25 个通过验证的稳定论文记录；本地 PDF 清单包含 URL、状态、字节数、SHA-256 与失败原因。

- [ ] **Step 1: 从两条检索链建立候选集**

Codex Web 用 ACL/arXiv/OpenReview 定向检索；Matrix 使用不同查询独立检索。只把原始论文页或出版方页面用于最终元数据。

- [ ] **Step 2: 逐篇核对出版状态与实验边界**

至少覆盖层级组织、拓扑、角色差异、协作评估、失败模式、上下文分布、成本与鲁棒性；2026 数量必须大于 2025。

- [ ] **Step 3: 下载公开 PDF 并计算摘要**

下载只使用官方 PDF URL；失败项保留原 URL、HTTP/网络错误与 `download_status=failed`，禁止用镜像冒充成功。

- [ ] **Step 4: 生成逐篇注释书目**

每个 `[Pxx]` 小节必须写明“研究了什么、怎么研究、支持什么、不能证明什么、在本材料哪里使用”。

- [ ] **Step 5: 运行研究数据验证并提交**

Run: `node scripts/validate-subagent-academic-materials.mjs --papers docs/references/subagent-multiagent-2025-2026-papers.json`

Expected: PASS，输出总数、2025/2026 分布、正式/预印本分布、PDF 成功/失败数量。

### Task 3: 权威报告与证据索引

**Files:**
- Modify: `docs/codex-v2-subagent-third-party-adaptation-zh.md`
- Modify: `docs/references/codex-v2-subagent-evidence-index.md`
- Modify: `scripts/validate-subagent-academic-materials.mjs`

**Interfaces:**
- Consumes: Task 2 的 `[Pxx]` 与 Codex/CCSM 本地源码证据。
- Produces: 从 Agent 基础到 CCSM 实现的完整因果链；验证器可检查每篇核心论文至少被正文引用一次。

- [ ] **Step 1: 先增加“核心论文必须被引用”的失败检查**

对报告和课件分别扫描 `data-paper="Pxx"` 或 `[Pxx]`，缺任一核心 ID 时非零退出。

- [ ] **Step 2: 重写报告的概念与研究章节**

明确 Model、Agent、Harness、Tool Call、Sub-Agent、Multi-Agent、ensemble；以收益条件和失败条件解释为什么拆分，不能写成“Agent 越多越好”。

- [ ] **Step 3: 补齐组织、能力向量和系统评价**

比较星形、层级、链式、debate、树和图；评价任务分解、选角、工具、状态、通信、验证、终止、成本、鲁棒性与安全。

- [ ] **Step 4: 重写 Codex V1/V2 与 CCSM 章节**

在细节前分别给 V1/V2 概括，单列 V1→V2 原因；用源码路径/提交/函数证据说明 Role runtime、Provider 物化、消息投影和 fail-closed 边界。

- [ ] **Step 5: 扩展证据索引并提交**

证据索引分为论文、OpenAI 官方资料、Codex 源码、CCSM 源码和运行验收五层，不把间接证据写成运行成功。

### Task 4: 68 页 HTML 内容骨架与内容测试

**Files:**
- Modify: `docs/presentations/codex-v2-subagent-report/index.html`
- Modify: `docs/presentations/codex-v2-subagent-report/deck.js`
- Create: `docs/presentations/codex-v2-subagent-report/content.test.mjs`

**Interfaces:**
- Consumes: Task 3 报告章节、`[Pxx]` 和设计规格 68 页地图。
- Produces: 68 个有稳定 `data-slide-id` 的 `.slide`；每页唯一 plain-language 文本；论文按钮通过 `data-paper` 进入说明。

- [ ] **Step 1: 写页面结构失败测试**

检查 66–70 页、目标 68、页面 ID 唯一、每页一个且唯一的大白话块、12 个章节的导览/总结、V1/V2 概括页、V1→V2 转换页、全部核心论文引用。

- [ ] **Step 2: 运行测试确认当前 32 页不通过**

Run: `node docs/presentations/codex-v2-subagent-report/content.test.mjs`

Expected: FAIL，报告页数和缺失章节。

- [ ] **Step 3: 按规格迁移并扩展到 68 页**

每页包含完整可见中文段落、图/表/结构示意和证据 caption；术语首次出现提供直白中文定义；旧 32 页的有效源码证据与案例迁入对应新页。

- [ ] **Step 4: 接入论文引用详情交互**

点击 `[Pxx]` 可查看题名、年份、状态、研究范围、支持结论与边界；键盘 Enter/Space 可操作，Esc 可关闭。

- [ ] **Step 5: 运行内容测试并提交**

Run: `node docs/presentations/codex-v2-subagent-report/content.test.mjs`

Expected: PASS，输出 `68 slides`、`68 unique plain-language blocks` 与完整引用覆盖。

### Task 5: 三主题、阅读布局与响应式样式

**Files:**
- Modify: `docs/presentations/codex-v2-subagent-report/style.css`
- Modify: `docs/presentations/codex-v2-subagent-report/style.test.mjs`
- Modify: `docs/presentations/codex-v2-subagent-report/README.md`

**Interfaces:**
- Consumes: Task 4 的 HTML class 与交互状态。
- Produces: `engineering-whiteprint`、`academic-paper`、`tokyo-night` 三主题共享的不透明内容表面和双尺寸阅读布局。

- [ ] **Step 1: 扩展样式失败测试**

检查所有承载正文/表格/图/代码的表面都引用有效不透明 token；代码字号 ≥12.5px；小视口只有纵向滚动；focus-visible 清晰。

- [ ] **Step 2: 运行现有样式测试并观察新增失败**

Run: `node docs/presentations/codex-v2-subagent-report/style.test.mjs`

- [ ] **Step 3: 实现阅读型排版与学术引用样式**

使用主题 token，不写孤立 literal color；背景网格停留在页面底层；表格、引用、章节页、结构图和论文详情形成稳定层级。

- [ ] **Step 4: 更新操作说明并提交**

README 写明键盘导航、主题切换、总览、论文引用、术语解释、900×650 阅读方式与离线打开方法。

### Task 6: 浏览器验收与视觉证据

**Files:**
- Create: `docs/presentations/codex-v2-subagent-report/browser.test.mjs`
- Create/Modify: `artifacts/codex-v2-subagent-report/*.png`

**Interfaces:**
- Consumes: 完整 deck。
- Produces: 可重复浏览器验收输出与关键页截图。

- [ ] **Step 1: 写 Playwright 验收脚本**

在 1280×720 和 900×650、三主题下逐页检查 `scrollWidth <= clientWidth`、只有一页 active、正文表面计算后 alpha=1、控制台无 error/warning；测试 hash、总览、主题、术语、论文详情与 Esc。

- [ ] **Step 2: 运行全页浏览器验收**

Run: `node docs/presentations/codex-v2-subagent-report/browser.test.mjs`

Expected: 408 个页面/主题/视口组合无横向溢出，所有交互断言通过。

- [ ] **Step 3: 等待过渡后截图关键页**

至少截取封面、Agent 定义、Sub-Agent 定义、拓扑、能力向量、V1 概括、V1→V2、V2 架构、第三方障碍、CCSM 时序与总结；每张截图导航后等待 ≥900ms。

- [ ] **Step 4: 人工核对截图并提交视觉阶段**

确认文字未裁切、网格未穿透、caption 和 `[Pxx]` 可读，900×650 无元素被固定层遮挡。

### Task 7: 最终验收、项目记忆与推送

**Files:**
- Modify: `memory.md`
- Modify: `docs/presentations/codex-v2-subagent-report/README.md`

**Interfaces:**
- Consumes: Tasks 1–6 全部产物。
- Produces: 可追溯最终验收记录、项目知识和远端分支。

- [ ] **Step 1: 执行全套新鲜验证**

Run: `node scripts/validate-subagent-academic-materials.mjs --papers docs/references/subagent-multiagent-2025-2026-papers.json && node docs/presentations/codex-v2-subagent-report/content.test.mjs && node docs/presentations/codex-v2-subagent-report/style.test.mjs && node docs/presentations/codex-v2-subagent-report/browser.test.mjs && git diff --check`

- [ ] **Step 2: 按目标逐项完成审计**

把论文数量/年份、概念边界、V1/V2 概括、组织评价、CCSM 实现、68 页、三主题、双尺寸和交互分别映射到文件或测试证据；任何缺项都返回对应任务修正。

- [ ] **Step 3: 更新 `memory.md`**

记录论文数据源、关键研究结论与反例、Codex/CCSM 表述边界、验证命令、截图路径及仍存在的不确定性；修正任何已过期的旧知识。

- [ ] **Step 4: 最终提交并推送**

提交只包含本任务文件，明确排除用户的未跟踪 PNG；确认工作树只剩该用户文件后推送 `fork/bigstrongsun/subagent-v2-capability-injection`。

## Self-Review

- 规格覆盖：七个任务覆盖论文数据、注释书目/PDF、报告、证据索引、68 页 HTML、三主题交互、双尺寸视觉和 memory/push。
- 占位符扫描：计划不含 TBD、TODO 或“稍后实现”；每个任务都有明确文件、验证和提交边界。
- 接口一致性：`papers.json` 的稳定 `Pxx` 是所有下游材料的唯一论文身份；内容测试和研究验证器复用同一 ID 集合。
- 风险边界：Matrix 学术召回不足、PDF 下载失败和预印本状态都有显式记录路径，不会被掩盖成成功。

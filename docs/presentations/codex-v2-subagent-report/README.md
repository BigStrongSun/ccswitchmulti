# Codex V2 Sub-Agent 学术专著课件

这是一份 68 页、可独立阅读的 HTML 学术型技术课件，对应权威 Markdown 报告和论文数据：

- `../../codex-v2-subagent-third-party-adaptation-zh.md`
- `../../references/codex-v2-subagent-evidence-index.md`
- `../../references/subagent-multiagent-2025-2026-annotated-bibliography.md`
- `../../references/subagent-multiagent-2025-2026-papers.json`

## 阅读方式

直接通过本地 HTTP 服务打开 `index.html`。支持：

- `←` / `→`：翻页；
- `O`：页面总览；
- `T`：切换 `engineering-whiteprint`、`academic-paper`、`tokyo-night` 主题；
- `[Pxx]`：打开论文详情，查看研究问题、方法范围、支持结论和不可外推边界，`Esc` 关闭；
- 每页标题下的“这页用大白话说”：用一至两句中文解释本页到底在讲什么、为什么重要；专业术语仍保留，以便和源码对应；
- `#/页码`：深链到指定页面。

课件从 html-ppt `document-deck` 模板派生，运行时、字体、主题与动画样式已经复制到本目录 `assets/`，可离线托管。页面正文不依赖 presenter notes。

在 900×650 小窗口中使用纵向滚动阅读；设计不允许横向溢出。所有正文卡片、表格、图示和弹窗使用不透明主题表面，背景网格只应出现在页面空白区。

## 内容边界

课件先解释 Agent、Sub-Agent、Multi-Agent、组织拓扑和系统评价，再进入 Codex V1/V2、custom role、第三方模型障碍以及 CCSM 控制面/数据面。它不是安装运维手册，也不声称 CCSM 可以解密 OpenAI ciphertext、重写 Codex orchestrator，或将 role description 当作确定性模型路由。

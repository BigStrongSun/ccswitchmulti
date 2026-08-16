# Codex Sub-Agent 双主题视觉实现计划

1. 保存当前安装版在深色/浅色下的 Sub-Agent 与相邻状态页截图，建立真实基线。
2. 先新增失败测试，覆盖：页面渐变双主题契约、V1/V2 卡语义、模型状态色、能力 chip、折叠区语义和保存栏状态色。
3. 在 `CodexSubagentProfileEditor` 内增加少量 tone helper，避免在 JSX 中重复状态判断，不修改配置数据流。
4. 在 `CodexRouterWorkspacePage` 中让 V1/V2 协议卡始终使用对应语义色；已启用按钮仍为灰色 disabled。
5. 运行聚焦测试、全量前端测试、typecheck、格式检查和 Rust 全量回归。
6. 统一升级为 `3.19.1-21`，构建本地发布产物。
7. 使用独立隐藏 PowerShell 事务完成 kill/wait、卸载、安装、隐藏拉起、health/version/hash 校验和失败回滚。
8. 在安装版中重新截取深色和浅色 Sub-Agent 页面，对比基线并确认 `/health` 为 200。

本计划只做本地提交、构建和安装，不推送、不创建 PR、不发布 GitHub Release。

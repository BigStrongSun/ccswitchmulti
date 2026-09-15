# 2026-09-14 MODT 历史修复错误分类修复

## 根因与实现
MODT 取证详见父目录 memory-2026-09-14-modt-history-repair-diagnosis.md。旧 historyRepairErrorMessage 将 Codex/ChatGPT/app-server/running/进程/运行 任意关键词当成进程占用证据，导致分页保护、数据库权限、普通请求错误都附加退出重试提示。

删除宽泛关键词推断。仅识别明确的 codex_paginated_history_immutable: 错误码，解释旧式 Provider/可见性迁移不支持分页或无法安全检查的历史，退出重启不能解除保护，保留原始错误细节。其他错误原样返回，后端真实进程保护自带的退出指引保留。

## 测试和边界
- TDD RED：5 个非进程错误用例全部复现错误追加退出提示，5 failed / 5 passed。
- GREEN：历史面板 10/10，会话管理页 17/17，共 27/27；typecheck 通过。
- 测试同时验证 dry-run 失败后不打开确认框、不进入 apply，并保留真实进程错误。
- 未更改 Rust guard、迁移算法、真实历史或配置。分页历史仍不能由旧式 Provider/可见性迁移入口改写；这不是已完成数据恢复。
- 未打包、发布、安装到 MODT 或进行安装态 UI 验收。
- 内置 Web 与 Matrix 独立检索未获得相关官方资料，证据不足以作上游状态判断；本次分类修复依据本地调用链和 MODT 已采集元数据。
- 保留已有 protocol_compatibility.rs 未提交改动及其他无关文件。
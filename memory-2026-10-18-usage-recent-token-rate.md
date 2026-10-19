# 最近五分钟平均 Token 消耗速度

## 2026-10-19 补齐本地构建遗漏

- 根因是发布集成分支与日常主 checkout 的已完成功能未对齐：速度功能原提交 `6f8bcdc2c` 留在 `bigstrongsun/te-provider-type`，`3.20.4-4` 构建源 `5ab3e514c` 不包含它。该包不能被描述为包含速度统计。
- 仅移入独立功能提交为 `ea606246c`；没有引入整个主分支、TP 变化或内部 AgentMesh。前端 Hero/query/test 与原功能提交完全相同，后端 `get_usage_summary` 及 UsageDashboard 调用契约兼容，不新增 IPC 或迁移。
- 新版本四处统一为 `3.20.4-5`，同包保留官方 Provider 上下文编辑和模型探测。合并后聚焦 9 文件 109 tests（44 usage + 65 Provider/context）与 `pnpm typecheck` 通过；原桌面/窄屏截图的组件与相关样式未变，复用该证据。不重复运行未变更的 Rust 全库测试。
- 本阶段是已有功能的本地集成和版本打包，无需新外部 API 事实，未重复联网检索。产物目标为主项目忽略目录 `release/local-v3.20.4-5`；保留 `3.20.4-4` 旧包供溯源，不安装、不重启、不发布。构建完成与哈希待实际产物核验后记录。
- 从 clean source `a4df25110a6e928e379d5cce23f3250cd65c2e56` 实际构建，2026-10-19 11:29:26 开始、11:46:49 成功导出，退出 0；helper 7m56s、前端 8.63s、main 8m39s。本次隔离 target 在 finally 清理（3.2 GiB）。独立兼容性审查无阻断；四处版本源及 PE FileVersion/ProductVersion 均为 `3.20.4-5`，ProductName=CCSwitchMulti。
- 最终生产 renderer `index-BAhma40j.js` 同时包含 `usage.recentRate`、`recent-rate`、`tok/s` 与 `codexConfig.officialContextSet1M`；SHA-256 为 `66A9D0DBE5ED6E03CF2807BE9F4747414EE8AA34F23720EB7ED24AF89F1C8AA7`。确认的是实际生产 bundle，不仅是源文件。
- 导出 `SHA256SUMS.txt` 16/16 重算匹配，metadata commit/version 正确。NSIS 为 13,985,492 bytes / SHA-256 `AC307C2985FD0F8CC0878A8A43DF97AED73DDE0DB64F69B00D839E8456854190`；portable ZIP 为 16,514,805 bytes，内部 EXE 与 raw EXE 一致，SHA-256 `C521FD37140F6F4B2E775CB0980AB7F6FDAF3BB7534B7558E3E03EE79B2D4658`。本地 updater metadata 版本及 signature 与 `.sig` 一致，未另行公钥验签。未安装或运行新版，也未发布。

## 实现与口径

- 统计概览 `UsageHero` 新增近五分钟平均速度，单位 `tok/s`。
- `useUsageRecentRate` 复用 `usageApi.getUsageSummary`，每次查询重新取当前时间，查询 `[now - 300, now]`，使用 `realTotalTokens / 300`。包含归一化输入、输出、缓存写入及缓存读取，与现有真实消耗总量一致，不是单请求输出生成速度。
- 跟随应用、供应商和模型筛选；独立于历史日期选择。沿用概览刷新间隔及手动刷新，关闭自动刷新时不额外轮询。
- 后端 `services/usage_stats.rs::get_usage_summary` 已使用时间范围过滤、缓存语义归一化及现有去重策略，五分钟部分日范围不会加入整日 rollup；无需新增 IPC 或数据库迁移。
- 使用日志记录时间归属窗口；尚未写入日志的在途用量和本地会话采集延迟不属于实时逐 token 流量。

## 验证与边界

- 单元测试覆盖移动窗口、筛选、平均速度与 UI 状态；集成测试及类型检查结果以本次提交说明为准。
- Playwright 使用实际 Hero 组件和预置查询缓存检查 1280px / 390px，截图位于忽略目录 `.local-archives/usage-rate-20261018/`。这是布局验证，不是安装态真实用量验收。
- 内置 Web 查阅 SQLite 聚合文档；Matrix 独立检索两次均返回 `fetch failed`，未形成第二条外部证据。实现依据本地接口、SQL 和测试，不依赖外部新 API。
- 未修改版本、安装二进制、重启 CCSM 或推送。现有 `memory.md`、协议命令及供应商布局预览的无关修改保留，不纳入本次提交。

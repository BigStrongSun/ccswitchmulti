# Codex 自动采集与流量 UI 合并验收

- 用户明确要求本地合并；目标 main 起点 `7c9a2e79`，功能分支 `bigstrongsun/codex-traffic-observability` 终点 `c8dacb09`，共同祖先 `216f870e`。
- 主目录有其他任务的 `src-tauri/src/commands/protocol_compatibility.rs` 未提交修改、`.tmp/` 与 `docs/provider-settings-layout-preview.html` 未跟踪内容。因此先在 `.tmp/traffic-merge-candidate` 隔离生成并验证合并提交，再把 main 快进到已验证结果；不 stash、不清理他人文件。
- 唯一冲突为根 memory.md 的追加记录，保留 main 看门狗记录与功能分支流量/采集/UI 记录。lib.rs 自动合并后同时保留 watchdog supervisor、唯一 session_collection 周期协调器与状态命令注册；schema24 与 main 没有版本竞争。
- Terra 执行独立只读范围/迁移审计；主代理负责冲突处理、测试与最终 Git 集成。原功能工作区仍承载交互预览，本次不删除它。
- 此任务是本地 Git DAG 与源码整合，不引入外部事实或新 API，故未联网搜索。历史 UI 外部规范检索证据不足与视觉验收未完成的限制仍然有效。
- 不构建安装包、不安装、不重启、不修改代理路由、不推送。前端生产构建不等于已安装版本；主子净账及效率排名仍受原始证据边界约束。

## 合并树验证

- Rust `cargo test --lib --no-default-features -- --test-threads=1`：4178 passed、0 failed、7 ignored，74 秒；LOCALAPPDATA 隔离到临时目录，日志 `C:/Users/sunda/AppData/Local/Temp/ccsm-traffic-merge-rust.log`。
- 前端全量：1607 passed、1 failed，共1608；报告 `C:/Users/sunda/AppData/Local/Temp/ccsm-traffic-merge-frontend.json`。唯一失败为 AddProviderDialog 的“普通 Codex 新增没有 receipt”断言，伴随 restore_codex_provider_protocol_evidence 缺少 MSW handler。Terra 在干净且精确 main@7c9a2e79 的既有构建工作区独立复现相同测试、相同 commitCodex 调用次数 0 断言（7 passed/1 failed）。确认为基线失败，不在流量功能合并中扩改；不得宣称全前端全绿。
- TypeScript --noEmit、Vite production build、git diff --cached --check、33 个已暂存变更文件 UTF-8 严格解码/no BOM/no U+FFFD 通过。构建仍有浏览器数据过期和 chunk 警告，Rust 仍有既有 unused 字段与测试无效 UTF8 字面量警告。

# 2026-08-16 Codex Multi-agent V2 前五模型宣传顺序

- 官方 Codex 当前的 V1 与 V2 共用 `spawn_agent_models_description()`；模型选择器可见目录经过过滤后仍执行 `.take(MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION)`，常量窗口为 5。官方 issue `openai/codex#34166` 也在 V2 下复现：工具说明只宣传五个模型，但被省略的第六个有效模型仍可在 `fork_turns = "none"` 时显式调用。
- 不要在 CCSwitchMulti 下游修改官方 reserved `spawn_agent` schema 来扩大模型列表。官方问题 `#32031`、`#32674`、`#32988` 及本项目历史运行证据都表明，ChatGPT auth 会校验保留工具 schema，字段漂移可能被后端拒绝。
- CCSwitchMulti 的正确产品边界是控制五模型宣传窗口，而不是删除其它模型：`settingsConfig.modelCatalog.spawnAgentModels` 决定 catalog 的优先顺序；V2 managed roles 仍从完整可路由 profile/catalog 生成。
- V2 工作台顺序固定为：第一步配置 V2 模型与能力；第二步选择并排序 `spawn_agent` 工具说明宣传的前五模型。V1 与 V2 共用并保留同一 `spawnAgentModels` 顺序，切换协议不能清空它。
- 现场 `qwen3.8` 未出现的根因不是模型不可路由：它已存在于九模型完整目录中，但原前五全是 GPT。2026-08-16 本机已事务备份数据库，并将生效 MultiRouter 与两个 live catalog 的前五统一为 `deepseek-v4-flash / gpt-5.6-sol / qwen3.8 / gpt-5.6-luna / gpt-5.6-terra`；新 Codex 会话才能读取新的工具说明。
- 新任务 `01a00954-710a-76b1-a6ae-dffa57eeb621` 证明只调整 JSON 数组顺序仍无效：Codex 会按每条模型的数值 `priority` 再排序。CCSM 的同 slug 官方元数据合并此前把官方 GPT 的 `priority=1/2/3` 当成权威字段，覆盖了路由目录生成的 `1000+index`；Qwen/DeepSeek 因而仍排在五个 GPT 后面。根修边界是让路由目录拥有 `priority`，同时继续保留官方 transport、reasoning、service tier 与展示元数据；端到端 cache 测试必须同时断言模型数组顺序和连续 priority。
- 最终发布必须基于当时权威主线版本递增，不能把旧修复分支的版本直接安装。2026-08-16 已把 V2 前五 UI、设计边界与 priority 根修线性合入 `fork/main@v3.19.2-3`，将版本统一递增为 `v3.19.2-4`；远端 `main` 与发布提交 `b775bae8` 一致。
- 正式本地发布流水线使用本机 Tauri updater 私钥成功生成 `CCSwitchMulti_3.19.2-4_x64-setup.exe`、签名、portable ZIP、raw EXE 与 `latest.json`。已安装 raw EXE 的 SHA-256 为 `9D39138D54852F53567278259796ABD20DBDB71AC7CD6F5B2B78AE25B5535655`，Windows FileVersion/ProductVersion 均为 `3.19.2-4`，`15721/health` 正常。
- 安装后两个 live catalog 的前九模型 priority 均连续为 `1000..1008`；前五为 `deepseek-v4-flash / gpt-5.6-sol / qwen3.8 / gpt-5.6-luna / gpt-5.6-terra`。全新独立 Codex 任务 `01a009be-80b0-72e0-adc2-26190678b40c` 实际读取 `spawn_agent` 工具说明并返回同一前五，明确确认包含 `qwen3.8`；这是最终验收，不再以文件数组顺序代替工具 schema 实测。
- 发布构建需要避免 post-commit 流水线竞态：cherry-pick 期间提前启动的流水线可能先读取旧版本。必须等待锁释放，再从最终发布提交手工运行一次完整 `pnpm release:local`，并同时核对入口版本、Rust crate 版本、NSIS 包名、EXE 版本元数据与哈希。

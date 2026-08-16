# 2026-08-16 Codex Multi-agent V2 前五模型宣传顺序

- 官方 Codex 当前的 V1 与 V2 共用 `spawn_agent_models_description()`；模型选择器可见目录经过过滤后仍执行 `.take(MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION)`，常量窗口为 5。官方 issue `openai/codex#34166` 也在 V2 下复现：工具说明只宣传五个模型，但被省略的第六个有效模型仍可在 `fork_turns = "none"` 时显式调用。
- 不要在 CCSwitchMulti 下游修改官方 reserved `spawn_agent` schema 来扩大模型列表。官方问题 `#32031`、`#32674`、`#32988` 及本项目历史运行证据都表明，ChatGPT auth 会校验保留工具 schema，字段漂移可能被后端拒绝。
- CCSwitchMulti 的正确产品边界是控制五模型宣传窗口，而不是删除其它模型：`settingsConfig.modelCatalog.spawnAgentModels` 决定 catalog 的优先顺序；V2 managed roles 仍从完整可路由 profile/catalog 生成。
- V2 工作台顺序固定为：第一步配置 V2 模型与能力；第二步选择并排序 `spawn_agent` 工具说明宣传的前五模型。V1 与 V2 共用并保留同一 `spawnAgentModels` 顺序，切换协议不能清空它。
- 现场 `qwen3.8` 未出现的根因不是模型不可路由：它已存在于九模型完整目录中，但原前五全是 GPT。2026-08-16 本机已事务备份数据库，并将生效 MultiRouter 与两个 live catalog 的前五统一为 `deepseek-v4-flash / gpt-5.6-sol / qwen3.8 / gpt-5.6-luna / gpt-5.6-terra`；新 Codex 会话才能读取新的工具说明。

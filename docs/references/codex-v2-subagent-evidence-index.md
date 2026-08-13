# Codex V2 Sub-Agent 与 CCSM 第三方适配证据索引

更新时间：2026-08-13

这份索引为《Codex V2 Sub-Agent 与 CCSM 第三方模型适配》技术报告提供证据账本。它把“2025–2026 学术研究”“官方已说明的能力”“官方源码中的运行机制”“公开问题复现”“CCSM 当前实现”和“本地/发布验证”分开，避免把论文 benchmark、issue 复现或构建结果写成超出其范围的承诺。

## 证据等级

| 标记 | 含义 | 可支持的表述 |
|---|---|---|
| PAPER | 2025–2026 正式论文 | 指定模型、任务、组织结构和实验范围内的研究机制与反例 |
| O-DOC | OpenAI 官方文档 | 产品能力、公开配置字段、推荐用法 |
| O-SRC | OpenAI Codex 官方源码 | 当前实现的数据结构、分支和调用顺序 |
| O-ISSUE | OpenAI 官方仓库 issue | 已报告的现场复现；不自动代表官方设计结论 |
| C-SRC | CCSwitchMulti 当前源码 | CCSM 已实现的配置、路由和协议行为 |
| C-TEST | CCSM 自动化测试/提交 | 回归契约及其引入、修复历史 |
| RUNTIME | 本地或 GitHub Actions 运行证据 | 指定环境中真实发生的结果 |

## 0. 论文研究证据

核心论文的唯一结构化目录是 [`subagent-multiagent-2025-2026-papers.json`](subagent-multiagent-2025-2026-papers.json)，逐篇研究问题、方法、实验范围、支持结论和不可外推边界见 [`subagent-multiagent-2025-2026-annotated-bibliography.md`](subagent-multiagent-2025-2026-annotated-bibliography.md)。26 篇均为 ACL Anthology 正式论文，其中 2026 年 16 篇、2025 年 10 篇；26 份官方 PDF 与 SHA-256 见 [`papers/subagent-multiagent-2025-2026/README.md`](papers/subagent-multiagent-2025-2026/README.md)。

| 研究结论 | 等级 | 核心证据与边界 |
|---|---|---|
| 多 Agent 的系统价值是条件性的：可分片上下文、并行搜索和专业化流水线可以获益。 | PAPER | P01 ExtAgents、P08 MOSA、P10 长 triple-set 生成；结论限定于论文各自可分解且可整合的任务。 |
| 更多 Agent 和更多通信不天然更好。 | PAPER | P06 SILO-BENCH、P09 Debate、P12 AgentSlimming、P13 Diversity Collapse；分别覆盖协调鸿沟、同质辩论、冗余节点和结构耦合。 |
| 拓扑是任务相关的设计变量，不存在由现有证据支持的全局最优拓扑。 | PAPER | P03 动态拓扑、P17 MultiAgentBench、P18 信息传播、P22 AnyMAC。 |
| Role 需要表达任务相关能力和差异，但描述不是确定性路由。 | PAPER | P04 伙伴 trait、P19 学习式角色差异、P20 Theory of Mind、P26 团队初始化；自然语言 Role 与论文机制不可直接等同。 |
| 评价对象必须从模型扩展到 harness、拓扑、编排、过程、成本和安全。 | PAPER | P02 ACI、P05 MASEval、P11 风险优先评价、P14 内部失效、P15 过程评价、P21 Collab-Overcooked。 |

论文只用于解释一般机制、实验反例和设计问题；Codex V1/V2 字段、调用顺序与 CCSM 行为仍必须分别由 O-DOC/O-SRC 和 C-SRC/C-TEST/RUNTIME 证明。

## 1. Sub-Agent 的意义与公开定位

| 结论 | 等级 | 证据 |
|---|---|---|
| Sub-Agent 可以并行执行专门任务，再由主线程收集结果。 | O-DOC | [OpenAI Subagents 文档](https://learn.chatgpt.com/docs/agent-configuration/subagents)，`Why subagent workflows help` 与核心术语章节。 |
| 把探索日志、测试输出和中间噪声移出主线程，可减轻 context pollution/context rot；写密集型并行任务需要更谨慎。 | O-DOC | 同上，官方文档明确列出主线程聚焦、并行探索和写冲突边界。 |
| 每个 child 有自己的 Agent thread，支持的客户端可单独打开查看。 | O-DOC | 同上，`Agent thread` 定义与 App/CLI/IDE 行为说明。 |

## 2. Custom Agent 与能力描述

| 结论 | 等级 | 证据 |
|---|---|---|
| standalone custom agent 文件至少定义 `name`、`description`、`developer_instructions`。 | O-DOC | [OpenAI Subagents 文档：Custom agent file schema](https://learn.chatgpt.com/docs/agent-configuration/subagents)。 |
| `description` 是 Codex 何时使用该 agent 的人类可读 guidance；`developer_instructions` 定义 agent 被选中后的核心行为。 | O-DOC | 同上字段表。 |
| custom agent 可以覆盖普通 session config，包括 model、reasoning、sandbox、MCP 和 skills。 | O-DOC | 同上，官方说明 custom agent 文件作为 spawned session 的配置层加载。 |
| model/effort 解析是逐项的：agent 文件值优先；否则 explicit spawn → `[agents]` default → parent。 | O-DOC | 同上 `Choosing models and reasoning` 及文件 schema 前说明。 |
| 官方配置结构把 role `description` 定义为 spawn tool guidance，把 `config_file` 定义为 role-specific config layer。 | O-SRC | [`codex-rs/config/src/config_toml.rs` 的 `AgentsToml` / `AgentRoleToml`](https://github.com/openai/codex/blob/main/codex-rs/config/src/config_toml.rs)。 |

## 3. V1 与 V2 运行时边界

| 结论 | 等级 | 证据 |
|---|---|---|
| V1 的 max depth 只影响 V1，V2 使用并发线程限制。 | O-SRC/O-DOC | `AgentsToml.max_depth` 源码注释；[配置参考](https://learn.chatgpt.com/docs/config-file/config-reference) 的 `agents.max_concurrent_threads_per_session`。 |
| V2 `spawn_agent` 参数包含 message、task_name、agent_type、model、reasoning_effort、service_tier、fork_turns。 | O-SRC | [`multi_agents_v2/spawn.rs` 的 `SpawnAgentArgs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs)。 |
| `fork_turns` 缺省为 `all`；可为 `none`、`all` 或正整数字符串。 | O-SRC | 同上 `fork_mode()`。 |
| V2 handler 先构建 child config，再应用 model/role/service tier/runtime override，最后创建 task path 和 child thread，并记录 parent thread/turn。 | O-SRC | 同上 `Handler::spawn` 主调用链。 |
| `task_name` 用于 canonical agent path；role 由 `agent_type` 进入 `apply_spawn_agent_role`。 | O-SRC | 同上 `role_name`、`thread_spawn_source` 与 `apply_spawn_agent_role`。 |

V1 工具族和 V2 工具族的精确差异还以调研固定的 OpenAI Codex commit `646f7c0a91b8e327d263335da68ae8ef212895ce` 及本仓库设计文档 [`2026-08-09-codex-subagent-v1-v2-settings-design.md`](../superpowers/specs/2026-08-09-codex-subagent-v1-v2-settings-design.md) 为代码审计锚点。报告将其表述为源码观察，不称为长期稳定 API。

## 4. Reserved schema 与 V2 第三方 child 障碍

| 结论 | 等级 | 证据 |
|---|---|---|
| 将 metadata 暴露到 server-reserved `collaboration.spawn_agent` 会改变 schema，并可能在模型推理前被拒绝。 | O-ISSUE + C-TEST | [openai/codex#32674](https://github.com/openai/codex/issues/32674) 的最小复现；CCSM `src-tauri/src/codex_config.rs` 中 `hide_spawn_agent_metadata=true` 和 reserved schema 回归。 |
| V2 可把 child 指令封装为 OpenAI-specific `agent_message`；外部 Responses Provider 不一定接受该 item 或解密其内容。 | O-ISSUE | [openai/codex#33551](https://github.com/openai/codex/issues/33551)。 |
| V2 协作消息加密会降低本地审计可见性。 | O-ISSUE | [#28058](https://github.com/openai/codex/issues/28058)、[#32753](https://github.com/openai/codex/issues/32753)。 |
| `codex exec` 场景存在 parent 无法消费 encrypted child output 的公开复现，但不能外推到所有 interactive App/TUI。 | O-ISSUE | [#33267](https://github.com/openai/codex/issues/33267) 明确区分 exec 与 interactive observation。 |
| 模型目录强制 V2、隐藏 routing metadata、full-history fork 和 task_name/role 分离等问题存在组合复现。 | O-ISSUE + O-SRC | [#32705](https://github.com/openai/codex/issues/32705)；关键机制分别由当前官方源码再次核对。 |

报告的限定语：这些 issue 证明具体版本和配置下存在复现，不证明 OpenAI 永久不支持第三方 V2，也不证明所有 V2 流程均失败。

## 5. CCSM capability compiler

| 结论 | 等级 | 证据 |
|---|---|---|
| `settingsConfig.codexRouting.subagentV2` 保存问卷和 overrides，backend 是唯一编译器。 | C-SRC/C-TEST | `src-tauri/src/codex_subagent_profiles.rs`；设计与契约见 [`2026-08-10-codex-subagent-capability-injection-design.md`](../superpowers/specs/2026-08-10-codex-subagent-capability-injection-design.md)。 |
| 编译器生成 role name、description、developer instructions、nickname、model、fixed router provider 和 reasoning。 | C-SRC | `compile_subagent_v2_profiles`、`generated_description_for_provider`、`generated_instructions_for_provider`。 |
| description 表达擅长/排除任务和选择偏好；manual override 完全替换自动 selection text。 | C-SRC/C-TEST | `generated_description_for_provider` 及 `codex_subagent_v2_manual_description_fully_replaces_policy_text`。 |
| 不可路由 profile 保留配置但不生成 role；provider kind 来自 Provider record/context，而非模型名猜测。 | C-SRC/C-TEST | `codex_subagent_route_classification_with_context`、profile status/preview 回归。 |
| role 名冲突按安全规则分配，用户自写无 marker 文件不覆盖。 | C-SRC/C-TEST | `src-tauri/src/codex_subagent_profiles.rs` 命名分配；`src-tauri/src/codex_config.rs` managed file ownership tests。 |

关键历史提交：`b4e99cca`（RED）、`6532f2e1`（compiler）、`c31e00ce`、`5362a9e3`、`344f2f24`（持久化、Provider 分类和预览契约根修）。

## 6. CCSM config/catalog/role 投影

| 结论 | 等级 | 证据 |
|---|---|---|
| V2 显式启用 `features.multi_agent_v2` 并保持 `hide_spawn_agent_metadata=true`。 | C-SRC/C-TEST | `src-tauri/src/codex_config.rs::ensure_codex_multi_agent_feature` 及 tests。 |
| 混合路由使用非保留 `agents` namespace；纯 Official 不强制替换 reserved namespace。 | C-SRC/C-TEST | `mixed_router_uses_non_reserved_agents_tool_namespace`、`official_only_router_does_not_force_non_reserved_tool_namespace`。 |
| role 文件把 child model 绑定到统一 `codex_model_router_v2`，由运行时路由决定真实上游。 | C-SRC | `src-tauri/src/codex_subagent_profiles.rs` compiled role 与 `src-tauri/src/codex_config.rs` role rendering。 |
| live projection 只在目标 Provider 是 effective current 时执行；非 current 保存返回 `NotRequired`。 | C-SRC/C-TEST | `src-tauri/src/services/provider/mod.rs::finish_codex_subagent_v2_mutation` 与 `updating_valid_non_current_subagent_v2_does_not_touch_live_projection_files`。 |

## 7. CCSM 数据面：明文协作与第三方投影

| 结论 | 等级 | 证据 |
|---|---|---|
| CCSM 不修改 reserved `collaboration.*` schema；只在 mixed-router 的非保留 `agents.*` 协作工具上移除 message 的 encrypted schema marker。 | C-SRC/C-TEST | `src-tauri/src/proxy/providers/codex.rs` request-local `codexRouterPlaintextV2Collaboration`，相关 schema sanitizer tests；设计见 [`2026-08-05-codex-cross-provider-v2-subagent-payload-design.md`](../superpowers/specs/2026-08-05-codex-cross-provider-v2-subagent-payload-design.md)。 |
| 该 mixed-router 决策随已解析 route 以 request-local boolean 传播，不复制整个 router 或二次路由。 | C-SRC/C-TEST | `codexRouterPlaintextV2Collaboration` 的写入和 `forwarder.rs` 回归。 |
| 只有实际第三方 Codex Responses 目标才执行 `agent_message` 投影；Official/OAuth、非 Responses 和普通 OpenAI API 请求跳过。 | C-SRC/C-TEST | `forwarder.rs::should_project_codex_agent_messages_for_provider` 与 `agent_message_projection_runs_only_for_third_party_codex_responses`。 |
| plaintext `agent_message` 投影为 `type=message, role=user`；opaque ciphertext fail closed，不发送空 Payload。 | C-SRC/C-TEST | `src-tauri/src/proxy/providers/codex_multi_agent.rs` 四条单测。 |
| 第三方 Chat/Anthropic 路径在投影之后进入既有 transport converter。 | C-SRC/C-TEST | `forwarder.rs` 调用顺序；`projected_agent_message_reaches_chat_as_user_text`。 |
| CCSM 不把 prompt、reasoning、response 正文额外写入数据库/sidecar；日志只记录计数和 route/provider identity。 | C-SRC + 设计约束 | payload design 的 Security and Privacy；forwarder 日志不输出 message body。 |

关键历史提交：`aa64e5bf`（加密 RED）、`4c2854ac`（mixed router policy）、`21b0ee7a`/`b61bc5b1`（route materialization 保留）、`c47f6b4f`/`8990f746`（第三方 payload 投影）、`f4a89fd8`（V6 版本）。

## 8. MultiRouter Provider materialization 与跨 transport 回放

| 结论 | 等级 | 证据 |
|---|---|---|
| 路由按 model 的 exact/prefix/default 等规则解析，再通过 `targetProviderId` 物化实际 Provider。 | C-SRC/C-TEST | `src-tauri/src/proxy/providers/codex.rs::resolve_codex_model_routed_providers` 与 `materialize_codex_routed_provider_from_target`。 |
| 物化结果必须保留 route apiFormat、模型 override、认证和 request-local compatibility marker。 | C-SRC/C-TEST | `codex.rs` materialization tests；`src-tauri/src/proxy/forwarder.rs` retry/route chain。 |
| reasoning 与 web search replay 需要按目标 transport 归一化，不能把第三方合成 ID/明文 reasoning 原样送回 Official。 | C-SRC/C-TEST | `4eb154d7`、`063b45dc`、`3f351514`、`8d721273` 等提交与对应 tests。 |

## 9. V23 配置解析根修

| 结论 | 等级 | 证据 |
|---|---|---|
| Codex `model_catalog_json` 当前按绝对路径语义读取，CCSM 旧相对文件名会触发 `AbsolutePathBuf` 错误。 | O-SRC + C-TEST | OpenAI config path 类型；CCSM `model_catalog_json_field_writes_absolute_path_required_by_codex`。旧行为由提交 `7811383b` 引入，最早进入 V3.16.2 系列。 |
| `max_threads` 是 canonical 并发字段的 legacy alias；两个键同时存在会被 Serde 视为重复字段。 | O-DOC/O-SRC + C-TEST | [官方配置参考](https://learn.chatgpt.com/docs/config-file/config-reference)、`AgentsToml` alias、CCSM `catalog_projection_canonicalizes_agent_thread_aliases`。旧补键逻辑由 `2aef8a2e` 引入，最早进入 `v3.16.3-23`。 |
| V23 写绝对 catalog path，并迁移/删除 legacy alias，只保留用户 canonical 值。 | C-SRC/C-TEST/RUNTIME | `4b6f7dfb`、`786248c5`；完整 Rust `2956 passed / 0 failed / 2 ignored`；GitHub Release run `31577095852` 五平台成功。 |

## 10. 当前发布证据

- Release：[CCSwitchMulti v3.19.1-23](https://github.com/BigStrongSun/ccswitchmulti/releases/tag/v3.19.1-23)，正式非 prerelease。
- GitHub Actions：[run 31577095852](https://github.com/BigStrongSun/ccswitchmulti/actions/runs/31577095852)，Windows x64/ARM64、Linux x64/ARM64、macOS、Publish Release 与 Assemble latest.json 全部成功。
- `latest.json` 与平台资产均存在签名；Release API 资产含服务端 SHA-256 digest。

## 11. 明确不作出的结论

- 不声称 CCSM 可以解密 OpenAI ciphertext；它的兼容路径是让 mixed-router 非保留工具产生可投递明文，并在目标第三方已知后做标准消息投影。
- 不声称 description 是确定性硬路由；它是父模型可见的语义选择 guidance。
- 不声称生成 TOML 就证明真实选角；验收还需 child rollout、model/provider、工具执行和路由日志。
- 不把公开 issue 的版本性故障表述为 OpenAI 永久架构限制。
- 不把 Windows 本地测试或 GitHub macOS 构建等同于受影响用户机器的完整交互验收。

## 12. 搜索渠道说明

- Codex 内置 Web：命中并打开 OpenAI 官方 Subagents/Configuration 文档、Codex 官方源码和相关官方仓库 issue。
- Matrix WebSearch：独立搜索同一主题，但只返回泛化 OpenAI/Codex 结果，没有获得等价一手证据。
- 因此关键结论使用已打开的官方文档、官方源码、CCSM 当前源码和测试；Matrix 结果仅证明第二条搜索链已执行，不作为正证据。

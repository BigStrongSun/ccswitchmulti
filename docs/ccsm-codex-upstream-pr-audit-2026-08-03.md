# CCSwitchMulti Codex 上游 PR 审计

日期：2026-08-03

审计对象：`farion1231/cc-switch` 上 open、尚未被原仓库合并的 Codex 相关 PR，
以及上游 `v3.19.1` 发布内容。

## 已合入本地

- 上游 `v3.19.1` 已合并到本地 `main`，保留 CCSwitchMulti 自定义层。
- `farion1231/cc-switch#5878`（Responses SSE 中途断流自动重连 + 思考期保活）
  已 cherry-pick 并合入本地 `main`，`streaming_retry` 17 个测试通过。
- 本地 `BigStrongSun/ccswitchmulti#25` 的 hosted web_search 完整 SSE 生命周期
  已合入。

## 上游 v3.19.1 关键内容

- DeepSeek / Volcengine Agentplan / Tencent Hunyuan Codex 原生 Responses 预设。
- DeepSeek 官方 Codex 模型目录镜像，保留 freeform `apply_patch` 与官方 GPT-5
  harness。
- 切回官方 Codex 时清理残留第三方 `auth.json` 的 401 修复。
- GPT-5.6 terra / luna 等模型定价更新。
- Claude Desktop 用量去重、Grok Build 代理修复、无用代码删除。

## 建议优先评审的未合入 Codex PR

| PR | 标题 | 实时状态 | 备注 |
| --- | --- | --- | --- |
| #6069 | fix: extend max_completion_tokens handling to gpt-5+ models | MERGEABLE / BLOCKED | 已适配本地；补齐上游 review 指出的 Responses→Chat 路径，并额外覆盖默认输出预算路径 |
| #6062 | fix(codex): sync active rollouts when mtime is unchanged | DRAFT | 活跃会话同步 |
| #6037 | fix(codex): support Windows junction config directory | open | 修复 Codex 目录 junction 切换问题 |
| #6035 | feat(codex): aggregate multiple providers' models into Codex | open | 模型聚合功能 |
| #6009 | feat(codex): add configurable sub-agent defaults | open | 子 Agent 默认模型/推理档 |
| #5997 | fix(usage): prevent Codex session sync gaps | open | 会话同步缺口 |
| #5979 | fix(codex): avoid repeated deferred session reads | open | 历史同步性能 |
| #5854 | fix(usage): handle interleaved Codex token counters | open | token 统计交错 |
| #5830 | fix(codex): restore strict legacy thread names | open | 旧线程名兼容 |
| #5826 | fix(codex): avoid replaying synthetic reasoning items | DRAFT / MERGEABLE / BLOCKED | reasoning 回放 |
| #5895 | fix(codex): preserve assistant turns across Responses conversion | open | assistant turn 保真 |
| #5883 | fix(codex): omit reasoning for Ark coding routes | open | 火山 Ark 路由 |
| #5764 | fix(codex): trim MCP command and accept string args | open | MCP 参数兼容 |
| #5765 | fix(codex): tolerate invalid escape in tool-call arguments | open | 工具参数容错 |
| #5705 | fix(codex): skip managed node_repl MCP | open | 避免 stale MCP 注入 |
| #5681 | fix(proxy): support Codex Alpha Search and Claude hosted WebSearch | MERGEABLE / BLOCKED | 与 hosted tools 强相关 |
| #5536 | fix(codex): support remote compaction via chat providers | CONFLICTING | 需要解冲突 |
| #5535 | fix(codex): preserve Responses Lite model capabilities | CONFLICTING | 需要解冲突 |
| #5484 | fix(codex): proxy legacy Images API endpoint | MERGEABLE / BLOCKED | Image API 兼容 |
| #5406 | fix(codex/anthropic): prevent malformed function_call arguments from locking sessions | MERGEABLE / BLOCKED | 工具参数卡死 |
| #5265 | fix(codex): sync Desktop custom models and reasoning levels | MERGEABLE / BLOCKED | Desktop 模型同步 |
| #5056 | Add Codex reasoning continuation proxy | open | reasoning 续传 |
| #4908 | Add Codex image generation handling modes | CONFLICTING | 与本地 image bridge 有关 |
| #5083 | fix(proxy): normalize Codex Responses SSE events | CONFLICTING | 与流重连有关 |
| #5283 | fix(codex): sync live config when updating current provider | open | 配置同步 |
| #5386 | fix(codex): prefer freshest credential source | open | 凭据优先级 |
| #5292 | fix(codex): prevent custom API keys in official auth | open | 官方认证保护 |
| #5469 | fix(codex): use provider auth for third-party live routes | open | 第三方路由认证 |
| #5271 | [codex] normalize Codex tool schemas | open | 工具 schema |
| #4220 | Codex/sanitize chat tool history | open | 工具历史清洗 |
| #5328 | [codex] Preserve Codex image input support in generated catalog | draft | 图片能力 catalog |
| #5104 | fix(codex): strip image generation tools from responses | open | 与 image bridge 有关 |
| #3850 | fix(codex): model catalog schema compatibility with Codex CLI | open | catalog schema |
| #4431 | fix(codex): show imported custom models in Codex Desktop | open | 自定义模型显示 |
| #5788 / #5689 | multiple active Codex prompts | open / DRAFT | 多 Prompt |
| #5990 | upstream CCSwitchMulti MultiRouter integration | DRAFT / CONFLICTING | 上游合入方案；原清单中的 #5989 经 GitHub REST 核实不存在 |

## 验证说明

- `MERGEABLE / BLOCKED` 表示 GitHub 认为可合并但当前被 CI/状态卡住；这类不能
  直接照单全收，需要看具体 failing check。
- `CONFLICTING` 表示与当前 upstream main 冲突，不能直接 cherry-pick。
- 未标 mergeable 的 PR 需要逐条打开 diff 做业务验证，不能仅凭标题判断有效。
- 本审计只验证了已合入本地的 #5878 和 hosted web_search SSE 修复；其余 PR 为
  候选清单，未做端到端业务验证。
- #6069 已按 OpenAI 官方 OpenAPI schema、上游 #5215 复现错误和实时 review 交叉
  校验后适配；没有照搬其首版 diff。本地还修复了 provider 默认输出预算仍写
  `max_tokens` 的遗漏，并保留 CCSwitchMulti 的 Grok reasoning 支持。
- 表中状态于 2026-08-03 通过 GitHub REST API 逐条复核；GitHub 的 `mergeable`
  结果会随目标分支和 CI 状态变化，后续采用前仍需重新查询。

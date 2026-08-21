# 2026-08-21 main 分支与发布边界审计

## 触发原因

`v3.19.2-13` 的发布提交 `687f503b` 之后，`main` 又产生了输入能力来源链修复
`5ea19111`。此前只核对了安装版和 release commit，没有把 release commit 之后的
`main` 增量作为发布前置条件，导致源码已修复但安装版未包含。

## 当前结论

- 当前 `main`：`5ea19111`。
- `v3.19.2-13` release commit：`687f503b`。
- `5ea19111` 不在 `687f503b` 祖先范围内，因此 `3.19.2-13` 不包含输入能力来源链修复。
- Bug11 的 UUID route/alias/projection 修复在 `687f503b` 之前，已包含在
  `3.19.2-13`。

## 本轮分支审计

审计对象包括本地 `refs/heads/*`、`refs/remotes/fork/bigstrongsun/*`，并结合此前
全量 fork/upstream 分支审计记录复核。未发现需要整枝合入 `main` 的生产分支。

| 分支 | 结论 | 依据 |
|---|---|---|
| `bigstrongsun/provider-model-reasoning-ui` | 已合入 | tip 已是 `main` 祖先 |
| `bigstrongsun/codex-multirouter-ssot-v2` | 已合入 | tip 已是 `main` 祖先 |
| `bigstrongsun/integrate-pr-6653` | 已合入 | tip 已是 `main` 祖先 |
| `bigstrongsun/ultra-orchestration` | 语义已选择性合入 | `77d011c8` 已将 Ultra 集成进 `main`；分支 tip 因手工整合不保持祖先关系，不整枝回合 |
| `bigstrongsun/fix-responses-commentary-tool-calls` | patch-equivalent | 分支 `246a475f` 与 `main` 的 `5b820624` patch-id 相同 |
| `bigstrongsun/fix-unsupported-responses-tools` | 被当前树更完整实现替代 | 分支 `68c42605` 的拒绝 unsupported tool 语义已由 `main` 的 `6dc7e007` 吸收并保留 hosted-tool 处理 |
| `bigstrongsun/fix-responses-lite-additional-tools` | 被当前树替代 | 属旧 Responses-Lite bridge 线，不回放旧实现 |
| `bigstrongsun/ccsm-agent-mesh` | 独立原型，禁止合入 | 只新增未接入现有 HTTP proxy、Provider 生命周期和凭据边界的 AgentMesh backend |
| `bigstrongsun/commentary-reasoning-experiment` | intentional no-go | official continuation 真实 gate 触发 400/404，实验停止 |
| `bigstrongsun/portable-reasoning-experiment-nogo` | intentional no-go | official continuation 真实 gate 触发失败，不能把 codec/state machine 整树带入 release |
| `bigstrongsun/subagent-v2-capability-injection` | docs/research only | 分支新增主要是课件、论文和验收材料，不含待补 runtime patch |
| `release/v3.19.2-8`、旧 backup/release 线 | 历史线 | 不作为当前 release 合并来源 |
| `upstream/main` / 官方 v3.20.0 线 | 外部产品线 | 与当前 CCSwitchMulti release 线不同，不能整枝合入 |

## 发布门禁

以后构建 release 必须同时满足：

1. 目标 release commit 是当前 `main` 的祖先；
2. `main..HEAD` 没有未纳入 release 的生产提交；
3. 所有本地/自有 fork 分支的独有提交均有明确分类：已合入、patch-equivalent、
   superseded、no-go、docs-only 或独立产品线；
4. 安装版验证必须绑定实际构建提交，而不能只看应用版本号。


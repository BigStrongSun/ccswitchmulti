# 2026-08-25 Qwen 工具循环与旧路由档案继承

## 现场证据

- rollout：`C:\Users\sunda\.codex\sessions\2026\08\25\rollout-2026-08-25T10-40-49-01a032c7-1005-7242-9f9b-88c56e16d13d_01a036ca-b4ce-7980-93b1-55ae67ea0a16.jsonl`。
- 循环区间约为 10:44:54-10:48:53，两条相同命令各出现 13 次。每次均有新 call ID、成功的 function output 和新的上游请求。
- `codex-router.log` 的 request bytes 从 242269 按轮增长到 416701，说明历史和工具结果在持续进入下一请求；没有 CCSM 内部 retry/reconnect 重放证据。
- live SQLite 中目标 Provider `274cfc2c-e4eb-4572-ba6f-7fdcc0b6008c/qwen3.8` 为 `verified + open_ai_chat + readable reasoning`，但 `codex-multirouter/router-274c.../qwen3.8` 是旧 `partial`。

## 根因边界

- 已确认的 CCSM 缺陷：运行时只读取 route profile；旧 route `partial` 令 `ReasoningProjection::None`，使真实 Qwen reasoning 不以独立 Responses reasoning item 进入 Codex 历史。后续恢复只能给 tool-call assistant 消息补统一的 `tool call` 占位。
- 已排除：UI 重复渲染、同一 SSE tool call 重放、命令失败自动重试、工具结果整体缺失。
- 尚不能证明循环完全由 CCSM 单独造成；vLLM/Qwen 存在收到 tool result 后仍重复调用的公开同类问题。CCSM 必须先修复其可确认的历史降级，再做安装态 canary。

## 实现规则

- `resolve_route_or_equivalent_provider_profile` 先接受 route 精确档案。
- route 档案不可接受时，只读取 `codexResolvedTargetProviderId` 指向的 standalone Provider 档案；克隆完整 `ProbeTargetKey`，仅替换 provider/route identity，因此公开模型、上游模型、transport、endpoint、auth kind 和 credential fingerprint 必须保持完全一致。
- reasoning 自动投影只接受当前版本、未过期、`Verified`、选中 Chat 的档案。
- transport 选择维持原有准入规则，但同样允许完全等价的目标 Provider 档案，解决升级后不重新保存 Provider 的问题。
- 不完整 route identity、缺失 target Provider ID、endpoint/credential/model 不一致均 fail closed。

## 验收边界

- 源码回归覆盖旧 route partial→目标 Provider verified 的 reasoning/transport 继承，以及 endpoint 不同拒绝继承。
- Fresh 验证：Codex Provider 123/123、Responses→Chat 151/151、Chat 流转换 41/41、MultiRouter mutation 17/17；完整 `cargo test --lib --no-default-features` 为 3499 passed、0 failed、6 ignored，`cargo check --tests --no-default-features` 通过并保留 5 条既有 dead-code warning。rustfmt、diff 和 UTF-8 无 BOM 检查通过。
- 当前运行中的 `3.19.2-17` 未替换；loop 任务发生在旧运行态。发布/安装后需要确认 rollout 重新出现 reasoning item，并观察相同成功命令是否仍连续重复。

## 3.19.2-17 在线数据修复

- 用户要求不升级版本先恢复当前 Qwen 路由。2026-08-25 12:24 对 live SQLite 做在线备份，文件为 `C:\Users\sunda\.cc-switch\backups\cc-switch.db.bak-qwen-route-profile-20260825T122419`。
- 写入前确认 standalone Qwen Provider 与 route 的公开/上游模型、Chat transport、endpoint fingerprint、authentication kind 和 credential fingerprint 全部一致；仅把 Provider 的 `verified` result、版本和有效期复制到已有 route target identity，未修改 Provider、Router 配置或凭据。
- 更新使用单个 `BEGIN IMMEDIATE` 事务并做读回断言。CCSM 未停止或重启；`127.0.0.1:15721` 仍由原 PID 27916 监听，`/health` 返回 healthy，新只读连接确认 route 为 `verified/open_ai_chat`。
- 该数据修复会让现有 `3.19.2-17` 的精确 route lookup 立即启用 raw reasoning projection，但只修当前 Qwen route；源码提交 `584a1834` 仍是防止其他旧路由和未来升级重复出现问题的长期修复。

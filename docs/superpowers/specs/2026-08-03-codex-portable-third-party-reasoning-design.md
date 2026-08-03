# Codex 第三方 Reasoning 可移植桥设计

日期：2026-08-03
状态：设计已确认，等待书面评审后进入测试驱动实现

## 问题

Codex 的 `ResponseItem::Reasoning` 同时支持两种可读表示：`summary` 保存 reasoning summary，`content` 保存 raw reasoning。DeepSeek V4 Flash 原生 Responses 返回 `content[].type=reasoning_text`；Codex 会实时显示并把它原样写入 rollout。任务继续使用同一第三方 provider 时，这种行为正确。

用户从 MultiRouter 恢复内置 OpenAI provider 后，CCSM 完全退出请求链路，Codex 直接通过官方 WebSocket 访问 ChatGPT Codex backend。该 backend 在现场任务中拒绝回放的第三方 raw reasoning，报错要求 `reasoning.content` 数组最大长度为 0。CCSM 无法在官方运行期间进行请求时清理。

方案还必须满足：

- 第三方 reasoning 在 Codex 中实时显示。
- 对话和 reasoning 不复制到 CCSM 数据库或旁路文件。
- 恢复官方后不依赖 CCSM 进程、本地代理或 official-only shim。
- 重新启用 MultiRouter 后，第三方工具调用仍能获得其要求回传的 reasoning 文本。
- 不修改 OpenAI 官方响应，不伪造 OpenAI `encrypted_content`。

## 已确认的现状

- Codex app-server 将 `summary` 映射为 `item/reasoning/summaryTextDelta`，将 raw `content` 映射为 `item/reasoning/textDelta`。
- 上游 CC Switch `v3.19.1` 对原生 `openai_responses` 基本透传，因此第三方 raw reasoning 会进入 Codex 历史。
- 同一上游版本的 Chat -> Responses 转换器已经把第三方 reasoning 合成为 `summary_text`，流式路径发出 `response.reasoning_summary_text.delta`；原生 Responses 与转换路径目前行为不一致。
- DeepSeek 思考模式在工具调用场景要求后续请求回传 `reasoning_content`，不能只删除 raw reasoning。

## 目标

- 让 CCSM 管理的非官方 Codex Responses 输出从产生时就是可被官方回放的便携历史。
- 保留逐 delta 的实时 reasoning UI。
- 转换完全无状态；可逆信息只存在 Codex 自己的 rollout 中。
- 第三方请求重新经过 CCSM 时，能够从便携历史恢复目标上游需要的 raw reasoning。
- OpenAI official route、官方 WebSocket 及官方 reasoning/compaction 保持原样。

## 非目标

- 不在 CCSM 中持久化 prompt、response、raw reasoning 或对话副本。
- 不为第三方生成或伪造 OpenAI `encrypted_content`。
- 不把普通 assistant message、commentary 或工具输出当作 reasoning 载体。
- 本阶段不批量重写已有 rollout；存量污染任务的离线修复另立任务。
- 不修改 Codex 客户端或 OpenAI 官方协议。
- 不把所有 provider 一次性纳入；先以现场 DeepSeek V4 Flash 原生 Responses 链路证明协议闭环，再扩展经过同一能力判定的第三方原生 Responses provider。

## 核心决策

CCSM 对非官方原生 Responses 的 raw reasoning 建立无状态双向桥：

```text
第三方 response.reasoning_text.delta
        ↓ 响应桥
Codex response.reasoning_summary_text.delta
        ↓ Codex 持久化
reasoning.summary[].summary_text，content=[]
        ↓ 下一次第三方请求进入 CCSM
第三方请求桥恢复 reasoning_text / reasoning_content
```

文本不缩写、不落入 CCSM 存储。这里使用 `summary` 是协议兼容表示，不宣称第三方 raw reasoning 在语义上变成了真正摘要。

## Provider 边界

响应桥只在以下条件同时成立时启用：

1. 应用是 Codex。
2. effective provider 是非官方 provider。
3. effective transport 是原生 Responses，而不是 Chat/Messages/Anthropic 转换。
4. provider capability 显式启用 portable reasoning bridge；第一阶段只为已验证的 DeepSeek V4 Flash 原生 Responses 路由启用。

以下路径不得启用：

- 内置 OpenAI official、native Codex auth、managed OAuth 和官方账号池 route。
- 普通 Chat -> Responses 路径；它已经输出 summary。
- Responses -> Anthropic/Messages 等其它应用转换。
- error body、非 Responses SSE、未知二进制或无法安全解析的响应。

## 响应方向

### 流式 SSE

桥接器必须维护 Responses item 生命周期和索引，不得只替换一行事件名。对于 `type=reasoning` item：

- 将 `response.reasoning_text.delta` 转为 `response.reasoning_summary_text.delta`。
- 将 `response.reasoning_text.done` 转为 `response.reasoning_summary_text.done`。
- 必要时补齐 `response.reasoning_summary_part.added/done`，但不得重复上游已有 part 生命周期。
- `response.output_item.done.item` 中把 raw `content[].reasoning_text` 移入 `summary[].summary_text`，最终 `content` 为空数组或省略为官方实测接受的形态。
- 维持原 `output_index`、`content_index/summary_index`、事件顺序、终止状态和 usage。
- reasoning item ID 使用确定性、官方前缀合法且可识别来源的 `rs_ccswitch_...`。同一 item 在 added、delta、done、completed 中必须保持一致。
- message、function call、tool result 和其它 output item 不因本桥改变语义。

如果上游同时给出非空 genuine summary 和 raw content，第一阶段明确拒绝静默合并：记录兼容诊断并保持原样。该组合必须用独立测试和产品决策处理，避免重复显示或把两种语义混在同一 summary 数组。

### 非流式 JSON

对 `response.output[]` 中每个 `type=reasoning` item执行同一规则：仅在 summary 为空且 raw content 可完整解析时转换。无法完整解析时保持原响应并记录不含正文的诊断，不允许丢字。

## 请求方向

当目标仍是启用了该能力的第三方原生 Responses provider 时，请求桥识别 `rs_ccswitch_...` reasoning item：

- 读取 `summary[].summary_text` 的完整文本。
- 按目标 provider 接口恢复 `content[].reasoning_text`，或其明确要求的 `reasoning_content` 结构。
- 清空桥接产生的 summary，避免同一文本重复发送。
- 保持 reasoning 与后续 message/function call 的顺序。
- 不改变 `call_id`；工具调用配对继续以 `call_id` 为真值。

桥只反向处理 CCSM 明确生成的 marker ID。OpenAI 官方生成的 `rs_...`、官方 summary 和 `encrypted_content` 不得冒充第三方 raw reasoning。第三方请求中遇到官方 opaque reasoning/compaction 时沿现有 provider 兼容边界处理，本设计不扩大其语义。

## 为什么不使用其它载体

- `reasoning.content`：Codex原生且语义正确，但现场官方 backend 不接受，无法满足官方直连。
- assistant `output_text`：会污染正常对话和模型回答顺序。
- commentary：属于 agent message phase，不是 reasoning，回放语义不同。
- `encrypted_content`：只能由对应 OpenAI backend 生成和验证，CCSM不能伪造。
- CCSM旁路数据库：违反数据所有权和官方阶段零依赖约束。

因此 `summary` 是 CCSM-only 约束下唯一保留 reasoning item、实时显示和官方可回放能力的候选载体；是否最终可用必须由真实官方端点实验决定。

## 风险与失败策略

1. **语义风险**：完整 raw reasoning 被承载于名为 summary 的字段。UI和下游可能按摘要对待。产品必须明确这是便携编码。
2. **长度风险**：ChatGPT Codex私有 backend可能对 summary 有未公开的长度限制。必须用短、中、长 reasoning 实测。
3. **配对风险**：官方 backend可能要求 reasoning item 后紧随关联 message/tool item。桥接器不得改变排序。
4. **压缩风险**：官方远程压缩可能重新处理长 summary。必须把压缩成功纳入验收。
5. **反向恢复风险**：DeepSeek原生 Responses可能要求特定 item ID或结构。必须通过真实工具调用多步循环验证，不能只验证纯文本问答。
6. **中断风险**：SSE在 reasoning中途断开时不得提交一个伪 completed item；沿现有流截断错误路径结束。
7. **未来协议风险**：未知 reasoning事件保持透传并记录事件类型，不猜测转换。

如果任一官方A/B证明长summary、配对或压缩不被接受，本方案停止，不再继续堆叠兼容补丁；下一选择是修改Codex的provider-aware history projection。

## 实施分层

1. 新增独立的 native Responses portable reasoning 模块，负责纯 JSON item 转换和 SSE状态机；不把逻辑继续塞进通用 handler。
2. provider capability/route判定集中在 Codex provider模块，forwarder和handler复用同一真值。
3. 响应handler仅在能力命中时包裹转换流；其它路径仍使用现有 passthrough。
4. 请求forwarder在发给同一能力provider前执行反向转换。
5. 诊断只记录 provider、session、item数量和失败类别，不记录 reasoning正文。

## TDD与验证矩阵

实现前先添加稳定失败测试：

1. 流式 raw reasoning delta/done 被转换成 summary生命周期，并逐delta保留全文。
2. `output_item.done` 与 `response.completed.output` 中不再残留 raw content。
3. 多个 reasoning item、reasoning + function call、reasoning + final message保持索引和顺序。
4. 非流式 reasoning item转换完整且无重复。
5. `rs_ccswitch_...` summary对 DeepSeek请求可逆恢复raw reasoning。
6. OpenAI official route字节级不受影响。
7. Chat -> Responses、Anthropic和Messages转换路径不受影响。
8. 同时存在genuine summary和raw content时不静默改写。
9. SSE截断、错误body、未知事件不会生成伪成功。
10. 日志不包含reasoning文本。

自动测试通过后进行真实闭环：

```text
新建DeepSeek V4 Flash任务
→ 观察raw reasoning逐字实时显示
→ 完成至少一次工具调用
→ 退出Codex
→ 一键恢复内置OpenAI并停止CCSM
→ 同一任务选择官方模型继续，官方WebSocket返回200
→ 触发并完成官方压缩
→ 再退出Codex并启用MultiRouter
→ 同一任务切回DeepSeek
→ 多步工具调用继续且上游不报reasoning_text缺失
```

同时检查rollout：第三方阶段只持久化可识别的summary，不包含`reasoning.content`；官方阶段保持官方原生summary/encrypted状态；CCSM数据库和日志中不存在对话或reasoning正文。

## 存量历史

本设计防止新记录继续污染。已经包含第三方raw reasoning的旧任务仍会在官方直连时失败。只有新桥闭环通过后，才设计独立的离线、备份优先、可回滚存量规范化；不得把存量迁移混入第一阶段协议实验。

## 放弃的方案

- 官方阶段保留CCSM official-only shim：违反官方直连和WebSocket要求。
- CCSM保存第三方私有reasoning并请求时回填：违反对话不落入CCSM要求。
- 原地把所有历史永久迁成官方格式：破坏重新启用MultiRouter的可逆性。
- 把reasoning降级为普通assistant/commentary消息：污染对话语义。
- 伪造OpenAI encrypted reasoning：无法验证且存在安全风险。

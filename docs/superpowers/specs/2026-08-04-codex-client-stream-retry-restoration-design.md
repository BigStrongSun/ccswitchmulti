# Codex 客户端流重试恢复设计

## 背景

CCSwitchMulti `v3.16.5-22` 没有覆盖 Codex provider 的 `stream_max_retries`，因此沿用 Codex 默认的 5 次 Responses 流重试。提交 `72c8ca22` 为避免可能已在途的请求被放大，把托管 provider 的 `request_max_retries` 和 `stream_max_retries` 都设为 0。后续只恢复了请求建立阶段的重试，并在代理内增加了“尚未向客户端发出语义事件”时的安全 SSE 重连，`stream_max_retries` 仍保持 0。

这形成了一个恢复缺口：当正文、Reasoning、output item 或工具事件已经到达 Codex 后，CCSM 不再重放是正确的；但 Codex 自身也被配置为不重试，于是流中断直接终止 turn。

## 目标

- 恢复 Codex 官方默认的 5 次流级 sampling retry。
- 保留 CCSM 的语义输出前透明重连、心跳、Response Grace、请求阶段重试和官方上游 Zstd。
- 已有语义输出后的请求重建、会话历史和工具状态继续由 Codex 自己管理，CCSM 不实现第二套事件去重状态机。
- 普通直连官方、第三方协议转换和 Auto Failover 的既有边界不因本次修改而改变。

## 方案比较

### 方案 A：托管 provider 显式写 `stream_max_retries = 5`（采用）

优点是行为明确、可测试，直接对齐当前 Codex 默认值和 `v3.16.5-22` 的有效行为。CCSM 只在无语义输出时透明恢复；有语义输出后的断流交还 Codex 官方状态机。

### 方案 B：删除 `stream_max_retries` 字段

可以继承 Codex 默认值，但未来 Codex 默认值变化会静默改变 CCSM 行为，配置投影和兼容形态也更难审计，因此不采用。

### 方案 C：CCSM 在语义输出后自行重放并去重

需要在代理内复刻 Codex 的会话历史、item 生命周期、工具执行和 UI 增量状态，容易与客户端状态分叉，也无法可靠撤回已经交付的字节，因此不采用。

## 数据流

1. 建流前连接或 HTTP 错误继续由 CCSM 传输重试和 Codex `request_max_retries=2` 处理。
2. HTTP/SSE 已建立但尚未交付语义事件时，由 CCSM 的 Responses 包装器最多透明重连 5 次，并抑制重复 `response.created`。
3. 一旦交付正文、Reasoning、output item、工具或终止事件，CCSM 封死自身重放通道；如果上游随后异常断开，下游看到不完整 Responses 流。
4. Codex 以 `stream_max_retries=5` 识别缺失 `response.completed` 的可重试流错误，使用自身 session/turn 状态重新发起 sampling request。
5. 显式非重试错误、用尽预算或用户取消仍正常终止，不进行无限重试。

## 验收

- 托管 Codex 配置的 `request_max_retries` 保持 2，`stream_max_retries` 明确为 5。
- 配置投影相关测试继续覆盖普通 provider、MultiRouter 和 official takeover。
- 代理层“语义输出前重连、语义输出后不自行重放”的 20 项测试保持通过。
- `codex_config`、`services::proxy`、`streaming_retry` 相关测试、`cargo check`、rustfmt 和 `git diff --check` 通过。
- 指南和项目记忆不再声称 Codex 流重试必须为 0。


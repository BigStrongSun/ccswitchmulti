# CCSM 传输重试放大的归属修复（2026-10-20）

## 用户目标与边界

用户明确要求修 CCSM 的故障放大，不是更换节点。本批不再操作路由器、DNS、节点或订阅。保留已约定的 Codex `request_max_retries=2`、`stream_max_retries=10`；不通过篡改 HTTP 状态、伪造成功或关闭全部恢复来掩盖失败。

验收点：Codex 每次代理发送仅一次上游建连；未知执行结果不在代理重放；原生 SSE 传输恢复不再由两个 owner 重叠执行；明确容量拒绝与 Claude 转换路径保持独立策略；本地生成的 502 可与上游 HTTP 502 区分。

## 根因

- `forwarder.rs` 旧 `UPSTREAM_TRANSPORT_RETRY_LIMIT=5` 让同一个客户端 POST 在 CCSM 内最多执行 6 次 send。两个生产入口（普通转发和 raw）都进入该循环。
- TLS 建连 EOF 被分类为 `ForwardFailed`，耗尽后 `ProxyError::into_response` 生成本地 HTTP 502，并非 ChatGPT 已经返回 HTTP 502。
- CCSM 同时投影 Codex request=2 / stream=10；客户端 HTTP 层与 sampling 层各有预算。既有真实日志记录同 turn 三个本地 502、每个含 6 次 TLS 失败，随后明确 `retrying sampling request (1/10 in 210ms)`，约 51 秒至少 19 次连接。不能错误宣称 stream 层不处理最终 HTTP 502。
- 原生 Responses SSE 在无语义输出时还拥有代理重连预算 5，与客户端 stream 恢复再次叠加。初始 transport 循环还允许 native `ResponsePending` 重放，虽然上游是否已执行未知；“没拿到响应头/没下发输出”不是幂等性证明。
- `max_retries=6` 是 Provider 尝试预算，不是上述隐式 transport retry 的开关。不能靠改这一 UI 数字治理同 Provider 的隐藏循环，也不能按所有配置数字直接相乘声称现场真实发生了理论最大次数。

本地 Codex 源码 `codex-rs/codex-client/src/retry.rs` 的 `0..=max_attempts` 说明 request=2 可产生 3 次 HTTP 请求；最终错误经 `map_api_error` 与 `is_retryable` 决定是否进入 sampling 恢复。先前“HTTP 502 不会进入 stream 重试”的推断已撤销。保留客户端预算后仍可能出现客户端自身有限重试，本修复消除的是 CCSM 再乘一次的隐藏 owner，不承诺全链路只连接一次。

## 实现

- `TransportRetryOwner` 以 `AppType` 选择，不以 adapter 展示名猜测。Codex 普通/raw 两入口为 ClientManaged；每次调用执行一次发送后返回原错误。非 Codex 保留既有有限安全建连恢复。
- `ResponsePending` 不再因 native streaming opt-in 进入代理重放，保留 HTTP 424、不设置 Retry-After 的未知结果边界。
- 原生 Codex `StreamReconnector` 明确禁止传输层重连；向客户端发送合法 `response.failed` 后由客户端消耗自己的恢复预算。明确容量拒绝的恢复独立保留；容量恢复连接本身失败时不将它伪装成新的容量拒绝继续循环。Claude Responses 转换仍可由代理负责传输恢复。
- 本地 `ForwardFailed` 的 HTTP 502 保持兼容，但 JSON 标明 `code=ccsm_upstream_transport_error`、`source=ccsm`、`upstream_status=null`；真实上游 HTTP 502 原响应体保持原样。
- Codex 配置预算不变；修正注释中已经过期的“未知结果映射 429”描述，实际代码为 424。
- 容量重连必须继续满足既有容量 classifier：最多读取 64 KiB/1 秒错误体；quota/auth 429/503、读失败、超限和无效 UTF-8 不继续消耗容量预算，两种 owner 均遵守。初始 TLS 真实回环测试同时断言 send 闭包与服务端 accept 均为 1；另测下一次独立客户端调用可成功，避免把禁重放误做成永久熔断。

## 工作面与交付状态

- 主 checkout 为 `C:\Users\sunda\Documents\LLMservice\cc-switch`，本批从原处修改，不新建 worktree。开始时已有 dirty `memory.md` 两行、`protocol_compatibility.rs` 一行删除及未跟踪 HTML；不归入本批，不覆盖或清理。
- 现役安装版仍为 3.20.4-7；主 checkout 为 3.20.4-3 历史线，已有 clean 集成 worktree `.worktrees/fix-codex-pre-response-retry` 已正式发布 3.20.4-8。修复将先在主工作面集中提交，再定向集成到该现有集成线验收候选，不从过期版本覆盖现役安装。
- 测试、构建、安装分别记录；当前条目创建时尚未完成验证或安装，不把源码改动说成已生效。所有输出归属本项目 `.local-archives/20261020-transport-owner/` 或约定 release/target 生命周期目录。
- 集成线在本批测试期间由另一个任务合入四个贡献者 PR，现 HEAD `844ca809d` 且 clean；其新增行为已有 4356/9 ignored 全库结果、1702 前端测试与 Clippy 记录（`memory-2026-10-20-pr-triage.md`）。后续候选基于该集成线，不覆盖并发任务文件，不回退到旧主 checkout。

## 验证记录

- 修复源码提交：主 checkout `92259a8b5`，现有集成线摘取为 `bc8ae9274`；唯一冲突为 memory 顶部索引，保留两边全部知识项。集成候选元数据设为 `3.20.4-9`（四处版本源一致），仅作本地修复验收，不打标签或公开推送。

- 首轮 183/49/3/1 聚焦测试通过，生产 Clippy 发现测试专用 `StreamReconnector::new` dead_code；已用 `cfg(test)` 修正，不用 allow 掩盖。
- 最终聚焦回归全部通过：forwarder 183、streaming_retry 52、error 3、managed_codex_retry_budget 1，共 239 passed/0 failed；生产 `cargo clippy --lib -- -D warnings` exit 0。Rustfmt、diff whitespace、UTF-8 严格解码/no BOM/no U+FFFD 检查通过。原始日志保存在 `.local-archives/20261020-transport-owner/verify-final.log`。目标生命周期由 `Invoke-WithLocalReleaseCargoTarget` 负责，失败也回收。
- Luna 集中审查发现 ProxyManaged 容量否决仍继续的条件缺口，已统一终止并增加 quota503/错误体超时负向测试；只读审查未发现其它 owner 传递阻断。没有将真实 TLS 回环测试当成生产上游成功证明。

## 检索

内置 Web 搜索并打开 [AWS 关于重试乘法与单点重试的官方说明](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/) 和 [OpenAI Codex 配置参考](https://developers.openai.com/codex/config-reference)；Matrix 独立搜索返回 `fetch failed`，不声称外部双链一致。具体根因由版本匹配的 CCSM 源码、CCSM/客户端日志和本地 Codex 源码交叉核验，外部材料只支持设计原则。

# 2026-08-22 官方模型切到 DeepSeek V4 Flash 首个请求 400（reasoning_text）根因与修复

## 现象

- 从官方模型（gpt-5.6-*，router-codex-official）切到 DeepSeek V4 Flash
  （DeepSeek-responses 路由，api.deepseek.com/v1/responses 原生透传）后，第一条
  请求被上游 400 拒绝：The `reasoning_text` in the thinking mode must be passed
  back to the API.；用户点“继续”后重试成功（请求体比失败时大 244 字节，说明
  客户端在两次请求之间改写了历史）。
- 证据：codex-router.log 2026-08-22 02:58:05 session=01a00e49 model=deepseek-v4-flash
  responses_to_chat=false status=400；02:58:31 重试 status=200。

## 根因

- Codex 会话历史里的 reasoning item 绝大多数只有 summary + encrypted_content
  （官方 gAAAAA 密文），没有 content 字段。对 rollout-2026-08-17T13-55-19-01a00e49
  的统计：3883 个 reasoning item 中 3775 个没有可读 content（2658 个
  summary+密文、567 个仅密文、548 个仅 summary），只有 110 个带 content。
- 第三方原生 Responses 透传路径（forwarder.rs 的
  should_normalize_codex_responses_passthrough_control_messages 分支）此前只做
  function_call arguments 规整和控制消息提升，reasoning item 原样转发。
- DeepSeek 官方文档（api-docs.deepseek.com/api/create-response/）：reasoning input
  item 的 content 必须是 reasoning_text parts 列表；summary / encrypted_content
  不在其 schema 内。官方 OAuth 路径有专门的 reasoning 归一化
  （normalize_codex_oauth_reasoning_item / normalize_foreign_encrypted_reasoning_item），
  第三方路径没有对应处理，于是切到 DeepSeek 的第一条请求必然 400。

## 修复（提交 384b6159，合并入 main 为 567f94b8）

- openai_compat.rs 新增 normalize_third_party_responses_reasoning_items（及两个
  私有辅助函数），只挂在 forwarder.rs 的第三方原生 Responses 透传分支；official
  OAuth 路径完全不受影响（OAuth 依赖 summary/encrypted_content 回放 reasoning）：
  1) content 有可读文本 -> 保留 content（归一为 reasoning_text part），删除
     summary / encrypted_content / internal_chat_message_metadata_passthrough；
  2) content 不可读但 summary 有可读文本 -> 用 summary 文本重建 content；
  3) 两者都无可读文本（只剩不透明密文）-> 丢弃该 item（密文对任何第三方都不可用）。
- 归一化是幂等的，Lite 降级重试路径（normalize_codex_responses_lite_fallback_request
  复用同一请求体）天然覆盖。

## 验证

- 新增 5 个回归测试：summary-only 转 content、content 保留并剥离私有字段、
  仅密文 item 丢弃、幂等+多段 summary 拼接、official OAuth 路径保持不变。
- cargo test --lib 全量：3279 passed / 0 failed / 5 ignored。
- UTF-8 严格解码、无 BOM、无 U+FFFD、git diff --check 通过。

## 注意

- 当前安装实例尚不包含该修复，需下次构建/安装后生效。
- “官方模型速度档/推理档丢失”问题（Bug 1）是另一条线，本次只修了 DeepSeek 400
  （Bug 2）。

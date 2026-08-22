# 2026-08-22 DeepSeek V4 Flash Vision Exp 官方目录接入

## 官方能力来源

- DeepSeek 官方 Chat Completions API：
  `https://api-docs.deepseek.com/api/create-chat-completion/`。模型枚举包含
  `deepseek-v4-flash-vision-exp`；用户消息可用 text/image parts，图片支持 HTTP(S)
  和 data URL（JPEG/PNG/GIF/WebP），`detail` 支持 low/high/original/auto；
  reasoning 使用 `reasoning_effort=low|high|max`，默认 high，medium/xhigh 映射为 high，
  并可用 `thinking.type=disabled`；支持 function tools。
- DeepSeek 官方 Models & Pricing：
  `https://api-docs.deepseek.com/quick_start/pricing/`。正式 ID 为
  `deepseek-v4-flash-vision-exp`，上下文为 1M（1048576），最大输出为 384K（393216），
  并列出 JSON Output、Tool Calls、Responses API、Anthropic API 和 Chat Prefix Completion。

## CCSwitchMulti 投影与边界

- `codexProviderPresets.ts` 将该模型作为 DeepSeek 官方内置项：text/image、1M context、
  `low/high/max`（default high）与可关闭 thinking。
- DeepSeek Native Responses catalog template 保留 Flash 的原生 tool/harness 字段，并对该模型
  设置 `input_modalities=[text,image]` 与 `supports_image_detail_original=true`。
- `codex_reasoning.rs` 的精确内置回退也识别该模型，避免旧保存目录缺少 reasoning 元数据时
  退回 Unknown。
- 根因修复：`codex_catalog_model_name_is_text_only` 曾用 `starts_with("deepseekv4")`，会把未来
  DeepSeek V4 视觉型号也强行降为文本；现在只精确匹配已知文本模型 Flash/Pro。
- 当前 Codex model catalog schema 与投影路径不承载 `max_output_tokens`，因此没有把官方 384K
  虚构为未消费的字段。该事实已记录，后续 schema 增加输出上限字段时再投影。

## 验证

- `pnpm typecheck` 通过。
- 定向 Vitest：2 files / 16 tests 通过。
- `cargo test --manifest-path src-tauri/Cargo.toml deepseek --lib`：40 passed。
- 新增精确文本判定和内置 reasoning fallback 的 Rust 测试均通过；目录 JSON 可解析。

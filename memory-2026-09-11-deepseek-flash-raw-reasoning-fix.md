# 2026-09-11 DeepSeek Flash raw reasoning metadata projection fix

## 根因

- `deepseek-flash` 通过官方 `api.deepseek.com` Responses 时，上游事件和 Codex
  rollout 已经包含非空 `reasoning.content[].reasoning_text`；CCSM 原生 Responses
  流没有吞掉正文。
- MultiRouter 编译器的 `CodexModelCapabilitySummary` 只投影了窗口、模态、推理能力
  等字段，丢失了 Codex Desktop 用来区分 raw reasoning 与 summary 的
  `reasoning_summary_format` / `default_reasoning_summary`。
- 路由重新生成 `model_catalog_json` 后，DeepSeek alias 退回通用模板，Desktop 因而按
  summary-only 路径投影，表现为没有可见推理。

## 修复

- 在 `codex_multirouter/compiler.rs` 保留显式的 raw-reasoning 元数据；对精确的官方
  `api.deepseek.com` + `openai_responses` + `deepseek*` 组合，在旧 Provider 行缺字段时
  补齐官方语义 `reasoningSummaryFormat=experimental` 和
  `defaultReasoningSummary=none`。
- 在 `codex_multirouter/projection.rs` 将这两个字段写回 Router 的 `modelCatalog`。
- 在 `codex_config.rs` 生成最终 Codex catalog 时，对 DeepSeek alias 保留 snake_case 与
  Desktop camelCase 两组字段，避免通用模板再次覆盖 raw 语义。
- 没有把 raw reasoning 复制成 summary，也没有修改 SSE 事件语义；GLM 的 Chat projection
  修复仍与此独立。

## 回归验证

- `projected_models_preserve_raw_reasoning_renderer_metadata`：通过；官方 DeepSeek 旧行
  没有元数据时，路由投影得到 `experimental/none`。
- `router_catalog_marks_deepseek_alias_as_raw_reasoning`：通过；最终 catalog 同时包含
  `reasoning_summary_format/default_reasoning_summary` 与 camelCase 别名。
- `codex_multirouter::projection::tests`：16/16 通过。
- `cargo fmt --all` 与 `git diff --check`：通过。

## 验收边界

- 当前运行中的安装版仍是旧的 3.20.2-3；本次改动只在源码和测试中完成，尚未覆盖安装、
  重启 Desktop 后的真实 UI canary。安装验收需要先构建新的候选 EXE，再用现有安全的
  CCSM 替换事务验证生成 catalog 和 DeepSeek 任务的可见 raw reasoning。

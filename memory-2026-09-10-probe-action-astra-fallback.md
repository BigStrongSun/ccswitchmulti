# 2026-09-10 Provider Set action required 与 Astra fallback

## 根因

`codex_provider_set_manual_intent_required` 是保存阶段的意图门禁。manual 草稿没有 Provider 级 Responses/Chat 选择时，前端会跳过探测并直接 prepare；后端拒绝写入是正确行为。错误发生在通用 Protocol Lab 把 prepare/commit 的前置条件统一降为 `failed`，导致探测对话框把“没有执行新探测”显示成“探测中断、没有结果”。若此前确实完成过探测，同一错误分类还会掩盖已经取得的 outcome、progress 和 receipts。

Astra 已有在线 OAuth 元数据解析和 reasoning 投影支持，但 OAuth 在线失败后的 UI fallback 只读取 cache，配置 fallback 则依赖设备安装的 Codex bundled catalog。本机 `codex-cli 0.147.0` 的 bundled catalog 没有 Astra；因此 CCSM Release 自身不能保证离线目录含新官方模型。

## 实现边界

- `action_required` 与 `failed` 分离，保留探测证据；没有证据时明确说明未执行新探测。
- 不绕过 prepare/commit 的手动意图校验，不创建虚假 probe record，不自动猜测 Responses 或 Chat。
- 关闭对话框只取消本次应用；用户明确协议后可使用既有 receipts，不再次收费探测。
- packaged official catalog 只提供发行基线；本机官方 cache 覆盖它，当前 Codex bundled 最后覆盖。未来官方元数据更新不会被 CCSM 快照压回旧值。
- OAuth 离线 picker 与 `codex_official_models_cache()` 共用来源链，并继续过滤第三方模型 ID。

## Astra 当前条目

来源为 2026-09-10 检查的 OpenAI 官方 API 模型文档与 `openai/codex` main `codex-rs/models-manager/models.json`：

- slug: `gpt-6-astra`
- default reasoning: `low`
- efforts: `low`, `medium`, `high`, `xhigh`, `max`, `ultra`
- Codex catalog context: `272000`; max context: `872000`
- image input、original image detail、WebSocket preference 均为 true

API 产品页显示的 1,050,000 总上下文与 Codex picker 的 context/max 字段不是同一个投影视角；CCSM packaged 条目跟随 Codex 官方 `models.json`，不把 API 页面总窗口硬写进 Codex picker。

## 验证

- RED: `useProtocolLabWorkflow` 收到 manual intent 错误后为 `failed`；Dialog 显示探测失败；Rust 找不到 packaged catalog loader。
- GREEN: Protocol Lab/adapter/dialog/Router workspace 前端 114/114。
- GREEN: packaged catalog 2/2，官方 merge 2/2，OAuth model parser 9/9。
- GREEN: 全量前端 188 个文件、1559 项测试通过；全量 Rust library 4083 项通过、7 项忽略；`src-tauri/tests/*.rs` 独立集成目标全部通过。
- `pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer`、`cargo check --all-targets`、CI 同口径 `cargo clippy -- -D warnings`、`cargo fmt --check`、`git diff --check` 均通过。
- 相对 `v3.20.2-1` 的 19 个文本变更文件均通过 UTF-8 严格解码，且无 BOM、无 U+FFFD。

## 提交与发布准备

- 功能提交：`dcb0291f`（分支 `bigstrongsun/fix-probe-action-astra-fallback`）。
- 合入 `main`：`8ac3e419`（no-FF merge）。
- `v3.20.2-2` 版本与发布说明：`a9bf7944`；Prettier 格式收尾：`5cca3b8`。
- 当前待把发布分支 `bigstrongsun/release-v3.20.2-2` 合回 `main`，再推送 `fork`、创建并推送 annotated tag `v3.20.2-2`，以触发 GitHub Release 工作流。

发布分支已由 `70d1aa80` 合入并推送 `main`；annotated tag object `95792f18` peel 为该提交。tag 后首轮 main CI 的 Windows backend 暴露进程级 `CODEX_SQLITE_HOME` 测试覆盖会泄漏到普通并行测试，根修 `ecd56eda` 改用仅测试态的线程本地覆盖，生产读取语义和 tag 二进制均不变。修复后串行全量 Rust 4084 passed/7 ignored，等待新 main CI 与 tag Release 最终状态。

本轮源代码验证没有安装、替换或重启本机应用，也没有触碰真实 Provider、历史、SQLite、代理监听或 `127.0.0.1:15721`。

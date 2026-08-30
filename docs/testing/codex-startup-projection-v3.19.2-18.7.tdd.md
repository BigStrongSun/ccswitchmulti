# Codex 启动模型目录对账 18.7 TDD 证据

## 来源

本次没有外部 `*.plan.md`；用户旅程根据问题现场整理。

## 用户旅程

作为没有开启 Codex 本地托管或 MultiRouter 的用户，我希望打开 CCSwitchMulti 不会自动重写模型目录，以便保留我已经隐藏或排序的模型。

作为已经开启 Codex 本地托管的用户，我希望启动时仍能刷新 CCSwitchMulti 自有目录，以便升级后修复过期的模型能力字段。

## RED / GREEN 证据

| 保证 | 测试与验证 | 结果 |
|---|---|---|
| Codex 托管关闭、Live 仍残留旧 `model_catalog_json` 指针时，启动对账不写目录 | `cargo test --lib services::proxy::tests::codex_startup_skips_owned_catalog_when_takeover_disabled -- --exact --nocapture`（修复前） | RED：测试执行后在 `proxy.rs:6109` 断言失败 |
| 同一场景在修复后保持目录中的旧字段不变 | 同上（修复后） | GREEN：`1 passed; 0 failed` |
| Codex 托管启用时，幂等刷新仍修复过期目录字段 | `cargo test --lib services::proxy::tests::codex_idempotent_takeover_refreshes_stale_owned_model_catalog -- --exact --nocapture` | GREEN：`1 passed; 0 failed` |
| Codex 相关代理、恢复、切换回归不受影响 | `cargo test --lib services::proxy::tests::codex_ -- --nocapture` | GREEN：`37 passed; 0 failed` |
| 前端现有单元与集成测试保持通过 | `pnpm test:unit` | GREEN：`148 passed; 1213 passed` |
| TypeScript 类型保持正确 | `pnpm typecheck` | GREEN；首次因工作树缺少已锁定的 `pinyin-pro` 依赖而失败，执行 `pnpm install --frozen-lockfile` 后通过 |

## 修复保证

`reconcile_codex_owned_projection_on_startup` 现在先读取 `proxy_config.codex.enabled`。只有该值为 `true` 时，旧目录指针才会进入自有目录对账；关闭托管时直接返回 `Ok(false)`，不会调用 `takeover_live_config_best_effort`，也不会重写生成目录。

版本元数据已经统一为 `3.19.2-18.7`，并新增 18.7 中文发布说明。

## 覆盖率与已知缺口

仓库没有配置 Rust 覆盖率脚本，环境也没有安装 `cargo-llvm-cov`，因此本次没有虚报 80% 覆盖率。已执行受影响的 37 个 Rust Codex 测试和完整前端 Vitest 套件；真实桌面启动/E2E 未在本次环境中执行。

## 合并证据

本次没有创建 checkpoint commit，因为用户尚未授权提交或推送；当前改动保留在 18.7 修复分支工作树中，方便世豪审阅后再决定是否提交。

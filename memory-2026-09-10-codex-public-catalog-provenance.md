# 2026-09-10 Codex 无 OAuth 官方目录与 cache 来源根修

## 根因

`v3.20.2-2` 的 OAuth fallback 把 `codex_official_models_cache()` 返回值再按模型名前缀过滤。过滤的直接原因不是官方目录需要限制命名，而是 `models_cache.json` 被 CCSM 接管后已经混入 MultiRouter 第三方模型；当 official backup 缺失时，旧实现又会把这个 CCSM-owned 混合 cache 当成官方来源。

旧同步路径还有一个升级污染：从 owned cache 重建 backup 时保留 `etag=cc-switch-model-catalog`，因此部分设备可能已有 `models_cache.cc-switch-backup.json`，但它仍是 CCSM-owned 混合目录。只删除名称过滤会把 Qwen/DeepSeek 灌进 official route，只保留过滤又会误删未来不符合 `gpt-*` 等旧前缀的官方模型。

没有在 CCSM 配置 ChatGPT OAuth 时，旧实现也没有独立动态来源，只能依赖随包 JSON、本机历史 cache 或当前 Codex CLI bundled，因此未来模型仍可能要求 CCSM 发版或用户升级 Codex。

## 根修

- 官方身份改为来源判定，不再按模型名称猜测。当前 cache 或 backup 只要带 CCSM ownership etag，就不能进入官方来源链。
- 历史 poisoned backup 在同步时用可信官方来源重建并清除 owned etag；退出接管时如果仍遇到 poisoned backup，删除 owned current/backup，让 Codex 自己重建，绝不恢复第三方混合目录。
- 新增无需 CCSM OAuth 的 OpenAI/Codex 公共 catalog 刷新，固定读取 `openai/codex` 的 `codex-rs/models-manager/models.json`。10 秒超时、8 MiB 上限、HTTP/JSON/非空校验，失败继续使用本地来源。
- 公共 catalog 写入 CCSM 独立的 `codex-official-models-cache.json`，不复用 Codex `models_cache.json`。写入前移除 `model_messages`、`base_instructions` 等指令字段，远程 main 只提供模型发现和能力元数据。
- 合并顺序：CCSM packaged baseline < 公共官方目录缓存 < 未接管的官方 cache/backup < 当前 Codex CLI bundled。同 ID 以本机运行时来源为准，公共目录仍可增加未来模型 ID。
- 前端统一称为“备用官方目录”，不再把公共在线刷新错误描述成“本地缓存”。

## TDD 与验证

- RED 精确复现：owned current 泄漏 Qwen/DeepSeek；未来 `aurora-code` 被前缀过滤删除；public 参数不参与合并；公共 HTTP fetch 未实现；owned backup 被读取、同步不重建、退出时被恢复。
- GREEN：Codex OAuth models 10/10，official source merge 4/4，poisoned backup rebuild 1/1，移除 model catalog 3/3。
- 前端向导与工作台 117/117；`pnpm typecheck`、`pnpm format:check`、`pnpm build:renderer` 通过。
- `cargo fmt --check`、`cargo check --all-targets --no-default-features`、`cargo clippy --lib --no-default-features` 通过。
- `cargo clippy --all-targets --no-default-features -- -D warnings` 被仓库既有、与本次无关的约 30 条 lint 阻断，包括 `profile_roundtrip.rs` await-holding-lock 和多个旧测试 lint；本次改动文件没有出现在诊断中。
- 真实公共 catalog 检查返回 9 个模型并包含 `gpt-6-astra`。Codex 内置 Web 与 Matrix WebSearch 独立核对 OpenAI 官方页面/仓库；Matrix 搜索无结果，直接读取官方 raw catalog 成功。

本轮只修改源码、测试和项目 memory；未安装、替换或重启 CCSM/Codex，未操作真实 Provider、SQLite、代理监听或 `127.0.0.1:15721`。

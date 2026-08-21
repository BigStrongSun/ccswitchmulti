# 2026-08-22 官方 GPT 模型服务档/推理档再次丢失的根因与动态来源修复

## 现象

- Codex 官方 GPT-5.6 Sol/Luna/Terra 在 MultiRouter 模型目录中丢失
  `service_tiers=priority` 与 `additional_speed_tiers=["fast"]`。
- 推理档退回通用 `low/medium/high/xhigh`，默认被写成 `medium`；官方目录中
  Sol 默认是 `low`，且 Sol/Terra 还有 `max` 与 Codex 产品层 `ultra`。

## 根因

- CCSM 接管后 `models_cache.json` 是 CCSM-owned；官方元数据本应来自
  `models_cache.cc-switch-backup.json`，但本机 backup 为 `{"models":[]}`。
- `codex_official_models_cache()` 只在 backup 为空时继续用已接管 cache，因此
  official 来源为空，reasoning resolver 落到 Unknown，provider inline models
  又回退到 `CODEX_REASONING_EFFORTS` + `CODEX_DEFAULT_REASONING_EFFORT`。
- 以前只修了“同 slug merge 时保留官方字段”，没有修复官方来源本身为空的问题，
  所以同一问题会再次出现。

## 正确来源与修复

- 本机 Codex CLI 的 `codex debug models --bundled` 就是随安装提供的官方目录；
  实测 0.149.0-alpha.4 输出包含 GPT-5.6 的默认推理、完整档位、服务档。
- `codex_official_models_cache()` 现在先取 cache/backup，再把 bundled 官方目录
  按 slug 覆盖到结果上；空 backup 或旧 backup 都不会让新模型丢失。
- `enrich_codex_catalog_with_official_metadata` 和
  `sync_codex_models_cache_with_cc_switch_catalog` 统一走该来源。
- 不维护 GPT 模型名单；OpenAI 发布新模型后只要本机 Codex 官方目录里有它，
  速度和推理 metadata 就会自动进入投影。

## 验证

- `cargo test codex_config --lib`：210 passed。
- `cargo test codex_reasoning --lib`：23 passed。
- 新增回归：空 backup 使用 bundled 官方目录；旧 backup 被 bundled 同 slug
  最新字段覆盖；第三方别名不继承 ChatGPT 语义（merge 测试既有）。

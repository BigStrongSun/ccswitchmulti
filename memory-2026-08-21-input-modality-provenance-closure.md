# 2026-08-21 输入能力来源链收口与分支复核

## 发现

- `main` 已有输入能力判定链和后端 `declarations` 字段，但前端只显示最终值与来源，没有把每一段声明和“是否采用”呈现给用户。
- `resolve_input_modality_provenance` 的旧实现存在来源/结果错配：当 profile 显式值恰好与 catalog 相同、而 route 声明冲突时，来源会被标成 route，但最终值仍来自 profile。
- 旧冲突检测只比较 route/catalog/名字注册表，漏掉 profile 自身。
- `codex_catalog_model_specs` 中由 route 或模型名注册表推导出的 `text_only` 不能冒充 catalog 显式声明；否则已确认纯文本模型会错误显示为 catalog 来源。

## 修复

- 固定唯一优先级：`profile_explicit > route > catalog > name_registry > unknown`。
- 冲突诊断纳入 profile，并保留每个来源的原始声明和 adopted 状态。
- catalog 来源只接受 catalog 自身的显式 `inputModalities`、`supportsImage`、`textOnly` 等字段，不再把 route/name 推导值归属给 catalog。
- Sub-Agent 后端状态卡现在逐段显示判定链、声明值和最终采用的来源。

## 验证

- Rust `cargo test --manifest-path src-tauri/Cargo.toml codex_config --lib`：208/208。
- 输入能力专项 Rust 测试：6/6。
- `CodexSubagentV2ProfileEditor.test.tsx`：126/126。
- `pnpm run typecheck`、Prettier、`git diff --check` 通过。
- 新增回归覆盖 profile 与 catalog 同值但 route 冲突的场景，以及 profile/route/catalog 三方冲突展示。

## 分支边界

- `bigstrongsun/provider-model-reasoning-ui`、`bigstrongsun/codex-multirouter-ssot-v2` 已是 `main` 祖先。
- Ultra、Responses Lite、commentary/tool-call、unsupported-tools、portable reasoning 等分支的生产语义已在 main 等价或更完整实现，不整枝回合。
- 当前仍有独立生产代码的是 `bigstrongsun/ccsm-agent-mesh`；它仍是未接入 CCSM HTTP 代理、Provider 生命周期和凭据边界的 AgentMesh 原型，不属于本次修复。
- `upstream/main` 的 `v3.20.0` 线路不是当前 `main` 祖先，不能作为本线 release 或合并依据。

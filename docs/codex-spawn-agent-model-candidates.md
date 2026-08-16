# Codex spawn_agent model candidates

Tracking issue: https://github.com/BigStrongSun/ccswitchmulti/issues/1

## Root cause

Codex exposes subagent models through two independent surfaces:

- the model list shown in the `spawn_agent` tool description
- custom agent role files under `~/.codex/agents/*.toml`

The `spawn_agent` model override description is still limited to five picker-visible models.

The relevant upstream implementation is in `codex-rs/core/src/tools/handlers/multi_agents_spec.rs`:

- `MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION: usize = 5`
- `spawn_agent_models_description()` filters `show_in_picker` and then calls `.take(5)`

This was originally a prompt-description limit, not the runtime model override limit. Older Codex builds accepted a visible `model` parameter on the `spawn_agent` tool and validated it against the full available model list.

Newer GPT/Codex builds treat the multi-agent tool as a reserved function tool whose schema must remain compatible with the configured backend schema. CCSwitchMulti keeps `hide_spawn_agent_metadata = true`; in the currently validated Codex `0.147.0-alpha.6.5`, this hides `service_tier` while preserving `agent_type`, `model`, and `reasoning_effort`. Default automatic selection uses `agent_type` and custom roles rather than relying on direct model overrides.

CCSwitchMulti cannot raise this visible count above five through catalog/config fields alone. Codex Desktop/app-server can list more models through other paths such as hidden-model APIs, but the `spawn_agent` tool description is generated in Codex core and applies the fixed `.take(5)` limit before the tool schema reaches the model.

Recent Codex builds also read custom agent roles from `~/.codex/agents/*.toml`. CCSwitchMulti writes managed role files from the complete current routable catalog. This role registry is intentionally independent of the five-model direct override description window.

## Why DeepSeek disappeared

CCSwitchMulti writes the full Codex model catalog, including OpenAI, Qwen, DeepSeek, and Spark models. Codex then builds the `spawn_agent` tool description from the first five picker-visible entries. If both DeepSeek entries are after those five entries, the model exists in the full catalog but is not shown in the tool description, so the main agent is unlikely to discover the slug automatically.

Before this fix, CCSwitchMulti incorrectly generated managed roles from that same first-five slice. A model such as `deepseek-v4-pro` in position six therefore remained routable but had no `deepseek-pro.toml`, so the parent Codex could not discover the Pro role for automatic selection.

## CCSwitchMulti fix

CCSwitchMulti now supports a private catalog field:

```json
{
  "modelCatalog": {
    "models": [
      { "model": "gpt-5.5", "displayName": "GPT-5.5" },
      { "model": "qwen3.6", "displayName": "Qwen3.6 Local" },
      { "model": "deepseek-v4-flash", "displayName": "DeepSeek V4 Flash" }
    ],
    "spawnAgentModels": ["gpt-5.5", "qwen3.6", "deepseek-v4-flash"]
  }
}
```

The field remains available under `高级：子 Agent 模型覆盖`. Users can select up to five catalog models and adjust their order. Those models are promoted to the front of the generated Codex catalog, so they enter Codex upstream's five-model direct `spawn_agent.model` description window. This is an advanced compatibility control, not the normal Pro/Flash selection workflow.

CCSwitchMulti-managed custom agent files are synchronized from the complete current routable catalog. A role is pruned only when its model leaves that catalog, not merely when it falls outside the direct override top five. User-authored role files without the marker are preserved; if a user owns the base role name, CCSwitchMulti writes `ccswitch-<role>` instead.

DeepSeek roles have deliberately distinct contracts:

- `deepseek-flash` pins `deepseek-v4-flash`, `codex_model_router_v2`, and medium reasoning for long-context reading, read-heavy exploration, architecture tracing, evidence collection, and lightweight verification.
- `deepseek-pro` pins `deepseek-v4-pro`, `codex_model_router_v2`, and high reasoning for complex debugging, cross-module reasoning, architecture decisions, high-risk review, and complex implementation.

The main MultiRouter wizard no longer asks users to select subagent models. Once Codex decides to delegate, the role descriptions guide automatic Flash/Pro selection through `agent_type`.

CCSwitchMulti also writes `[features.multi_agent_v2] hide_spawn_agent_metadata = true` during Codex config projection. This keeps the reserved `collaboration.spawn_agent` schema compatible with newer GPT models while preserving model routing through the generated role files.

If `spawnAgentModels` is absent, CCSwitchMulti keeps the fallback heuristic that promotes representative Qwen and DeepSeek models into the first five.

## Invariants

- The full catalog is preserved; non-selected models are not removed.
- Unknown selected model ids are ignored.
- The setting changes generated catalog order for direct model overrides; it does not add or remove managed roles.
- User-authored custom agent role files are not overwritten or removed.
- It does not change default model selection, route matching, upstream auth, OAuth preservation, speed tiers, history provider buckets, or request statistics attribution.

## Verification

Relevant tests:

- `cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_uses_user_spawn_agent_model_priority --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_prioritizes_cross_provider_models_for_spawn_agent_description --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml managed_agent_files_prune_stale_cc_switch_roles --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml managed_agent_files_include_deepseek_roles_beyond_direct_override_window --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml removing_model_catalog_prunes_managed_agents --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml spawn_agent_priority_diagnostics --lib`

Relevant frontend checks:

- `pnpm run typecheck`
- `pnpm run build:renderer`

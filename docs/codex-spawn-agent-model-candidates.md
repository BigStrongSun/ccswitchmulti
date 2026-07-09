# Codex spawn_agent model candidates

Tracking issue: https://github.com/BigStrongSun/ccswitchmulti/issues/1

## Root cause

Codex exposes subagent models through two related surfaces:

- the model list shown in the `spawn_agent` tool description
- custom agent role files under `~/.codex/agents/*.toml`

The `spawn_agent` model override description is still limited to five picker-visible models.

The relevant upstream implementation is in `codex-rs/core/src/tools/handlers/multi_agents_spec.rs`:

- `MAX_MODEL_OVERRIDES_IN_SPAWN_AGENT_DESCRIPTION: usize = 5`
- `spawn_agent_models_description()` filters `show_in_picker` and then calls `.take(5)`

This is a prompt-description limit, not the runtime model override limit. The `model` parameter schema is a free string, and runtime validation in `multi_agents_common.rs` checks the full available model list. A model such as `deepseek-v4-flash` can work when passed explicitly, even if it is not listed in the first five visible suggestions.

CCSwitchMulti cannot raise this visible count above five through catalog/config fields alone. Codex Desktop/app-server can list more models through other paths such as hidden-model APIs, but the `spawn_agent` tool description is generated in Codex core and applies the fixed `.take(5)` limit before the tool schema reaches the model.

Recent Codex builds also read custom agent roles from `~/.codex/agents/*.toml`. CCSwitchMulti writes managed role files for the same first-five window so users can pick low-cost Qwen, DeepSeek, Spark, or local workers from the agent role surface as well.

## Why DeepSeek disappeared

CCSwitchMulti writes the full Codex model catalog, including OpenAI, Qwen, DeepSeek, and Spark models. Codex then builds the `spawn_agent` tool description from the first five picker-visible entries. If both DeepSeek entries are after those five entries, the model exists in the full catalog but is not shown in the tool description, so the main agent is unlikely to discover the slug automatically.

This explains why the same config can work when explicitly prompted with `deepseek-v4-flash`, while DeepSeek is missing from the visible candidate text.

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

The field is edited from the Codex provider model mapping UI. Users can select up to five catalog models and adjust their order. Those models are promoted to the front of the generated Codex catalog, so they enter Codex upstream's five-model `spawn_agent` description window.

The same first-five list is used to synchronize CCSwitchMulti-managed custom agent files. Files with the managed marker are rewritten for current candidates and pruned when a model leaves the first-five window, so old generated roles do not keep appearing in Codex's agent list. User-authored role files without the marker are preserved.

If `spawnAgentModels` is absent, CCSwitchMulti keeps the fallback heuristic that promotes representative Qwen and DeepSeek models into the first five.

## Invariants

- The full catalog is preserved; non-selected models are not removed.
- Unknown selected model ids are ignored.
- The setting changes generated catalog order and CCSwitchMulti-managed custom agent roles.
- User-authored custom agent role files are not overwritten or removed.
- It does not change default model selection, route matching, upstream auth, OAuth preservation, speed tiers, history provider buckets, or request statistics attribution.

## Verification

Relevant tests:

- `cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_uses_user_spawn_agent_model_priority --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml codex_model_catalog_prioritizes_cross_provider_models_for_spawn_agent_description --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml managed_agent_files_prune_stale_cc_switch_roles --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml removing_model_catalog_prunes_managed_agents --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml codex_config::tests --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml spawn_agent_priority_diagnostics --lib`

Relevant frontend checks:

- `pnpm run typecheck`
- `pnpm run build:renderer`

# Task 9 — V2 selection remediation report

## Scope and root cause

`selectionPolicy=balanced` previously rendered a preferred profile as having no provider bias.  That wording did not distinguish the specialized profile from Codex's built-in generic `default`, `worker`, and `explorer` roles, so it could not resolve their semantic competition.  The two known Flash/Pro defaults were also still initialized as `eligible` in both the Rust explicit initializer and the TypeScript new-plan defaults.

No persisted profile migration was added: only new plans and the explicit legacy initialization action receive the new default.  Existing manually persisted `eligible` or `fallback` values remain untouched.

## Research and cross-check

- Codex built-in web search: official Vitest documentation confirms focused file-path execution for the frontend regression command.
- Matrix WebSearch: the session did not expose a lazy `tool_search` connector, so the mandated `C:\Users\sunda\Documents\本地设备\scripts\matrix-websearch-mcp.js` stdio bridge was initialized directly, its `search/open/find` tool list was verified, and an independent Vitest search returned the official Vitest site.
- Local source and the task brief jointly identified the decisive state: only the balanced+preferred copy branch and the two new/explicit-init default fixtures required change.  No V1, Qwen, reserved schema, parent model, proxy forwarding, or field override path was changed.

## RED

Added independent literal Rust expectations for a balanced+preferred role that explicitly beats built-in generic `default`, `worker`, and `explorer` roles on a declared-strength match, while provider identity does not break other ties.  The existing exact explicit-initializer and frontend atomic-payload tests now require both Flash and Pro to be `preferred`.

Commands and expected failures:

```text
cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2_balanced_preferred_profile_outranks_builtin_generic_roles -- --nocapture
# failed: generated description/instructions still said "this preferred third-party profile has no provider bias"

cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2_explicit_init_has_exact_flash_and_pro_presets -- --nocapture
# failed: Flash and Pro initializer values were Preference::Eligible rather than Preference::Preferred

pnpm exec vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx --exclude ".worktrees/**"
# failed: 88 passed, 1 failed; legacy initialization payload still contained preference: "eligible" for Flash and Pro
```

RED commit: `fd886d39689ec8ae2485f5e408ecfaded14a4313`.

## GREEN

- Changed the balanced+preferred automatic selection sentence in `codex_subagent_profiles.rs` to prefer a matching specialized role over the built-in generic `default`, `worker`, and `explorer` roles, without provider identity breaking remaining ties.  Automatic descriptions remain four English sentences and instructions retain their existing role boundaries.
- Changed Rust explicit legacy initialization and TypeScript new-plan defaults for both known Flash/Pro presets to `preferred`.
- Updated only the existing exact frontend new-plan fixtures that assert the changed default.  Existing fixture profiles that intentionally exercise `eligible` and `fallback` were retained.

Passing verification:

```text
cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2_balanced_preferred_profile_outranks_builtin_generic_roles -- --nocapture
# 1 passed, 0 failed

cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2_explicit_init_has_exact_flash_and_pro_presets -- --nocapture
# 1 passed, 0 failed

pnpm exec vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx --exclude ".worktrees/**"
# 89 passed, 0 failed

pnpm typecheck
# exit 0

cargo fmt --manifest-path src-tauri/Cargo.toml --check
# exit 0

git diff --check
# exit 0
```

GREEN commit: this implementation-and-report commit.

## Self-review and concerns

The change is limited to the new-plan/explicit-init defaults and balanced+preferred generated copy.  `eligible`, `fallback`, `official_first`, `third_party_first`, field-level overrides, V1, Qwen, reserved spawn schema, parent-model selection, and proxy forwarding retain their prior branches.  Version remains `3.19.1-19`; no build, installation, push, or release operation was run.

Concerns: focused Rust runs retain pre-existing dead-code warnings (`initialize_legacy_subagent_v2` and `openai_cache_read_tokens`), and Vitest emits the existing stale `baseline-browser-mapping` advisory.  Neither warning is introduced by this change.  The Matrix lazy-tool discovery API was unavailable in this session, but the configured Matrix stdio bridge itself initialized and returned independent search results.

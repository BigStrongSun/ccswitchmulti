# Codex Sub-Agent V2 Capability Injection Design

Date: 2026-08-10
Target version: `3.19.1-19`

## Goal and boundary

CCSwitchMulti (CCSM) adds a questionnaire-driven, per-model V2 custom-agent profile editor. It turns a user's stated model capabilities into official Codex custom-agent fields, while retaining V1 direct overrides and every existing Codex transport/schema contract.

This is configuration generation, not a new Codex orchestration implementation. CCSM never changes the parent model, `spawn_agent` schema, or any spawn/follow-up calls. The built-in `default`, `worker`, and `explorer` agents remain untouched. CCSM writes only official custom-agent fields and uses the fixed provider `codex_model_router_v2` for generated CCSM roles.

## Persisted contract

`settingsConfig.codexRouting.subagentV2` is the sole persisted V2 profile source:

```ts
type SubagentV2SelectionPolicy =
  | "balanced"
  | "official_first"
  | "third_party_first";
type QuestionnaireOptimization = "speed" | "balanced" | "quality";
type QuestionnaireWriteScope =
  | "read_only"
  | "bounded_changes"
  | "complex_changes";
type QuestionnairePreference = "preferred" | "eligible" | "fallback";
type QuestionnaireReasoningEffort =
  | "auto"
  | "low"
  | "medium"
  | "high"
  | "xhigh";

type CodexSubagentV2 = {
  schemaVersion: 1;
  selectionPolicy: SubagentV2SelectionPolicy;
  profiles: Record<NormalizedProfileKey, CodexSubagentV2Profile>;
};

type CodexSubagentQuestionnaire = {
  taskStrengths: CodexSubagentTaskStrength[]; // 1-5 unique enum members
  optimization: QuestionnaireOptimization;
  writeScope: QuestionnaireWriteScope;
  preference: QuestionnairePreference;
  reasoningEffort: QuestionnaireReasoningEffort;
};
```

The persisted `profiles` map key is uniquely defined as `trim(model)`, followed by Unicode NFKC normalization and Unicode Default Case Folding. Backend code must own this exact fixed algorithm (for example via `unicode-normalization` or an equivalent implementation), and tests must fix its expected output. `profile.model` separately preserves the original visible/routable spelling and is the role `model` value. This is not ASCII conversion.

If two stored keys normalize to the same key, the related configuration is a validation error: all conflicting profiles produce no role, the UI shows the collision and requires a user merge, and the backend must never silently choose a last write. Compatibility/case examples: `"Ｆｏｏ"` and `"foo"` normalize to the same key; `"Straße"` and `"STRASSE"` collide through default case folding. A catalog alias change updates technical catalog data only; it never rewrites the profile map key or `profile.model`. A profile that cannot be matched remains stored and unroutable.

A profile contains `model`, `enabled`, `questionnaire`, and optional `overrides`. Its `taskStrengths: CodexSubagentTaskStrength[]` is a unique list selecting one through five of the following values:

`long_context_reading`, `repository_exploration`, `evidence_collection`, `summarization`, `complex_debugging`, `architecture_design`, `bounded_implementation`, `complex_implementation`, `testing`, and `high_risk_review`.

It also has `optimization`, `writeScope`, `preference`, and `reasoningEffort`, each restricted to the unions above. Optional overrides are `roleName`, `description`, `developerInstructions`, `nicknameCandidates`, and `modelReasoningEffort`; override effort excludes `auto` and is only `low | medium | high | xhigh`.

Questionnaire and explicit overrides persist unchanged. All auto-generated fields are derived at preview/materialization time and are never persisted as substitute input. Restoring one field deletes only that field's override; it must not reset the profile or unrelated overrides.

## Compilation and routing rules

The backend is the sole compiler. Frontend code renders editable inputs and backend previews, but must not duplicate role-selection, description, nickname, effort, provider, or TOML compilation rules.

The backend preview command returns: `providerKind`, requested/effective role names, description, developerInstructions, nicknameCandidates, model, fixed provider `codex_model_router_v2`, optional effort, context window, TOML preview, and warnings. Status surfaces provider kind, routability, auto versus override, enabled state, requested/effective role name and path, plus a non-generation reason.

Provider kind reuses the existing official ChatGPT backend/provider classification; it is never inferred from a model name. Catalog refresh preserves all profiles. A profile that no longer routes remains stored, but does not produce a role.

Generated descriptions are two to four English sentences. They say what task types match and what task types are excluded. A manual description replaces the generated selection text completely; for that role only, global policy no longer affects its selection text.

Automatic effort is deterministic. Use `high` if `taskStrengths` contains any of `complex_debugging`, `architecture_design`, `complex_implementation`, or `high_risk_review`. Otherwise use `low` only when `optimization="speed"` and every selected strength is one of `long_context_reading`, `repository_exploration`, `evidence_collection`, or `summarization`; use `medium` in every other case. An explicit `modelReasoningEffort` replaces this result. Examples: `[architecture_design]` produces `high`; speed with `[repository_exploration, summarization]` produces `low`; speed with `[repository_exploration, testing]` produces `medium`.

Selection policy is applied after a matching task profile is identified:

- `preferred` model matches override provider bias; `fallback` is never promoted.
- `official_first` keeps final integration, release decisions, ambiguous writes, and high-risk writes official unless an explicitly preferred profile matches.
- `third_party_first` promotes matching preferred or eligible third-party roles.
- `balanced` adds no provider bias.

These fields guide Codex's semantic role selection on a best-effort basis. Even `preferred` changes selection guidance rather than creating deterministic hard routing; CCSM does not intercept or rewrite the parent's spawn decision.

## Compatibility and lifecycle

V1 stays the first five direct model overrides. V2 profile materialization happens only while V2 is active; toggling modes preserves inactive configuration. Legacy configurations missing `subagentV2` retain legacy managed-role behavior until the user performs one-click initialization.

Initialization sets global `selectionPolicy` to `balanced`. It creates profiles only for models that are both present in the current catalog and actually routable through the candidate provider. When the corresponding model is routable, initialization enables these two presets:

- DeepSeek Flash: `optimization="speed"`, `writeScope="read_only"`, `preference="preferred"`, `reasoningEffort="medium"`, and `taskStrengths=[long_context_reading, repository_exploration, evidence_collection, summarization, testing]`.
- DeepSeek Pro: `optimization="quality"`, `writeScope="complex_changes"`, `preference="preferred"`, `reasoningEffort="high"`, and `taskStrengths=[complex_debugging, architecture_design, complex_implementation, high_risk_review, testing]`.

Every other actually routable catalog model begins as a disabled draft until configured and enabled; unavailable Flash/Pro models are not seeded as phantom profiles. Existing user-authored role files are never overwritten. Role-name overrides normalize exactly as follows: trim; ASCII-lowercase; map every maximal run of invalid characters to `-`; repeatedly replace `--` with `-`, `__` with `_`, and either `-_` or `_-` with `-` until stable; then trim leading/trailing `-` or `_`. Only lowercase ASCII letters, digits, dashes, and underscores remain, and an empty result is an error. `default`, `worker`, and `explorer` are forbidden after normalization. Conflicts dedupe case-insensitively in this exact order: requested base, `ccswitch-<base>`, then `ccswitch-<base>-2`, `ccswitch-<base>-3`, and so on until unused. Examples: `"  Deep Seek  "` becomes `deep-seek`; `"深度模型"` and `"!!!"` are rejected as empty; `"Pro!!!Review"` becomes `pro-review`; `"A__B"` becomes `a_b`; `"Foo__-- Bar"` becomes `foo-bar`; and repeated conflicts for `review` resolve as `review`, `ccswitch-review`, then `ccswitch-review-2`. Nicknames contain one to three nonempty, unique values using only ASCII alphanumerics, spaces, dashes, or underscores.

CCSM preserves `hide_spawn_agent_metadata=true`, mixed routing `tool_namespace="agents"`, the reserved schema, the current V2 body projection, and Qwen behavior. Diagnostics must exclude credentials, task text, and encrypted content.

## UX

The UI has four areas: selection policy, questionnaire, final fields, and TOML preview. The wizard and MultiRouter workspace use one editor and one config source. Final fields distinguish derived values from overrides and support field-level restoration.

## Evidence basis

The official [Subagents documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) says custom role descriptions guide selection, role model/effort can override spawn/default/parent resolution, and local Codex delegation is triggered by a direct user request or applicable `AGENTS.md`/skill instructions. The official [config reference](https://learn.chatgpt.com/docs/config-file/config-reference) confirms supported config keys. Local confirmation used `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/agent/role.rs` and `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/tools/spec_plan.rs`.

Matrix WebSearch independently ran on 2026-08-10. Its search results did not include equivalent official first-party hits, while direct Matrix fetches of both official pages succeeded. Primary conclusions therefore use the official documentation, local official source, and local runtime evidence.

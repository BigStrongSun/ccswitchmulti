# Codex Sub-Agent Reasoning Coordination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make CCSwitchMulti resolve one authoritative reasoning capability per routed model, let each Sub-Agent role delegate effort selection to native Codex or explicitly select model default/fixed/disabled behavior, and prove the saved policy reaches Codex role files and provider-native requests.

**Architecture:** Keep provider declarations in `CodexModelReasoningCapability`, but derive a normalized `ResolvedSubagentReasoningCapability` in Rust before catalog generation, profile compilation, preview, validation, or request conversion. Persist an explicit profile runtime policy instead of overloading the questionnaire's old `auto` value; delegated roles omit `model_reasoning_effort`, while model-default, fixed, and disabled roles write a validated value. The React editor consumes only the backend resolution result and the existing mutation response remains the save/read-back authority.

**Tech Stack:** Rust/Tauri, Serde, TOML, TypeScript, React 18, Vitest/Testing Library, Cargo tests.

**Approved Design:** `docs/superpowers/specs/2026-08-14-codex-main-subagent-reasoning-coordination-design.md`

## Global Constraints

- `自动获取` resolves capability evidence only; it must never choose effort from task strengths or optimization answers.
- Native Codex precedence remains: explicit role TOML > `spawn_agent.reasoning_effort` > `[agents].default_subagent_reasoning_effort` > target-model default on model override > parent turn effort > parent-model default.
- There is no universal Sub-Agent default of `high`; defaults and valid efforts come from the resolved target-model catalog.
- Unknown providers may expose the full candidate vocabulary only in the manual declaration editor; unconfirmed values must not enter the runtime model catalog.
- Main effort choices are `low`, `medium`, `high`, `xhigh`, `max`, and `ultra`; `none` and `minimal` are capability-gated special choices.
- DeepSeek V4 provider-native effort values are `low`, `high`, and `max`; confirmed mappings are `low -> low`, `medium -> high`, `high -> high`, `xhigh -> high`, `max -> max`; no `ultra` mapping is inferred.
- Reasoning disable is protocol-specific: OpenAI Chat uses `thinking.type=disabled`, Responses uses `reasoning.effort=none`, and boolean providers use their declared boolean parameter.
- A role selecting a model that does not advertise the resolved effort must fail preview/save; no silent clamping is allowed.
- Preserve user-owned role files and unrelated dirty changes. Every implementation task ends in a local Git commit whose final message line is `本次提交由BigStrongsSun完成`.
- Repository text stays UTF-8 without BOM; verify strict decoding and absence of U+FFFD before completion.

---

## File Structure

- `src-tauri/src/proxy/providers/codex_reasoning.rs`: provider declaration validation and normalized reasoning capability resolution.
- `src-tauri/src/codex_subagent_profiles.rs`: persisted runtime policy, schema migration, capability-aware compilation, and optional role effort rendering.
- `src-tauri/src/codex_config.rs`: catalog-to-compiler projection, read-only capability IPC, preview/status projection, transactional write/read-back verification.
- `src-tauri/src/proxy/providers/codex.rs`: convert resolved capability into Chat transformer configuration.
- `src-tauri/src/proxy/providers/transform_codex_chat.rs`: protocol-level enable/disable and effort mapping.
- `src/config/codexProviderPresets.ts`: maintained provider declarations, including the current DeepSeek V4 preset.
- `src/types.ts`: frontend provider capability declaration types.
- `src/types/codexSubagentV2.ts`: persisted profile policy and backend capability response types.
- `src/lib/api/codexSubagentV2.ts`: Tauri client for resolved capability reads.
- `src/components/codex/CodexSubagentProfileEditor.tsx`: capability source display and runtime policy controls.
- `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`: editor, migration payload, preview, save, and read-back behavior.
- `src/components/providers/forms/CodexFormFields.tsx`: automatic, maintained, and manual capability-source controls with structured fields.
- `src/components/providers/forms/CodexFormFields.reasoning.test.tsx`: capability editor interaction and persistence regressions.
- `src/components/providers/forms/ProviderForm.tsx`: final provider capability validation before persistence.
- `src/config/codexProviderPresets.reasoning.test.ts`: maintained provider declaration regression coverage.
- `memory.md`: final repository knowledge and acceptance evidence.

---

### Task 1: Normalize Provider Reasoning Capabilities

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex_reasoning.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src/config/codexProviderPresets.ts`
- Modify: `src/types.ts`
- Test: `src-tauri/src/proxy/providers/codex_reasoning.rs`
- Test: `src-tauri/src/codex_config.rs`
- Test: `src/config/codexProviderPresets.reasoning.test.ts`

**Interfaces:**
- Consumes: existing `CodexModelReasoningCapability` declarations from `modelCatalog.models[].reasoning`.
- Produces: `resolve_subagent_reasoning_capability(capability: Option<&CodexModelReasoningCapability>) -> ResolvedSubagentReasoningCapability` and a catalog projection whose selectable values exactly match the resolved capability.
- Test helpers: `deepseek_capability() -> CodexModelReasoningCapability` returns the literal maintained declaration shown in Step 4; `efforts(values: &[&str]) -> Vec<CodexReasoningEffort>` parses test literals through the production enum parser.

- [ ] **Step 1: Write failing Rust resolver tests**

Add tests proving DeepSeek keeps provider values separate from Codex-selectable values, `none` is added only when disable is confirmed, mapped values are accepted only when their target is provider-supported, and `ultra` remains unavailable without evidence:

```rust
#[test]
fn deepseek_resolution_separates_provider_and_codex_efforts() {
    let capability = deepseek_capability();
    let resolved = resolve_subagent_reasoning_capability(Some(&capability));
    assert_eq!(resolved.provider_accepted_efforts, efforts(&["low", "high", "max"]));
    assert_eq!(resolved.codex_selectable_efforts, efforts(&[
        "none", "low", "medium", "high", "xhigh", "max"
    ]));
    assert_eq!(resolved.effort_map.get("medium"), Some(&"high".to_string()));
    assert!(!resolved.codex_selectable_efforts.contains(&CodexReasoningEffort::Ultra));
}

#[test]
fn unknown_capability_does_not_advertise_candidate_efforts() {
    let resolved = resolve_subagent_reasoning_capability(None);
    assert_eq!(resolved.support_kind, ReasoningSupportKind::Unknown);
    assert!(resolved.codex_selectable_efforts.is_empty());
}
```

- [ ] **Step 2: Run the focused Rust tests and verify RED**

Run: `cargo test --lib proxy::providers::codex_reasoning::tests -- --nocapture`

Expected: compilation or assertion failure because the normalized resolver and separated fields do not exist.

- [ ] **Step 3: Implement the normalized Rust contract**

Add these types and keep raw declaration parsing backward compatible:

```rust
pub enum ReasoningSupportKind { EffortLevels, BooleanOnly, Unsupported, Unknown }
pub enum ReasoningConfidence { Confirmed, Declared, Unverified }
pub enum CodexReasoningEffort { None, Minimal, Low, Medium, High, XHigh, Max, Ultra }

pub struct ResolvedSubagentReasoningCapability {
    pub support_kind: ReasoningSupportKind,
    pub source: Option<String>,
    pub confidence: ReasoningConfidence,
    pub codex_selectable_efforts: Vec<CodexReasoningEffort>,
    pub provider_accepted_efforts: Vec<CodexReasoningEffort>,
    pub provider_default_effort: Option<CodexReasoningEffort>,
    pub disable_allowed: bool,
    pub effort_map: BTreeMap<CodexReasoningEffort, CodexReasoningEffort>,
}
```

Define this shared effort enum in `codex_reasoning.rs` so provider resolution, Sub-Agent compilation, catalog projection, and request transformation use the same vocabulary. Derive selectable values from provider values plus verified mapping keys. Add `None` only when `disable_allowed=true`; never synthesize `Ultra`. Reject a mapping whose target is absent from provider values.

- [ ] **Step 4: Correct the maintained DeepSeek declaration and frontend type**

Set the maintained declaration to:

```ts
const deepSeekV4Reasoning: CodexModelReasoningCapability = {
  supported: true,
  supportedEfforts: ["low", "high", "max"],
  defaultEffort: "high",
  disableAllowed: true,
  upstream: {
    format: "string",
    parameter: "reasoning_effort",
    effortMap: {
      low: "low",
      medium: "high",
      high: "high",
      xhigh: "high",
      max: "max",
    },
  },
  source: "builtin",
};
```

Add `ultra` to `CodexReasoningEffort`, but do not place it in DeepSeek's declaration.

- [ ] **Step 5: Make Codex catalog projection use the resolved selectable set**

In `codex_config.rs`, replace direct projection of raw `supported_efforts` with `resolved.codex_selectable_efforts`; project `provider_default_effort` as `default_reasoning_level`. Add assertions that DeepSeek catalog output contains `none/low/medium/high/xhigh/max`, defaults to `high`, and omits `ultra`.

- [ ] **Step 6: Run focused Rust and frontend tests**

Run:

```powershell
cargo test --lib proxy::providers::codex_reasoning::tests -- --nocapture
cargo test --lib codex_config::tests -- --nocapture
pnpm vitest run src/config/codexProviderPresets.reasoning.test.ts
```

Expected: all focused tests pass.

- [ ] **Step 7: Commit the capability boundary**

```powershell
git add src-tauri/src/proxy/providers/codex_reasoning.rs src-tauri/src/codex_config.rs src-tauri/src/proxy/providers/codex.rs src/config/codexProviderPresets.ts src/config/codexProviderPresets.reasoning.test.ts src/types.ts
git commit -m "feat: unify Codex reasoning capability resolution" -m "本次提交由BigStrongsSun完成"
```

### Task 2: Replace Task-Based Auto Effort with an Explicit Runtime Policy

**Files:**
- Modify: `src-tauri/src/codex_subagent_profiles.rs`
- Modify: `src/types/codexSubagentV2.ts`
- Test: `src-tauri/src/codex_subagent_profiles.rs`

**Interfaces:**
- Consumes: `ResolvedSubagentReasoningCapability` from Task 1.
- Produces: `ReasoningRuntimePolicy`, schema-version-2 profile persistence, and deterministic migration from schema version 1.
- Test helpers: `legacy_profile(questionnaire_effort: &str, override_effort: Option<&str>) -> Value`, `profile(config: &CodexSubagentV2) -> &ParsedCodexSubagentProfile`, and `fixed(effort: CodexReasoningEffort) -> CodexSubagentReasoningPolicy` are local fixtures defined beside the migration tests.

- [ ] **Step 1: Write schema migration RED tests**

Cover all legacy meanings:

```rust
#[test]
fn legacy_auto_migrates_to_delegated() {
    let parsed = parse_persisted_subagent_v2(&legacy_profile("auto", None)).unwrap();
    assert_eq!(profile(&parsed).reasoning.policy, ReasoningRuntimePolicy::Delegated);
}

#[test]
fn legacy_explicit_effort_migrates_to_fixed() {
    let parsed = parse_persisted_subagent_v2(&legacy_profile("high", None)).unwrap();
    assert_eq!(profile(&parsed).reasoning, fixed(CodexReasoningEffort::High));
}

#[test]
fn legacy_override_has_priority_during_migration() {
    let parsed = parse_persisted_subagent_v2(&legacy_profile("auto", Some("xhigh"))).unwrap();
    assert_eq!(profile(&parsed).reasoning, fixed(CodexReasoningEffort::XHigh));
}
```

- [ ] **Step 2: Run the migration tests and verify RED**

Run: `cargo test --lib codex_subagent_profiles::tests::legacy_ -- --nocapture`

Expected: failure because profiles still persist questionnaire `reasoningEffort` and `auto_effort()`.

- [ ] **Step 3: Add the schema-version-2 policy types**

Use one explicit object:

```rust
pub enum ReasoningRuntimePolicy { Delegated, ModelDefault, Fixed, Disabled }

pub struct CodexSubagentReasoningPolicy {
    pub policy: ReasoningRuntimePolicy,
    pub effort: Option<CodexReasoningEffort>,
}
```

Validation rules:

- `fixed` requires exactly one effort other than `none`.
- `delegated`, `model_default`, and `disabled` reject a stored effort.
- `disabled` is accepted only when the resolved capability allows it.
- Remove the local four-value `ModelReasoningEffort` enum and use Task 1's shared `CodexReasoningEffort` throughout profile parsing and compilation.
- Serialize new profiles as `schemaVersion: 2`; parse schema version 1 through a dedicated migration function and serialize it back only in version 2 form.

- [ ] **Step 4: Remove task-based effort selection**

Delete `auto_effort()` and its strength/optimization-derived tests. Keep task strengths for role description/routing only. Change newly created DeepSeek profiles to:

```ts
reasoning: { policy: "delegated" }
```

- [ ] **Step 5: Update the TypeScript persistence contract**

Define the matching discriminated union:

```ts
export type CodexSubagentReasoningPolicy =
  | { policy: "delegated" }
  | { policy: "model_default" }
  | { policy: "fixed"; effort: CodexSubagentExplicitReasoningEffort }
  | { policy: "disabled" };
```

Set `CodexSubagentV2Config.schemaVersion` to `2` and remove `questionnaire.reasoningEffort` plus `overrides.modelReasoningEffort` from newly serialized data.

- [ ] **Step 6: Run parser and serialization tests**

Run: `cargo test --lib codex_subagent_profiles::tests -- --nocapture`

Expected: schema v1 is read without data loss, all output uses schema v2, and task answers no longer select effort.

- [ ] **Step 7: Commit the policy migration**

```powershell
git add src-tauri/src/codex_subagent_profiles.rs src/types/codexSubagentV2.ts
git commit -m "refactor: model Sub-Agent reasoning as runtime policy" -m "本次提交由BigStrongsSun完成"
```

### Task 3: Compile Capability-Aware Role TOML

**Files:**
- Modify: `src-tauri/src/codex_subagent_profiles.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/codex_subagent_profiles.rs`
- Test: `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consumes: schema-version-2 profile policy and normalized capability.
- Produces: `GeneratedRole.reasoning_effort: Option<CodexReasoningEffort>` plus preview/status metadata describing the selected policy and effective fixed value.
- Test helpers: `delegated()`, `model_default()`, `fixed(effort)`, and `disabled()` construct valid policy objects; `compile_role(policy, capability)` invokes the real compiler and TOML renderer; `assert_compile_error` checks the exact validation code without writing files.

- [ ] **Step 1: Write compiler RED tests for all four policies**

Add independent cases:

```rust
assert!(!compile_role(delegated(), deepseek()).toml.contains("model_reasoning_effort"));
assert!(compile_role(model_default(), deepseek()).toml.contains("model_reasoning_effort = \"high\""));
assert!(compile_role(fixed(CodexReasoningEffort::Max), deepseek()).toml.contains("model_reasoning_effort = \"max\""));
assert!(compile_role(disabled(), deepseek()).toml.contains("model_reasoning_effort = \"none\""));
assert_compile_error(fixed(CodexReasoningEffort::Ultra), deepseek(), "unsupported_reasoning_effort");
```

Also prove a role that pins a different model but delegates preserves the missing effort field, leaving native Codex to validate the inherited/spawn value.

- [ ] **Step 2: Run compiler tests and verify RED**

Run: `cargo test --lib codex_subagent_profiles::tests::reasoning_policy_ -- --nocapture`

Expected: failure because `GeneratedRole.effort` and `RoleToml.model_reasoning_effort` are mandatory.

- [ ] **Step 3: Carry capability into the compiler boundary**

Extend `CatalogModel`:

```rust
pub struct CatalogModel {
    pub model: String,
    pub provider_kind: ProviderKind,
    pub routable: bool,
    pub context_window: u64,
    pub reasoning: ResolvedSubagentReasoningCapability,
}
```

Resolve policies in one function:

```rust
fn compile_reasoning_policy(
    policy: &CodexSubagentReasoningPolicy,
    capability: &ResolvedSubagentReasoningCapability,
) -> Result<Option<CodexReasoningEffort>, CompileError>;
```

- [ ] **Step 4: Make TOML effort optional and verifiable**

Change `GeneratedRole` and `RoleToml` to `Option<CodexReasoningEffort>` with `skip_serializing_if = "Option::is_none"`. Update `codex_agent_file_matches_expected()` and mutation verification so an omitted effort is positively verified as absent, not ignored.

- [ ] **Step 5: Update preview and status contracts**

Return:

```rust
reasoning_policy: ReasoningRuntimePolicy,
model_reasoning_effort: Option<CodexReasoningEffort>,
reasoning_capability: ResolvedSubagentReasoningCapability,
```

Preview text must say `delegated`, `fixed high`, `model default high (fixed)`, or `disabled`; it must not invent `medium` when preview is absent.

- [ ] **Step 6: Run compiler/config suites**

Run:

```powershell
cargo test --lib codex_subagent_profiles::tests -- --nocapture
cargo test --lib codex_config::tests -- --nocapture
```

Expected: all role generation, collision, legacy, preview, and verification tests pass.

- [ ] **Step 7: Commit optional role effort generation**

```powershell
git add src-tauri/src/codex_subagent_profiles.rs src-tauri/src/codex_config.rs
git commit -m "feat: compile capability-aware Sub-Agent role effort" -m "本次提交由BigStrongsSun完成"
```

### Task 4: Expose Backend-Resolved Capabilities and Preserve Transactional Save Proof

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/codexSubagentV2.ts`
- Modify: `src/types/codexSubagentV2.ts`
- Test: `src-tauri/src/codex_config.rs`
- Test: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`

**Interfaces:**
- Consumes: the same catalog specs used for model catalog and role compilation.
- Produces: `get_codex_subagent_reasoning_capabilities(settingsConfig) -> Record<string, CodexSubagentReasoningCapability>`.

- [ ] **Step 1: Write IPC contract RED tests**

Assert exact camelCase output and no credential-bearing fields:

```json
{
  "deepseek-v4-pro": {
    "supportKind": "effort_levels",
    "source": "builtin",
    "confidence": "confirmed",
    "codexSelectableEfforts": ["none", "low", "medium", "high", "xhigh", "max"],
    "providerAcceptedEfforts": ["low", "high", "max"],
    "providerDefaultEffort": "high",
    "disableAllowed": true,
    "effortMap": {"low":"low","medium":"high","high":"high","xhigh":"high","max":"max"}
  }
}
```

- [ ] **Step 2: Run IPC tests and verify RED**

Run: `cargo test --lib codex_config::tests::codex_subagent_reasoning_capabilities -- --nocapture`

Expected: command and response type are absent.

- [ ] **Step 3: Implement and register the read-only command**

Resolve from `settingsConfig` using the same model-spec builder as catalog generation. Return only model identifiers and reasoning metadata. Register the command in `src-tauri/src/lib.rs` and add the typed client method.

- [ ] **Step 4: Strengthen save/read-back tests**

For each runtime policy, verify `update_codex_subagent_v2` returns:

- `databasePersisted=true` only after re-reading schema-version-2 JSON.
- `roleFilesStatus=verified` only when role file presence and exact effort absence/value match.
- pending live projection remains explicit and does not turn a database-only save into full success.
- a failed role read-back leaves the previous file intact and returns the existing controlled error path.

- [ ] **Step 5: Run backend and API contract tests**

Run:

```powershell
cargo test --lib codex_config::tests -- --nocapture
pnpm vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx
```

Expected: capability IPC, mutation verification, and existing preview/status calls pass.

- [ ] **Step 6: Commit the backend authority boundary**

```powershell
git add src-tauri/src/codex_config.rs src-tauri/src/lib.rs src/lib/api/codexSubagentV2.ts src/types/codexSubagentV2.ts src/components/codex/CodexSubagentV2ProfileEditor.test.tsx
git commit -m "feat: expose verified Sub-Agent reasoning capabilities" -m "本次提交由BigStrongsSun完成"
```

### Task 5: Build the Capability-Source and Runtime-Policy Editors

**Files:**
- Modify: `src/components/codex/CodexSubagentProfileEditor.tsx`
- Modify: `src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`
- Modify: `src/components/providers/forms/CodexFormFields.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Create: `src/components/providers/forms/CodexFormFields.reasoning.test.tsx`
- Modify: `src/components/providers/forms/ProviderForm.reasoning.test.ts`
- Modify: `src/types/codexSubagentV2.ts`

**Interfaces:**
- Consumes: resolved capability map and schema-version-2 runtime policy.
- Produces: capability evidence display plus policy controls that serialize only valid combinations.

- [ ] **Step 1: Write UI RED tests for capability and policy separation**

Cover these visible behaviors:

- Capability section displays source, confidence, provider-native values, Codex-selectable values, default, mapping, and disable support.
- Runtime policy defaults to `允许主 Agent / spawn 指定` for a newly created profile.
- Delegated preview contains no `model_reasoning_effort`.
- Model-default preview says `固定为模型当前默认 high`.
- Fixed dropdown shows only resolved selectable strengths; `none` is not mixed into the low-to-ultra dropdown.
- Disabled policy appears only when `disableAllowed=true`.
- Unknown capability offers a manual declaration link/state but does not enable runtime values.
- Save success remains disabled until mutation verification reports database and role-file proof.
- Capability source offers exactly `自动发现`, `使用 CCSM 受维护声明`, and `手动声明`; choosing manual uses structured controls and does not require raw JSON.
- An expert raw-JSON panel remains available, but invalid JSON or an invalid default/mapping cannot mutate the draft.

- [ ] **Step 2: Run editor tests and verify RED**

Run: `pnpm vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx`

Expected: failures because the editor still shows one fixed low/medium/high/xhigh override control.

- [ ] **Step 3: Load capability data with stale-response protection**

Call `getReasoningCapabilities(settingsConfig)` whenever the draft provider catalog changes. Reuse the editor's existing request-generation/cancellation pattern so an older response cannot overwrite a newer draft.

- [ ] **Step 4: Replace the old override select**

Render two groups:

```text
模型推理能力来源
  来源 / 置信度 / 原生档位 / Codex 可选档位 / 映射 / 可否关闭

Sub-Agent 运行时策略
  允许主 Agent / spawn 指定
  使用模型默认（固定）
  固定档位
  关闭推理
```

When `fixed` is selected, show only `low/medium/high/xhigh/max/ultra` values present in `codexSelectableEfforts`; show `minimal` separately when confirmed. Do not render an enabled `disabled` option unless capability permits it.

- [ ] **Step 5: Make effective behavior explicit beside preview**

Use stable text:

- `由主 Agent、spawn 参数或 Codex 默认值决定`
- `固定为模型当前默认 high`
- `固定 high；主 Agent 无法覆盖`
- `关闭推理；上游将使用 thinking.type=disabled`

The displayed upstream mapping comes from the backend result, never a frontend provider-name switch.

- [ ] **Step 6: Replace raw JSON as the primary capability editor**

In `CodexFormFields.tsx`, add a source selector with these write semantics:

- `automatic`: remove the row's explicit `reasoning` declaration and display backend-resolved evidence as read-only.
- `builtin`: clone the matching maintained preset into the row with `source: "builtin"`.
- `manual`: create or retain a `source: "user"` declaration edited through supported values, default, disable support, upstream format/parameter, and per-effort mapping controls.

Keep the CodeMirror JSON editor under an `专家 JSON` disclosure. Before updating the draft, run the same validation as `ProviderForm.tsx`: default must be provider-accepted, every mapping target must be provider-accepted, `none` is a disable signal rather than a provider effort, and unknown strings are rejected. After provider save, compare the returned `modelCatalog.models[].reasoning` with the normalized draft and show an explicit persisted/failed state.

- [ ] **Step 7: Run frontend verification**

Run:

```powershell
pnpm vitest run src/components/codex/CodexSubagentV2ProfileEditor.test.tsx
pnpm vitest run src/components/providers/forms/CodexFormFields.reasoning.test.tsx src/components/providers/forms/ProviderForm.reasoning.test.ts
pnpm typecheck
pnpm prettier --check src/components/codex/CodexSubagentProfileEditor.tsx src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/providers/forms/CodexFormFields.tsx src/components/providers/forms/CodexFormFields.reasoning.test.tsx src/components/providers/forms/ProviderForm.tsx src/types/codexSubagentV2.ts src/lib/api/codexSubagentV2.ts
```

Expected: editor tests, type checking, and formatting pass.

- [ ] **Step 8: Commit the structured editor**

```powershell
git add src/components/codex/CodexSubagentProfileEditor.tsx src/components/codex/CodexSubagentV2ProfileEditor.test.tsx src/components/providers/forms/CodexFormFields.tsx src/components/providers/forms/CodexFormFields.reasoning.test.tsx src/components/providers/forms/ProviderForm.tsx src/components/providers/forms/ProviderForm.reasoning.test.ts src/types/codexSubagentV2.ts src/lib/api/codexSubagentV2.ts
git commit -m "feat: separate Sub-Agent capability and runtime policy" -m "本次提交由BigStrongsSun完成"
```

### Task 6: Verify Provider-Native Disable and Effort Mapping

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/providers/transform_codex_chat.rs`
- Test: `src-tauri/src/proxy/providers/codex.rs`
- Test: `src-tauri/src/proxy/providers/transform_codex_chat.rs`

**Interfaces:**
- Consumes: normalized capability and incoming Codex `reasoning.effort`.
- Produces: provider-native request fields with explicit off semantics and no duplicated/invalid effort field.
- Test helpers: `deepseek_chat(effort: &str) -> Value` returns only reasoning-related fields from the real Responses-to-Chat transformer; `deepseek_chat_request(effort: &str)` builds the full transformer input; `assert_transform_error` checks the controlled `ProxyError` text.

- [ ] **Step 1: Write protocol RED tests**

Add exact body assertions:

```rust
assert_eq!(deepseek_chat("none"), json!({
    "thinking": {"type": "disabled"}
}));
assert_eq!(deepseek_chat("medium")["reasoning_effort"], "high");
assert_eq!(deepseek_chat("xhigh")["reasoning_effort"], "high");
assert_eq!(deepseek_chat("max")["reasoning_effort"], "max");
assert_transform_error(deepseek_chat_request("ultra"), "not supported");
```

Also assert Responses keeps `reasoning.effort=none` and boolean-only providers emit only their declared false switch.

- [ ] **Step 2: Run transformer tests and verify RED**

Run: `cargo test --lib proxy::providers::transform_codex_chat::tests -- --nocapture`

Expected: DeepSeek disable or separated supported/mapped set assertions fail on the pre-change implementation.

- [ ] **Step 3: Apply the resolved mapping without silent clamping**

Construct `CodexChatReasoningConfig` from the normalized capability. Ensure the Chat transformer treats `none` as a disable signal before effort validation, omits top-level `reasoning_effort` when disabled, and validates positive effort against the resolved selectable set before mapping.

- [ ] **Step 4: Run proxy suites**

Run:

```powershell
cargo test --lib proxy::providers::codex::tests -- --nocapture
cargo test --lib proxy::providers::transform_codex_chat::tests -- --nocapture
```

Expected: exact DeepSeek, Responses, OpenRouter, GLM, and boolean-provider tests pass without changing unrelated routing behavior.

- [ ] **Step 5: Commit provider-native mapping**

```powershell
git add src-tauri/src/proxy/providers/codex.rs src-tauri/src/proxy/providers/transform_codex_chat.rs
git commit -m "fix: preserve provider-native reasoning semantics" -m "本次提交由BigStrongsSun完成"
```

### Task 7: End-to-End Verification and Repository Memory

**Files:**
- Modify: `memory.md`
- Test: generated temporary Codex catalog and managed role files under test-owned directories only.

**Interfaces:**
- Consumes: all prior tasks.
- Produces: acceptance evidence that persistence, catalog, role TOML, native spawn behavior, and upstream conversion agree.

- [ ] **Step 1: Run complete static and unit gates**

Run:

```powershell
pnpm vitest run
pnpm typecheck
pnpm build:renderer
cargo test --lib
cargo check --lib
cargo fmt --check
git diff --check
```

Expected: all commands pass; pre-existing warnings are recorded without being misreported as new failures.

- [ ] **Step 2: Verify strict text encoding**

Run a Python UTF-8 strict-decode check over every changed text file; assert no UTF-8 BOM and no U+FFFD. Re-read the Chinese policy labels from the source after the check.

- [ ] **Step 3: Exercise the four policy paths in a test-owned Codex home**

For one model with full supported efforts and one DeepSeek model, generate and parse role files, then assert:

- delegated role omits `model_reasoning_effort`;
- model-default role writes the catalog default;
- fixed role writes the selected supported value;
- disabled role writes `none` only for a model whose catalog includes it;
- unsupported `ultra` fails before writing any role file.

- [ ] **Step 4: Perform real native spawn acceptance without disrupting the live CCSM listener**

Use a separate test-owned Codex configuration or an independent transactional runner. Record the child model/effort for:

- parent `high`, delegated role, spawn omitted;
- parent `high`, global Sub-Agent default `low`, spawn omitted;
- global default `low`, spawn explicit `max`;
- role fixed `low`, spawn explicit `max`;
- DeepSeek disabled and DeepSeek unsupported `ultra`.

Do not stop `127.0.0.1:15721` from the current dependent session. If isolated real spawn cannot be completed safely, report that exact acceptance item as unverified rather than substituting configuration inspection.

- [ ] **Step 5: Update repository memory with current evidence**

Replace stale auto-effort notes with the implemented schema, exact migration behavior, test counts, commit identifiers, and the boundary between unit proof and live runtime proof. Do not record credentials, request bodies containing user data, or machine secrets.

- [ ] **Step 6: Commit verification knowledge**

```powershell
git add memory.md
git commit -m "docs(memory): record Sub-Agent reasoning coordination evidence" -m "本次提交由BigStrongsSun完成"
```

- [ ] **Step 7: Final clean-tree audit**

Run:

```powershell
git status --short
git log --oneline --decorate -8
```

Expected: no uncommitted files created by this implementation and each task has a traceable local commit.

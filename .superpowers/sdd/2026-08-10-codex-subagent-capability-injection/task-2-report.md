# Task 2 Backend RED — authoritative current evidence

## Boundary

This commit remains strictly RED-only. `src-tauri/src/lib.rs` is still the sole crate-root
`#[cfg(test)]` identity for `codex_subagent_profiles`; no production compiler, preview command,
migration, role projection, routing behavior, or UI was implemented. The wished-for boundary uses
owned `String` values for runtime text, typed enums for version/policy/questionnaire/effort/provider/
status/reason values, a real `CompileRequest` split between persisted V2 state, catalog routing data,
and occupied role names, controlled diagnostic reason codes, and an explicit invalid-raw profile
variant. All four RED sentinels still return `Err(NotImplemented)` and have no production caller.

Each observable behavior below is an independent `#[test]`. There is no multi-behavior sequence in
which the first `Err(NotImplemented)` assertion prevents a later sentinel invocation. The only
multi-value assertion first computes both configured renderer results and then compares the complete
five-element relationship vector.

## Independent RED inventory: 59 tests

- Schema/parser/defaults/overrides — **15**: missing `selectionPolicy` defaults to balanced; missing
  `schemaVersion`; `schemaVersion != 1`; invalid `selectionPolicy`, `optimization`, `writeScope`,
  `preference`, questionnaire effort, and override effort enums; independently missing
  `taskStrengths`, `optimization`, `writeScope`, `preference`, and `reasoningEffort`; literal
  round-trip of every override.
- Strength membership/count — **6**: zero rejects, one accepts, five unique accepts, six rejects,
  duplicate rejects, and unknown enum member rejects.
- Profile key collisions — **2**: full-width NFKC collision and `Straße`/`STRASSE` Default Case
  Folding collision; both require all colliders to produce no roles while retaining original model
  spelling in status.
- Effort — **4**: complex/architecture -> high, speed + read/explore-only -> low, speed + testing ->
  medium, and explicit `xhigh` overrides auto.
- Selection policy — **5**: balanced adds no bias, official-first retains high-risk official bias,
  third-party-first promotes eligible, preferred overrides provider bias, and fallback is never
  promoted. Generated roles retain fixed `codex_model_router_v2`.
- Overrides and role names — **8**: manual description fully replaces policy selection text;
  description restoration retains developer instructions; mixed-separator ASCII normalization;
  empty-normalized rejection; separate built-in `default`, `worker`, and `explorer` rejection; and
  case-insensitive occupancy resolution through `review`, `ccswitch-review`,
  `ccswitch-review-2`, then `ccswitch-review-3`.
- Nicknames — **7**: zero rejects, one accepts, three accepts, four rejects, empty rejects, duplicate
  rejects, and non-ASCII/non-allowlisted punctuation rejects.
- Persistence/lifecycle — **8**: malformed raw value preservation with invalid/no-role status; V1
  preserves V2 input but materializes no V2 role; catalog alias changes preserve original profile
  model and become unroutable; V2 enabled+routable generates role plus routable status; disabled and
  unroutable each remain status-visible without a role; missing V2 state preserves legacy managed
  behavior; explicit initialization returns the exact Flash/Pro presets.
- Diagnostic redaction — **1**: sanitizer output must equal the literal allowlisted
  model/role/policy/status/reason payload and exact serialized JSON, excluding reason detail, API key,
  task body, encrypted content, and arbitrary secret source fields.
- Real current managed-role boundary — **3**: manual description, explicit effort, and two-input
  configured-materialization relationship independently prove the current hardcoded
  `render_codex_managed_agent_toml` path cannot consume configured questionnaire/override inputs.

## Exact RED result

`cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2 -- --nocapture`

- Exit: **1** (Cargo test failure; compile and harness startup succeeded).
- Lib test result: **0 passed / 59 failed / 0 ignored / 0 measured / 2837 filtered**.
- The 56 profile/compiler/init/sanitizer failures are literal `assert_eq!` mismatches with actual
  `Err(NotImplemented)` and expected typed `Ok(...)` or `Err(Validation { ... })`.
- The three integration-level renderer failures are literal mismatches: manual-description and
  explicit-effort checks produce `false` versus `true`; the dual-input relationship produces
  `[false, false, false, false, false]` versus `[true, true, true, true, true]`.
- No test failed from a compile error, environment error, fixture panic, missing file, or panic before
  the intended assertion.

## Controls

| Command | Exit | Current result |
|---|---:|---|
| `cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2 -- --nocapture` | 1 | Expected RED: 59 literal assertion mismatches; 2837 lib tests filtered. |
| `cargo test --manifest-path src-tauri/Cargo.toml codex_managed_agent -- --nocapture` | 0 | No matching tests; lib reports 2896 filtered and every integration test binary also exits successfully. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 0 | Formatting control passed. |
| `git diff --check` | 0 | Whitespace control passed. |

The compiler emits existing/test-only dead-code warnings because complete typed enum contracts and
sensitive diagnostic source fields are intentionally present before GREEN consumes them; warnings do
not alter compilation or the expected RED cause.

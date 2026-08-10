# Codex Sub-Agent V2 Capability Injection Implementation Plan

Target: `3.19.1-19`. Scope is local commits, build, installation, and live acceptance only: no push, PR, or GitHub release.

## Global constraints

Preserve V1 direct override behavior, official provider classification, V2 body projection, reserved schema, mixed `agents` namespace, `hide_spawn_agent_metadata=true`, and Qwen behavior. Do not change the parent model, Codex spawn schema, or calls. Treat user-authored role files as immutable. Keep diagnostics free of credentials, task text, and encrypted contents. Each RED/GREEN or debugging change is separately committed with a detailed Chinese message ending in a final paragraph exactly `本次提交由BigStrongsSun完成`.

## 1. Documents

- [ ] Add the approved design and this executable plan. Cross-check every public contract: persisted `subagentV2`, compiler ownership, provider classification, field restoration, V1/V2 lifecycle, role collision/nickname rules, UI, diagnostics, presets, and transaction-install boundary.
- [ ] Verify no incomplete marker, incompatible policy, or unqualified model-name provider inference remains.
- [ ] Commit only the two documents for this task.

## 2. Backend RED

- [ ] Add failing Rust tests for schema parsing/defaulting, profile preservation over catalog refresh, backend-only compilation, auto effort, policy semantics, override deletion, nickname/role normalization, collision handling, unroutable non-generation, legacy initialization, and diagnostics redaction.
- [ ] Cover V1 activation preserving inactive V2 data and V2 activation only materializing enabled/routable profiles.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2 -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml codex_managed_agent -- --nocapture
```

Expected RED result: each test fails for the missing capability-injection contract rather than unrelated compilation/environment errors. Commit only the tests.

## 3. Backend GREEN

- [ ] Implement typed persistence and a single backend compiler/preview command.
- [ ] Materialize only official custom-agent fields with fixed `codex_model_router_v2`; reuse existing provider kind classification.
- [ ] Implement V2-only role generation, field-level override deletion, profile preservation, legacy one-click initialization, and the two DeepSeek presets.
- [ ] Preserve all existing V1, reserved-schema, V2 body-projection, mixed-routing, and Qwen tests.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2 -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml codex_managed_agent -- --nocapture
cargo fmt --manifest-path src-tauri/Cargo.toml --check
```

Acceptance: generated preview contains all specified fields; manual description replaces policy-generated selection text; provider policy and effort truth table pass; disabled/unroutable profiles persist but create no role.

## 4. Frontend RED

- [ ] Add failing Vitest/Testing Library coverage for a shared wizard/workspace editor, its four areas, valid enum choices, 1–5 strengths, final derived/override display, one-field restore, preview/status, legacy initialize action, and refresh persistence.
- [ ] Include status assertions for provider kind, routability, auto/override state, enabled state, requested/effective role/path, and non-generation reason.

Run:

```powershell
pnpm vitest run src/components/codex --exclude ".worktrees/**"
```

Expected RED result: assertions fail because the questionnaire/editor/preview state is absent, not because of fixture or duplicate-worktree contamination. Commit only test changes.

## 5. Frontend GREEN

- [ ] Build one editor used by both the wizard and MultiRouter workspace from the same `subagentV2` source.
- [ ] Render selection policy, questionnaire, final fields, and backend TOML preview; send backend preview/status rather than recompiling client-side.
- [ ] Make validation explicit: strengths are 1–5, nickname values meet syntax/count/uniqueness rules, and built-in/duplicate role names cannot be saved.

Run:

```powershell
pnpm vitest run src/components/codex --exclude ".worktrees/**"
pnpm typecheck
```

Acceptance: policy changes update derived preview; manual field overrides visibly supersede only their counterpart; restoring a field removes only that override; wizard and workspace show the same saved profile after remount.

## 6. Compatibility, diagnostics, and memory

- [ ] Add regression tests for legacy configurations without `subagentV2`, mode switching, catalog refresh, disabled drafts, user-role collisions, V1's first-five direct overrides, built-in role protection, diagnostics redaction, and unchanged Qwen behavior.
- [ ] Update project `memory.md` with the actual architecture, commands, test evidence, known limits, and transaction-install safety rule after implementation evidence exists.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
pnpm vitest run --exclude ".worktrees/**" --no-file-parallelism
pnpm typecheck
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo fmt --manifest-path src-tauri/Cargo.toml --check
git diff --check
```

Acceptance: legacy roles remain legacy until one-click initialization; user files are untouched; sensitive data cannot appear in diagnostic payloads or tests.

## 7. Full verification and version commit

- [ ] Change all four version sources (`package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`) to `3.19.1-19` and regenerate lockfile through Cargo.
- [ ] Re-run the full commands from Task 6 and verify exact version equality.
- [ ] Commit the version and verified implementation state locally.

Acceptance: all full checks are green, `git diff --check` is empty, and no unrelated worktree changes are staged.

## 8. Build, transaction install, and live acceptance

- [ ] Build with `pnpm tauri build`, export the successful Windows artifacts, record absolute path, size, file/product version, SHA-256, and signature in `memory.md`.
- [ ] Install only through one independent hidden PowerShell process. That single transaction must preflight and back up, verify target PID, kill/wait and release the port, uninstall, install, hidden-relaunch, validate health/version/hash/routing, and roll back on any failure.
- [ ] Never stop CCSM alone from the interactive shell; do not stop Codex Desktop while the current task depends on it.
- [ ] Validate in the installed UI: shared editor in wizard/workspace, policy/profile persistence, status, preview, V1 preserved direct overrides, and V2 generated roles.
- [ ] Use new sessions for live canaries. Verify a no-model-name Flash task and Pro task select the expected enabled roles, use real read-only tools and follow-up, carry the expected model/provider/version in rollout data, and each upstream route returns HTTP 200. Hand-written `model=` calls are supplementary only.

Run:

```powershell
pnpm tauri build
Get-FileHash $installerPath -Algorithm SHA256
git status --short
```

Acceptance: the transaction proves rollback safety and installed `3.19.1-19`; live acceptance proves actual generated-role behavior rather than source/config presence. Record exact IDs, hashes, versions, health evidence, limitations, and final status in `memory.md`.

## Research evidence

Official [Subagents documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) establishes custom-role selection descriptions, role model/reasoning precedence over spawn/default/parent resolution, and local delegation triggers. The official [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) is the config-key authority. Local official source confirmation is `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/agent/role.rs` and `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/tools/spec_plan.rs`.

Matrix WebSearch independently searched on 2026-08-10 and found no equivalent official first-party result, but direct Matrix fetch of the official pages succeeded. Use official docs/source and local runtime evidence for primary conclusions; retain that Matrix search-discovery limitation as an uncertainty.

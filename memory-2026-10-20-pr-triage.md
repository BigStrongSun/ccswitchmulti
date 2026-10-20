# 2026-10-20 CCSwitchMulti PR Triage

## Ownership And Scope

- Request: process other contributors' PRs on public `BigStrongSun/ccswitchmulti`.
- Use the retained integration worktree `cc-switch/.worktrees/fix-codex-pre-response-retry`, branch `bigstrongsun/ccsm-4.0-integration`. No new worktree or runtime resources were created. Retain it for the unfinished 4.0 integration; close that lifecycle when its integration purpose ends.
- Publish only to `fork/main`; `origin`/`upstream` are `farion1231/cc-switch`. Do not publish AgentMesh.
- Public baseline: `7f48b3ef771e115913662da4b6cdf9fbe6e3a9d4`, immutable Release `v3.20.4-8`. No version bump, tag move, new Release, installation, restart, live DB/config/auth write or paid upstream image request.
- Main checkout's pre-existing dirty `memory.md`, `src-tauri/src/commands/protocol_compatibility.rs` and `docs/provider-settings-layout-preview.html` remain untouched.

## Accepted Contributor Changes

| PR   | Merge Commit | Result                                                                                                                                                     |
| ---- | ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| #119 | `be1e4f6a8`  | Conservative docs-only classification, successful frontend/rustfmt prerequisites before three-platform backend compilation, preserved backend check names. |
| #117 | `478c7e20c`  | Await the save call before rejecting its Promise and wait for both save-failure state and error panel.                                                     |
| #113 | `9ade80ca0`  | Resolve against current official context editor; gate automatic default-model projection on ownership of local Codex config.                               |
| #108 | `21f56b6`    | Resolve native official image target and preserve original image JSON/model through local-only raw forwarding.                                             |

Original contributor commits and authorship are preserved via merge commits. Contributor PR #113's description refers to an older baseline: **current public main already persists the canonical official catalog**. Do not reintroduce the broader old predicate or claim catalog loss as the current root cause. The remaining real bug is authentication-only saving with an empty/disabled default: automatic catalog fallback could rewrite the native TOML model without owning local config. The new regression failed on current main (`official-disabled` became `official-active`); gating on `shouldPersistCodexLocalConfig` passes while preserving official catalog/context editing and account model discovery.

For #108, native authentication must also have official provider identity; xAI/Copilot are not made official by native-auth-like settings. Only a local Codex client with an explicitly resolved image provider takes raw forwarding. External clients retain existing authentication gates and forwarding behavior. Explicit third-party image routes remain honored; missing/disabled routes stay fail-closed. Original image bytes/model cannot inherit text-route model overrides.

For #119, classifier errors/unknown paths choose full checks, full Git diff endpoints are used, and only regular non-executable docs qualify. All three `Backend Checks (...)` names remain. Docs/prerequisite failures use lightweight Ubuntu gates instead of three-platform compilation. Branch protection API returned `Branch not protected` (404), so no active required-status list could be verified. Actual integrated main CI must confirm both full and docs-only scheduling.

## Superseded PRs Closed With Explanation

| PR   | Main Replacement Verified As Baseline Ancestor                                                |
| ---- | --------------------------------------------------------------------------------------------- |
| #107 | `acfacae4a`: broader header ownership across stale versions, third-party/raw/WebSocket paths. |
| #61  | `be0a7a13a`, followed by #69 merge `de5f05d5b`: plugin registration.                          |
| #26  | `dab419287`: deleted-provider model order, later moved to SSOT coordination.                  |
| #24  | `2905ce2ed`, `75382f89b`: enabled/disabled model behavior.                                    |
| #19  | `475cd008b`: provider identity/aggregation/cache statistics.                                  |

All five were closed using GitHub comments thanking contributors and identifying replacement commits and regression coverage. Contributor branches were not deleted.

## Dependency PRs Kept Open

- #122: `REQUEST_CHANGES`. CI `39384820068` shows deterministic API breakage in the 38-item Cargo group: zip 9 `ZipFile::name()` is now a `Result<Cow<str>, ZipError>`, rusqlite .40 numeric ToSql/FromSql compatibility changes, and deep-link ParseOption construction needs adaptation. Split major upgrades and include source/test adaptation; do not merge the lockfile alone.
- #112: `REQUEST_CHANGES`. Run `39147295236`, frontend job `117733615772`: TypeScript 7 removes `baseUrl`; current non-relative imports fail typechecking. Split the 56-item frontend group and adapt tsconfig or stage upgrades. macOS job `117733615789` additionally has a port-recovery test failure; its cause is not established as the frontend upgrade.
- #1 stale 10->11, #2 checkout 6->7, #3 labeler 5->7, #4 setup-node 6->7: individual `COMMENT` reviews request focused workflow validation. These are unvalidated upgrade candidates, not proven product defects. Review covers action runtime/runner requirements, checkout fork permissions, labeler trigger/ref boundaries and setup-node runtime versus project Node 20.

## Validation On Final Integrated Code Tree

- Python classifier: 21/21 real-Git regressions; YAML parse and three backend matrix check-name assertions pass.
- `pnpm typecheck`, `pnpm format:check`: exit 0.
- `pnpm run test:unit -- --reporter=dot`: 209 files, 1702 tests passed, exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib image_edits_tests -- --test-threads=1`: 13 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`: 4356 passed, 9 ignored, 0 failed; child process TZ removed for the already diagnosed Windows/TZ interpretation difference, no system setting changed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: exit 0.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`: exit 0.
- Cached diff whitespace and changed source strict UTF-8/no BOM/no U+FFFD checks pass.
- Test TEMP/TMP belong to ignored project path `release/acceptance/pr-triage/test-temp`; existing project Cargo target reused. Do not treat loopback fixtures as a real upstream image-generation or installed-app E2E test.
- Independent Luna reviews covered recent PRs/CI, legacy PR supersession and official/image boundaries, plus dependency blockers. No unresolved merge blocker remains for the four accepted PRs. An additional external-client image credential non-forwarding handler E2E is a possible follow-up; existing forwarder authentication tests remain.

## Verified Publication And Full CI

- Normal fast-forward push updated public `fork/main` to `844ca809dbbd64d9569f3dc86b0637a34daa8326`; remote SHA was checked again after CI completed. GitHub confirms #119, #117, #113 and #108 are `MERGED` with the merge commits listed above.
- [Full CI 39397265757](https://github.com/BigStrongSun/ccswitchmulti/actions/runs/39397265757) completed with all six jobs `success`: Change Scope, Rust Formatting, Frontend Checks and Backend Checks for Windows, Ubuntu and macOS. All three backend jobs actually executed strict Clippy and Cargo tests after successful frontend/formatting prerequisites; no backend check was skipped or replaced by a docs gate in this run.
- Five superseded PR closures and all six dependency reviews exist on GitHub; #122 and #112 are `CHANGES_REQUESTED`, and only those six dependency PRs remain open at verification time.
- This follow-up changes only root Markdown memory files after the full run has completed, exercising the docs-only path without cancelling full validation. Expected: frontend/rustfmt skipped, the same three backend names succeed using Ubuntu result gates, and all backend checkout/toolchain/cache/compile/test steps skipped. The actual result and run link are maintained in [PR #119 verification comment](https://github.com/BigStrongSun/ccswitchmulti/pull/119#issuecomment-6287652955), avoiding a recursive documentation commit solely to record its own CI ID.
- Public source/CI state and unchanged Release/installation state remain separate. Release `v3.20.4-8` still points to `7f48b3ef771e115913662da4b6cdf9fbe6e3a9d4`; these four PRs are source-main improvements, not included in that existing package.
- Search: built-in web checked public repository/official GitHub Actions documentation; independent Matrix search was discovered and attempted but returned `fetch failed`. Current `git`/`gh` source, PR and CI evidence was used to resolve stale indexing; no claim of successful Matrix corroboration.

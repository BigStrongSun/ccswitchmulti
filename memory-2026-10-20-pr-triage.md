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
- The attempted docs-only verification did **not** produce a docs-only push: concurrent commits advanced the shared integration branch while the first CI was running. The classifier correctly selected full CI for the accumulated source diff. This is not evidence of a classifier defect; docs-only remote behavior remains unverified. Corrected status is maintained in [PR #119 verification comment](https://github.com/BigStrongSun/ccswitchmulti/pull/119#issuecomment-6287652955).
- Release `v3.20.4-8` still points to `7f48b3ef771e115913662da4b6cdf9fbe6e3a9d4`, verified with `git ls-remote`; these four PRs are source-main improvements, not included in that existing package. This task did not install or restart an application. Other concurrent tasks' installation state is not established by this PR task.
- Search: built-in web checked public repository/official GitHub Actions documentation; independent Matrix search was discovered and attempted but returned `fetch failed`. Current `git`/`gh` source, PR and CI evidence was used to resolve stale indexing; no claim of successful Matrix corroboration.

## Concurrent Push Scope Incident

- While this task waited for full CI, another task cherry-picked `bc8ae9274` (transport retry ownership) and committed `5d5f0209c` (local `3.20.4-9` candidate metadata) onto the same integration branch. Reflog timestamps are 17:03:30 and 17:04:05 +08:00, after this task's first push of `844ca809d` at 16:58.
- At 17:17 this task committed only its two memory files as `7ea5663ef`, but pushed `HEAD` without rechecking the ancestry against the last verified remote. That accidentally published both intervening commits. Their own project note explicitly scoped them to a local candidate without public push.
- Remote main was verified at `7ea5663ef81942bb7121a555b7c454d23bec7299`; [CI 39399660268](https://github.com/BigStrongSun/ccswitchmulti/actions/runs/39399660268) runs full validation for that expanded tree. The successful `844ca809d` CI cannot be represented as full validation of `7ea5663ef`.
- Immediately stopped further pushes and asked the user whether to withdraw the two local-only commits from public main while preserving their local candidate and the four PR merges. No force push, revert, reset, candidate removal or source overwrite has been performed. Resolution is pending user direction; this correction is local documentation only until that scope is settled.
- Future push boundary: verify **the entire remote-tip-to-intended-tip commit range and tree diff immediately before every push**, not only staged files, then push the reviewed exact SHA instead of movable `HEAD`. An existing integration worktree is not exclusive merely because it was clean at task start. Source validation belongs to its exact SHA.

## Authorized Retention And Acceptance Follow-Up

- User explicitly resolved the scope question: the two commits are already on GitHub, keep them if acceptance succeeds. No withdrawal, revert or force push is requested. The earlier pending-direction statement above records the incident at that time, not the current instruction.
- [CI 39399660268](https://github.com/BigStrongSun/ccswitchmulti/actions/runs/39399660268) completed `success` for source `7ea5663ef81942bb7121a555b7c454d23bec7299`, all six jobs including actual frontend and Windows/Ubuntu/macOS strict Clippy/Cargo tests. This validates the original transport fix plus the four PR merges, not a future source fix.
- Focused integrated review found a real omission: `bc8ae9274` labeled `ForwardFailed` only in generic `ProxyError::into_response`. Codex Responses/image and Chat handlers construct their own error JSON and bypassed that metadata, still returning `cc_switch_forward_failed` without explicit origin/status fields. Therefore the diagnostic contract was incomplete on the principal Codex path, despite existing tests passing.
- Root fix: `ProxyError::apply_local_transport_error_metadata` and one shared canonical code are reused by generic, Codex and Chat response builders. Only local `ForwardFailed` receives `code=ccsm_upstream_transport_error`, `source=ccsm`, explicitly present `upstream_status=null`; HTTP 502, detailed messages, provider/model/endpoint and protocol-specific envelopes remain. Actual upstream errors retain their own status/code/message. `ResponsePending` remains 424, nonretryable, with no Retry-After; transport retry ownership itself is unchanged.
- Regression RED: `cargo test --lib transport_502 -- --test-threads=1` passed the generic case and failed both new Codex/Chat cases on the old code (`cc_switch_forward_failed` versus canonical code). Production was modified only after this result.
- GREEN on the updated tree: Codex-filtered library 1658 passed/3 ignored; generic error module 3/3; image route module 13/13. New tests exercise actual response builders for text/image/Chat, assert null-field presence, preserve real upstream 502 code/message/status, and protect 424/no-Retry-After behavior. An interrupted first GREEN invocation had no recoverable completion evidence and was rerun; it is not counted as a separate success.
- Independent Luna diff review of the completed fix found no blocker. Chat keeps provider/model/endpoint in its existing message format; Codex/image tests assert their structured context. No paid upstream image call, installed-app UI acceptance, candidate rebuild or installation is claimed.
- Frontend source/dependencies are unchanged by the follow-up, so the successful 1702-test frontend CI evidence remains reusable; the next pushed source tree will still run the repository's complete CI. Local production `cargo clippy -- -D warnings`, rustfmt, diff whitespace and strict UTF-8/no BOM/no U+FFFD checks pass; final remote outcome follows this source commit.

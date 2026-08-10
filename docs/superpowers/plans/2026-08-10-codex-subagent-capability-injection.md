# Codex Sub-Agent V2 Capability Injection Implementation Plan

Target: `3.19.1-19`. Scope is local commits, build, installation, and live acceptance only: no push, PR, or GitHub release.

## Global constraints

Preserve V1 direct override behavior, official provider classification, V2 body projection, reserved schema, mixed `agents` namespace, `hide_spawn_agent_metadata=true`, and Qwen behavior. Do not change the parent model, Codex spawn schema, or calls. Treat user-authored role files as immutable. Keep diagnostics free of credentials, task text, and encrypted contents. Each RED/GREEN or debugging change is separately committed with a detailed Chinese message ending in a final paragraph exactly `本次提交由BigStrongsSun完成`.

## 1. Documents

- [ ] Add the approved design and this executable plan. Cross-check every public contract: persisted `subagentV2`, compiler ownership, provider classification, field restoration, V1/V2 lifecycle, role collision/nickname rules, UI, diagnostics, presets, and transaction-install boundary.
- [ ] Verify no incomplete marker, incompatible policy, or unqualified model-name provider inference remains.
- [ ] Commit only the two documents for this task.

## 2. Backend RED

- [ ] Add failing Rust tests for schema parsing/defaulting, one-to-five unique `taskStrengths` membership, the fixed trim/NFKC/default-case-fold profile-key algorithm, collision validation with no generated roles, profile preservation over catalog refresh, backend-only compilation, auto effort, policy semantics, override deletion, nickname/role normalization, collision handling, unroutable non-generation, legacy initialization, and diagnostics redaction.
- [ ] Cover V1 activation preserving inactive V2 data and V2 activation only materializing enabled/routable profiles.

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml codex_subagent_v2 -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml codex_managed_agent -- --nocapture
```

Expected RED result: each test fails for the missing capability-injection contract rather than unrelated compilation/environment errors. Commit only the tests.

## 3. Backend GREEN

- [ ] Implement typed persistence and a single backend compiler/preview command. Implement the exact effort truth table: any complex/debug/architecture/high-risk strength is `high`; speed plus only read/explore/evidence/summarize strengths is `low`; otherwise `medium`; explicit effort wins.
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

- [ ] Add regression tests for legacy configurations without `subagentV2`, mode switching, catalog refresh, disabled drafts, NFKC/default-case-fold profile-key collisions, canonical `profile.model` preservation across catalog aliases, user-role collisions, V1's first-five direct overrides, built-in role protection, diagnostics redaction, and unchanged Qwen behavior.
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

- [ ] Build with `pnpm tauri build`, then export only to `C:\Users\sunda\Documents\LLMservice\CCSwitchMulti-3.19.1-19-local`; create the directory if needed and fail unless it contains exactly one NSIS installer. Record absolute path, size, file/product version, SHA-256, and signature in `memory.md`.
- [ ] Install only through one independent hidden PowerShell process. That single transaction must preflight and back up, verify target PID, kill/wait and release the port, uninstall, install, hidden-relaunch, validate health/version/hash/routing, and roll back on any failure.
- [ ] Never stop CCSM alone from the interactive shell; do not stop Codex Desktop while the current task depends on it.
- [ ] Validate in the installed UI: shared editor in wizard/workspace, policy/profile persistence, status, preview, V1 preserved direct overrides, and V2 generated roles.
- [ ] Use new sessions for live canaries. Verify a no-model-name Flash task and Pro task select the expected enabled roles, use real read-only tools and follow-up, carry the expected model/provider/version in rollout data, and each upstream route returns HTTP 200. Run a no-model-name official integration/release path as the provider-policy control. Hand-written `model=` calls are supplementary only.

Run:

```powershell
pnpm tauri build
$exportDir = 'C:\Users\sunda\Documents\LLMservice\CCSwitchMulti-3.19.1-19-local'
New-Item -ItemType Directory -Force -Path $exportDir | Out-Null
& .\scripts\export-latest-ccswitchmulti.ps1 -ReleaseRoot $exportDir -SkipBuild
$installers = @(Get-ChildItem -LiteralPath (Join-Path $exportDir 'windows\installer') -File -Filter 'CCSwitchMulti_3.19.1-19_x64-setup.exe')
if ($installers.Count -ne 1) { throw "Expected exactly one NSIS installer; found $($installers.Count)" }
$installerPath = $installers[0].FullName
$installerHash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
$artifactExe = Get-ChildItem -LiteralPath (Join-Path $exportDir 'windows\raw-exe') -File -Filter 'CCSwitchMulti_3.19.1-19_x64.exe' | Select-Object -First 1
if (-not $artifactExe) { throw 'Export did not contain the versioned raw executable for installed-hash verification' }
$installedHash = (Get-FileHash -LiteralPath $artifactExe.FullName -Algorithm SHA256).Hash
```

Run the script's existing Pester suite before installing, then start exactly one independent hidden transaction process; all named parameters below are the script's real parameters:

```powershell
Invoke-Pester .\scripts\tests\install-ccswitchmulti-transaction.Tests.ps1 -Output Detailed
$installedExecutable = 'C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe'
$installDirectory = Split-Path -Parent $installedExecutable
$uninstallExecutable = Join-Path $installDirectory 'uninstall.exe'
$ccsmPid = (Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1 -ExpandProperty OwningProcess)
if (-not $ccsmPid) { throw 'No CCSM listener owns port 15721' }
$currentHash = (Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash
$currentVersion = (Get-Item -LiteralPath $installedExecutable).VersionInfo.FileVersion
$backupRoot = 'C:\Users\sunda\.cc-switch\backups\ccsm-3.19.1-19-transaction'
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
$transactionLog = Join-Path $backupRoot 'transaction-result.json'
$transactionArgs = @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Resolve-Path .\scripts\install-ccswitchmulti-transaction.ps1),
  '-InstallerPath',$installerPath,'-ExpectedInstallerHash',$installerHash,
  '-ExpectedCurrentVersion',$currentVersion,'-ExpectedCurrentHash',$currentHash,
  '-ExpectedInstalledVersion','3.19.1-19','-ExpectedInstalledHash',$installedHash,
  '-CurrentPid',$ccsmPid,'-InstalledExecutable',$installedExecutable,'-InstallDirectory',$installDirectory,
  '-UninstallExecutable',$uninstallExecutable,'-ConfigPath',(Join-Path $env:USERPROFILE '.cc-switch'),
  '-RegistryKey','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti',
  '-Port','15721','-HealthUri','http://127.0.0.1:15721/health','-TimeoutSeconds','90','-BackupRoot',$backupRoot)
$transaction = Start-Process powershell.exe -WindowStyle Hidden -PassThru -Wait -ArgumentList $transactionArgs -RedirectStandardOutput $transactionLog -RedirectStandardError "$transactionLog.stderr"
if (-not (Test-Path -LiteralPath $transactionLog)) { throw 'Transaction did not create result JSON' }
try { $result = Get-Content -Raw $transactionLog | ConvertFrom-Json -ErrorAction Stop } catch { throw "Transaction result JSON is invalid: $($_.Exception.Message)" }
switch ($result.Status) {
  'Success' {
    if ($transaction.ExitCode -ne 0 -or $result.Error -or $result.RollbackError) { throw "Success result/exit mismatch: $($result | ConvertTo-Json -Compress)" }
  }
  'RolledBack' {
    if ($transaction.ExitCode -eq 0 -or -not $result.Error -or $result.RollbackError) { throw "Invalid rollback result: $($result | ConvertTo-Json -Compress)" }
    $rollbackOwner = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1 -ExpandProperty OwningProcess
    if (-not $rollbackOwner -or -not (Test-Path -LiteralPath $installedExecutable)) { throw 'Rollback did not restore a listening installed runtime' }
    if ((Invoke-WebRequest -UseBasicParsing http://127.0.0.1:15721/health).StatusCode -ne 200) { throw 'Rollback health is not HTTP 200' }
    if ((Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash -ne $currentHash) { throw 'Rollback restored hash mismatch' }
    if ((Get-Item -LiteralPath $installedExecutable).VersionInfo.FileVersion -ne $currentVersion) { throw 'Rollback restored version mismatch' }
    Write-Warning "Transaction safely rolled back; installation is incomplete. TransactionId=$($result.TransactionId)"
    throw 'Safe rollback: do not treat this task as installed or accepted'
  }
  'RollbackFailed' { throw "Rollback failed: $($result | ConvertTo-Json -Compress)" }
  default { throw "Unsupported transaction status: $($result.Status)" }
}
```

The transaction receives the expected installed hash from the versioned raw executable resolved in the first block, not from the NSIS installer hash.

After a successful transaction, inspect the structured result/log and prove the installed runtime:

```powershell
$result | Format-List Status,TransactionId,BackupPath,NewPid,Error,RollbackError
Get-Content -LiteralPath (Join-Path $result.BackupPath 'transaction.jsonl')
$version = (Get-Item -LiteralPath $installedExecutable).VersionInfo
if ($version.FileVersion -ne '3.19.1-19' -or $version.ProductVersion -ne '3.19.1-19') { throw 'Installed PE version mismatch' }
$owner = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1 -ExpandProperty OwningProcess
if ($owner -ne $result.NewPid) { throw "Unexpected 15721 owner: $owner" }
if ((Invoke-WebRequest -UseBasicParsing http://127.0.0.1:15721/health).StatusCode -ne 200) { throw 'Health is not HTTP 200' }
if ((Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash -ne $installedHash) { throw 'Installed hash mismatch' }
```

Rollback acceptance is explicit: a failed transaction is accepted only when its JSON result has `Status='RolledBack'`, a nonempty `Error`, empty `RollbackError`, and `transaction.jsonl` includes `rollback-success`; `RollbackFailed` is a release blocker. Do not run an interactive-shell stop/kill of CCSM before or during this procedure.

No dedicated capability-injection canary script exists. Start a fresh interactive Codex CLI session with `codex --cd C:\Users\sunda\Documents\LLMservice\cc-switch`, then enter each exact prompt without naming a model:

```text
Delegate one read-only subagent to explore the repository and collect evidence about the Codex routing implementation. Run git rev-parse --short HEAD and one targeted rg query, report both results, then wait for the child.
```

```text
Send a follow-up to the same child: run git branch --show-current and return only the branch name.
```

```text
Delegate one subagent to diagnose a complex cross-module routing ownership issue, propose an architecture decision, and perform no writes. Use targeted source reads and report evidence.
```

```text
Send a follow-up to the same child: re-check one cited source location and report whether the conclusion still holds.
```

```text
For the final integration/release decision, use an official subagent path only. Review the release-readiness evidence without writes, cite one test and one artifact fact, then wait for the child.
```

For each parent/child created after the test timestamp, query local rollout and router evidence without reading task contents or credentials:

```powershell
$since = (Get-Date).AddMinutes(-30)
$rollouts = Get-ChildItem "$env:USERPROFILE\.codex\sessions" -Recurse -Filter '*.jsonl' | Where-Object LastWriteTime -gt $since
$rollouts | Select-Object FullName,LastWriteTime,Length
rg -n 'agent_role|model_provider|model_reasoning_effort|multi_agent_version|function_call|function_call_output' $rollouts.FullName
rg -n 'route_resolved|request_prepared|upstream_status.*status=200|responses_to_chat' "$env:USERPROFILE\.cc-switch\logs\codex-router.log"
```

Acceptance: Flash prompt yields the configured Flash role/model/provider with read-only tool calls and follow-up; Pro yields the configured Pro role/model/provider with read-only tool calls and follow-up; the official control stays official for integration/release. All three evidence sets show role, model, provider, V2 version, real tool activity, and the correct router route with HTTP 200. Do not print prompt text, encrypted content, cookies, tokens, or credentials in the recorded evidence.

```powershell
git status --short
```

Acceptance: the transaction proves rollback safety and installed `3.19.1-19`; live acceptance proves actual generated-role behavior rather than source/config presence. Record exact IDs, hashes, versions, health evidence, limitations, and final status in `memory.md`.

## Research evidence

Official [Subagents documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) establishes custom-role selection descriptions, role model/reasoning precedence over spawn/default/parent resolution, and local delegation triggers. The official [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) is the config-key authority. Local official source confirmation is `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/agent/role.rs` and `C:/Users/sunda/Documents/LLMservice/codex-official/codex-rs/core/src/tools/spec_plan.rs`.

Matrix WebSearch independently searched on 2026-08-10 and found no equivalent official first-party result, but directly fetched both official pages successfully. Use official docs/source and local runtime evidence for primary conclusions; retain that Matrix search-discovery limitation as an uncertainty.

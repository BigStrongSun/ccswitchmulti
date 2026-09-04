# Windows v3.19.2-29 Build, Install and Release Plan

**Goal:** Build the protocol-concurrency and wizard-cache fixes into a Windows installer, verify the installed application, then publish GitHub Release.

**Architecture:** Keep candidate identity pinned, use the existing NSIS export/signing pipeline, and use the recovery-capable installation transaction only during an approved interruption window. GitHub publication is gated on installed-runtime acceptance, not merely unit tests.

**Tech Stack:** Rust/Tauri, React/TypeScript, NSIS, PowerShell, GitHub Actions.

**Spec:** User requested local Windows installer build, installation and testing, followed by GitHub Release only after passing.

## Constraints

- Preserve unrelated untracked files; no secrets in logs or searches.
- Current Codex routes through installed CCSM port 15721. Before interrupting that host dependency, explain the impact and obtain a maintenance-window confirmation.
- No push/tag/release before installed tests pass. No forced tag replacement.
- Keep UTF-8 without BOM and local commits with the requested attribution footer.

## Tasks

- [x] Inspect current source, installed process, GitHub latest and official Tauri NSIS documentation using independent built-in and Matrix searches.
- [x] Bump package.json, Cargo.toml, Cargo.lock and tauri.conf.json to 3.19.2-29 and commit candidate identity: 18b6a52e.
- [x] Run frontend tests/typecheck/format, Rust tests/check/format and installation-transaction tests: 1357 frontend, 3813 Rust passed / 6 ignored, 71 Pester, all static gates passed.
- [x] Build initial signed NSIS installer using scripts/export-latest-ccswitchmulti.ps1; verify artifact version, SHA-256 and expected NSIS embedded executable hash.
- [x] After user approval, run scripts/install-ccswitchmulti-transaction.ps1 detached; second transaction succeeded (first returned RollbackFailed, see memory.md).
- [x] Rebuild and install the save-refetch fix plus evidence-backed adaptation summary (a875d3e9); the initial v29 candidate does not contain them.
- [ ] Verify installed executable hash/version, registry identity, listener health, and wizard close/reopen plus failed-model exclusion using the installed UI. Test concurrency with controlled endpoints where practical; report any real-upstream coverage limits.
- [ ] Record acceptance in memory.md and release notes, commit, create an annotated v3.19.2-29 tag and push to fork only after all prior gates pass.
- [ ] Check platform workflows, published release, latest.json signatures and remote asset digests. Report source, install and remote release states separately.

## Build checkpoint

- Started `pwsh -NoProfile -File scripts/export-latest-ccswitchmulti.ps1 -ReleaseRoot C:/Users/sunda/Documents/LLMservice/ccswitchmulti-v3.19.2-29-candidate` at 2026-09-04 11:53 local time, execution session 61355.
- Initial build finished successfully. Installer SHA256: B9D42CFD49C61E3D54F534FBAA4135EA8B541148B2DBC41EDCA478BF04D14623.
- Second approved installation transaction succeeded. Current installed v29 PID 52588, executable SHA256 FA098572082EAA2CB680A224040DA141B712FEA2DDA9EF0892D6D999668B398E, health healthy at 12:34. User stopped UI automation with Esc; do not continue UI operations.
- Release paused after user reported save-page probe gate regression. Source regression reproduces normalized save snapshots being mistaken for external edits. Fix passes 21 wizard and 1357 frontend tests, typecheck and renderer build, but is not yet installed. No push or tag creation performed.

## Retest checkpoint (13:24 onward)

- a875d3e9 adds default-visible adaptation explanations and redacted actual-retry progress. Gates: frontend 1361, Rust 3814 passed /6 ignored, typecheck, cargo check --tests, formatting, strict UTF-8; system Windows PowerShell Pester 71/71. Bundled pwsh Pester failures remain an environment limitation, not a passing gate.
- Rebuilt to `C:/Users/sunda/Documents/LLMservice/ccswitchmulti-v3.19.2-29-retest`; installer SHA256 3813EE3474B96759EBB7BBF590196C0A487DF66C153C461330BF9A589504F45C.
- Detached transaction `ccsm-20260904-132351-78769011171548d7896db005c4800fb7` succeeded. Fresh installed PID46100, version3.19.2-29, hash A306F539AF18DFB01A39DD0E9BF3F76831260F485336B23D1ACDD0D8F3F64BFC and health matched.
- Browser fixture rendered real components and explanations. Native desktop acceptance is incomplete: accessibility state and screenshots disagreed and clicks were offset. Stopped further UI input without saving QA providers or switching the current route. GitHub publication remains gated on reliable installed interaction acceptance.

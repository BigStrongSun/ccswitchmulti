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
- [ ] Build signed NSIS installer using scripts/export-latest-ccswitchmulti.ps1 into a new version-specific directory; verify artifact version, SHA-256 and expected NSIS embedded executable hash.
- [ ] After maintenance-window confirmation, run scripts/install-ccswitchmulti-transaction.ps1 detached with exact PID/path/hash/version, config backup, health endpoint and rollback.
- [ ] Verify installed executable hash/version, registry identity, listener health, and wizard close/reopen plus failed-model exclusion using the installed UI. Test concurrency with controlled endpoints where practical; report any real-upstream coverage limits.
- [ ] Record acceptance in memory.md and release notes, commit, create an annotated v3.19.2-29 tag and push to fork only after all prior gates pass.
- [ ] Check platform workflows, published release, latest.json signatures and remote asset digests. Report source, install and remote release states separately.

## Build checkpoint

- Started `pwsh -NoProfile -File scripts/export-latest-ccswitchmulti.ps1 -ReleaseRoot C:/Users/sunda/Documents/LLMservice/ccswitchmulti-v3.19.2-29-candidate` at 2026-09-04 11:53 local time, execution session 61355.
- History-repair sidecar release compilation finished in 7m39s; renderer production build passed. Main executable release compilation remains in progress at this checkpoint; an installer has not yet been verified.
- No install, restart, push or tag creation has been performed. Maintenance-window confirmation is required before interrupting this session's CCSM dependency.

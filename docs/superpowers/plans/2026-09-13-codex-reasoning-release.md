# CCSwitchMulti 3.20.2-9 Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and verify a local Windows x64 EXE from the repaired `main` branch, record the `3.20.2-9` release metadata on `main`, and publish a new GitHub release without interrupting the currently running CCSM service.

**Architecture:** The release source is the clean tracked `main` tree at the reasoning-probe binding fix. Version metadata is synchronized across npm, Cargo, Tauri, lockfile, and bilingual release notes. The local release pipeline builds in an isolated Cargo target and exports traceable artifacts; GitHub publication uses a new immutable tag and the repository release workflow for cross-platform assets.

**Tech Stack:** Rust/Cargo, Tauri 2.10.1, pnpm, PowerShell release pipeline, Git tags, GitHub Actions/CLI.

**Spec:** User request in the active Codex task: publish a new release after the Desktop reasoning probe-binding fix, first build a local EXE from the latest `main` branch currently at `3.20.2-8`.

## Global Constraints

- Build from `main` at the repaired commit; do not use an older `v3.20.2-8` tag as the release source.
- Bump the release to `3.20.2-9` consistently in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.
- Preserve unrelated untracked `.tmp/` and `docs/provider-settings-layout-preview.html` files.
- Do not kill, replace, or restart the current CCSM process while building or verifying the local artifact.
- Do not claim a release is published until the remote tag, GitHub release, assets, and workflow state are independently verified.

### Task 1: Freeze release source and metadata

**Files:**
- Modify: `package.json:3`
- Modify: `src-tauri/Cargo.toml:3`
- Modify: `src-tauri/Cargo.lock:770`
- Modify: `src-tauri/tauri.conf.json:4`
- Create: `docs/release-notes/v3.20.2-9-zh.md`
- Create: `docs/release-notes/v3.20.2-9-en.md`

- [ ] Confirm `main` is clean for tracked files and HEAD is the reasoning fix commit.
- [ ] Change all four version fields from `3.20.2-8` to `3.20.2-9`.
- [ ] Write bilingual notes describing the generic Desktop raw-reasoning lifecycle mapping and Responses-probe fail-closed binding.
- [ ] Run JSON/TOML parsing, `cargo fmt --check`, and `git diff --check`.
- [ ] Commit the release metadata on `main` with the required BigStrongsSun attribution.

### Task 2: Build and export the local Windows artifact

**Files:**
- Use: `scripts/local-release-pipeline.ps1`
- Use: `scripts/export-latest-ccswitchmulti.ps1`
- Output: the repository-configured local release root resolved by `Resolve-CcswitchmultiReleaseRoot`

- [ ] Run the local release pipeline from the committed `main` source with typecheck enabled and reason `codex-desktop-reasoning-3.20.2-9`.
- [ ] Verify the exported Windows raw EXE and NSIS/portable artifacts exist, have the expected versioned names, and include `RELEASE-METADATA.md` and `SHA256SUMS.txt`.
- [ ] Recompute and record SHA-256 values; inspect PE metadata/version and verify the executable is traceable to the release commit.
- [ ] Run a side-by-side health/startup check that does not bind the production port or alter the running CCSM installation.

### Task 3: Publish and verify the release

**Files:**
- Create: Git tag `v3.20.2-9`
- Use: `.github/workflows/release.yml`

- [ ] Check remote authentication and confirm `v3.20.2-9` does not already exist.
- [ ] Push the release commit and annotated tag to the configured release remote only after local artifact verification.
- [ ] Observe the release workflow until build, publish, and `latest.json` assembly finish; retry reads without mutating unrelated state if GitHub APIs transiently fail.
- [ ] Verify the published release tag, asset names, signatures, checksums, and workflow conclusion independently.
- [ ] Record the source commit, local EXE hash, remote tag SHA, release URL, and any platform caveats in a release audit note committed to `main`.

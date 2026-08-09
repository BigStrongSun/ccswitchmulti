# Codex Sub-Agent V1/V2 Live Hardening Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Fix the two defects discovered during installed V2 acceptance, rebuild and transactionally install a new CCSwitchMulti version, then complete fresh V1/V2 and UI acceptance.

**Root causes already proven:** The takeover paths project a live `{auth, config}` snapshot and only restore `modelCatalog`; they omit the provider-owned `codexRouting`, so the catalog transport policy cannot stamp one consistent multi-agent protocol version. Separately, the generated DeepSeek role instructions do not constrain Windows command selection or search scope, which caused broad recursive scans, invalid PowerShell commands, and excessive context use during the Pro canary.

## Global Constraints

- Fix the shared projection boundary; do not patch individual takeover callers or individual models.
- Merge only projection-owned provider metadata (`modelCatalog` and `codexRouting`) into the live snapshot; preserve live auth/config precedence.
- Keep V1 and V2 behavior, existing user roles, and `spawnAgentModels` intact.
- Managed role instructions must prefer Windows PowerShell and `rg`/`rg --files`, avoid broad recursive scans of `node_modules`/`target`/user directories, avoid unjustified escalation on read-only work, and stop once evidence is sufficient.
- Qwen behavior remains out of scope.
- Every RED and GREEN stage is a separate local commit whose final message line is `本次提交由BigStrongsSun完成`.
- Do not stop the currently running CCSM except inside a pre-launched transaction that independently performs kill, wait, uninstall, install, relaunch, health/version/hash verification, and rollback on failure.
- Do not stop Codex Desktop. Do not push, create a PR, or publish a GitHub Release.

### Task 1: Preserve Codex Routing During Takeover Projection

**Files:**
- Modify: `src-tauri/src/services/proxy.rs`
- Test: inline tests in `src-tauri/src/services/proxy.rs`

- [ ] Add a regression test whose provider settings contain enabled V2 `codexRouting` and a model catalog while the live snapshot contains only auth/config. Prove the projection input retains live auth/config and receives provider `modelCatalog` plus `codexRouting`.
- [ ] Run the focused test and verify it fails for the missing routing metadata. Commit RED.
- [ ] Fix the shared `codex_settings_for_model_catalog_projection` boundary to merge only projection-owned metadata with live values taking precedence where already present.
- [ ] Run the focused test, neighboring takeover/catalog tests, the catalog-wide V2 transport-policy test, Rust formatting, and `cargo check --lib`. Commit GREEN.

### Task 2: Make Managed DeepSeek Roles Efficient On Windows

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Test: inline managed-role tests in `src-tauri/src/codex_config.rs`

- [ ] Add failing behavior tests for the generated Flash and Pro role instructions, covering targeted PowerShell/`rg` use, excluded heavy directories, no escalation metadata for ordinary read-only work, and early completion after sufficient evidence. Commit RED.
- [ ] Add concise shared Windows execution guidance while retaining mutually exclusive Flash/Pro task descriptions and model-specific reasoning defaults. Commit GREEN after focused tests and Rust checks.

### Task 3: Build And Transactionally Install A New Version

**Files:**
- Modify: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`
- Modify: repository transaction/install script if required
- Modify: `memory.md`

- [ ] Run full frontend and Rust regression checks.
- [ ] Bump all version sources from `3.19.1-15` to `3.19.1-16`, commit, build the Windows installer/updater artifacts, and record hashes/signatures.
- [ ] Pre-launch a self-contained transaction that backs up the installed binary/config state, kills only the recorded CCSM process, waits for port 15721 to release, uninstalls, installs, relaunches hidden, waits for the port, verifies health/version/hash, and automatically restores service on failure.

### Task 4: Fresh Installed Acceptance And Audit

**Files:**
- Modify: `memory.md`
- Runtime: installed CCSM, SQLite provider state, Codex config/catalog/roles, new Codex sessions, router logs, installed UI

- [ ] Verify takeover projection stamps one selected multi-agent version across every catalog model.
- [ ] Run a new automatic V2 Flash canary and one follow-up without naming a model.
- [ ] Run a new automatic V2 Pro canary and one follow-up without naming a model; verify the Windows guidance materially prevents the prior broad-scan/tool-error pattern.
- [ ] Switch to V1 in the installed UI, save/refresh/restart, verify preserved direct overrides, and run a direct-override V1 canary.
- [ ] Switch back to V2, verify persistence and both managed roles, audit source/tests/artifacts/runtime evidence, update `memory.md`, commit evidence, and leave the worktree clean.

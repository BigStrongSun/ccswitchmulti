# Updater Check Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every routine release check use the same proxy-aware backend updater as download and installation.

**Architecture:** Rust owns updater networking and returns serializable release metadata through one Tauri command. TypeScript maps that command result into the existing UpdateContext contract and does not construct a second updater client.

**Tech Stack:** Rust, Tauri 2.10 updater, TypeScript, Vitest.

## Global Constraints

- Preserve Tauri NSIS `/UPDATE` as the normal Windows update strategy.
- Keep full transaction uninstall/reinstall as a recovery-only path.
- Do not move, copy, or restore live SQLite files during a normal in-app update.
- Do not stage pre-existing changes in `memory.md` or `src-tauri/src/codex_config.rs`.

---

### Task 1: Backend release metadata contract

**Files:**
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `updater_builder_with_runtime_proxy(&AppHandle)`.
- Produces: command `check_app_update() -> Result<Option<AppUpdateInfo>, String>` with camelCase fields `currentVersion`, `availableVersion`, `notes`, and `pubDate`.

- [ ] **Step 1: Write a failing Rust unit test**

Add a pure mapping function test that constructs literal current/release metadata and asserts every serialized UI field, including absent notes and date.

- [ ] **Step 2: Run the focused Rust test and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml check_app_update_info --lib`

Expected: compilation failure because `AppUpdateInfo` and its mapping function do not exist.

- [ ] **Step 3: Implement the minimal backend command**

Create the serializable DTO and pure mapping function, use `updater_builder_with_runtime_proxy` for `check()`, and register `check_app_update` in the invoke handler.

- [ ] **Step 4: Run the focused Rust test and verify GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml check_app_update_info --lib`

Expected: PASS.

### Task 2: Frontend uses the backend check

**Files:**
- Create: `src/lib/updater.test.ts`
- Modify: `src/lib/updater.ts`

**Interfaces:**
- Consumes: Tauri invoke command `check_app_update`.
- Produces: the existing `checkForUpdate()` return union used by `UpdateContext`.

- [ ] **Step 1: Write a failing frontend unit test**

Mock only the external Tauri IPC boundary. Return a complete literal backend payload and assert the real `checkForUpdate()` result contains the expected current version, available version, notes, and date. Add a second test where `null` becomes `up-to-date`.

- [ ] **Step 2: Run the focused Vitest file and verify RED**

Run: `pnpm vitest run src/lib/updater.test.ts`

Expected: FAIL because `checkForUpdate()` still calls the JavaScript updater plugin instead of IPC.

- [ ] **Step 3: Implement the minimal frontend change**

Replace `getVersion()` and `@tauri-apps/plugin-updater` use in `checkForUpdate()` with `invoke<AppUpdateInfo | null>("check_app_update")`. Preserve the public result contract and remove dead options that no longer affect behavior.

- [ ] **Step 4: Run focused frontend and Rust tests**

Run: `pnpm vitest run src/lib/updater.test.ts`

Run: `cargo test --manifest-path src-tauri/Cargo.toml check_app_update_info --lib`

Expected: both PASS.

### Task 3: Repository and installed-state verification

**Files:**
- Modify: `memory.md` using only a new task-specific hunk.

**Interfaces:**
- Consumes: completed backend/frontend updater chain.
- Produces: reproducible validation evidence and project memory entry.

- [ ] **Step 1: Run static and regression validation**

Run: `pnpm typecheck`

Run: `pnpm vitest run src/lib/updater.test.ts`

Run: `cargo test --manifest-path src-tauri/Cargo.toml check_app_update_info --lib`

Run: `pnpm build:renderer`

- [ ] **Step 2: Validate updater configuration and release assets**

Confirm local `tauri-plugin-updater` is `2.10.0`, updater endpoint targets CCSwitchMulti, and the current release manifest contains `windows-x86_64` with a matching signature.

- [ ] **Step 3: Record the root cause and platform boundaries**

Append a dated memory entry covering commit `0d693c8a`, the split updater clients, NSIS `/UPDATE`, macOS bundle replacement, Linux package/AppImage behavior, and SQLite shutdown ownership.

- [ ] **Step 4: Commit only task-owned files**

Stage the updater source, updater tests, plan/spec, and only the new memory hunk. Commit with a detailed message ending in `本次提交由BigStrongsSun完成`.

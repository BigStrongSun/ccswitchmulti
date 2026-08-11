# Codex MultiRouter Atomic Enable and macOS ChatGPT.app Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make first-time Codex MultiRouter activation atomic, surface activation failures to the wizard, and discover both legacy `Codex.app` and unified `ChatGPT.app` on macOS before publishing CCSwitchMulti `3.19.1-21`.

**Architecture:** The wizard will call only the existing provider-switch path, allowing `ProviderService::switch` to select the existing locked takeover transaction before takeover is active. Provider actions will return an explicit success/failure outcome so ordinary callers can retain toast behavior while the wizard can reject false success. macOS discovery will resolve a bundle by `com.openai.codex` and `Info.plist` metadata instead of deriving identity and executable name from `Codex.app`.

**Tech Stack:** React 18, TypeScript, TanStack Query, Vitest, Rust, Tauri 2, macOS `mdfind`/`plutil`, GitHub Actions, Tauri updater signatures.

## Global Constraints

- Preserve the existing Sub-Agent V1/V2 schema, route selection, transport behavior, database schema, user-created agent files, Windows MSIX discovery, and Linux AppImage discovery.
- Do not call `startProxyServer` or `setProxyTakeoverForApp` from the MultiRouter wizard activation path before switching the target provider.
- A failed provider switch must keep the wizard open and must not dispatch `ENABLE_SUCCESS`.
- Treat Desktop model-picker/CDP repair as best-effort after a verified takeover; it must not become an HTTP routing prerequisite.
- Use `com.openai.codex` and `CFBundleExecutable` as macOS authority while retaining old `Codex.app` compatibility.
- Do not overwrite the existing uncommitted changes in `memory.md` or `src-tauri/src/codex_config.rs`; stage only task-owned hunks/files.
- Every implementation/debug stage receives its own local commit, and every commit message ends with `本次提交由BigStrongsSun完成`.
- Release version is `3.19.1-21`; do not move or reuse an existing remote tag.

---

### Task 1: Return a Strict Provider Switch Outcome and Remove Pre-Takeover

**Files:**
- Create: `src/lib/codexMultiRouterEnable.ts`
- Create: `src/lib/codexMultiRouterEnable.test.ts`
- Modify: `src/hooks/useProviderActions.ts`
- Modify: `src/App.tsx`

**Interfaces:**
- Produces: `ProviderSwitchOutcome = { ok: true; result: SwitchResult } | { ok: false; error: Error }`.
- Produces: `enableCodexMultiRouterPlan(provider, switchProvider)` which resolves only after an `ok: true` outcome and throws the original outcome error otherwise.
- Consumes: the existing `switchProviderMutation.mutateAsync(provider.id)` and `ProviderService::switch` backend command.

- [ ] **Step 1: Write the failing unit test for strict activation**

```ts
it("switches only the target provider and returns its successful result", async () => {
  const switchProvider = vi.fn().mockResolvedValue({
    ok: true,
    result: { warnings: [] },
  });
  const provider = { id: "codex-multirouter" } as Provider;

  await enableCodexMultiRouterPlan(provider, switchProvider);

  expect(switchProvider).toHaveBeenCalledOnce();
  expect(switchProvider).toHaveBeenCalledWith(provider);
});

it("throws the original switch failure so the wizard cannot report success", async () => {
  const error = new Error("atomic takeover verification failed");
  const switchProvider = vi.fn().mockResolvedValue({ ok: false, error });

  await expect(
    enableCodexMultiRouterPlan({ id: "codex-multirouter" } as Provider, switchProvider),
  ).rejects.toBe(error);
});
```

- [ ] **Step 2: Run the new test and verify RED**

Run: `pnpm vitest run src/lib/codexMultiRouterEnable.test.ts`

Expected: FAIL because `codexMultiRouterEnable` and its exported activation function do not exist.

- [ ] **Step 3: Implement the outcome contract and strict helper**

Implement the helper as the only operation before returning success:

```ts
export async function enableCodexMultiRouterPlan(
  provider: Provider,
  switchProvider: (provider: Provider) => Promise<ProviderSwitchOutcome>,
): Promise<SwitchResult> {
  const outcome = await switchProvider(provider);
  if (!outcome.ok) throw outcome.error;
  return outcome.result;
}
```

Change `useProviderActions.switchProvider` to return `Promise<ProviderSwitchOutcome>`:

- return `{ ok: true, result }` after mutation, plugin sync, warnings, and success toast;
- normalize caught values with `error instanceof Error ? error : new Error(extractErrorMessage(error))` and return `{ ok: false, error }`;
- return `{ ok: false, error }` for the official-provider takeover guard after its existing toast.

Change `handleEnableCodexMultiRouterPlan` to call `enableCodexMultiRouterPlan(provider, switchProvider)` directly. Remove its calls to `proxyApi.startProxyServer()` and `proxyApi.setProxyTakeoverForApp("codex", true)`. Keep post-success query invalidations unchanged.

- [ ] **Step 4: Run focused frontend tests and typecheck**

Run:

```powershell
pnpm vitest run src/lib/codexMultiRouterEnable.test.ts src/components/codex/CodexMultiRouterWizard.test.tsx
pnpm typecheck
```

Expected: all focused tests pass and TypeScript exits 0.

- [ ] **Step 5: Commit Task 1**

Stage only the four Task 1 files and commit with a detailed body explaining that the wizard now reaches the backend's not-yet-taken-over atomic branch and that failures remain observable.

---

### Task 2: Resolve macOS Codex Bundles by Bundle Metadata

**Files:**
- Modify: `src-tauri/src/codex_desktop.rs`

**Interfaces:**
- Produces: a pure helper that accepts bundle path, bundle identifier, and executable name and returns a candidate only for `com.openai.codex`.
- Produces: a macOS-only metadata reader using `/usr/bin/plutil -extract ... raw` against `Contents/Info.plist`.
- Consumes: running application bundle paths, remembered executable paths, common candidates, and Spotlight results.

- [ ] **Step 1: Replace the old single-layout test with failing metadata tests**

Add tests equivalent to:

```rust
#[test]
fn macos_codex_bundle_metadata_accepts_legacy_and_unified_shells() {
    assert_eq!(
        macos_codex_bundle_executable_from_metadata(
            Path::new("/Applications/Codex.app"),
            "com.openai.codex",
            "Codex",
        ),
        Some(PathBuf::from("/Applications/Codex.app/Contents/MacOS/Codex")),
    );
    assert_eq!(
        macos_codex_bundle_executable_from_metadata(
            Path::new("/Applications/ChatGPT.app"),
            "com.openai.codex",
            "ChatGPT",
        ),
        Some(PathBuf::from("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT")),
    );
}

#[test]
fn macos_chatgpt_bundle_rejects_non_codex_identity() {
    assert_eq!(
        macos_codex_bundle_executable_from_metadata(
            Path::new("/Applications/ChatGPT.app"),
            "com.openai.chat",
            "ChatGPT",
        ),
        None,
    );
}
```

Also add a pure ancestor test proving both `Codex.app` and `ChatGPT.app` can be found while unrelated `.app` ancestors are rejected by the identity-aware runtime path.

- [ ] **Step 2: Run the Rust test filter and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_desktop::tests::macos_ -- --nocapture`

Expected: FAIL because the metadata helper is missing and the existing helper hardcodes `Codex`.

- [ ] **Step 3: Implement bundle-ID and Info.plist discovery**

Implement these behaviors:

- define `CODEX_DESKTOP_BUNDLE_IDENTIFIER: &str = "com.openai.codex"`;
- pure metadata validation rejects empty names, names containing path separators, and other bundle identifiers;
- macOS runtime reads `CFBundleIdentifier` and `CFBundleExecutable` with absolute `/usr/bin/plutil` and resolves `Contents/MacOS/<value>`;
- System Events queries the running application process by bundle identifier and returns its application bundle path;
- common candidates include `Codex.app` and `ChatGPT.app` in system and user Applications directories;
- Spotlight query prioritizes bundle identifier and retains `Codex.app` only as a legacy fallback;
- executable validation accepts the metadata-proven executable rather than a hardcoded filename;
- reverse bundle lookup recognizes either bundle name and revalidates metadata before returning it.

Do not accept an arbitrary standalone ChatGPT application merely because the directory or executable is named `ChatGPT`.

- [ ] **Step 4: Run focused and platform-neutral Rust tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_desktop::tests::macos_ -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_desktop::tests::platform_desktop_shell_is_distinct_from_cli_launcher -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_desktop::tests::linux_desktop_entry_exec_parser_keeps_absolute_exec_paths -- --nocapture
```

Expected: all selected tests pass.

- [ ] **Step 5: Commit Task 2**

Stage only `src-tauri/src/codex_desktop.rs` and commit with the macOS package migration, identity boundary, and legacy compatibility recorded in the body.

---

### Task 3: Apply Codex Post-Takeover Lifecycle to the Atomic Path

**Files:**
- Modify: `src-tauri/src/services/proxy.rs`

**Interfaces:**
- Produces: a single post-success helper used by both manual per-app takeover and locked provider-switch takeover.
- Consumes: `ensure_codex_guardian_started()` and `try_repair_codex_model_picker_after_takeover()`.

- [ ] **Step 1: Add a failing policy test**

Extract a pure policy function returning whether Codex Desktop post-takeover lifecycle is required and add:

```rust
#[test]
fn atomic_takeover_requests_desktop_repair_only_for_codex() {
    assert!(should_run_codex_post_takeover(&AppType::Codex));
    assert!(!should_run_codex_post_takeover(&AppType::Claude));
    assert!(!should_run_codex_post_takeover(&AppType::Gemini));
}
```

Add a regression assertion around the atomic success path showing that it calls the shared post-success helper after `verify_takeover_activation_after_write` and before returning `Ok(())`.

- [ ] **Step 2: Run the test and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib services::proxy::tests::atomic_takeover_requests_desktop_repair_only_for_codex -- --nocapture`

Expected: FAIL because the post-takeover policy/helper does not exist.

- [ ] **Step 3: Implement one best-effort post-success helper**

Add a helper that performs no action for non-Codex apps and, for Codex, calls guardian startup followed by model-picker repair. Invoke it:

- in the already-enabled valid takeover return path;
- after normal per-app takeover success;
- after locked atomic provider-switch takeover verification succeeds.

Do not call it before takeover verification and do not propagate model-picker repair warnings as takeover failures.

- [ ] **Step 4: Run proxy-focused tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib services::proxy::tests::atomic_takeover_requests_desktop_repair_only_for_codex -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib services::provider -- --nocapture
```

Expected: tests pass with no new failures.

- [ ] **Step 5: Commit Task 3**

Stage only `src-tauri/src/services/proxy.rs` and commit the lifecycle unification separately.

---

### Task 4: Full Verification, Project Memory, Version, and GitHub Release

**Files:**
- Modify only owned hunks: `memory.md`
- Modify: `package.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/tauri.conf.json`
- Create: `docs/release-notes/v3.19.1-21-zh.md`

**Interfaces:**
- Produces: version `3.19.1-21`, annotated tag `v3.19.1-21`, GitHub Release assets, signed updater manifest, and durable repository knowledge.
- Consumes: existing release workflow and the GitHub repository configured by `origin`.

- [ ] **Step 1: Run pre-release verification**

Run fresh:

```powershell
pnpm vitest run
pnpm typecheck
pnpm build:renderer
pnpm format:check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
git diff --check
```

If `format:check` reports only pre-existing unrelated files, record the exact files and additionally run Prettier check against every changed frontend file. Any relevant failure must be fixed before release.

- [ ] **Step 2: Record project knowledge without absorbing unrelated dirty hunks**

Append a dated `memory.md` entry covering:

- trigger matrix and why not every macOS user saw the issue;
- `v3.16.4-3` non-atomic wizard introduction and `v3.16.4-16` old-shell discovery introduction;
- the strict outcome contract and target-first atomic data flow;
- `com.openai.codex` plus `CFBundleExecutable` discovery boundary;
- exact tests and current Windows-only limitation for real Mac UI validation.

Use interactive staging so only this new memory hunk is committed; preserve the pre-existing two-line dirty change.

- [ ] **Step 3: Commit memory knowledge**

Commit only the newly added memory hunk with a detailed `docs(memory)` message ending in the required attribution.

- [ ] **Step 4: Prepare release `3.19.1-21`**

Update all four version authorities to exactly `3.19.1-21`. Write Chinese release notes describing user-visible fixes, root causes, validation, safe upgrade steps, database compatibility, and the remaining real-mac validation boundary. Confirm `git tag -l v3.19.1-21` and `git ls-remote --tags origin refs/tags/v3.19.1-21` are both empty before creating the tag.

- [ ] **Step 5: Commit the release candidate**

Stage only version authorities and `docs/release-notes/v3.19.1-21-zh.md`. Commit with a detailed release body ending in the required attribution.

- [ ] **Step 6: Re-run release-candidate gates**

Run fresh after the version commit:

```powershell
pnpm vitest run
pnpm typecheck
pnpm build:renderer
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml --lib
git diff --check HEAD~4..HEAD
```

Expected: all required gates pass; the worktree still contains only the preserved pre-existing dirty changes.

- [ ] **Step 7: Push branch and publish GitHub Release**

Create an annotated `v3.19.1-21` tag on the verified release commit, push the current branch and tag to `origin`, and monitor the repository's release workflow until all Windows, Linux, macOS, publish, and `latest.json` jobs complete successfully.

- [ ] **Step 8: Verify the remote release rather than trusting workflow status alone**

Use `gh release view v3.19.1-21 --json ...` and direct downloads to verify:

- release is public, non-draft, non-prerelease, and `releases/latest` points to it;
- expected Windows x64/ARM64, Linux x64/ARM64, and macOS assets exist;
- remote `latest.json` reports `3.19.1-21` and all six platform URLs target this tag;
- all six updater signatures are non-empty and match corresponding `.sig` assets;
- direct asset URLs return HTTP 200 and downloaded hashes match GitHub server digests where available.

- [ ] **Step 9: Commit final release evidence to project memory**

Append the exact tag object/peeled commit, Actions run ID, job results, asset count, manifest platform keys, signatures, hashes, and any remaining uncertainty to a new isolated `memory.md` hunk. Commit only that hunk with the required attribution, then push the branch without moving the release tag.

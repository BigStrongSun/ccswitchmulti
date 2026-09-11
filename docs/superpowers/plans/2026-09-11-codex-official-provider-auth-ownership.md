# Codex OpenAI Official Provider Auth Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `OpenAI Official` the single owner of Codex official authentication and allow Desktop, fixed CCSM OAuth, or the OAuth account pool with or without MultiRouter.

**Architecture:** Persist a secret-free `codexOfficialAuth` reference on the canonical official Provider. Resolve that reference at the effective-Provider boundary shared by direct official takeover and MultiRouter routes, while retaining legacy Router fields only as migration input. Move the visible control to the official Provider UI and keep account membership and quota policy in the authentication center.

**Tech Stack:** Rust, Serde, Tauri commands/services, React, TypeScript, TanStack Query, Vitest, Testing Library, Cargo test/Clippy.

**Spec:** `docs/superpowers/specs/2026-09-11-codex-official-provider-auth-ownership-design.md`

## Global Constraints

- Do not write OAuth tokens into a Provider, Router, log, error, test snapshot, or Codex `auth.json`.
- `codex-official` defaults to `desktop_current_login` when neither new Provider state nor compatible legacy Router state exists.
- Account-pool scheduling, reserve percentage, affinity, cooldown, and capacity-retry algorithms remain unchanged.
- Provider changes must affect direct official takeover and MultiRouter official routes without rewriting every route.
- Conflicting legacy Router policies must be surfaced and preserved until the user explicitly saves a Provider-level choice.
- During development run focused tests; run the affected consolidated gate once after implementation.

---

### Task 1: Define the Provider-Owned Authentication Contract

**Files:**
- Modify: `src-tauri/src/provider.rs`
- Modify: `src/types.ts`
- Create: `src/lib/codexOfficialAuth.ts`
- Create: `src/lib/codexOfficialAuth.test.ts`

**Interfaces:**
- Produces Rust `CodexOfficialAuthMode` and `CodexOfficialAuthConfig` with `Default` set to `DesktopCurrentLogin`.
- Produces `ProviderMeta.codex_official_auth: Option<CodexOfficialAuthConfig>` serialized as `codexOfficialAuth`.
- Produces TypeScript `CodexOfficialAuthMode`, `CodexOfficialAuthConfig`, `readCodexOfficialAuth(provider)`, and `writeCodexOfficialAuth(provider, auth)`.

- [ ] **Step 1: Add a failing TypeScript contract test**

```ts
it("defaults official providers to the Desktop login without mutating input", () => {
  const provider = officialProvider({});
  expect(readCodexOfficialAuth(provider)).toEqual({ mode: "desktop_current_login" });
  expect(provider.meta?.codexOfficialAuth).toBeUndefined();
});

it("normalizes a fixed managed account reference", () => {
  const provider = officialProvider({
    codexOfficialAuth: { mode: "managed_oauth", accountId: " account-1 " },
  });
  expect(readCodexOfficialAuth(provider)).toEqual({
    mode: "managed_oauth",
    accountId: "account-1",
  });
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `pnpm vitest run src/lib/codexOfficialAuth.test.ts`

Expected: FAIL because `codexOfficialAuth.ts` and the Provider metadata field do not exist.

- [ ] **Step 3: Implement the TypeScript contract and Rust Serde types**

```ts
export type CodexOfficialAuthMode =
  | "desktop_current_login"
  | "managed_oauth"
  | "account_pool";

export type CodexOfficialAuthConfig = {
  mode: CodexOfficialAuthMode;
  accountId?: string;
};
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexOfficialAuthMode {
    #[default]
    DesktopCurrentLogin,
    ManagedOauth,
    AccountPool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexOfficialAuthConfig {
    pub mode: CodexOfficialAuthMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}
```

Normalize `accountId` only for `managed_oauth`; remove it for Desktop and pool modes. `writeCodexOfficialAuth` must clone the Provider and preserve all unknown metadata.

- [ ] **Step 4: Run the focused test and Rust Provider serialization tests**

Run: `pnpm vitest run src/lib/codexOfficialAuth.test.ts`

Run: `cargo test --manifest-path src-tauri/Cargo.toml provider::tests::codex_official_auth --lib`

Expected: all focused tests PASS.

- [ ] **Step 5: Commit the contract**

Commit subject: `feat(codex): add provider-owned official auth contract`

Commit body must explain the secret-free reference, Desktop default, normalization, and compatibility intent, ending with `本次提交由BigStrongsSun完成`.

### Task 2: Resolve Authentication for Direct and Routed Official Providers

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`

**Interfaces:**
- Consumes `ProviderMeta.codex_official_auth` from Task 1.
- Produces `materialize_codex_official_auth(provider: &Provider, fallback: Option<&CodexRouteAuthPolicy>) -> Provider`.
- Keeps `provider_requests_codex_account_pool(&Provider)` as the single account-pool expansion predicate.

- [ ] **Step 1: Add failing direct and routed runtime tests**

```rust
#[test]
fn standalone_official_provider_materializes_account_pool_without_router() {
    let provider = official_provider_with_auth(CodexOfficialAuthMode::AccountPool, None);
    let effective = materialize_codex_official_auth(&provider, None);
    assert_eq!(effective.settings_config[CODEX_ACCOUNT_POOL_ENABLED], true);
    assert!(effective.settings_config.get("auth").is_none());
}

#[test]
fn routed_official_provider_reads_latest_target_auth_without_route_rewrite() {
    let route = official_route_with_provider_config();
    let target = official_provider_with_auth(CodexOfficialAuthMode::ManagedOauth, Some("account-1"));
    let effective = materialize_codex_routed_provider_from_target(&route, &target);
    assert_eq!(effective.meta.unwrap().auth_binding.unwrap().account_id.as_deref(), Some("account-1"));
}
```

Add a forwarder test proving a standalone-official Provider with `AccountPool` expands to the existing ordered pool candidates even when `codexRouting` is absent.

- [ ] **Step 2: Run focused Rust tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib standalone_official_provider_materializes_account_pool_without_router routed_official_provider_reads_latest_target_auth_without_route_rewrite codex_account_pool_works_without_multirouter`

Expected: FAIL because standalone official providers are not materialized from Provider metadata and route policy currently wins unconditionally.

- [ ] **Step 3: Implement one shared materialization path**

Map Provider mode to an existing `CodexRouteAuthPolicy`, then reuse the established
credential-removal and materialization function:

```rust
let policy = match config.mode {
    CodexOfficialAuthMode::DesktopCurrentLogin => CodexRouteAuthPolicy {
        source: CodexRouteAuthSource::NativeCodexAuth,
        account_id: None,
    },
    CodexOfficialAuthMode::ManagedOauth => CodexRouteAuthPolicy {
        source: CodexRouteAuthSource::ManagedCodexOauth,
        account_id: config.account_id.clone(),
    },
    CodexOfficialAuthMode::AccountPool => CodexRouteAuthPolicy {
        source: CodexRouteAuthSource::AccountPool,
        account_id: None,
    },
};
apply_codex_v2_auth_policy(&policy, &mut settings, &mut meta);
```

Call it for a direct `is_codex_official_provider` before forwarder candidate expansion. For MultiRouter, target Provider metadata is authoritative; use the route policy only when the target lacks the new field and the route is a legacy explicit official-auth route. Never apply target official authentication to non-official Providers.

- [ ] **Step 4: Run runtime and credential-isolation tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib standalone_official_provider_materializes_account_pool_without_router routed_official_provider_reads_latest_target_auth_without_route_rewrite codex_account_pool_works_without_multirouter account_pool_auth_materialization_does_not_override_resolved_protocol`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib external_agent_api`

Expected: all selected tests PASS; existing External API tests prove no Desktop credential reuse.

- [ ] **Step 5: Commit runtime ownership**

Commit subject: `feat(codex): decouple official oauth routing from multirouter`

Commit body must describe direct official pool expansion, Provider precedence, legacy fallback, and unchanged scheduling, ending with `本次提交由BigStrongsSun完成`.

### Task 3: Reproject the Correct Codex Authentication Facade

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/services/proxy.rs`
- Modify: `src-tauri/src/commands/codex_oauth.rs`

**Interfaces:**
- Produces `classify_codex_provider_auth_facade(provider, providers_by_id, pool_policy)` for direct and routed official Providers.
- Renames/generalizes `reproject_current_codex_multirouter_for_pool_policy` to `reproject_current_codex_auth_facade_for_pool_policy`.

- [ ] **Step 1: Add failing facade tests**

```rust
#[test]
fn standalone_official_pool_facade_tracks_desktop_membership() {
    let provider = official_provider_with_auth(CodexOfficialAuthMode::AccountPool, None);
    assert_eq!(
        classify_codex_provider_auth_facade(&provider, None, Some(&pool_with_native(true))),
        CodexMultiRouterAuthFacade::NativeMixed,
    );
    assert_eq!(
        classify_codex_provider_auth_facade(&provider, None, Some(&pool_with_native(false))),
        CodexMultiRouterAuthFacade::FullyManaged,
    );
}
```

Add a service test that changes pool membership while `codex-official` is current, verifies exact `requires_openai_auth`/`experimental_bearer_token` output, and byte-compares `auth.json` before and after.

- [ ] **Step 2: Run facade tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib standalone_official_pool_facade codex_pool_policy_reprojects_current_official_provider_and_preserves_auth_json`

Expected: FAIL because reprojection exits unless current Provider has enabled routing.

- [ ] **Step 3: Generalize classification and reprojection**

Load the canonical target Provider for Router classification, inspect its Provider-owned auth mode, and use pool Desktop membership only for `account_pool`. For direct official Providers, apply the same classifier. Preserve the current `requires_openai_auth = true` UI/account facade convention for fully managed mode while adding/removing `PROXY_MANAGED` exactly as existing tests require.

- [ ] **Step 4: Run focused projection tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_pool_policy_reprojects standalone_official_pool_facade codex_multirouter_takeover_facade`

Expected: all selected tests PASS and `auth.json` remains byte-identical.

- [ ] **Step 5: Commit facade projection**

Commit subject: `fix(codex): project official auth facade from provider state`

Commit body must document direct/Router parity, account-pool membership handling, and `auth.json` preservation, ending with `本次提交由BigStrongsSun完成`.

### Task 4: Migrate Legacy Router-Owned Authentication Safely

**Files:**
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Test: `src-tauri/tests/provider_commands.rs`

**Interfaces:**
- Produces `CodexOfficialAuthMigrationStatus { state, inherited_auth, conflicting_router_ids }` serialized in camelCase.
- Produces an idempotent Provider service method that migrates consistent legacy policies and reports conflicts without changing them.

- [ ] **Step 1: Add failing migration tests**

```rust
#[test]
fn consistent_legacy_router_auth_migrates_to_official_provider() {
    // Two Routers both select account_pool.
    // Assert codex-official.meta.codexOfficialAuth == account_pool and redundant fields are removed.
}

#[test]
fn conflicting_legacy_router_auth_is_reported_without_mutation() {
    // One Router selects Desktop and one selects account_pool.
    // Assert conflict IDs are returned and every stored Provider is byte-equivalent.
}
```

- [ ] **Step 2: Run migration tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test provider_commands codex_official_auth_migration`

Expected: FAIL because no Provider-owned migration API exists.

- [ ] **Step 3: Implement transactional migration**

Within one Provider-service transaction, gather only enabled official routes that resolve to canonical `codex-official`. If explicit legacy policies normalize to one value, write it to the Provider and remove redundant `officialAuth`/official-route copies. If values differ, return `conflict` with sorted Router IDs and do not write. If none exist, persist Desktop default. Re-running after success must be a no-op.

- [ ] **Step 4: Run migration and Provider propagation tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test provider_commands codex_official_auth_migration`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_driven_router`

Expected: migration tests PASS and existing Provider propagation remains green.

- [ ] **Step 5: Commit migration**

Commit subject: `fix(codex): migrate router auth ownership without ambiguity`

Commit body must document the all-equal rule, fail-closed conflict state, transaction boundary, idempotence, and no credential mutation, ending with `本次提交由BigStrongsSun完成`.

### Task 5: Move the Authentication Entry to OpenAI Official

**Files:**
- Create: `src/components/providers/forms/CodexOfficialAuthSection.tsx`
- Create: `src/components/providers/forms/CodexOfficialAuthSection.test.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/components/providers/ProviderCard.tsx`
- Modify: `src/components/providers/ProviderActions.tsx`
- Modify: `src/components/codex/CodexRouterWorkspacePage.tsx`
- Modify: `src/components/codex/CodexRouterWorkspacePage.test.tsx`
- Modify: `src/components/providers/forms/CodexOAuthSection.tsx`
- Modify: `src/components/providers/forms/CodexOAuthSection.test.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/ja.json`

**Interfaces:**
- Consumes Task 1 TypeScript helpers and existing `useCodexOauth`/account-pool query APIs.
- Produces a permanently visible Provider-card `认证设置` action for canonical `codex-official`.
- Produces `CodexOfficialAuthSection` that updates `ProviderMeta.codexOfficialAuth` through the existing Provider form submit.

- [ ] **Step 1: Add failing UI tests**

```tsx
it("shows an always-visible auth settings action on OpenAI Official", () => {
  renderOfficialCard();
  expect(screen.getByRole("button", { name: "认证设置" })).toHaveClass("opacity-100");
});

it("does not render official authentication in MultiRouter settings", () => {
  renderRouterSettings();
  expect(screen.queryByText("官方 ChatGPT 认证方式")).not.toBeInTheDocument();
});

it("describes the account pool as available to OpenAI Official without MultiRouter", () => {
  render(<CodexOAuthSection showAccountQuota />);
  expect(screen.getByText(/无需启用 MultiRouter/)).toBeInTheDocument();
});
```

Add tests for the three Provider modes, fixed-account selector, unavailable-account validation, conflict warning, and the account-pool settings deep link.

- [ ] **Step 2: Run focused UI tests and verify RED**

Run: `pnpm vitest run src/components/providers/forms/CodexOfficialAuthSection.test.tsx src/components/codex/CodexRouterWorkspacePage.test.tsx src/components/providers/forms/CodexOAuthSection.test.tsx`

Expected: FAIL because the Provider section/action does not exist and the old Router selector remains.

- [ ] **Step 3: Implement the Provider UI and remove Router ownership**

Render `CodexOfficialAuthSection` only when `appId === "codex"`, `provider.id === "codex-official"`, and `category === "official"`. The card action must remain visible outside the hover-only action container and open the existing edit dialog focused at the new section. Remove `officialAuth` from `MultiRouterSettingsDraft`, stop rewriting route `authPolicy` in `applyMultiRouterSettingsDraft`, and retain legacy display only inside the migration conflict warning.

- [ ] **Step 4: Update authentication-center wording and locales**

Replace the current scope copy with text equivalent to: `账号池由 OpenAI Official 使用；无需启用 MultiRouter。MultiRouter 中的官方模型会继承同一设置。` Ensure all three locale JSON files contain the same semantic boundary and remain strict UTF-8 without BOM.

- [ ] **Step 5: Run focused UI tests and typecheck**

Run: `pnpm vitest run src/components/providers/forms/CodexOfficialAuthSection.test.tsx src/components/codex/CodexRouterWorkspacePage.test.tsx src/components/providers/forms/CodexOAuthSection.test.tsx`

Run: `pnpm typecheck`

Expected: all focused tests and TypeScript checks PASS.

- [ ] **Step 6: Commit UI ownership**

Commit subject: `feat(codex): expose official auth on provider card`

Commit body must document the always-visible entry, removal from Router settings, independent pool wording, conflict display, and preserved account-policy editor, ending with `本次提交由BigStrongsSun完成`.

### Task 6: Consolidated Verification and Project Memory

**Files:**
- Modify: `memory.md`

**Interfaces:**
- Consumes all previous tasks.
- Produces one auditable acceptance record and no release/install mutation.

- [ ] **Step 1: Run the affected frontend gate once**

Run: `pnpm vitest run src/lib/codexOfficialAuth.test.ts src/components/providers/forms/CodexOfficialAuthSection.test.tsx src/components/codex/CodexRouterWorkspacePage.test.tsx src/components/providers/forms/CodexOAuthSection.test.tsx`

Run: `pnpm typecheck`

Run: `pnpm prettier --check src/lib/codexOfficialAuth.ts src/lib/codexOfficialAuth.test.ts src/components/providers/forms/CodexOfficialAuthSection.tsx src/components/providers/forms/CodexOfficialAuthSection.test.tsx src/components/providers/ProviderCard.tsx src/components/providers/ProviderActions.tsx src/components/codex/CodexRouterWorkspacePage.tsx src/components/codex/CodexRouterWorkspacePage.test.tsx src/components/providers/forms/CodexOAuthSection.tsx src/components/providers/forms/CodexOAuthSection.test.tsx src/i18n/locales/en.json src/i18n/locales/zh.json src/i18n/locales/ja.json`

Expected: all commands PASS.

- [ ] **Step 2: Run the affected Rust gate once**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_official_auth`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib codex_pool_policy_reprojects`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test provider_commands codex_official_auth_migration`

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --lib --no-default-features -- -D warnings`

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

Expected: all commands PASS or a pre-existing unrelated baseline warning is reported with exact evidence and no false green claim.

- [ ] **Step 3: Validate text integrity and diff scope**

Run a strict UTF-8/no-BOM/no-U+FFFD check over every changed text file, then run `git diff --check` and `git status --short`.

Expected: strict decoding succeeds, no BOM or replacement character exists, diff check is clean, and only task files remain changed.

- [ ] **Step 4: Record durable project knowledge**

Append a dated `OpenAI Official Provider 认证所有权` section to `memory.md` describing the root cause, new ownership boundary, legacy conflict behavior, independent non-MultiRouter pool runtime, exact commits, and verification results. If a test or behavior remains unresolved, record it explicitly instead of marking the feature complete.

- [ ] **Step 5: Commit the acceptance record**

Commit subject: `docs(codex): record official auth ownership acceptance`

Commit body must list the exact tests and results, state that no build/install/restart/release occurred, and end with `本次提交由BigStrongsSun完成`.

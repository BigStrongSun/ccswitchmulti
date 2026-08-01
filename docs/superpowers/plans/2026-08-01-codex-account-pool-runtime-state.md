# Codex Account Pool Runtime State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fragmented Codex OAuth account-pool maps with one bounded, generation-aware runtime state and make pool-specific credential, quota, and transient failures drive safe account failover.

**Architecture:** Add a pure Rust `codex_oauth_pool` state machine owned by `CodexOAuthManager` through one async mutex. OAuth persistence supplies credential generations and lifecycle events; the forwarder classifies real attempt results and submits them to the state machine without moving account-specific failures into persistent Provider health.

**Tech Stack:** Rust 2021, Tokio synchronization, Serde JSON migration defaults, SHA-256 from the existing `sha2` dependency, existing `cargo test` unit/integration harnesses.

## Global Constraints

- Preserve the released fixed-priority plus `reservePercent` scheduling contract; do not add quota, round-robin, or fill-first strategies.
- Keep the existing public account-pool JSON shape and `CodexOAuthStore.version = 1`.
- Never persist, display, or log bearer tokens, refresh tokens, access-token digests, or native-credential digests.
- Managed credential generation increments only on a completed login for the same account ID; ordinary token refresh and refresh-token rotation keep the generation.
- Independent official routes keep 401/403 non-retryable; only explicit account-pool candidates gain credential failover.
- No version bump, tag, GitHub Release, installation, or deployment belongs to this plan.
- Every production behavior follows red-green-refactor and every task ends in a local Git commit whose final message paragraph is `本次提交由BigStrongsSun完成`.
- Wait for the preceding post-commit local release process to finish before modifying files for the next task, because the known lock-ownership bug can otherwise overlap builds.

---

### Task 1: Introduce one atomic pool runtime owner and lifecycle purge

**Files:**
- Create: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs:14-24`
- Modify: `src-tauri/src/proxy/providers/codex_oauth_auth.rs:370-426,857-1010,1020-1178`
- Test: `src-tauri/src/proxy/providers/codex_oauth_auth.rs:1777-1875`

**Interfaces:**
- Produces: `CodexPoolRuntimeState`, `PoolSessionBinding`, `PoolAccountRuntime`, `prepare_account`, `purge_account`, `purge_all`, `retain_enabled_accounts`, `bind_session`, `bound_account`, `record_remaining_percent`, `quota_refresh_due`, and `cool_down_account`.
- Consumes: account IDs, credential generations, Unix millisecond timestamps, existing `CodexAccountPoolPolicy` entries, and existing manager lifecycle calls.

- [ ] **Step 1: Write failing manager regressions for stale lifecycle state**

Add tests using the real `CodexOAuthManager`:

```rust
async fn add_test_account(manager: &CodexOAuthManager, account_id: &str, refresh: &str) {
    manager
        .add_account_internal(
            account_id.to_string(),
            refresh.to_string(),
            Some(format!("{account_id}@example.test")),
            None,
            None,
        )
        .await
        .unwrap();
}

async fn set_single_managed_pool_enabled(
    manager: &CodexOAuthManager,
    account_id: &str,
    enabled: bool,
    reserve_percent: f64,
) {
    manager
        .set_account_pool_policy(CodexAccountPoolPolicy {
            enabled: true,
            entries: vec![
                CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: false,
                    reserve_percent: 0.0,
                },
                CodexAccountPoolEntry {
                    account_id: account_id.to_string(),
                    enabled,
                    reserve_percent,
                },
            ],
            desktop_account_id: None,
        })
        .await
        .unwrap();
}

async fn enable_single_managed_pool(
    manager: &CodexOAuthManager,
    account_id: &str,
    reserve_percent: f64,
) {
    set_single_managed_pool_enabled(manager, account_id, true, reserve_percent).await;
}

#[tokio::test]
async fn removing_and_readding_account_does_not_restore_old_pool_binding() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    add_test_account(&manager, "acc-a", "refresh-a").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.bind_pool_session("thread-old", "acc-a").await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    manager.remove_account("acc-a").await.unwrap();
    add_test_account(&manager, "acc-a", "refresh-b").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    assert!(manager.ordered_pool_entries("thread-old").await.is_empty());
}

#[tokio::test]
async fn invalidated_account_is_never_returned_by_pool_policy() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    add_test_account(&manager, "acc-a", "refresh-a").await;
    enable_single_managed_pool(&manager, "acc-a", 0.0).await;

    manager.mark_account_invalid_after_refresh_failure("acc-a").await.unwrap();

    assert!(manager.ordered_pool_entries("thread-new").await.is_empty());
}

#[tokio::test]
async fn disabling_then_reenabling_pool_entry_does_not_restore_old_binding() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    add_test_account(&manager, "acc-a", "refresh-a").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.bind_pool_session("thread-old", "acc-a").await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    set_single_managed_pool_enabled(&manager, "acc-a", false, 10.0).await;
    set_single_managed_pool_enabled(&manager, "acc-a", true, 10.0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    assert!(manager.ordered_pool_entries("thread-old").await.is_empty());
}

#[tokio::test]
async fn clearing_auth_does_not_restore_old_pool_binding_after_relogin() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    add_test_account(&manager, "acc-a", "refresh-a").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.bind_pool_session("thread-old", "acc-a").await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    manager.clear_auth().await.unwrap();
    add_test_account(&manager, "acc-a", "refresh-b").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    assert!(manager.ordered_pool_entries("thread-old").await.is_empty());
}
```

The helpers call the real private `add_account_internal` and `set_account_pool_policy`; they do not bypass manager side effects by writing maps directly.

- [ ] **Step 2: Run the lifecycle regressions and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib removing_and_readding_account_does_not_restore_old_pool_binding -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib invalidated_account_is_never_returned_by_pool_policy -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib disabling_then_reenabling_pool_entry_does_not_restore_old_binding -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib clearing_auth_does_not_restore_old_pool_binding_after_relogin -- --nocapture
```

Expected: the first, third, and fourth tests return `acc-a` because stale binding survives; the second returns `acc-a` because normalization accepts any existing key.

- [ ] **Step 3: Add the minimal atomic state module**

Declare the module in `providers/mod.rs`:

```rust
pub(crate) mod codex_oauth_pool;
```

Create the state with current behavior plus atomic purge:

```rust
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PoolSessionBinding {
    pub(crate) account_id: String,
    pub(crate) credential_generation: u64,
    pub(crate) last_used_at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PoolAccountRuntime {
    pub(crate) credential_generation: u64,
    pub(crate) remaining_percent: Option<f64>,
    pub(crate) quota_checked_at_ms: Option<i64>,
    pub(crate) reauth_required: bool,
    pub(crate) cooldown_until_ms: Option<i64>,
    pub(crate) consecutive_failures: u32,
    pub(crate) last_failure_at_ms: Option<i64>,
    pub(crate) soft_avoid_until_ms: Option<i64>,
}

#[derive(Debug, Default)]
pub(crate) struct CodexPoolRuntimeState {
    bindings: HashMap<String, PoolSessionBinding>,
    accounts: HashMap<String, PoolAccountRuntime>,
}
```

In this task the manager supplies generation `0`. `prepare_account` only ensures an account runtime exists; generation-change rejection is intentionally left absent so Task 2 can drive it with a failing test. Implement `purge_account`, `purge_all`, and `retain_enabled_accounts` as the only deletion paths. Move current bind, remaining-percent, quota-TTL, and fixed 60-second cooldown behavior behind this state. Give `bind_session` and `bound_account` explicit `now_ms` arguments so later TTL tests do not sleep.

- [ ] **Step 4: Wire manager lifecycle to the state owner**

Replace the four manager fields with:

```rust
pool_runtime: Arc<Mutex<CodexPoolRuntimeState>>,
```

Update manager wrappers to lock once, call a state method, and release before disk/network work. `remove_account`, `clear_auth`, invalidation, and same-ID login must call `purge_account`/`purge_all`. `set_account_pool_policy` must calculate enabled account IDs after normalization and call `retain_enabled_accounts`.

Change normalization to retain managed entries only when:

```rust
accounts.get(&entry.account_id).is_some_and(CodexAccountData::is_usable)
```

and only append missing accounts whose `CodexAccountData::is_usable()` is true.

- [ ] **Step 5: Run lifecycle GREEN and existing pool regression**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib removing_and_readding_account_does_not_restore_old_pool_binding -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib invalidated_account_is_never_returned_by_pool_policy -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib disabling_then_reenabling_pool_entry_does_not_restore_old_binding -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib clearing_auth_does_not_restore_old_pool_binding_after_relogin -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib account_pool_honors_order_reserve_cooldown_and_session_binding -- --nocapture
```

Expected: each command reports `1 passed; 0 failed`.

- [ ] **Step 6: Format, inspect, and commit Task 1**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
git status --short
```

Stage only the new module, module declaration, OAuth manager, tests, and `memory.md`. Commit with a detailed root-cause description ending in the required attribution.

---

### Task 2: Bound affinity and validate credential generations

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Modify: `src-tauri/src/proxy/providers/codex_oauth_auth.rs:180-205,500-600,857-895,1158-1195,1313-1338`
- Modify: `src-tauri/src/proxy/forwarder.rs:189-273`
- Test: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Test: `src-tauri/src/proxy/providers/codex_oauth_auth.rs`

**Interfaces:**
- Produces: `CODEX_POOL_AFFINITY_IDLE_TTL_MS`, `CODEX_POOL_AFFINITY_MAX_ENTRIES`, deterministic `bind_session`/`bound_account`, persisted `credential_generation`, `CodexPoolCandidate { entry, credential_generation }`, async `reload_from_disk`, and `ordered_pool_entries(session_id, native_authorization) -> Vec<CodexPoolCandidate>`.
- Consumes: Task 1 runtime owner and manager purge methods.

- [ ] **Step 1: Add failing pure-state TTL, LRU, and stale-generation tests**

Inside `codex_oauth_pool.rs`, test observable binding behavior with literal timestamps:

```rust
#[test]
fn affinity_expires_after_twenty_four_hours_of_idle_time() {
    let mut state = CodexPoolRuntimeState::default();
    state.prepare_account("a", 1);
    state.bind_session("thread", "a", 1, 1_000);

    assert_eq!(state.bound_account("thread", 1_000 + CODEX_POOL_AFFINITY_IDLE_TTL_MS), Some("a".into()));
    assert_eq!(state.bound_account("thread", 1_001 + CODEX_POOL_AFFINITY_IDLE_TTL_MS), None);
}

#[test]
fn affinity_lru_evicts_the_least_recently_used_entry() {
    let mut state = CodexPoolRuntimeState::default();
    state.prepare_account("a", 1);
    for index in 0..CODEX_POOL_AFFINITY_MAX_ENTRIES {
        state.bind_session(&format!("thread-{index}"), "a", 1, index as i64);
    }
    assert_eq!(state.bound_account("thread-0", 10_000), Some("a".into()));
    state.bind_session("overflow", "a", 1, 20_000);

    assert_eq!(state.bound_account("thread-1", 20_000), None);
    assert_eq!(state.bound_account("thread-0", 20_000), Some("a".into()));
}

#[test]
fn old_generation_cannot_reuse_or_rebind_new_credentials() {
    let mut state = CodexPoolRuntimeState::default();
    state.prepare_account("a", 1);
    state.bind_session("thread", "a", 1, 1_000);
    state.prepare_account("a", 2);

    assert_eq!(state.bound_account("thread", 1_001), None);
    assert!(!state.bind_session("late", "a", 1, 1_002));
}
```

The mutation each test catches is respectively removing TTL pruning, evicting by insertion order rather than last use, and accepting a late old-generation result.

- [ ] **Step 2: Run pure-state tests and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_pool::tests::affinity_ -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib old_generation_cannot_reuse_or_rebind_new_credentials -- --nocapture
```

Expected: assertions fail because Task 1 state has no TTL/LRU/generation enforcement.

- [ ] **Step 3: Implement bounded generation-aware affinity**

Use exact constants:

```rust
pub(crate) const CODEX_POOL_AFFINITY_IDLE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
pub(crate) const CODEX_POOL_AFFINITY_MAX_ENTRIES: usize = 2_048;
```

`bound_account` must prune expired bindings, validate the bound account runtime generation, update `last_used_at_ms` on a hit, and return the account ID. `bind_session` must reject unknown or stale generations, preserve one binding per session, then evict the minimum `last_used_at_ms` until the map is at most 2048 entries.

- [ ] **Step 4: Add failing manager tests for persisted managed generation**

Add these real-manager behaviors:

```rust
#[tokio::test]
async fn legacy_generation_relogin_advances_and_invalidates_old_binding() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("codex_oauth_auth.json"),
        r#"{
          "version": 1,
          "accounts": {
            "acc-a": {
              "account_id": "acc-a",
              "email": "a@example.test",
              "refresh_token": "refresh-old",
              "authenticated_at": 1
            }
          },
          "default_account_id": "acc-a",
          "pool_policy": { "enabled": false, "entries": [] }
        }"#,
    )
    .unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    assert_eq!(manager.accounts.read().await["acc-a"].credential_generation, 0);
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.bind_pool_session("legacy-thread", "acc-a", 0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    add_test_account(&manager, "acc-a", "refresh-new").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    assert_eq!(manager.accounts.read().await["acc-a"].credential_generation, 1);
    assert!(manager.ordered_pool_entries("legacy-thread", None).await.is_empty());
}

#[tokio::test]
async fn native_authorization_change_invalidates_old_binding_without_storing_token() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    manager
        .set_account_pool_policy(CodexAccountPoolPolicy {
            enabled: true,
            entries: vec![CodexAccountPoolEntry {
                account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                enabled: true,
                reserve_percent: 10.0,
            }],
            desktop_account_id: None,
        })
        .await
        .unwrap();
    let first = manager
        .ordered_pool_entries("native-thread", Some("Bearer native-a"))
        .await
        .pop()
        .unwrap();
    manager
        .bind_pool_session(
            "native-thread",
            NATIVE_CODEX_ACCOUNT_ID,
            first.credential_generation,
        )
        .await;
    manager.record_pool_remaining_percent(NATIVE_CODEX_ACCOUNT_ID, 9.0).await;
    assert_eq!(manager.ordered_pool_entries("native-thread", Some("Bearer native-a")).await.len(), 1);

    let changed = manager
        .ordered_pool_entries("native-thread", Some("Bearer native-b"))
        .await;
    assert_eq!(changed.len(), 1);
    assert_ne!(changed[0].credential_generation, first.credential_generation);
    manager.record_pool_remaining_percent(NATIVE_CODEX_ACCOUNT_ID, 9.0).await;
    assert!(manager
        .ordered_pool_entries("native-thread", Some("Bearer native-b"))
        .await
        .is_empty());
    assert!(!manager.storage_path.exists() || !std::fs::read_to_string(&manager.storage_path).unwrap().contains("native-a"));
}
```

Extend an existing refresh-success test with:

```rust
let generation_before = manager.accounts.read().await["acc-a"].credential_generation;
assert_eq!(generation_before, 1);
let _ = manager.get_valid_token_for_account("acc-a").await.unwrap();
assert_eq!(manager.accounts.read().await["acc-a"].credential_generation, 1);
```

Add this external-reload regression:

```rust
#[tokio::test]
async fn external_account_removal_purges_runtime_before_same_id_returns() {
    let temp = tempfile::tempdir().unwrap();
    let manager = CodexOAuthManager::new(temp.path().to_path_buf());
    add_test_account(&manager, "acc-a", "refresh-a").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.bind_pool_session("old-thread", "acc-a", 1).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    std::fs::write(
        &manager.storage_path,
        r#"{ "version": 1, "accounts": {}, "pool_policy": { "enabled": false, "entries": [] } }"#,
    )
    .unwrap();
    manager.reload_from_disk().await.unwrap();
    add_test_account(&manager, "acc-a", "refresh-b").await;
    enable_single_managed_pool(&manager, "acc-a", 10.0).await;
    manager.record_pool_remaining_percent("acc-a", 9.0).await;

    assert_eq!(manager.accounts.read().await["acc-a"].credential_generation, 1);
    assert!(manager.ordered_pool_entries("old-thread", None).await.is_empty());
}
```

These expectations are derived as literals; assertions do not call the generation helper.

- [ ] **Step 5: Implement managed and native credential generation**

Add to `CodexAccountData`:

```rust
#[serde(default)]
pub credential_generation: u64,
```

`add_account_internal` sets a new account to `1` and an existing account to `old.credential_generation.saturating_add(1).max(1)`. Token refresh updates token fields only.

Add a runtime-only candidate type so the generation selected under the manager/runtime locks travels with the request without changing serialized pool policy:

```rust
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodexPoolCandidate {
    pub(crate) entry: CodexAccountPoolEntry,
    pub(crate) credential_generation: u64,
}
```

For Desktop native auth, hash the trimmed incoming Authorization with `sha2::Sha256`, store only `[u8; 32]` in manager runtime, and increment a process-local native generation when the digest changes. Update:

```rust
pub async fn ordered_pool_entries(
    &self,
    session_id: &str,
    native_authorization: Option<&str>,
) -> Vec<CodexPoolCandidate>
```

`expand_codex_account_pool` passes the incoming Authorization string only to this method; neither side logs it. It copies `candidate.credential_generation` into the private numeric `codexPoolCredentialGeneration` request setting at expansion time, while serialized `CodexAccountPoolEntry` remains unchanged. Existing tests pass `None` unless they exercise the native candidate. The pre-Task-3 success recorder must read this setting and pass it to `bind_pool_session`, so a late success never binds under the current generation by lookup.

Keep synchronous disk loading only for constructor initialization, when runtime state is necessarily empty. Add `async fn reload_from_disk(&self)` for post-construction refreshes: parse the complete store, replace persistent account/default/policy state, then reconcile `pool_runtime` against usable account IDs and their stored generations before returning. Replace both refresh-path calls to `load_from_disk_sync()` with `reload_from_disk().await`; never use `try_lock` or `blocking_lock` to make runtime reconciliation best-effort.

- [ ] **Step 6: Run generation/affinity GREEN and Task 1 regressions**

Run the new pure-state filters, all `credential_generation` tests, `account_pool_honors_order_reserve_cooldown_and_session_binding`, and all four Task 1 lifecycle tests. Expected: all selected tests pass with zero failures.

- [ ] **Step 7: Format, inspect, and commit Task 2**

Run `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `git diff --check`, inspect `git diff` for any token logging, update `memory.md`, and commit only Task 2 files with the required final attribution paragraph.

---

### Task 3: Add explicit pool outcome transitions and transient soft-avoid

**Files:**
- Modify: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Modify: `src-tauri/src/proxy/providers/codex_oauth_auth.rs:897-945`
- Test: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Test: `src-tauri/src/proxy/providers/codex_oauth_auth.rs`

**Interfaces:**
- Produces: `CodexPoolAttemptOutcome`, `record_outcome_at(account_id, generation, session_id, outcome, now) -> bool`, `is_account_selectable`, and manager `record_pool_attempt`.
- Consumes: Task 2 generation-aware runtime state.

- [ ] **Step 1: Add failing table-driven outcome tests**

Define expected transitions with literal outcomes:

```rust
#[test]
fn credential_and_quota_outcomes_unbind_and_block_account() {
    let mut state = seeded_bound_state();
    assert!(state.record_outcome_at("a", 1, "thread", CodexPoolAttemptOutcome::Credential { status: Some(401) }, 1_000));
    assert_eq!(state.bound_account("thread", 1_001), None);
    assert!(!state.is_account_selectable("a", 1, 1_001));
    assert!(state.record_outcome_at("a", 1, "recovered", CodexPoolAttemptOutcome::Success, 1_002));
    assert!(state.is_account_selectable("a", 1, 1_003));
    assert_eq!(state.bound_account("recovered", 1_003), Some("a".into()));

    state.prepare_account("b", 1);
    state.bind_session("quota-thread", "b", 1, 2_000);
    assert!(state.record_outcome_at("b", 1, "quota-thread", CodexPoolAttemptOutcome::Quota { status: 429 }, 2_001));
    assert_eq!(state.bound_account("quota-thread", 2_002), None);
    assert!(!state.is_account_selectable("b", 1, 62_000));
    assert!(state.is_account_selectable("b", 1, 62_001));
}

#[test]
fn third_transient_failure_soft_avoids_then_success_recovers() {
    let mut state = seeded_bound_state();
    for now in [1_000, 2_000, 3_000] {
        assert!(state.record_outcome_at("a", 1, "thread", CodexPoolAttemptOutcome::Transient { status: Some(503) }, now));
    }
    assert_eq!(state.bound_account("thread", 3_001), None);
    assert!(!state.is_account_selectable("a", 1, 32_999));
    assert!(state.is_account_selectable("a", 1, 33_000));

    assert!(state.record_outcome_at("a", 1, "thread-2", CodexPoolAttemptOutcome::Success, 34_000));
    assert!(state.is_account_selectable("a", 1, 34_001));
}

#[test]
fn neutral_and_old_generation_outcomes_do_not_mutate_current_state() {
    let mut state = seeded_bound_state();
    state.prepare_account("a", 2);
    assert!(!state.record_outcome_at("a", 1, "late", CodexPoolAttemptOutcome::Credential { status: Some(401) }, 2_000));
    assert!(state.record_outcome_at("a", 2, "current", CodexPoolAttemptOutcome::Neutral, 2_001));
    assert!(state.is_account_selectable("a", 2, 2_002));
}
```

Add the exact window-reset and escalation regressions below. The first catches accidentally comparing against the first failure rather than the immediately preceding failure. The table uses hand-derived deadlines and also proves the fourth duration remains clamped for the seventh and later failures:

```rust
#[test]
fn transient_failure_after_five_minute_gap_restarts_at_one() {
    let mut state = seeded_bound_state();
    for now in [1_000, 2_000, 302_001] {
        assert!(state.record_outcome_at(
            "a",
            1,
            "thread",
            CodexPoolAttemptOutcome::Transient { status: None },
            now,
        ));
    }
    assert!(state.is_account_selectable("a", 1, 302_002));

    for now in [303_000, 304_000] {
        assert!(state.record_outcome_at(
            "a",
            1,
            "thread",
            CodexPoolAttemptOutcome::Transient { status: None },
            now,
        ));
    }
    assert!(!state.is_account_selectable("a", 1, 304_001));
}

#[test]
fn transient_soft_avoid_escalates_and_clamps_at_thirty_minutes() {
    for (failure_count, expected_duration_ms) in [
        (3_u32, 30_000_i64),
        (4, 120_000),
        (5, 600_000),
        (6, 1_800_000),
        (7, 1_800_000),
    ] {
        let mut state = seeded_bound_state();
        let mut last_failure_at_ms = 0;
        for index in 1..=failure_count {
            last_failure_at_ms = i64::from(index) * 1_000;
            assert!(state.record_outcome_at(
                "a",
                1,
                "thread",
                CodexPoolAttemptOutcome::Transient { status: Some(503) },
                last_failure_at_ms,
            ));
        }
        let deadline = last_failure_at_ms + expected_duration_ms;
        assert!(
            !state.is_account_selectable("a", 1, deadline - 1),
            "failure_count={failure_count} must remain avoided before {deadline}"
        );
        assert!(
            state.is_account_selectable("a", 1, deadline),
            "failure_count={failure_count} must recover at {deadline}"
        );
    }
}
```

- [ ] **Step 2: Run outcome tests and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_pool::tests -- --nocapture
```

Expected: new transition assertions fail because outcome recording and soft-avoid are absent.

- [ ] **Step 3: Implement minimal outcome state machine**

Use exact constants:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexPoolAttemptOutcome {
    Success,
    Credential { status: Option<u16> },
    Quota { status: u16 },
    Transient { status: Option<u16> },
    Neutral,
}

const QUOTA_COOLDOWN_MS: i64 = 60_000;
const TRANSIENT_FAILURE_WINDOW_MS: i64 = 5 * 60_000;
const TRANSIENT_FAILURE_THRESHOLD: u32 = 3;
const TRANSIENT_SOFT_AVOID_MS: [i64; 4] = [30_000, 120_000, 600_000, 1_800_000];
```

Credential sets runtime reauth and clears every binding for the account. Quota applies the fixed cooldown and clears every binding. Transient increments within the window; at threshold it clears bindings and applies the escalation indexed by `consecutive_failures - threshold`, clamped to the array. Success clears reauth/transient state and binds only when generation matches. Neutral changes nothing. Expired cooldown/soft-avoid fields are cleared lazily during selection.

- [ ] **Step 4: Replace manager's independent success/429 methods with one outcome wrapper**

Expose:

```rust
pub async fn record_pool_attempt(
    &self,
    account_id: &str,
    credential_generation: u64,
    session_id: &str,
    outcome: CodexPoolAttemptOutcome,
) -> bool
```

Keep `cool_down_pool_account` only if a still-existing non-forwarder caller requires it; otherwise remove it and update its tests. Candidate selection must call `is_account_selectable` before reserve filtering.

- [ ] **Step 5: Run outcome GREEN and all OAuth pool tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_pool::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_auth -- --nocapture
```

Expected: every selected test passes; no test asserts directly on private map contents when a manager-visible behavior is available.

- [ ] **Step 6: Format, inspect, and commit Task 3**

Run rustfmt, `git diff --check`, inspect state-transition logs for secret-free fields, update `memory.md`, and create the required detailed commit.

---

### Task 4: Feed real forwarder failures into the pool state machine

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs:180-340,680-835,901-1605,4444-4500`
- Modify: `src-tauri/src/proxy/providers/codex_oauth_pool.rs`
- Test: `src-tauri/src/proxy/forwarder.rs:6530-6550,8195-8260`

**Interfaces:**
- Produces: `provider_codex_pool_account(provider) -> Option<(&str, u64)>`, `classify_codex_pool_attempt(error) -> CodexPoolAttemptOutcome`, `retryable_failure_affects_provider_health(provider, error) -> bool`, pool-specific authentication retry classification, and account-scoped permit release that does not poison persistent Provider health.
- Consumes: Task 3 `record_pool_attempt` and candidate generation attached during expansion.

- [ ] **Step 1: Add failing forwarder classification tests**

Create a pool candidate by setting both fields during the test:

```rust
fn test_codex_pool_candidate(account_id: &str, generation: u64) -> Provider {
    let mut provider = test_provider_with_type(None);
    provider.id = format!("codex-router::account::{account_id}");
    provider.category = Some("official".to_string());
    provider.settings_config["codexPoolAccountId"] = Value::String(account_id.to_string());
    provider.settings_config["codexPoolCredentialGeneration"] = Value::Number(generation.into());
    provider
}
```

Add runnable tests with literal expectations:

```rust
#[test]
fn codex_pool_attempt_classification_maps_proxy_errors() {
    for (error, expected) in [
        (
            ProxyError::AuthError("token unavailable".to_string()),
            CodexPoolAttemptOutcome::Credential { status: None },
        ),
        (
            ProxyError::UpstreamError { status: 401, body: None },
            CodexPoolAttemptOutcome::Credential { status: Some(401) },
        ),
        (
            ProxyError::UpstreamError { status: 403, body: None },
            CodexPoolAttemptOutcome::Credential { status: Some(403) },
        ),
        (
            ProxyError::UpstreamError { status: 402, body: None },
            CodexPoolAttemptOutcome::Quota { status: 402 },
        ),
        (
            ProxyError::UpstreamError { status: 429, body: None },
            CodexPoolAttemptOutcome::Quota { status: 429 },
        ),
        (
            ProxyError::Timeout("upstream".to_string()),
            CodexPoolAttemptOutcome::Transient { status: None },
        ),
        (
            ProxyError::ForwardFailed("connect".to_string()),
            CodexPoolAttemptOutcome::Transient { status: None },
        ),
        (
            ProxyError::ProviderUnhealthy("temporary".to_string()),
            CodexPoolAttemptOutcome::Transient { status: None },
        ),
        (
            ProxyError::StreamIdleTimeout(30),
            CodexPoolAttemptOutcome::Transient { status: None },
        ),
        (
            ProxyError::UpstreamError { status: 500, body: None },
            CodexPoolAttemptOutcome::Transient { status: Some(500) },
        ),
        (
            ProxyError::UpstreamError { status: 502, body: None },
            CodexPoolAttemptOutcome::Transient { status: Some(502) },
        ),
        (
            ProxyError::UpstreamError { status: 503, body: None },
            CodexPoolAttemptOutcome::Transient { status: Some(503) },
        ),
    ] {
        assert_eq!(classify_codex_pool_attempt(&error), expected);
    }

    for status in [400, 405, 406, 413, 414, 415, 422, 501] {
        assert_eq!(
            classify_codex_pool_attempt(&ProxyError::UpstreamError { status, body: None }),
            CodexPoolAttemptOutcome::Neutral,
            "status={status} must not mutate account health"
        );
    }
}

#[test]
fn pool_codex_auth_failures_retry_the_next_account() {
    let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
    let pool = test_codex_pool_candidate("acc-a", 7);

    for error in [
        ProxyError::AuthError("re-login".to_string()),
        ProxyError::UpstreamError { status: 401, body: None },
        ProxyError::UpstreamError { status: 403, body: None },
    ] {
        assert_eq!(
            forwarder.categorize_proxy_error(&error, &pool),
            ErrorCategory::Retryable
        );
    }
}

#[test]
fn pool_candidate_failures_do_not_affect_persistent_provider_health() {
    let pool = test_codex_pool_candidate("acc-a", 7);
    let direct = test_codex_official_provider();

    for error in [
        ProxyError::AuthError("re-login".to_string()),
        ProxyError::UpstreamError { status: 429, body: None },
        ProxyError::Timeout("upstream".to_string()),
    ] {
        assert!(!retryable_failure_affects_provider_health(&pool, &error));
        assert!(retryable_failure_affects_provider_health(&direct, &error));
    }

    assert!(retryable_failure_affects_provider_health(
        &pool,
        &ProxyError::ConfigError("route configuration".to_string()),
    ));
}
```

Keep the existing `official_codex_auth_failures_are_not_retryable` test unchanged as the direct-official half of the boundary. `ProxyError` has no client-abort variant; do not invent one for this mapper. Client disconnect currently does not enter `record_codex_pool_attempt`, while Task 3's `Neutral` transition proves an explicitly submitted caller-neutral result does not mutate the state.

- [ ] **Step 2: Run classification tests and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_pool_attempt_classification -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib pool_codex_auth_failures_retry_the_next_account -- --nocapture
```

Expected: pool auth remains non-retryable and the new typed mapping is absent.

- [ ] **Step 3: Consume the attached candidate generation and record typed outcomes**

Task 2 already attaches the selected generation as an integer setting beside `codexPoolAccountId`. Add the exact extractor and replace `record_codex_pool_attempt(provider, success, status)` with a typed method that uses it and calls manager `record_pool_attempt`:

```rust
fn provider_codex_pool_account(provider: &Provider) -> Option<(&str, u64)> {
    let account_id = provider
        .settings_config
        .get("codexPoolAccountId")?
        .as_str()?;
    let credential_generation = provider
        .settings_config
        .get("codexPoolCredentialGeneration")?
        .as_u64()?;
    Some((account_id, credential_generation))
}
```

The error mapper must inspect `ProxyError` directly:

```rust
match error {
    ProxyError::AuthError(_) => CodexPoolAttemptOutcome::Credential { status: None },
    ProxyError::UpstreamError { status: 401 | 403, .. } =>
        CodexPoolAttemptOutcome::Credential { status: Some(*status) },
    ProxyError::UpstreamError { status: 402 | 429, .. } =>
        CodexPoolAttemptOutcome::Quota { status: *status },
    ProxyError::Timeout(_)
    | ProxyError::ForwardFailed(_)
    | ProxyError::ProviderUnhealthy(_)
    | ProxyError::StreamIdleTimeout(_) =>
        CodexPoolAttemptOutcome::Transient { status: None },
    ProxyError::UpstreamError {
        status: 400 | 405 | 406 | 413 | 414 | 415 | 422 | 501,
        ..
    } => CodexPoolAttemptOutcome::Neutral,
    ProxyError::UpstreamError { status, .. } if *status >= 500 =>
        CodexPoolAttemptOutcome::Transient { status: Some(*status) },
    _ => CodexPoolAttemptOutcome::Neutral,
}
```

Replace only the sites that already call `record_codex_pool_attempt(..., true, ...)` with `Success`. Do not add the currently missing media-retry or post-first-event success feedback in this stage; add a comment pointing to the deferred media/SSE-terminal phase.

- [ ] **Step 4: Override official auth retry only for explicit pool candidates**

At the top of `categorize_proxy_error`, check `provider_codex_pool_account(provider).is_some()`. For that case, credential/quota/transient outcomes return `Retryable`, while neutral errors continue through the existing caller classification. Keep the existing direct-official 401/403 block unchanged after the pool-specific branch.

This preserves explicit Router ownership: account candidates only exist after `provider_requests_codex_account_pool` expanded a selected pool route, and External Agent API still bypasses expansion.

In both raw and normal retry loops, route retryable pool failures through the account-scoped branch:

```rust
fn retryable_failure_affects_provider_health(
    provider: &Provider,
    error: &ProxyError,
) -> bool {
    provider_codex_pool_account(provider).is_none()
        || matches!(
            classify_codex_pool_attempt(error),
            CodexPoolAttemptOutcome::Neutral
        )
}

if retryable_failure_affects_provider_health(provider, &error) {
    self.router
        .record_result_with_health_provider(
            &provider.id,
            &persistent_provider_id,
            app_type_str,
            used_half_open_permit,
            false,
            Some(error.to_string()),
        )
        .await;
} else {
    self.router
        .release_permit_neutral(&provider.id, app_type_str, used_half_open_permit)
        .await;
}
```

The branch still continues to the next expanded candidate after recording the typed pool outcome. Do not write credential, quota, or transient account failures to the original Router/provider circuit breaker or database health row.

- [ ] **Step 5: Verify GREEN and the original boundary tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_pool_attempt_classification -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib pool_codex_auth_failures_retry_the_next_account -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib pool_candidate_failures_do_not_affect_persistent_provider_health -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib official_codex_auth_failures_are_not_retryable -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_account_pool_requires_an_explicit_router_marker -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_codex_auth_passthrough_is_limited_to_local_codex_requests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_auth_ownership_pool_managed_candidate_does_not_reuse_desktop_bearer -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib account_pool_honors_order_reserve_cooldown_and_session_binding -- --nocapture
```

Expected: every command runs at least one test and reports zero failures. If an `--exact` filter runs zero tests, fix the filter and rerun; zero tests is not evidence.

- [ ] **Step 6: Format, inspect, and commit Task 4**

Run rustfmt, `git diff --check`, inspect the forwarder diff for non-pool route changes, update `memory.md`, and commit the forwarder integration with the required final attribution.

---

### Task 5: Complete regression, static analysis, and repository knowledge

**Files:**
- Modify: `memory.md`
- Modify only if validation exposes a root-cause defect: files already owned by Tasks 1-4

**Interfaces:**
- Consumes: all Tasks 1-4 behavior and the approved design acceptance list.
- Produces: a verified branch state and an evidence-backed remaining-work boundary.

- [ ] **Step 1: Run focused account-pool and OAuth regression**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_pool -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_oauth_auth -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib codex_pool -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib official_codex_auth_failures_are_not_retryable -- --nocapture
```

Read every test count; any zero-test filter must be corrected.

- [ ] **Step 2: Run formatting and static checks**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
git diff --check
```

If Clippy reports a pre-existing warning, prove it against the parent commit before classifying it as baseline. Do not suppress or broadly allow warnings without a separate reviewed reason.

- [ ] **Step 3: Run the full Rust library suite**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: zero failed tests. Record passed, ignored, and filtered counts exactly.

- [ ] **Step 4: Review the approved design requirement by requirement**

Check all eleven acceptance bullets in `docs/superpowers/specs/2026-08-01-codex-account-pool-runtime-state-design.md`. Explicitly record any deferred item; do not call later-stage SSE, adaptive cooldown, retry-budget, or strategy work complete.

- [ ] **Step 5: Update project memory and commit verification**

Add the landed interfaces, constants, failure matrix, test counts, commit IDs, and remaining stages to `memory.md`. Run `git diff --check`, stage only intentional files, and commit with a detailed verification summary ending in `本次提交由BigStrongsSun完成`.

- [ ] **Step 6: Inspect the new post-commit pipeline without racing it**

Verify the new `local-release-pipeline.ps1` process creation time follows the Task 5 commit, preserve `scripts/logs/`, and report the pipeline as running until its real process tree exits. Do not remove its lock or stop it merely to make the worktree clean.

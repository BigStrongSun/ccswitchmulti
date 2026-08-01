use std::collections::{HashMap, HashSet};

const QUOTA_REFRESH_TTL_MS: i64 = 5 * 60 * 1_000;
const QUOTA_COOLDOWN_MS: i64 = 60_000;
const TRANSIENT_FAILURE_WINDOW_MS: i64 = 5 * 60_000;
const TRANSIENT_FAILURE_THRESHOLD: u32 = 3;
const TRANSIENT_SOFT_AVOID_MS: [i64; 4] = [30_000, 120_000, 600_000, 1_800_000];
pub(crate) const CODEX_POOL_AFFINITY_IDLE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
pub(crate) const CODEX_POOL_AFFINITY_MAX_ENTRIES: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexPoolAttemptOutcome {
    Success,
    Credential { status: Option<u16> },
    Quota { status: u16 },
    Transient { status: Option<u16> },
    Neutral,
}

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

impl CodexPoolRuntimeState {
    pub(crate) fn prepare_account(&mut self, account_id: &str, credential_generation: u64) {
        let generation_changed = self
            .accounts
            .get(account_id)
            .is_some_and(|runtime| runtime.credential_generation != credential_generation);
        if generation_changed {
            self.accounts.insert(
                account_id.to_string(),
                PoolAccountRuntime {
                    credential_generation,
                    ..Default::default()
                },
            );
            self.bindings
                .retain(|_, binding| binding.account_id != account_id);
        } else {
            self.accounts
                .entry(account_id.to_string())
                .or_insert_with(|| PoolAccountRuntime {
                    credential_generation,
                    ..Default::default()
                });
        }
    }

    fn prune_expired_bindings(&mut self, now_ms: i64) {
        self.bindings.retain(|_, binding| {
            now_ms.saturating_sub(binding.last_used_at_ms) <= CODEX_POOL_AFFINITY_IDLE_TTL_MS
        });
    }

    fn evict_lru_bindings(&mut self) {
        while self.bindings.len() > CODEX_POOL_AFFINITY_MAX_ENTRIES {
            let Some(lru_session_id) = self
                .bindings
                .iter()
                .min_by(|(session_a, binding_a), (session_b, binding_b)| {
                    binding_a
                        .last_used_at_ms
                        .cmp(&binding_b.last_used_at_ms)
                        .then_with(|| session_a.cmp(session_b))
                })
                .map(|(session_id, _)| session_id.clone())
            else {
                break;
            };
            self.bindings.remove(&lru_session_id);
        }
    }

    fn account_generation_matches(&self, account_id: &str, credential_generation: u64) -> bool {
        self.accounts
            .get(account_id)
            .is_some_and(|runtime| runtime.credential_generation == credential_generation)
    }

    pub(crate) fn purge_account(&mut self, account_id: &str) {
        self.accounts.remove(account_id);
        self.bindings
            .retain(|_, binding| binding.account_id != account_id);
    }

    pub(crate) fn purge_all(&mut self) {
        self.accounts.clear();
        self.bindings.clear();
    }

    pub(crate) fn retain_enabled_accounts(&mut self, enabled_account_ids: &HashSet<String>) {
        self.accounts
            .retain(|account_id, _| enabled_account_ids.contains(account_id));
        self.bindings
            .retain(|_, binding| enabled_account_ids.contains(&binding.account_id));
    }

    pub(crate) fn bind_session(
        &mut self,
        session_id: &str,
        account_id: &str,
        credential_generation: u64,
        now_ms: i64,
    ) -> bool {
        self.prune_expired_bindings(now_ms);
        if session_id.trim().is_empty()
            || !self.account_generation_matches(account_id, credential_generation)
        {
            return false;
        }
        self.bindings.insert(
            session_id.to_string(),
            PoolSessionBinding {
                account_id: account_id.to_string(),
                credential_generation,
                last_used_at_ms: now_ms,
            },
        );
        self.evict_lru_bindings();
        true
    }

    pub(crate) fn bound_account(&mut self, session_id: &str, now_ms: i64) -> Option<String> {
        let binding = self.bindings.get(session_id)?.clone();
        if now_ms.saturating_sub(binding.last_used_at_ms) > CODEX_POOL_AFFINITY_IDLE_TTL_MS
            || !self.account_generation_matches(&binding.account_id, binding.credential_generation)
        {
            self.bindings.remove(session_id);
            return None;
        }
        let binding = self.bindings.get_mut(session_id)?;
        binding.last_used_at_ms = now_ms;
        Some(binding.account_id.clone())
    }

    pub(crate) fn record_remaining_percent(
        &mut self,
        account_id: &str,
        remaining_percent: f64,
        now_ms: i64,
    ) -> bool {
        let Some(runtime) = self.accounts.get_mut(account_id) else {
            return false;
        };
        runtime.remaining_percent = Some(remaining_percent.clamp(0.0, 100.0));
        runtime.quota_checked_at_ms = Some(now_ms);
        true
    }

    pub(crate) fn remaining_percent(&self, account_id: &str) -> Option<f64> {
        self.accounts
            .get(account_id)
            .and_then(|runtime| runtime.remaining_percent)
    }

    pub(crate) fn quota_refresh_due(&self, account_id: &str, now_ms: i64) -> bool {
        self.accounts
            .get(account_id)
            .and_then(|runtime| runtime.quota_checked_at_ms)
            .is_none_or(|checked_at| now_ms.saturating_sub(checked_at) >= QUOTA_REFRESH_TTL_MS)
    }

    pub(crate) fn mark_quota_checked(&mut self, account_id: &str, now_ms: i64) -> bool {
        let Some(runtime) = self.accounts.get_mut(account_id) else {
            return false;
        };
        runtime.quota_checked_at_ms = Some(now_ms);
        true
    }

    pub(crate) fn cool_down_account(
        &mut self,
        account_id: &str,
        duration_ms: i64,
        now_ms: i64,
    ) -> bool {
        let Some(runtime) = self.accounts.get_mut(account_id) else {
            return false;
        };
        runtime.cooldown_until_ms = Some(now_ms.saturating_add(duration_ms.max(0)));
        self.bindings
            .retain(|_, binding| binding.account_id != account_id);
        true
    }

    pub(crate) fn record_outcome_at(
        &mut self,
        account_id: &str,
        credential_generation: u64,
        session_id: &str,
        outcome: CodexPoolAttemptOutcome,
        now_ms: i64,
    ) -> bool {
        if !self.account_generation_matches(account_id, credential_generation) {
            return false;
        }

        match outcome {
            CodexPoolAttemptOutcome::Success => {
                if let Some(runtime) = self.accounts.get_mut(account_id) {
                    runtime.reauth_required = false;
                    runtime.consecutive_failures = 0;
                    runtime.last_failure_at_ms = None;
                    runtime.soft_avoid_until_ms = None;
                }
                self.bind_session(session_id, account_id, credential_generation, now_ms);
            }
            CodexPoolAttemptOutcome::Credential { .. } => {
                if let Some(runtime) = self.accounts.get_mut(account_id) {
                    runtime.remaining_percent = None;
                    runtime.quota_checked_at_ms = None;
                    runtime.reauth_required = true;
                    runtime.cooldown_until_ms = None;
                    runtime.consecutive_failures = 0;
                    runtime.last_failure_at_ms = None;
                    runtime.soft_avoid_until_ms = None;
                }
                self.bindings
                    .retain(|_, binding| binding.account_id != account_id);
            }
            CodexPoolAttemptOutcome::Quota { .. } => {
                if let Some(runtime) = self.accounts.get_mut(account_id) {
                    runtime.cooldown_until_ms = Some(now_ms.saturating_add(QUOTA_COOLDOWN_MS));
                    runtime.consecutive_failures = 0;
                    runtime.last_failure_at_ms = None;
                    runtime.soft_avoid_until_ms = None;
                }
                self.bindings
                    .retain(|_, binding| binding.account_id != account_id);
            }
            CodexPoolAttemptOutcome::Transient { .. } => {
                let mut should_soft_avoid = false;
                if let Some(runtime) = self.accounts.get_mut(account_id) {
                    let within_window = runtime.last_failure_at_ms.is_some_and(|previous| {
                        now_ms >= previous
                            && now_ms.saturating_sub(previous) <= TRANSIENT_FAILURE_WINDOW_MS
                    });
                    runtime.consecutive_failures = if within_window {
                        runtime.consecutive_failures.saturating_add(1)
                    } else {
                        1
                    };
                    runtime.last_failure_at_ms = Some(now_ms);
                    if runtime.consecutive_failures >= TRANSIENT_FAILURE_THRESHOLD {
                        let escalation_index = usize::try_from(
                            runtime
                                .consecutive_failures
                                .saturating_sub(TRANSIENT_FAILURE_THRESHOLD),
                        )
                        .unwrap_or(usize::MAX)
                        .min(TRANSIENT_SOFT_AVOID_MS.len() - 1);
                        runtime.soft_avoid_until_ms =
                            Some(now_ms.saturating_add(TRANSIENT_SOFT_AVOID_MS[escalation_index]));
                        should_soft_avoid = true;
                    }
                }
                if should_soft_avoid {
                    self.bindings
                        .retain(|_, binding| binding.account_id != account_id);
                }
            }
            CodexPoolAttemptOutcome::Neutral => {}
        }
        true
    }

    pub(crate) fn is_account_selectable(
        &mut self,
        account_id: &str,
        credential_generation: u64,
        now_ms: i64,
    ) -> bool {
        let Some(runtime) = self.accounts.get_mut(account_id) else {
            return false;
        };
        if runtime.credential_generation != credential_generation {
            return false;
        }
        if runtime.reauth_required {
            return false;
        }
        if runtime
            .cooldown_until_ms
            .is_some_and(|cooldown_until| cooldown_until > now_ms)
        {
            return false;
        }
        runtime.cooldown_until_ms = None;
        if runtime
            .soft_avoid_until_ms
            .is_some_and(|soft_avoid_until| soft_avoid_until > now_ms)
        {
            return false;
        }
        runtime.soft_avoid_until_ms = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_bound_state() -> CodexPoolRuntimeState {
        let mut state = CodexPoolRuntimeState::default();
        state.prepare_account("a", 1);
        state.bind_session("thread", "a", 1, 0);
        state
    }

    #[test]
    fn affinity_expires_after_twenty_four_hours_of_idle_time() {
        let mut boundary_state = CodexPoolRuntimeState::default();
        boundary_state.prepare_account("a", 1);
        boundary_state.bind_session("thread", "a", 1, 1_000);

        assert_eq!(
            boundary_state.bound_account("thread", 1_000 + CODEX_POOL_AFFINITY_IDLE_TTL_MS),
            Some("a".into())
        );

        let mut expired_state = CodexPoolRuntimeState::default();
        expired_state.prepare_account("a", 1);
        expired_state.bind_session("thread", "a", 1, 1_000);
        assert_eq!(
            expired_state.bound_account("thread", 1_001 + CODEX_POOL_AFFINITY_IDLE_TTL_MS),
            None
        );
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

    #[test]
    fn credential_and_quota_outcomes_unbind_and_block_account() {
        let mut state = seeded_bound_state();
        assert!(state.record_outcome_at(
            "a",
            1,
            "thread",
            CodexPoolAttemptOutcome::Credential { status: Some(401) },
            1_000
        ));
        assert_eq!(state.bound_account("thread", 1_001), None);
        assert!(!state.is_account_selectable("a", 1, 1_001));
        assert!(state.record_outcome_at(
            "a",
            1,
            "recovered",
            CodexPoolAttemptOutcome::Success,
            1_002
        ));
        assert!(state.is_account_selectable("a", 1, 1_003));
        assert_eq!(state.bound_account("recovered", 1_003), Some("a".into()));

        state.prepare_account("b", 1);
        state.bind_session("quota-thread", "b", 1, 2_000);
        assert!(state.record_outcome_at(
            "b",
            1,
            "quota-thread",
            CodexPoolAttemptOutcome::Quota { status: 429 },
            2_001
        ));
        assert_eq!(state.bound_account("quota-thread", 2_002), None);
        assert!(!state.is_account_selectable("b", 1, 62_000));
        assert!(state.is_account_selectable("b", 1, 62_001));
    }

    #[test]
    fn third_transient_failure_soft_avoids_then_success_recovers() {
        let mut state = seeded_bound_state();
        for now_ms in [1_000, 2_000, 3_000] {
            assert!(state.record_outcome_at(
                "a",
                1,
                "thread",
                CodexPoolAttemptOutcome::Transient { status: Some(503) },
                now_ms
            ));
        }
        assert_eq!(state.bound_account("thread", 3_001), None);
        assert!(!state.is_account_selectable("a", 1, 32_999));
        assert!(state.is_account_selectable("a", 1, 33_000));

        assert!(state.record_outcome_at(
            "a",
            1,
            "thread-2",
            CodexPoolAttemptOutcome::Success,
            34_000
        ));
        assert!(state.is_account_selectable("a", 1, 34_001));
        assert!(state.record_outcome_at(
            "a",
            1,
            "thread-2",
            CodexPoolAttemptOutcome::Transient { status: None },
            35_000
        ));
        assert!(state.is_account_selectable("a", 1, 35_001));
    }

    #[test]
    fn neutral_and_old_generation_outcomes_do_not_mutate_current_state() {
        let mut state = seeded_bound_state();
        state.prepare_account("a", 2);
        assert!(!state.record_outcome_at(
            "a",
            1,
            "late",
            CodexPoolAttemptOutcome::Credential { status: Some(401) },
            2_000
        ));
        assert!(state.record_outcome_at(
            "a",
            2,
            "current",
            CodexPoolAttemptOutcome::Neutral,
            2_001
        ));
        assert!(state.is_account_selectable("a", 2, 2_002));

        for (now_ms, outcome) in [
            (3_000, CodexPoolAttemptOutcome::Transient { status: None }),
            (3_500, CodexPoolAttemptOutcome::Neutral),
            (4_000, CodexPoolAttemptOutcome::Transient { status: None }),
            (5_000, CodexPoolAttemptOutcome::Transient { status: None }),
        ] {
            assert!(state.record_outcome_at("a", 2, "current", outcome, now_ms));
        }
        assert!(!state.is_account_selectable("a", 2, 5_001));
    }

    #[test]
    fn transient_failure_after_five_minute_gap_restarts_at_one() {
        let mut state = seeded_bound_state();
        for now_ms in [1_000, 2_000, 302_001] {
            assert!(state.record_outcome_at(
                "a",
                1,
                "thread",
                CodexPoolAttemptOutcome::Transient { status: None },
                now_ms,
            ));
        }
        assert!(state.is_account_selectable("a", 1, 302_002));

        for now_ms in [303_000, 304_000] {
            assert!(state.record_outcome_at(
                "a",
                1,
                "thread",
                CodexPoolAttemptOutcome::Transient { status: None },
                now_ms,
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
}

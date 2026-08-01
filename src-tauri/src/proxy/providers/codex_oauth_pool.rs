use std::collections::{HashMap, HashSet};

const QUOTA_REFRESH_TTL_MS: i64 = 5 * 60 * 1_000;
pub(crate) const CODEX_POOL_AFFINITY_IDLE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
pub(crate) const CODEX_POOL_AFFINITY_MAX_ENTRIES: usize = 2_048;

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
    pub(crate) cooldown_until_ms: Option<i64>,
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

    pub(crate) fn is_account_selectable(&mut self, account_id: &str, now_ms: i64) -> bool {
        let Some(runtime) = self.accounts.get_mut(account_id) else {
            return false;
        };
        if runtime
            .cooldown_until_ms
            .is_some_and(|cooldown_until| cooldown_until > now_ms)
        {
            return false;
        }
        runtime.cooldown_until_ms = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

use std::collections::{HashMap, HashSet};

const QUOTA_REFRESH_TTL_MS: i64 = 5 * 60 * 1_000;

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
        self.accounts
            .entry(account_id.to_string())
            .or_insert_with(|| PoolAccountRuntime {
                credential_generation,
                ..Default::default()
            });
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
        if session_id.trim().is_empty() || !self.accounts.contains_key(account_id) {
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
        true
    }

    pub(crate) fn bound_account(&mut self, session_id: &str, _now_ms: i64) -> Option<String> {
        let binding = self.bindings.get(session_id)?.clone();
        if self.accounts.contains_key(&binding.account_id) {
            Some(binding.account_id)
        } else {
            self.bindings.remove(session_id);
            None
        }
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

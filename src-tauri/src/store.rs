use crate::database::Database;
use crate::protocol_compatibility::{
    ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord,
};
use crate::services::{ProxyService, UsageCache};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const PROTOCOL_PROBE_RECEIPT_TTL: Duration = Duration::from_secs(10 * 60);

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    protocol_probes_in_flight: Arc<Mutex<HashSet<String>>>,
    protocol_probe_released: Arc<tokio::sync::Notify>,
    protocol_probe_receipts: Arc<Mutex<HashMap<String, ProtocolProbeReceipt>>>,
    codex_provider_set_probe_receipts: Arc<Mutex<HashMap<String, CodexProviderSetProbeReceipt>>>,
    codex_startup_reconciliation_pending: AtomicBool,
}

#[derive(Clone)]
struct ProtocolProbeReceipt {
    result: ProtocolCompatibilityProbeResult,
    expires_at: Instant,
}

#[derive(Clone)]
struct CodexProviderSetProbeReceipt {
    record: ProtocolCompatibilityRecord,
    observations: Vec<ProtocolCompatibilityRecord>,
    expires_at: Instant,
    claim_token: Option<String>,
}

#[derive(Clone)]
pub(crate) struct CodexProviderSetProbeReceiptBundle {
    pub record: ProtocolCompatibilityRecord,
    pub observations: Vec<ProtocolCompatibilityRecord>,
}

pub(crate) struct CodexProviderSetProbeReceiptClaim {
    receipt_ids: Vec<String>,
    claim_token: String,
    bundles: Vec<CodexProviderSetProbeReceiptBundle>,
    receipts: Arc<Mutex<HashMap<String, CodexProviderSetProbeReceipt>>>,
    finished: bool,
}

impl CodexProviderSetProbeReceiptClaim {
    pub(crate) fn bundles(&self) -> &[CodexProviderSetProbeReceiptBundle] {
        &self.bundles
    }

    pub(crate) fn consume_after_database_commit(mut self) {
        let mut receipts = match self.receipts.lock() {
            Ok(receipts) => receipts,
            Err(poisoned) => {
                log::error!(
                    "Codex Provider Set receipt state was poisoned after database commit; recovering ownership to consume the committed claim"
                );
                poisoned.into_inner()
            }
        };
        for receipt_id in &self.receipt_ids {
            let owned = receipts
                .get(receipt_id)
                .and_then(|receipt| receipt.claim_token.as_deref())
                == Some(self.claim_token.as_str());
            if owned {
                receipts.remove(receipt_id);
            } else {
                log::error!(
                    "Committed Codex Provider Set receipt claim lost ownership before consumption: {receipt_id}"
                );
            }
        }
        self.finished = true;
    }
}

impl Drop for CodexProviderSetProbeReceiptClaim {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut receipts = match self.receipts.lock() {
            Ok(receipts) => receipts,
            Err(poisoned) => poisoned.into_inner(),
        };
        for receipt_id in &self.receipt_ids {
            if let Some(receipt) = receipts.get_mut(receipt_id) {
                if receipt.claim_token.as_deref() == Some(self.claim_token.as_str()) {
                    receipt.claim_token = None;
                }
            }
        }
    }
}

pub struct ProtocolProbeLease {
    key: String,
    in_flight: Arc<Mutex<HashSet<String>>>,
    released: Arc<tokio::sync::Notify>,
}

impl Drop for ProtocolProbeLease {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.remove(&self.key);
        }
        self.released.notify_waiters();
    }
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            protocol_probes_in_flight: Arc::new(Mutex::new(HashSet::new())),
            protocol_probe_released: Arc::new(tokio::sync::Notify::new()),
            protocol_probe_receipts: Arc::new(Mutex::new(HashMap::new())),
            codex_provider_set_probe_receipts: Arc::new(Mutex::new(HashMap::new())),
            // Tests, CLI helpers and command-only AppState instances do not run the
            // Desktop startup recovery pipeline. The real app explicitly opens the
            // gate before exposing its renderer.
            codex_startup_reconciliation_pending: AtomicBool::new(false),
        }
    }

    /// Prevent renderer consistency checks from observing the transient state
    /// between restoring the user's live config and re-projecting proxy takeover.
    pub(crate) fn begin_codex_startup_reconciliation(&self) {
        self.codex_startup_reconciliation_pending
            .store(true, Ordering::Release);
    }

    /// Publish that startup writers have settled and live config is safe to inspect.
    pub(crate) fn finish_codex_startup_reconciliation(&self) {
        self.codex_startup_reconciliation_pending
            .store(false, Ordering::Release);
    }

    pub(crate) fn codex_startup_reconciliation_pending(&self) -> bool {
        self.codex_startup_reconciliation_pending
            .load(Ordering::Acquire)
    }

    pub fn try_acquire_protocol_probe(&self, key: &str) -> Result<ProtocolProbeLease, String> {
        let mut in_flight = self
            .protocol_probes_in_flight
            .lock()
            .map_err(|_| "protocol probe lease state is unavailable".to_string())?;
        if !in_flight.insert(key.to_string()) {
            return Err("probe_in_progress".to_string());
        }
        Ok(ProtocolProbeLease {
            key: key.to_string(),
            in_flight: self.protocol_probes_in_flight.clone(),
            released: self.protocol_probe_released.clone(),
        })
    }

    /// Concurrent Provider preflights may share a physical target. Serialize
    /// only that target, leaving unrelated probes free to run.
    pub async fn acquire_protocol_probe(&self, key: &str) -> Result<ProtocolProbeLease, String> {
        tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                // Register before checking the lease so a release between the
                // check and await cannot be lost (notify_waiters semantics).
                let released = self.protocol_probe_released.notified();
                match self.try_acquire_protocol_probe(key) {
                    Ok(lease) => return Ok(lease),
                    Err(error) if error == "probe_in_progress" => released.await,
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .map_err(|_| "protocol probe queue timed out".to_string())?
    }

    pub(crate) fn remember_protocol_probe_receipt(
        &self,
        key: String,
        result: ProtocolCompatibilityProbeResult,
    ) -> Result<(), String> {
        let now = Instant::now();
        let mut receipts = self
            .protocol_probe_receipts
            .lock()
            .map_err(|_| "protocol probe receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now);
        receipts.insert(
            key,
            ProtocolProbeReceipt {
                result,
                expires_at: now + PROTOCOL_PROBE_RECEIPT_TTL,
            },
        );
        Ok(())
    }

    pub(crate) fn consume_protocol_probe_receipt(
        &self,
        key: &str,
    ) -> Result<Option<ProtocolCompatibilityProbeResult>, String> {
        let now = Instant::now();
        let mut receipts = self
            .protocol_probe_receipts
            .lock()
            .map_err(|_| "protocol probe receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now);
        Ok(receipts.remove(key).map(|receipt| receipt.result))
    }

    pub(crate) fn remember_codex_provider_set_probe_receipt(
        &self,
        record: ProtocolCompatibilityRecord,
        observations: Vec<ProtocolCompatibilityRecord>,
    ) -> Result<String, String> {
        let logical_target_matches = |observation: &ProtocolCompatibilityRecord| {
            observation.target.provider_id == record.target.provider_id
                && observation.target.route_id == record.target.route_id
                && observation.target.public_model == record.target.public_model
                && observation.target.upstream_model == record.target.upstream_model
                && observation.result.selected_transport == record.result.selected_transport
        };
        if observations.len() != 2
            || !observations.iter().all(logical_target_matches)
            || !observations.iter().any(|observation| {
                observation.target.transport
                    == crate::protocol_compatibility::TransportKind::OpenAiResponses
            })
            || !observations.iter().any(|observation| {
                observation.target.transport
                    == crate::protocol_compatibility::TransportKind::OpenAiChat
            })
        {
            return Err("codex_provider_set_probe_observation_mismatch".to_string());
        }
        let now = Instant::now();
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now || receipt.claim_token.is_some());
        let receipt_id = uuid::Uuid::new_v4().to_string();
        receipts.insert(
            receipt_id.clone(),
            CodexProviderSetProbeReceipt {
                record,
                observations,
                expires_at: now + PROTOCOL_PROBE_RECEIPT_TTL,
                claim_token: None,
            },
        );
        Ok(receipt_id)
    }

    pub(crate) fn get_codex_provider_set_probe_receipts(
        &self,
        receipt_ids: &[String],
    ) -> Result<Vec<CodexProviderSetProbeReceiptBundle>, String> {
        let now = Instant::now();
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now || receipt.claim_token.is_some());
        let mut seen = HashSet::new();
        receipt_ids
            .iter()
            .map(|receipt_id| {
                if !seen.insert(receipt_id.as_str()) {
                    return Err(format!(
                        "codex_provider_set_probe_receipt_duplicate: {receipt_id}"
                    ));
                }
                receipts
                    .get(receipt_id)
                    .map(|receipt| CodexProviderSetProbeReceiptBundle {
                        record: receipt.record.clone(),
                        observations: receipt.observations.clone(),
                    })
                    .ok_or_else(|| format!("codex_provider_set_probe_required: {receipt_id}"))
            })
            .collect()
    }

    pub(crate) fn claim_codex_provider_set_probe_receipts(
        &self,
        receipt_ids: &[String],
    ) -> Result<CodexProviderSetProbeReceiptClaim, String> {
        let now = Instant::now();
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now || receipt.claim_token.is_some());
        let mut seen = HashSet::new();
        for receipt_id in receipt_ids {
            if !seen.insert(receipt_id.as_str()) {
                return Err(format!(
                    "codex_provider_set_probe_receipt_duplicate: {receipt_id}"
                ));
            }
            let receipt = receipts
                .get(receipt_id)
                .ok_or_else(|| format!("codex_provider_set_probe_required: {receipt_id}"))?;
            if receipt.claim_token.is_some() {
                return Err(format!(
                    "codex_provider_set_probe_receipt_in_use: {receipt_id}"
                ));
            }
        }

        let claim_token = uuid::Uuid::new_v4().to_string();
        let bundles = receipt_ids
            .iter()
            .map(|receipt_id| {
                let receipt = receipts
                    .get_mut(receipt_id)
                    .expect("validated Provider Set receipt must remain present while locked");
                receipt.claim_token = Some(claim_token.clone());
                CodexProviderSetProbeReceiptBundle {
                    record: receipt.record.clone(),
                    observations: receipt.observations.clone(),
                }
            })
            .collect();
        drop(receipts);

        Ok(CodexProviderSetProbeReceiptClaim {
            receipt_ids: receipt_ids.to_vec(),
            claim_token,
            bundles,
            receipts: self.codex_provider_set_probe_receipts.clone(),
            finished: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol_compatibility::{ProbeReadiness, TransportKind};

    #[test]
    fn same_protocol_probe_target_has_one_in_flight_lease() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));

        let first = state
            .try_acquire_protocol_probe("target-a")
            .expect("first lease");
        assert!(state.try_acquire_protocol_probe("target-a").is_err());
        assert!(state.try_acquire_protocol_probe("target-b").is_ok());
        drop(first);
        assert!(state.try_acquire_protocol_probe("target-a").is_ok());
    }

    #[tokio::test]
    async fn queued_probe_lease_wakes_and_does_not_block_other_targets() {
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let first = state.try_acquire_protocol_probe("shared").unwrap();
        let mut second = Box::pin(state.acquire_protocol_probe("shared"));
        let mut third = Box::pin(state.acquire_protocol_probe("shared"));
        assert!(futures::poll!(&mut second).is_pending());
        assert!(futures::poll!(&mut third).is_pending());
        let unrelated = state.acquire_protocol_probe("other").await.unwrap();
        drop(unrelated);
        assert!(futures::poll!(&mut second).is_pending());
        drop(first);
        let second_lease = tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .unwrap()
            .unwrap();
        assert!(futures::poll!(&mut third).is_pending());
        drop(second_lease);
        let third_lease = tokio::time::timeout(Duration::from_secs(1), third)
            .await
            .unwrap()
            .unwrap();
        drop(third_lease);
        assert!(state.try_acquire_protocol_probe("shared").is_ok());
    }

    #[tokio::test]
    async fn cancelled_probe_waiter_does_not_own_or_leak_a_lease() {
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let first = state.try_acquire_protocol_probe("shared").unwrap();
        let mut waiter = Box::pin(state.acquire_protocol_probe("shared"));
        assert!(futures::poll!(&mut waiter).is_pending());
        drop(waiter);
        assert!(state.try_acquire_protocol_probe("shared").is_err());
        drop(first);
        assert!(state.try_acquire_protocol_probe("shared").is_ok());
    }

    #[test]
    fn expired_protocol_probe_receipt_is_never_reused_by_save() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));
        state
            .protocol_probe_receipts
            .lock()
            .expect("receipt lock")
            .insert(
                "target-a".to_string(),
                ProtocolProbeReceipt {
                    result: ProtocolCompatibilityProbeResult {
                        selected_transport: Some(TransportKind::OpenAiResponses),
                        readiness: ProbeReadiness::Verified,
                        branches: Vec::new(),
                    },
                    expires_at: Instant::now() - Duration::from_secs(1),
                },
            );

        assert_eq!(
            state
                .consume_protocol_probe_receipt("target-a")
                .expect("consume receipt"),
            None
        );
        assert!(state
            .protocol_probe_receipts
            .lock()
            .expect("receipt lock")
            .is_empty());
    }

    #[test]
    fn provider_set_receipt_keeps_selection_and_both_transport_observations_together() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));
        let record_for = |transport| {
            ProtocolCompatibilityRecord::new(
                crate::protocol_compatibility::ProbeTargetKey::new(
                    "provider-a",
                    None::<String>,
                    "model-a",
                    "upstream-a",
                    transport,
                    match transport {
                        TransportKind::OpenAiResponses => "https://relay.example/v1/responses",
                        TransportKind::OpenAiChat => "https://relay.example/v1/chat/completions",
                    },
                    "bearer",
                )
                .expect("target"),
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(TransportKind::OpenAiResponses),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                100,
                200,
            )
        };
        let selection = record_for(TransportKind::OpenAiResponses);
        let observations = vec![
            record_for(TransportKind::OpenAiResponses),
            record_for(TransportKind::OpenAiChat),
        ];

        let receipt_id = state
            .remember_codex_provider_set_probe_receipt(selection.clone(), observations.clone())
            .expect("remember receipt bundle");
        let bundles = state
            .get_codex_provider_set_probe_receipts(&[receipt_id.clone()])
            .expect("read receipt bundle");

        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].record, selection);
        assert_eq!(bundles[0].observations, observations);

        state
            .codex_provider_set_probe_receipts
            .lock()
            .expect("Provider Set receipt lock")
            .get_mut(&receipt_id)
            .expect("stored Provider Set receipt")
            .expires_at = Instant::now() - Duration::from_secs(1);
        let expired = match state.get_codex_provider_set_probe_receipts(&[receipt_id]) {
            Ok(_) => panic!("expired Provider Set receipt must not be returned"),
            Err(error) => error,
        };
        assert!(expired.contains("codex_provider_set_probe_required"));
        assert!(state
            .codex_provider_set_probe_receipts
            .lock()
            .expect("Provider Set receipt lock")
            .is_empty());
    }

    #[test]
    fn provider_set_receipt_claims_the_whole_set_releases_on_drop_and_consumes_once() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));
        let record_for = |provider_id: &str, transport| {
            ProtocolCompatibilityRecord::new(
                crate::protocol_compatibility::ProbeTargetKey::new(
                    provider_id,
                    None::<String>,
                    "model-a",
                    "upstream-a",
                    transport,
                    match transport {
                        TransportKind::OpenAiResponses => "https://relay.example/v1/responses",
                        TransportKind::OpenAiChat => "https://relay.example/v1/chat/completions",
                    },
                    "bearer",
                )
                .expect("target"),
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(TransportKind::OpenAiResponses),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                100,
                200,
            )
        };
        let remember = |provider_id: &str| {
            let selection = record_for(provider_id, TransportKind::OpenAiResponses);
            state
                .remember_codex_provider_set_probe_receipt(
                    selection,
                    vec![
                        record_for(provider_id, TransportKind::OpenAiResponses),
                        record_for(provider_id, TransportKind::OpenAiChat),
                    ],
                )
                .expect("remember receipt")
        };
        let first_id = remember("provider-a");
        let second_id = remember("provider-b");

        let second_claim = state
            .claim_codex_provider_set_probe_receipts(std::slice::from_ref(&second_id))
            .expect("claim second receipt");
        let conflict = match state
            .claim_codex_provider_set_probe_receipts(&[first_id.clone(), second_id.clone()])
        {
            Ok(_) => panic!("a claimed member must reject the whole claim set"),
            Err(error) => error,
        };
        assert!(conflict.contains("codex_provider_set_probe_receipt_in_use"));

        let first_claim = state
            .claim_codex_provider_set_probe_receipts(std::slice::from_ref(&first_id))
            .expect("failed whole-set claim must not partially claim its free member");
        drop(first_claim);
        drop(second_claim);

        let claim = state
            .claim_codex_provider_set_probe_receipts(&[first_id.clone(), second_id.clone()])
            .expect("drop releases both receipts for retry");
        assert_eq!(claim.bundles().len(), 2);
        claim.consume_after_database_commit();

        let consumed = match state.get_codex_provider_set_probe_receipts(&[first_id, second_id]) {
            Ok(_) => panic!("database success must permanently consume the claimed set"),
            Err(error) => error,
        };
        assert!(consumed.contains("codex_provider_set_probe_required"));
    }
}

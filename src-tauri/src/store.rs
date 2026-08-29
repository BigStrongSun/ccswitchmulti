use crate::database::Database;
use crate::protocol_compatibility::{
    ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord,
};
use crate::services::{ProxyService, UsageCache};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const PROTOCOL_PROBE_RECEIPT_TTL: Duration = Duration::from_secs(10 * 60);

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    protocol_probes_in_flight: Arc<Mutex<HashSet<String>>>,
    protocol_probe_receipts: Arc<Mutex<HashMap<String, ProtocolProbeReceipt>>>,
    codex_provider_set_probe_receipts: Arc<Mutex<HashMap<String, CodexProviderSetProbeReceipt>>>,
}

#[derive(Clone)]
struct ProtocolProbeReceipt {
    result: ProtocolCompatibilityProbeResult,
    expires_at: Instant,
}

#[derive(Clone)]
struct CodexProviderSetProbeReceipt {
    record: ProtocolCompatibilityRecord,
    expires_at: Instant,
}

pub struct ProtocolProbeLease {
    key: String,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl Drop for ProtocolProbeLease {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.remove(&self.key);
        }
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
            protocol_probe_receipts: Arc::new(Mutex::new(HashMap::new())),
            codex_provider_set_probe_receipts: Arc::new(Mutex::new(HashMap::new())),
        }
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
        })
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
    ) -> Result<String, String> {
        let now = Instant::now();
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now);
        let receipt_id = uuid::Uuid::new_v4().to_string();
        receipts.insert(
            receipt_id.clone(),
            CodexProviderSetProbeReceipt {
                record,
                expires_at: now + PROTOCOL_PROBE_RECEIPT_TTL,
            },
        );
        Ok(receipt_id)
    }

    pub(crate) fn get_codex_provider_set_probe_receipts(
        &self,
        receipt_ids: &[String],
    ) -> Result<Vec<ProtocolCompatibilityRecord>, String> {
        let now = Instant::now();
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        receipts.retain(|_, receipt| receipt.expires_at > now);
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
                    .map(|receipt| receipt.record.clone())
                    .ok_or_else(|| format!("codex_provider_set_probe_required: {receipt_id}"))
            })
            .collect()
    }

    pub(crate) fn forget_codex_provider_set_probe_receipts(
        &self,
        receipt_ids: &[String],
    ) -> Result<(), String> {
        let mut receipts = self
            .codex_provider_set_probe_receipts
            .lock()
            .map_err(|_| "Codex Provider Set receipt state is unavailable".to_string())?;
        for receipt_id in receipt_ids {
            receipts.remove(receipt_id);
        }
        Ok(())
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
}

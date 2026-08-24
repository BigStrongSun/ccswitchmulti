use crate::database::Database;
use crate::services::{ProxyService, UsageCache};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    protocol_probes_in_flight: Arc<Mutex<HashSet<String>>>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

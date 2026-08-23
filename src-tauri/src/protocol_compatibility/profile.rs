use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ProbeTargetKey, ProtocolCompatibilityProbeResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCompatibilityRecord {
    pub target: ProbeTargetKey,
    pub result: ProtocolCompatibilityProbeResult,
    pub tested_at: i64,
    pub expires_at: i64,
}

impl ProtocolCompatibilityRecord {
    pub fn new(
        target: ProbeTargetKey,
        result: ProtocolCompatibilityProbeResult,
        tested_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            target,
            result,
            tested_at,
            expires_at,
        }
    }

    pub fn storage_key(&self) -> String {
        storage_key_for_target(&self.target)
    }
}

pub(crate) fn storage_key_for_target(target: &ProbeTargetKey) -> String {
    let encoded = serde_json::to_vec(target).expect("probe target keys serialize");
    format!("{:x}", Sha256::digest(encoded))
}

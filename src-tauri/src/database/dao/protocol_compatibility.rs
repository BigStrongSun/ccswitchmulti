use rusqlite::params;

use crate::{
    error::AppError,
    protocol_compatibility::{
        profile::storage_key_for_target, ProbeTargetKey, ProtocolCompatibilityRecord,
    },
};

use super::super::{lock_conn, to_json_string, Database};

impl Database {
    pub fn save_protocol_compatibility_result(
        &self,
        record: &ProtocolCompatibilityRecord,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let profile_json = to_json_string(record)?;
        conn.execute(
            "INSERT INTO protocol_compatibility_profiles (
                target_key, provider_id, route_id, public_model, upstream_model,
                transport, endpoint_fingerprint, authentication_kind, readiness,
                profile_json, tested_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(target_key) DO UPDATE SET
                provider_id=excluded.provider_id,
                route_id=excluded.route_id,
                public_model=excluded.public_model,
                upstream_model=excluded.upstream_model,
                transport=excluded.transport,
                endpoint_fingerprint=excluded.endpoint_fingerprint,
                authentication_kind=excluded.authentication_kind,
                readiness=excluded.readiness,
                profile_json=excluded.profile_json,
                tested_at=excluded.tested_at,
                expires_at=excluded.expires_at",
            params![
                record.storage_key(),
                record.target.provider_id,
                record.target.route_id,
                record.target.public_model,
                record.target.upstream_model,
                serde_json::to_value(record.target.transport)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_string()),
                record.target.endpoint_fingerprint,
                record.target.authentication_kind,
                serde_json::to_value(record.result.readiness)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unverified".to_string()),
                profile_json,
                record.tested_at,
                record.expires_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn get_protocol_compatibility_result(
        &self,
        target: &ProbeTargetKey,
    ) -> Result<Option<ProtocolCompatibilityRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let storage_key = storage_key_for_target(target);
        let mut statement = conn
            .prepare(
                "SELECT profile_json FROM protocol_compatibility_profiles WHERE target_key = ?1",
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut rows = statement
            .query(params![storage_key])
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some(row) = rows
            .next()
            .map_err(|error| AppError::Database(error.to_string()))?
        else {
            return Ok(None);
        };
        let profile_json: String = row
            .get(0)
            .map_err(|error| AppError::Database(error.to_string()))?;
        serde_json::from_str(&profile_json)
            .map(Some)
            .map_err(|error| AppError::Database(format!("协议兼容性档案解析失败: {error}")))
    }

    pub fn prune_protocol_compatibility_results(&self, now: i64) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let deleted = conn
            .execute(
                "DELETE FROM protocol_compatibility_profiles WHERE expires_at < ?1",
                params![now],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(deleted as u64)
    }
}

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Transaction};

use crate::{
    error::AppError,
    protocol_compatibility::{
        profile::storage_key_for_target, ManualReasoningOverride, ProbeTargetKey,
        ProtocolCompatibilityRecord, ReasoningManualOverrideRecord, ReasoningProjection,
    },
};

use super::super::{lock_conn, to_json_string, Database};

impl Database {
    pub fn save_protocol_compatibility_result(
        &self,
        record: &ProtocolCompatibilityRecord,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        save_protocol_compatibility_result_in_transaction(&tx, record)?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
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

    pub fn expire_protocol_compatibility_result(
        &self,
        target: &ProbeTargetKey,
        now: i64,
    ) -> Result<bool, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let target_key = storage_key_for_target(target);
        let profile_json = tx
            .query_row(
                "SELECT profile_json FROM protocol_compatibility_profiles WHERE target_key = ?1",
                params![target_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let Some(profile_json) = profile_json else {
            return Ok(false);
        };
        let mut record: ProtocolCompatibilityRecord = serde_json::from_str(&profile_json)
            .map_err(|error| AppError::Database(format!("协议兼容性档案解析失败: {error}")))?;
        record.expires_at = record.expires_at.min(now.saturating_sub(1));
        tx.execute(
            "UPDATE protocol_compatibility_profiles
             SET profile_json = ?1, expires_at = ?2
             WHERE target_key = ?3",
            params![to_json_string(&record)?, record.expires_at, target_key],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_reasoning_manual_override(
        &self,
        target: &ProbeTargetKey,
        override_spec: ManualReasoningOverride,
        projection: ReasoningProjection,
        reason: &str,
        updated_at: i64,
        expected_revision: i64,
    ) -> Result<ReasoningManualOverrideRecord, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let target_key = storage_key_for_target(target);
        let current_revision = reasoning_override_revision(&tx, &target_key)?;
        if current_revision != expected_revision {
            return Err(AppError::InvalidInput("revision_conflict".to_string()));
        }

        let record = ReasoningManualOverrideRecord {
            target: target.clone(),
            revision: current_revision + 1,
            override_spec,
            projection,
            reason: reason.trim().to_string(),
            updated_at,
        };
        let target_json = to_json_string(target)?;
        let override_json = to_json_string(&record)?;
        tx.execute(
            "INSERT INTO codex_reasoning_manual_overrides (
                target_key, target_json, revision, override_json, reason, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(target_key) DO UPDATE SET
                target_json=excluded.target_json,
                revision=excluded.revision,
                override_json=excluded.override_json,
                reason=excluded.reason,
                updated_at=excluded.updated_at",
            params![
                target_key,
                target_json,
                record.revision,
                override_json,
                record.reason,
                record.updated_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(record)
    }

    pub fn get_reasoning_manual_override(
        &self,
        target: &ProbeTargetKey,
    ) -> Result<Option<ReasoningManualOverrideRecord>, AppError> {
        let conn = lock_conn!(self.conn);
        let target_key = storage_key_for_target(target);
        let override_json = conn
            .query_row(
                "SELECT override_json FROM codex_reasoning_manual_overrides WHERE target_key = ?1",
                params![target_key],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?
            .flatten();
        override_json
            .map(|json| {
                serde_json::from_str(&json).map_err(|error| {
                    AppError::Database(format!("Codex reasoning 手工覆盖解析失败: {error}"))
                })
            })
            .transpose()
    }

    pub fn get_reasoning_manual_override_revision(
        &self,
        target: &ProbeTargetKey,
    ) -> Result<i64, AppError> {
        let conn = lock_conn!(self.conn);
        reasoning_override_revision(&conn, &storage_key_for_target(target))
    }

    pub fn clear_reasoning_manual_override(
        &self,
        target: &ProbeTargetKey,
        expected_revision: i64,
        updated_at: i64,
    ) -> Result<i64, AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        let target_key = storage_key_for_target(target);
        let current_revision = reasoning_override_revision(&tx, &target_key)?;
        if current_revision != expected_revision {
            return Err(AppError::InvalidInput("revision_conflict".to_string()));
        }
        let next_revision = current_revision + 1;
        tx.execute(
            "INSERT INTO codex_reasoning_manual_overrides (
                target_key, target_json, revision, override_json, reason, updated_at
             ) VALUES (?1, ?2, ?3, NULL, '', ?4)
             ON CONFLICT(target_key) DO UPDATE SET
                target_json=excluded.target_json,
                revision=excluded.revision,
                override_json=NULL,
                reason='',
                updated_at=excluded.updated_at",
            params![
                target_key,
                to_json_string(target)?,
                next_revision,
                updated_at,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(next_revision)
    }
}

fn reasoning_override_revision(
    conn: &rusqlite::Connection,
    target_key: &str,
) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT revision FROM codex_reasoning_manual_overrides WHERE target_key = ?1",
        params![target_key],
        |row| row.get(0),
    )
    .optional()
    .map(|revision| revision.unwrap_or(0))
    .map_err(|error| AppError::Database(error.to_string()))
}

pub(super) fn save_protocol_compatibility_result_in_transaction(
    tx: &Transaction<'_>,
    record: &ProtocolCompatibilityRecord,
) -> Result<(), AppError> {
    let profile_json = to_json_string(record)?;
    tx.execute(
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

pub(super) fn delete_protocol_state_for_provider_in_transaction(
    tx: &Transaction<'_>,
    provider_id: &str,
) -> Result<(), AppError> {
    tx.execute(
        "DELETE FROM protocol_compatibility_profiles WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    delete_reasoning_overrides_matching(tx, |target| target.provider_id == provider_id)
}

pub(super) fn delete_protocol_state_for_routes_in_transaction(
    tx: &Transaction<'_>,
    provider_id: &str,
    route_ids: &HashSet<String>,
) -> Result<(), AppError> {
    if route_ids.is_empty() {
        return Ok(());
    }
    for route_id in route_ids {
        tx.execute(
            "DELETE FROM protocol_compatibility_profiles
             WHERE provider_id = ?1 AND route_id = ?2",
            params![provider_id, route_id],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }
    delete_reasoning_overrides_matching(tx, |target| {
        target.provider_id == provider_id
            && target
                .route_id
                .as_ref()
                .is_some_and(|route_id| route_ids.contains(route_id))
    })
}

fn delete_reasoning_overrides_matching(
    tx: &Transaction<'_>,
    matches: impl Fn(&ProbeTargetKey) -> bool,
) -> Result<(), AppError> {
    let keys = {
        let mut statement = tx
            .prepare("SELECT target_key, target_json FROM codex_reasoning_manual_overrides")
            .map_err(|error| AppError::Database(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut keys = Vec::new();
        for row in rows {
            let (target_key, target_json) =
                row.map_err(|error| AppError::Database(error.to_string()))?;
            let target: ProbeTargetKey = serde_json::from_str(&target_json).map_err(|error| {
                AppError::Database(format!(
                    "Codex reasoning 手工覆盖目标解析失败，无法安全清理: {error}"
                ))
            })?;
            if matches(&target) {
                keys.push(target_key);
            }
        }
        keys
    };
    for target_key in keys {
        tx.execute(
            "DELETE FROM codex_reasoning_manual_overrides WHERE target_key = ?1",
            params![target_key],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    }
    Ok(())
}

impl Database {
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
}

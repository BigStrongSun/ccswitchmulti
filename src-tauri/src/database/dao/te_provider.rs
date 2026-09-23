//! 专用 TE Provider 静态描述符注册表：与通用 Agent LLM 配置物理隔离。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

impl Database {
    /// TE static identity and its public Provider projection commit together.
    pub fn save_te_provider_with_descriptor(
        &self,
        provider: &crate::provider::Provider,
        descriptor: &Value,
    ) -> Result<(), AppError> {
        let descriptor = crate::commands::validate_static_te_descriptor(descriptor)?;
        if descriptor["providerId"].as_str() != Some(provider.id.as_str())
            || !provider.is_token_exchange()
            || provider.has_token_exchange_descriptor()
        {
            return Err(AppError::InvalidInput(
                "te_descriptor_provider_mismatch".into(),
            ));
        }
        let serialized = serde_json::to_string(&descriptor)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        super::providers::save_provider_in_transaction(&tx, "openclaw", provider)?;
        tx.execute(
            "INSERT INTO te_provider_descriptors (provider_id, descriptor) VALUES (?1, ?2)
             ON CONFLICT(provider_id) DO UPDATE SET descriptor = excluded.descriptor",
            params![provider.id, serialized],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn upsert_te_provider_descriptor(&self, descriptor: &Value) -> Result<(), AppError> {
        let descriptor = crate::commands::validate_static_te_descriptor(descriptor)?;
        let id = descriptor["providerId"]
            .as_str()
            .expect("validated provider id");
        let serialized = serde_json::to_string(&descriptor)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO te_provider_descriptors (provider_id, descriptor) VALUES (?1, ?2)
             ON CONFLICT(provider_id) DO UPDATE SET descriptor = excluded.descriptor",
            params![id, serialized],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }

    pub fn get_te_provider_descriptor(&self, provider_id: &str) -> Result<Option<Value>, AppError> {
        let conn = lock_conn!(self.conn);
        let serialized: Option<String> = conn
            .query_row(
                "SELECT descriptor FROM te_provider_descriptors WHERE provider_id = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| AppError::Database(error.to_string()))?;
        serialized
            .map(|raw| {
                let value: Value = serde_json::from_str(&raw)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                crate::commands::validate_static_te_descriptor(&value)
            })
            .transpose()
    }

    pub fn delete_te_provider_descriptor(&self, provider_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM te_provider_descriptors WHERE provider_id = ?1",
            [provider_id],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;

    fn te_record(id: &str, name: &str) -> (Provider, Value) {
        let mut provider = Provider::with_id(
            id.into(),
            name.into(),
            json!({
                "api":"openai-completions", "baseUrl":"http://127.0.0.1:9814/v1",
                "apiKey":"te-provider-placeholder-not-a-secret",
                "models":[{"id":"qwen3.8","name":"Qwen 3.8"}]
            }),
            None,
        );
        provider
            .meta
            .get_or_insert_with(Default::default)
            .provider_type = Some("token_exchange".into());
        let descriptor = json!({
            "providerType":"token-exchange", "providerId":id,
            "pluginId":"token-exchange", "protocolVersion":"te-provider.v1",
            "sidecarUrl":"http://127.0.0.1:9814", "healthPath":"/healthz",
            "expectedPartnerAic":"partner-aic", "models":[{"id":"qwen3.8","name":"Qwen 3.8"}]
        });
        (provider, descriptor)
    }

    #[test]
    fn descriptor_failure_rolls_back_new_provider_and_existing_provider_update(
    ) -> Result<(), AppError> {
        let db = Database::memory().expect("in-memory DB");
        let (candidate, descriptor) = te_record("te-atomic", "initial");
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch("CREATE TEMP TRIGGER te_fail_insert BEFORE INSERT ON te_provider_descriptors BEGIN SELECT RAISE(ABORT, 'denied'); END;")
                .expect("install deterministic failure");
        }
        assert!(db
            .save_te_provider_with_descriptor(&candidate, &descriptor)
            .is_err());
        assert!(db
            .get_provider_by_id("te-atomic", "openclaw")
            .unwrap()
            .is_none());
        assert!(db
            .get_te_provider_descriptor("te-atomic")
            .unwrap()
            .is_none());
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch("DROP TRIGGER te_fail_insert;").unwrap();
        }
        db.save_te_provider_with_descriptor(&candidate, &descriptor)
            .expect("seed pair");
        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch("CREATE TEMP TRIGGER te_fail_update BEFORE UPDATE ON te_provider_descriptors BEGIN SELECT RAISE(ABORT, 'denied'); END;")
                .expect("install update failure");
        }
        let (changed, mut changed_descriptor) = te_record("te-atomic", "changed");
        changed_descriptor["expectedPartnerAic"] = json!("changed-partner");
        assert!(db
            .save_te_provider_with_descriptor(&changed, &changed_descriptor)
            .is_err());
        assert_eq!(
            db.get_provider_by_id("te-atomic", "openclaw")
                .unwrap()
                .unwrap()
                .name,
            "initial"
        );
        assert_eq!(
            db.get_te_provider_descriptor("te-atomic").unwrap(),
            Some(descriptor)
        );
        Ok(())
    }

    #[test]
    fn registry_round_trip_keeps_generic_provider_configuration_independent() {
        let db = Database::memory().expect("in-memory DB");
        let valid = json!({
            "providerType":"token-exchange", "providerId":"te-local",
            "pluginId":"token-exchange", "protocolVersion":"te-provider.v1",
            "sidecarUrl":"http://127.0.0.1:9814", "healthPath":"/healthz",
            "expectedPartnerAic":"partner-aic", "models":[{"id":"qwen3.8","name":"Qwen 3.8"}]
        });
        db.upsert_te_provider_descriptor(&valid)
            .expect("persist static descriptor");
        assert_eq!(
            db.get_te_provider_descriptor("te-local").unwrap(),
            Some(valid.clone())
        );
        let mut invalid = valid;
        invalid["proxyKey"] = json!("secret");
        assert!(db.upsert_te_provider_descriptor(&invalid).is_err());
        assert!(db
            .get_te_provider_descriptor("te-local")
            .unwrap()
            .unwrap()
            .get("proxyKey")
            .is_none());
        db.delete_provider("openclaw", "te-local")
            .expect("idempotent delete");
        assert!(db.get_te_provider_descriptor("te-local").unwrap().is_none());
    }
}

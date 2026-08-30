use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::protocol_compatibility::ProtocolCompatibilityRecord;
use crate::provider::{Provider, ProviderMeta, UniversalProvider};
use indexmap::IndexMap;
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{HashMap, HashSet};

/// 将持久化的供应商配置规范化为 JSON 对象。
///
/// Provider 的前端契约要求 `settingsConfig` 始终可按对象读取。旧版导入、
/// 手工数据库编辑或历史写入可能留下 JSON `null`、标量、数组或损坏文本；
/// 它们不能越过 DAO 边界，否则首屏渲染会在读取配置字段时崩溃。
fn normalize_provider_settings_config(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(_) => value,
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

/// 解析 SQLite 中的供应商配置，并保证返回值符合 Provider 的对象契约。
fn parse_provider_settings_config(settings_config_str: &str) -> serde_json::Value {
    let parsed = serde_json::from_str(settings_config_str).unwrap_or(serde_json::Value::Null);
    normalize_provider_settings_config(parsed)
}

fn codex_route_ids(settings_config: &serde_json::Value) -> HashSet<String> {
    settings_config
        .pointer("/codexRouting/routes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|route| route.get("id").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|route_id| !route_id.is_empty())
        .map(str::to_string)
        .collect()
}

type OmoProviderRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<usize>,
    Option<String>,
    String,
);

#[derive(Debug, Clone)]
pub(crate) struct ProviderSetDatabaseMutation {
    pub app_type: String,
    pub provider_id: String,
    pub provider: Option<Provider>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderSetDatabaseTransaction {
    pub mutations: Vec<ProviderSetDatabaseMutation>,
    pub profile_owner_ids: HashSet<String>,
    pub records: Vec<ProtocolCompatibilityRecord>,
    pub observations: Vec<ProtocolCompatibilityRecord>,
    pub replace_profile_provider_ids: HashSet<String>,
    pub setting_keys_to_delete: Vec<String>,
    pub universal_provider: Option<UniversalProvider>,
    pub current_provider_after: Option<(String, String)>,
    pub official_seed_current_after: Option<(String, String)>,
}

fn ensure_official_seed_in_transaction(
    tx: &Transaction<'_>,
    app_type: &str,
    seed_id: &str,
) -> Result<(), AppError> {
    use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

    let exists = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE id = ?1 AND app_type = ?2)",
            params![seed_id, app_type],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    if exists {
        return Ok(());
    }
    let seed = OFFICIAL_SEEDS
        .iter()
        .find(|seed| seed.id == seed_id && seed.app_type.as_str() == app_type)
        .ok_or_else(|| {
            AppError::Database(format!(
                "unknown official seed: id={seed_id}, app_type={app_type}"
            ))
        })?;
    let settings_config = serde_json::from_str(seed.settings_config_json).map_err(|error| {
        AppError::Database(format!("Seed JSON parse failed for {}: {error}", seed.id))
    })?;
    let next_sort_index = tx
        .query_row(
            "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM providers WHERE app_type = ?1",
            params![app_type],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?
        .max(0) as usize;
    let mut provider = Provider::with_id(
        seed.id.to_string(),
        seed.name.to_string(),
        settings_config,
        Some(seed.website_url.to_string()),
    );
    provider.category = Some("official".to_string());
    provider.icon = Some(seed.icon.to_string());
    provider.icon_color = Some(seed.icon_color.to_string());
    provider.sort_index = Some(next_sort_index);
    provider.created_at = Some(chrono::Utc::now().timestamp_millis());
    save_provider_in_transaction(tx, app_type, &provider)?;
    Ok(())
}

fn save_provider_in_transaction(
    tx: &Transaction<'_>,
    app_type: &str,
    provider: &Provider,
) -> Result<HashSet<String>, AppError> {
    let mut meta_clone = provider.meta.clone().unwrap_or_default();
    let endpoints = std::mem::take(&mut meta_clone.custom_endpoints);
    let existing: Option<(bool, bool, String)> = tx
        .query_row(
            "SELECT is_current, in_failover_queue, settings_config
             FROM providers WHERE id = ?1 AND app_type = ?2",
            params![provider.id, app_type],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| AppError::Database(error.to_string()))?;
    let is_update = existing.is_some();
    let settings_config = normalize_provider_settings_config(provider.settings_config.clone());
    let (is_current, in_failover_queue, removed_route_ids) = match existing {
        Some((is_current, in_failover_queue, previous_settings)) => {
            let removed_route_ids = if app_type == "codex" {
                let previous = parse_provider_settings_config(&previous_settings);
                codex_route_ids(&previous)
                    .difference(&codex_route_ids(&settings_config))
                    .cloned()
                    .collect()
            } else {
                HashSet::new()
            };
            (is_current, in_failover_queue, removed_route_ids)
        }
        None => (false, provider.in_failover_queue, HashSet::new()),
    };
    let settings_json = serde_json::to_string(&settings_config).map_err(|error| {
        AppError::Database(format!("Failed to serialize settings_config: {error}"))
    })?;
    let meta_json = serde_json::to_string(&meta_clone)
        .map_err(|error| AppError::Database(format!("Failed to serialize meta: {error}")))?;

    if is_update {
        tx.execute(
            "UPDATE providers SET
                name = ?1, settings_config = ?2, website_url = ?3, category = ?4,
                created_at = ?5, sort_index = ?6, notes = ?7, icon = ?8,
                icon_color = ?9, meta = ?10, is_current = ?11, in_failover_queue = ?12
             WHERE id = ?13 AND app_type = ?14",
            params![
                provider.name,
                settings_json,
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                meta_json,
                is_current,
                in_failover_queue,
                provider.id,
                app_type,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    } else {
        tx.execute(
            "INSERT INTO providers (
                id, app_type, name, settings_config, website_url, category,
                created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                provider.id,
                app_type,
                provider.name,
                settings_json,
                provider.website_url,
                provider.category,
                provider.created_at,
                provider.sort_index,
                provider.notes,
                provider.icon,
                provider.icon_color,
                meta_json,
                is_current,
                in_failover_queue,
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        for (url, endpoint) in endpoints {
            tx.execute(
                "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![provider.id, app_type, url, endpoint.added_at],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        }
    }
    Ok(removed_route_ids)
}

impl Database {
    pub fn get_all_providers(
        &self,
        app_type: &str,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE app_type = ?1
             ORDER BY COALESCE(sort_index, 999999), created_at ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let provider_iter = stmt
            .query_map(params![app_type], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let settings_config_str: String = row.get(2)?;
                let website_url: Option<String> = row.get(3)?;
                let category: Option<String> = row.get(4)?;
                let created_at: Option<i64> = row.get(5)?;
                let sort_index: Option<usize> = row.get(6)?;
                let notes: Option<String> = row.get(7)?;
                let icon: Option<String> = row.get(8)?;
                let icon_color: Option<String> = row.get(9)?;
                let meta_str: String = row.get(10)?;
                let in_failover_queue: bool = row.get(11)?;

                let settings_config = parse_provider_settings_config(&settings_config_str);
                let meta: ProviderMeta = serde_json::from_str(&meta_str).unwrap_or_default();

                Ok((
                    id,
                    Provider {
                        id: "".to_string(), // Placeholder, set below
                        name,
                        settings_config,
                        website_url,
                        category,
                        created_at,
                        sort_index,
                        notes,
                        meta: Some(meta),
                        icon,
                        icon_color,
                        in_failover_queue,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = IndexMap::new();
        for provider_res in provider_iter {
            let (id, mut provider) = provider_res.map_err(|e| AppError::Database(e.to_string()))?;
            provider.id = id.clone();

            let mut stmt_endpoints = conn.prepare(
                "SELECT url, added_at FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 ORDER BY added_at ASC, url ASC"
            ).map_err(|e| AppError::Database(e.to_string()))?;

            let endpoints_iter = stmt_endpoints
                .query_map(params![id, app_type], |row| {
                    let url: String = row.get(0)?;
                    let added_at: Option<i64> = row.get(1)?;
                    Ok((
                        url,
                        crate::settings::CustomEndpoint {
                            url: "".to_string(),
                            added_at: added_at.unwrap_or(0),
                            last_used: None,
                        },
                    ))
                })
                .map_err(|e| AppError::Database(e.to_string()))?;

            let mut custom_endpoints = HashMap::new();
            for ep_res in endpoints_iter {
                let (url, mut ep) = ep_res.map_err(|e| AppError::Database(e.to_string()))?;
                ep.url = url.clone();
                custom_endpoints.insert(url, ep);
            }

            if let Some(meta) = &mut provider.meta {
                meta.custom_endpoints = custom_endpoints;
            }

            providers.insert(id, provider);
        }

        Ok(providers)
    }

    pub fn get_current_provider(&self, app_type: &str) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            Ok(Some(
                row.get(0).map_err(|e| AppError::Database(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn get_provider_by_id(
        &self,
        id: &str,
        app_type: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, in_failover_queue
             FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
            |row| {
                let name: String = row.get(0)?;
                let settings_config_str: String = row.get(1)?;
                let website_url: Option<String> = row.get(2)?;
                let category: Option<String> = row.get(3)?;
                let created_at: Option<i64> = row.get(4)?;
                let sort_index: Option<usize> = row.get(5)?;
                let notes: Option<String> = row.get(6)?;
                let icon: Option<String> = row.get(7)?;
                let icon_color: Option<String> = row.get(8)?;
                let meta_str: String = row.get(9)?;
                let in_failover_queue: bool = row.get(10)?;

                let settings_config = parse_provider_settings_config(&settings_config_str);
                let meta: ProviderMeta = serde_json::from_str(&meta_str).unwrap_or_default();

                Ok(Provider {
                    id: id.to_string(),
                    name,
                    settings_config,
                    website_url,
                    category,
                    created_at,
                    sort_index,
                    notes,
                    meta: Some(meta),
                    icon,
                    icon_color,
                    in_failover_queue,
                })
            },
        );

        match result {
            Ok(provider) => Ok(Some(provider)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn save_provider(&self, app_type: &str, provider: &Provider) -> Result<(), AppError> {
        self.save_provider_with_protocol_profiles(app_type, provider, &[])
    }

    pub fn save_provider_with_protocol_profile(
        &self,
        app_type: &str,
        provider: &Provider,
        record: &ProtocolCompatibilityRecord,
    ) -> Result<(), AppError> {
        self.save_provider_with_protocol_profiles(app_type, provider, std::slice::from_ref(record))
    }

    pub fn save_provider_with_protocol_profiles(
        &self,
        app_type: &str,
        provider: &Provider,
        records: &[ProtocolCompatibilityRecord],
    ) -> Result<(), AppError> {
        self.save_provider_with_protocol_profiles_for_related_providers(
            app_type,
            provider,
            records,
            &HashSet::new(),
        )
    }

    pub(crate) fn save_provider_with_protocol_profiles_for_related_providers(
        &self,
        app_type: &str,
        provider: &Provider,
        records: &[ProtocolCompatibilityRecord],
        related_provider_ids: &HashSet<String>,
    ) -> Result<(), AppError> {
        if records.iter().any(|record| {
            record.target.provider_id != provider.id
                && !related_provider_ids.contains(&record.target.provider_id)
        }) {
            return Err(AppError::InvalidInput(
                "protocol compatibility profile does not belong to the Provider being saved"
                    .to_string(),
            ));
        }
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        let removed_route_ids = save_provider_in_transaction(&tx, app_type, provider)?;

        for record in records {
            super::protocol_compatibility::save_protocol_compatibility_result_in_transaction(
                &tx, record,
            )?;
        }
        if app_type == "codex" {
            super::protocol_compatibility::delete_protocol_state_for_routes_in_transaction(
                &tx,
                &provider.id,
                &removed_route_ids,
            )?;
        }

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub(crate) fn apply_provider_set_with_protocol_state_and_setting_cleanup(
        &self,
        mutations: &[(&str, &str, Option<&Provider>)],
        profile_owner_id: Option<&str>,
        records: &[ProtocolCompatibilityRecord],
        observations: &[ProtocolCompatibilityRecord],
        related_provider_ids: &HashSet<String>,
        setting_keys_to_delete: &[String],
    ) -> Result<(), AppError> {
        self.apply_provider_set_with_protocol_state_setting_cleanup_and_universal_upsert(
            mutations,
            profile_owner_id,
            records,
            observations,
            related_provider_ids,
            setting_keys_to_delete,
            None,
        )
    }

    pub(crate) fn apply_provider_set_with_protocol_state_setting_cleanup_and_universal_upsert(
        &self,
        mutations: &[(&str, &str, Option<&Provider>)],
        profile_owner_id: Option<&str>,
        records: &[ProtocolCompatibilityRecord],
        observations: &[ProtocolCompatibilityRecord],
        related_provider_ids: &HashSet<String>,
        setting_keys_to_delete: &[String],
        universal_provider: Option<&UniversalProvider>,
    ) -> Result<(), AppError> {
        let mut profile_owner_ids = related_provider_ids.clone();
        if let Some(profile_owner_id) = profile_owner_id {
            profile_owner_ids.insert(profile_owner_id.to_string());
        }
        self.apply_provider_set_database_transaction(ProviderSetDatabaseTransaction {
            mutations: mutations
                .iter()
                .map(
                    |(app_type, provider_id, provider)| ProviderSetDatabaseMutation {
                        app_type: (*app_type).to_string(),
                        provider_id: (*provider_id).to_string(),
                        provider: provider.cloned(),
                    },
                )
                .collect(),
            profile_owner_ids,
            records: records.to_vec(),
            observations: observations.to_vec(),
            replace_profile_provider_ids: HashSet::new(),
            setting_keys_to_delete: setting_keys_to_delete.to_vec(),
            universal_provider: universal_provider.cloned(),
            current_provider_after: None,
            official_seed_current_after: None,
        })
    }

    pub(crate) fn apply_provider_set_database_transaction(
        &self,
        mutation: ProviderSetDatabaseTransaction,
    ) -> Result<(), AppError> {
        if mutation.current_provider_after.is_some()
            && mutation.official_seed_current_after.is_some()
        {
            return Err(AppError::InvalidInput(
                "Provider set transaction contains conflicting current-provider transitions"
                    .to_string(),
            ));
        }
        if mutation.records.iter().any(|record| {
            !mutation
                .profile_owner_ids
                .contains(&record.target.provider_id)
        }) {
            return Err(AppError::InvalidInput(
                "protocol compatibility profile does not belong to the Provider set being saved"
                    .to_string(),
            ));
        }
        if mutation.observations.iter().any(|record| {
            !mutation
                .profile_owner_ids
                .contains(&record.target.provider_id)
        }) {
            return Err(AppError::InvalidInput(
                "protocol probe observation does not belong to the Provider set being saved"
                    .to_string(),
            ));
        }
        let mut mutation_keys = HashSet::new();
        for item in &mutation.mutations {
            if item
                .provider
                .as_ref()
                .is_some_and(|provider| provider.id != item.provider_id)
            {
                return Err(AppError::InvalidInput(format!(
                    "Provider mutation id mismatch: expected {}",
                    item.provider_id
                )));
            }
            if !mutation_keys.insert((item.app_type.clone(), item.provider_id.clone())) {
                return Err(AppError::InvalidInput(format!(
                    "Provider set contains duplicate mutation for {}/{}",
                    item.app_type, item.provider_id
                )));
            }
        }

        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| AppError::Database(error.to_string()))?;
        if let Some(provider) = mutation.universal_provider.as_ref() {
            super::universal_providers::save_universal_provider_in_transaction(&tx, provider)?;
        }
        for provider_id in &mutation.replace_profile_provider_ids {
            tx.execute(
                "DELETE FROM protocol_compatibility_profiles WHERE provider_id = ?1",
                params![provider_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        }
        let mut deleted_codex_provider_ids = HashSet::new();
        let mut removed_codex_routes = HashMap::<String, HashSet<String>>::new();
        for item in &mutation.mutations {
            if let Some(provider) = item.provider.as_ref() {
                let removed_route_ids =
                    save_provider_in_transaction(&tx, &item.app_type, provider)?;
                if item.app_type == "codex" && !removed_route_ids.is_empty() {
                    removed_codex_routes
                        .entry(provider.id.clone())
                        .or_default()
                        .extend(removed_route_ids);
                }
            } else {
                tx.execute(
                    "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
                    params![item.provider_id, item.app_type],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                if item.app_type == "codex" {
                    deleted_codex_provider_ids.insert(item.provider_id.clone());
                }
            }
        }
        for record in &mutation.records {
            super::protocol_compatibility::save_protocol_compatibility_result_in_transaction(
                &tx, record,
            )?;
        }
        for observation in &mutation.observations {
            super::protocol_compatibility::save_protocol_probe_observation_in_transaction(
                &tx,
                observation,
            )?;
        }
        for key in &mutation.setting_keys_to_delete {
            tx.execute("DELETE FROM settings WHERE key = ?1", params![key])
                .map_err(|error| AppError::Database(error.to_string()))?;
        }
        for provider_id in deleted_codex_provider_ids {
            super::protocol_compatibility::delete_protocol_state_for_provider_in_transaction(
                &tx,
                &provider_id,
            )?;
        }
        for (provider_id, route_ids) in removed_codex_routes {
            super::protocol_compatibility::delete_protocol_state_for_routes_in_transaction(
                &tx,
                &provider_id,
                &route_ids,
            )?;
        }
        if let Some((app_type, provider_id)) = mutation.official_seed_current_after.as_ref() {
            ensure_official_seed_in_transaction(&tx, app_type, provider_id)?;
        }
        let current_provider_after = mutation
            .current_provider_after
            .as_ref()
            .or(mutation.official_seed_current_after.as_ref());
        if let Some((app_type, provider_id)) = current_provider_after {
            tx.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
                params![app_type],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
            let changed = tx
                .execute(
                    "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
                    params![provider_id, app_type],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
            if changed != 1 {
                return Err(AppError::InvalidInput(format!(
                    "Provider set current target does not exist: {app_type}/{provider_id}"
                )));
            }
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn delete_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        tx.execute(
            "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        if app_type == "codex" {
            super::protocol_compatibility::delete_protocol_state_for_provider_in_transaction(
                &tx, id,
            )?;
        }
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))
    }

    pub fn set_current_provider(&self, app_type: &str, id: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            params![id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_provider_settings_config(
        &self,
        app_type: &str,
        provider_id: &str,
        settings_config: &serde_json::Value,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = ?3",
            params![
                serde_json::to_string(&normalize_provider_settings_config(settings_config.clone()))
                    .map_err(|e| AppError::Database(format!(
                        "Failed to serialize settings_config: {e}"
                    )))?,
                provider_id,
                app_type
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// Atomically replace only `settingsConfig.codexRouting.subagentV2` in the latest Codex
    /// provider row. The read and write share one IMMEDIATE transaction so an editor snapshot
    /// can never overwrite catalog, alias, route, credential, or future-field refreshes that
    /// committed before this operation acquired the writer boundary.
    pub fn update_codex_subagent_v2<M, V>(
        &self,
        provider_id: &str,
        mutate: M,
        validate: V,
    ) -> Result<serde_json::Value, AppError>
    where
        M: FnOnce(&serde_json::Value) -> Result<serde_json::Value, AppError>,
        V: FnOnce(&serde_json::Value) -> Result<(), AppError>,
    {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let settings_config_str = tx
            .query_row(
                "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = 'codex'",
                params![provider_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    AppError::InvalidInput("Codex provider does not exist".to_string())
                }
                other => AppError::Database(other.to_string()),
            })?;
        let mut settings_config = parse_provider_settings_config(&settings_config_str);
        let subagent_v2 = mutate(&settings_config)?;
        let settings = settings_config.as_object_mut().ok_or_else(|| {
            AppError::Database("Provider settings normalization failed".to_string())
        })?;
        let routing = settings
            .entry("codexRouting".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !routing.is_object() {
            *routing = serde_json::json!({});
        }
        routing
            .as_object_mut()
            .ok_or_else(|| AppError::Database("Codex routing normalization failed".to_string()))?
            .insert("subagentV2".to_string(), subagent_v2);
        validate(&settings_config)?;
        let serialized = serde_json::to_string(&settings_config)
            .map_err(|e| AppError::Database(format!("Failed to serialize settings_config: {e}")))?;
        let changed = tx
            .execute(
                "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = 'codex'",
                params![serialized, provider_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if changed != 1 {
            return Err(AppError::Database(
                "Focused Codex subagent V2 update changed an unexpected row count".to_string(),
            ));
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(settings_config)
    }

    pub fn add_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        let added_at = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![provider_id, app_type, url, added_at],
        ).map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_custom_endpoint(
        &self,
        app_type: &str,
        provider_id: &str,
        url: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2 AND url = ?3",
            params![provider_id, app_type, url],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn set_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;
        tx.execute(
            "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
            params![app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        // OMO ↔ OMO Slim mutually exclusive: deactivate the opposite category
        let opposite = match category {
            "omo" => Some("omo-slim"),
            "omo-slim" => Some("omo"),
            _ => None,
        };
        if let Some(opp) = opposite {
            tx.execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1 AND category = ?2",
                params![app_type, opp],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        let updated = tx
            .execute(
                "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2 AND category = ?3",
                params![provider_id, app_type, category],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        if updated != 1 {
            return Err(AppError::Database(format!(
                "Failed to set {category} provider current: provider '{provider_id}' not found in app '{app_type}'"
            )));
        }
        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn is_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        match conn.query_row(
            "SELECT is_current FROM providers
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
            |row| row.get(0),
        ) {
            Ok(is_current) => Ok(is_current),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    pub fn clear_omo_provider_current(
        &self,
        app_type: &str,
        provider_id: &str,
        category: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE providers SET is_current = 0
             WHERE id = ?1 AND app_type = ?2 AND category = ?3",
            params![provider_id, app_type, category],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_current_omo_provider(
        &self,
        app_type: &str,
        category: &str,
    ) -> Result<Option<Provider>, AppError> {
        let conn = lock_conn!(self.conn);
        let row_data: Result<OmoProviderRow, rusqlite::Error> = conn.query_row(
            "SELECT id, name, settings_config, category, created_at, sort_index, notes, meta
             FROM providers
             WHERE app_type = ?1 AND category = ?2 AND is_current = 1
             LIMIT 1",
            params![app_type, category],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        );

        let (id, name, settings_config_str, _row_category, created_at, sort_index, notes, meta_str) =
            match row_data {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(AppError::Database(e.to_string())),
            };

        let settings_config = parse_provider_settings_config(&settings_config_str);
        let meta: crate::provider::ProviderMeta = if meta_str.trim().is_empty() {
            crate::provider::ProviderMeta::default()
        } else {
            serde_json::from_str(&meta_str).map_err(|e| {
                AppError::Database(format!(
                    "Failed to parse {category} provider meta (provider_id={id}): {e}"
                ))
            })?
        };

        Ok(Some(Provider {
            id,
            name,
            settings_config,
            website_url: None,
            category: Some(category.to_string()),
            created_at,
            sort_index,
            notes,
            meta: Some(meta),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }))
    }

    /// 判断 providers 表是否为空（全 app_type 一起算）。
    ///
    /// 用于区分"全新安装"和"升级用户"：在启动流程 import/seed 之前调用。
    /// 使用 `EXISTS` 短路查询，比 `COUNT(*)` 在将来表变大时更高效。
    pub fn is_providers_empty(&self) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM providers)", [], |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(!exists)
    }

    /// 仅获取指定 app 下所有 provider 的 id 集合。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询。
    /// 用于只需要做存在性检查的场景（如 additive 模式的 live 同步去重）。
    pub fn get_provider_ids(&self, app_type: &str) -> Result<HashSet<String>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![app_type], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut ids = HashSet::new();
        for row in rows {
            ids.insert(row.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(ids)
    }

    /// 判断指定 app 下是否已存在任意 provider。
    ///
    /// 启动阶段的 live import 需要使用这个更严格的判断：
    /// 只要该 app 已经有任何 provider（包括官方 seed），就不应再自动导入 `default`。
    pub fn has_any_provider_for_app(&self, app_type: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE app_type = ?1)",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(exists)
    }

    /// 判断指定 app 下是否存在非官方种子的供应商。
    ///
    /// 比 `get_all_providers` 轻量得多：只读 id 列、无 endpoint 子查询、首条命中即返回。
    /// 用于 `import_default_config` 决定是否跳过 live 导入。
    pub fn has_non_official_seed_provider(&self, app_type: &str) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::is_official_seed_id;
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare("SELECT id FROM providers WHERE app_type = ?1")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut rows = stmt
            .query(params![app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let id: String = row.get(0).map_err(|e| AppError::Database(e.to_string()))?;
            if !is_official_seed_id(&id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 计算指定 app 下一个可用的 sort_index（追加到末尾）。
    fn next_sort_index_for_app(&self, app_type: &str) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(sort_index) FROM providers WHERE app_type = ?1",
                params![app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(max.map(|v| (v + 1) as usize).unwrap_or(0))
    }

    /// 启动时调用：补齐缺失的官方预设供应商（Claude / Codex / Gemini）。
    ///
    /// 使用 settings flag `official_providers_seeded` 保证每个数据库只执行一次：
    /// - 全新用户：seed 三条官方预设
    /// - 老用户升级：同样会触发一次（flag 不存在），追加到末尾，不影响已有排序
    /// - 用户删除 seed 后：不再重建（flag 已为 true），尊重用户意图
    ///
    /// 与 `Database::save_provider` 的 UPSERT 语义配合，即使被意外重复调用
    /// 也不会覆盖用户当前激活的供应商（is_current 字段会被保留）。
    pub fn init_default_official_providers(&self) -> Result<usize, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        if self
            .get_bool_flag("official_providers_seeded")
            .unwrap_or(false)
        {
            return Ok(0);
        }

        let mut inserted = 0_usize;
        let now_ms = chrono::Utc::now().timestamp_millis();

        for seed in OFFICIAL_SEEDS {
            let app_type_str = seed.app_type.as_str();

            // 若该 id 已存在（极端情况：用户曾手动用过同 id），跳过
            if self.get_provider_by_id(seed.id, app_type_str)?.is_some() {
                continue;
            }

            let next_sort_index = self.next_sort_index_for_app(app_type_str)?;

            let settings_config: serde_json::Value =
                serde_json::from_str(seed.settings_config_json).map_err(|e| {
                    AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
                })?;

            let mut provider = Provider::with_id(
                seed.id.to_string(),
                seed.name.to_string(),
                settings_config,
                Some(seed.website_url.to_string()),
            );
            provider.category = Some("official".to_string());
            provider.icon = Some(seed.icon.to_string());
            provider.icon_color = Some(seed.icon_color.to_string());
            provider.sort_index = Some(next_sort_index);
            provider.created_at = Some(now_ms);

            self.save_provider(app_type_str, &provider)?;
            inserted += 1;
            log::info!(
                "✓ Seeded official provider: {} ({})",
                seed.name,
                app_type_str
            );
        }

        // 即使 inserted=0（例如用户手动创建过同 id）也设置 flag 防止反复检查
        self.set_setting("official_providers_seeded", "true")?;

        Ok(inserted)
    }

    /// 按 id 兜底插入单条 official seed（仅当目标表中该 id 不存在时插入）。
    ///
    /// 与 `init_default_official_providers` 不同：
    /// - 不触碰 `official_providers_seeded` 全局 flag，是 on-demand 修复
    /// - 只处理一条 seed，由调用方决定 id + app_type
    /// - 已存在则尊重用户自定义，不覆盖
    ///
    /// 返回 Ok(true) 表示插入了新行，Ok(false) 表示已存在被跳过。
    pub fn ensure_official_seed_by_id(
        &self,
        seed_id: &str,
        app_type: crate::app_config::AppType,
    ) -> Result<bool, AppError> {
        use crate::database::dao::providers_seed::OFFICIAL_SEEDS;

        let seed = OFFICIAL_SEEDS
            .iter()
            .find(|s| s.id == seed_id && s.app_type == app_type)
            .ok_or_else(|| {
                AppError::Database(format!(
                    "unknown official seed: id={seed_id}, app_type={}",
                    app_type.as_str()
                ))
            })?;

        let app_type_str = seed.app_type.as_str();

        if self.get_provider_by_id(seed_id, app_type_str)?.is_some() {
            return Ok(false);
        }

        let settings_config: serde_json::Value = serde_json::from_str(seed.settings_config_json)
            .map_err(|e| {
                AppError::Database(format!("Seed JSON parse failed for {}: {e}", seed.id))
            })?;

        let next_sort_index = self.next_sort_index_for_app(app_type_str)?;
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut provider = Provider::with_id(
            seed.id.to_string(),
            seed.name.to_string(),
            settings_config,
            Some(seed.website_url.to_string()),
        );
        provider.category = Some("official".to_string());
        provider.icon = Some(seed.icon.to_string());
        provider.icon_color = Some(seed.icon_color.to_string());
        provider.sort_index = Some(next_sort_index);
        provider.created_at = Some(now_ms);

        self.save_provider(app_type_str, &provider)?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };
    use crate::provider::UniversalProvider;
    use serde_json::json;

    fn provider(id: &str, state: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            id.to_string(),
            json!({"state": state}),
            None,
        )
    }

    fn profile(
        provider_id: &str,
        model: &str,
        transport: TransportKind,
    ) -> ProtocolCompatibilityRecord {
        ProtocolCompatibilityRecord::new(
            ProbeTargetKey::new(
                provider_id,
                None::<String>,
                model,
                model,
                transport,
                "https://relay.example/v1/responses",
                "bearer",
            )
            .expect("target"),
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(transport),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            100,
            200,
        )
    }

    fn upsert(provider: Provider) -> ProviderSetDatabaseMutation {
        ProviderSetDatabaseMutation {
            app_type: "codex".to_string(),
            provider_id: provider.id.clone(),
            provider: Some(provider),
        }
    }

    fn delete(provider_id: &str) -> ProviderSetDatabaseMutation {
        ProviderSetDatabaseMutation {
            app_type: "codex".to_string(),
            provider_id: provider_id.to_string(),
            provider: None,
        }
    }

    fn database_transaction(
        mutations: Vec<ProviderSetDatabaseMutation>,
    ) -> ProviderSetDatabaseTransaction {
        ProviderSetDatabaseTransaction {
            mutations,
            profile_owner_ids: HashSet::new(),
            records: Vec::new(),
            observations: Vec::new(),
            replace_profile_provider_ids: HashSet::new(),
            setting_keys_to_delete: Vec::new(),
            universal_provider: None,
            current_provider_after: None,
            official_seed_current_after: None,
        }
    }

    #[test]
    fn provider_set_transaction_rolls_back_source_leaf_profile_universal_and_current_state() {
        let db = Database::memory().expect("memory database");
        db.save_provider("codex", &provider("relay", "before"))
            .expect("seed source");
        db.set_current_provider("codex", "relay")
            .expect("activate source");
        let mut universal = UniversalProvider::new(
            "universal".to_string(),
            "Universal before".to_string(),
            "custom".to_string(),
            "https://before.example/v1".to_string(),
            "before-key".to_string(),
        );
        db.save_universal_provider(&universal)
            .expect("seed Universal definition");

        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER fail_second_protocol_leaf
                 BEFORE INSERT ON providers
                 WHEN NEW.id = 'relay--ccsm-chat'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected second leaf failure');
                 END;",
            )
            .expect("install failure trigger");
        }

        universal.name = "Universal after".to_string();
        let record = profile(
            "relay--ccsm-responses",
            "model-a",
            TransportKind::OpenAiResponses,
        );
        let mut transaction = database_transaction(vec![
            upsert(provider("relay", "after")),
            upsert(provider("relay--ccsm-responses", "after")),
            upsert(provider("relay--ccsm-chat", "after")),
        ]);
        transaction.profile_owner_ids = [
            "relay".to_string(),
            "relay--ccsm-responses".to_string(),
            "relay--ccsm-chat".to_string(),
        ]
        .into_iter()
        .collect();
        transaction.records = vec![record.clone()];
        let observation = profile("relay", "model-a", TransportKind::OpenAiChat);
        transaction.observations = vec![observation.clone()];
        transaction.universal_provider = Some(universal);
        transaction.current_provider_after = Some(("codex".to_string(), "relay".to_string()));

        let error = db
            .apply_provider_set_database_transaction(transaction)
            .expect_err("second leaf must abort transaction");
        assert!(error.to_string().contains("injected second leaf failure"));
        assert_eq!(
            db.get_provider_by_id("relay", "codex")
                .expect("read source")
                .expect("source remains")
                .settings_config["state"],
            "before"
        );
        assert!(db
            .get_provider_by_id("relay--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_none());
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_none());
        assert!(db
            .get_protocol_compatibility_result(&record.target)
            .expect("read profile")
            .is_none());
        assert!(db
            .list_protocol_probe_observations("relay")
            .expect("read observations")
            .is_empty());
        assert_eq!(
            db.get_universal_provider("universal")
                .expect("read Universal")
                .expect("Universal remains")
                .name,
            "Universal before"
        );
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("relay".to_string())
        );
    }

    #[test]
    fn provider_set_transaction_commits_probe_observations_with_provider_and_profile() {
        let db = Database::memory().expect("memory database");
        let selected = profile("relay", "model-a", TransportKind::OpenAiResponses);
        let responses = profile("relay", "model-a", TransportKind::OpenAiResponses);
        let chat = profile("relay", "model-a", TransportKind::OpenAiChat);
        let mut transaction = database_transaction(vec![upsert(provider("relay", "ready"))]);
        transaction.profile_owner_ids = ["relay".to_string()].into_iter().collect();
        transaction.records = vec![selected.clone()];
        transaction.observations = vec![responses.clone(), chat.clone()];

        db.apply_provider_set_database_transaction(transaction)
            .expect("commit Provider Set bundle");

        assert!(db
            .get_provider_by_id("relay", "codex")
            .expect("read provider")
            .is_some());
        assert_eq!(
            db.get_protocol_compatibility_result(&selected.target)
                .expect("read selected profile"),
            Some(selected)
        );
        let observations = db
            .list_protocol_probe_observations("relay")
            .expect("read observations");
        assert_eq!(observations.len(), 2);
        assert!(observations.contains(&responses));
        assert!(observations.contains(&chat));
    }

    #[test]
    fn provider_set_transaction_replaces_profiles_and_moves_current_leaf_to_source() {
        let db = Database::memory().expect("memory database");
        db.save_provider("codex", &provider("relay", "facade"))
            .expect("seed source");
        db.save_provider("codex", &provider("relay--ccsm-responses", "old leaf"))
            .expect("seed Responses leaf");
        db.save_provider("codex", &provider("relay--ccsm-chat", "old leaf"))
            .expect("seed Chat leaf");
        db.set_current_provider("codex", "relay--ccsm-chat")
            .expect("activate old leaf");
        let old_source = profile("relay", "old-model", TransportKind::OpenAiResponses);
        let old_chat = profile("relay--ccsm-chat", "model-b", TransportKind::OpenAiChat);
        db.save_protocol_compatibility_result(&old_source)
            .expect("seed old source profile");
        db.save_protocol_compatibility_result(&old_chat)
            .expect("seed old leaf profile");
        let new_source = profile("relay", "model-a", TransportKind::OpenAiResponses);

        let mut transaction = database_transaction(vec![
            upsert(provider("relay", "single")),
            delete("relay--ccsm-responses"),
            delete("relay--ccsm-chat"),
        ]);
        transaction.profile_owner_ids = ["relay".to_string()].into_iter().collect();
        transaction.records = vec![new_source.clone()];
        transaction.replace_profile_provider_ids = ["relay".to_string()].into_iter().collect();
        transaction.current_provider_after = Some(("codex".to_string(), "relay".to_string()));
        db.apply_provider_set_database_transaction(transaction)
            .expect("fold split Provider Set");

        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("relay".to_string())
        );
        assert!(db
            .get_provider_by_id("relay--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_none());
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_none());
        assert!(db
            .get_protocol_compatibility_result(&old_source.target)
            .expect("read old source profile")
            .is_none());
        assert!(db
            .get_protocol_compatibility_result(&old_chat.target)
            .expect("read old leaf profile")
            .is_none());
        assert_eq!(
            db.get_protocol_compatibility_result(&new_source.target)
                .expect("read new source profile"),
            Some(new_source)
        );
    }

    /// 回归旧数据库中合法 JSON `null`、标量、数组或损坏文本，读取时不能把非对象配置暴露给前端。
    #[test]
    fn provider_settings_config_parser_only_returns_objects() {
        for raw_config in ["null", "[]", "\"legacy\"", "not-json"] {
            assert_eq!(
                parse_provider_settings_config(raw_config),
                json!({}),
                "non-object config {raw_config:?} should normalize to an empty object"
            );
        }

        assert_eq!(
            parse_provider_settings_config(r#"{"env":{"ANTHROPIC_API_KEY":"sk-test"}}"#),
            json!({"env":{"ANTHROPIC_API_KEY":"sk-test"}})
        );
    }

    /// 验证列表读取、按 ID 读取和局部更新共享同一对象契约，避免首屏再次收到 `settingsConfig: null`。
    #[test]
    fn provider_dao_normalizes_non_object_settings_config_on_read_and_write() {
        let db = Database::memory().expect("create in-memory database");
        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta, in_failover_queue)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "legacy-null-config",
                    "codex",
                    "Legacy null config",
                    "null",
                    "{}",
                    false
                ],
            )
            .expect("insert legacy provider");
        }

        let providers = db
            .get_all_providers("codex")
            .expect("list providers with legacy config");
        assert_eq!(
            providers
                .get("legacy-null-config")
                .expect("legacy provider should be listed")
                .settings_config,
            json!({})
        );

        let provider = db
            .get_provider_by_id("legacy-null-config", "codex")
            .expect("load provider by id")
            .expect("legacy provider should exist");
        assert_eq!(provider.settings_config, json!({}));

        db.update_provider_settings_config("codex", "legacy-null-config", &serde_json::Value::Null)
            .expect("normalize null config on update");
        let conn = db.conn.lock().expect("lock database");
        let stored_config: String = conn
            .query_row(
                "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2",
                params!["legacy-null-config", "codex"],
                |row| row.get(0),
            )
            .expect("read normalized stored config");
        assert_eq!(stored_config, "{}");
    }

    /// Deterministic TOCTOU regression: the editor's stale snapshot is read first, then a
    /// catalog/alias refresh lands, and finally the focused V2 write must merge into that latest
    /// row instead of replacing it with the stale snapshot.
    #[test]
    fn codex_subagent_v2_atomic_update_preserves_interleaved_catalog_alias_refresh() {
        let db = Database::memory().expect("create in-memory database");
        let original = Provider::with_id(
            "router".to_string(),
            "Codex MultiRouter".to_string(),
            json!({
                "auth": { "apiKey": "credential-must-survive" },
                "modelCatalog": {
                    "models": [{ "model": "deepseek-v4-flash" }],
                    "aliases": { "flash": "deepseek-v4-flash" }
                },
                "codexRouting": {
                    "routes": [{ "id": "route-before-refresh" }],
                    "qwen": { "enabled": true },
                    "subagentVersion": "v2",
                    "subagentV2": {
                        "schemaVersion": 1,
                        "selectionPolicy": "balanced",
                        "profiles": {}
                    },
                    "futureRoutingField": { "preserve": true }
                },
                "futureProviderField": { "preserve": true }
            }),
            None,
        );
        db.save_provider("codex", &original).expect("seed provider");

        let stale_snapshot = db
            .get_provider_by_id("router", "codex")
            .expect("read stale editor snapshot")
            .expect("provider exists");

        let mut refreshed_settings = stale_snapshot.settings_config.clone();
        refreshed_settings["modelCatalog"]["aliases"]["flash"] = json!("deepseek-v4-flash-live");
        refreshed_settings["codexRouting"]["routes"] = json!([{ "id": "route-after-refresh" }]);
        refreshed_settings["codexRouting"]["catalogAliases"] =
            json!({ "flash": "deepseek-v4-flash-live" });
        db.update_provider_settings_config("codex", "router", &refreshed_settings)
            .expect("interleaved catalog/alias refresh");

        let edited_v2 = json!({
            "schemaVersion": 1,
            "selectionPolicy": "official_first",
            "profiles": {
                "deepseek-v4-flash": {
                    "model": "deepseek-v4-flash",
                    "enabled": true,
                    "questionnaire": {
                        "taskStrengths": ["repository_exploration"],
                        "optimization": "speed",
                        "writeScope": "read_only",
                        "preference": "eligible",
                        "reasoningEffort": "medium"
                    }
                }
            }
        });
        db.update_codex_subagent_v2("router", |_| Ok(edited_v2.clone()), |_| Ok(()))
            .expect("atomically merge V2 into latest provider row");

        let saved = db
            .get_provider_by_id("router", "codex")
            .expect("read merged provider")
            .expect("provider still exists");
        assert_eq!(
            saved.settings_config["codexRouting"]["subagentV2"],
            edited_v2
        );
        assert_eq!(
            saved.settings_config["modelCatalog"]["aliases"]["flash"],
            json!("deepseek-v4-flash-live")
        );
        assert_eq!(
            saved.settings_config["codexRouting"]["routes"],
            json!([{ "id": "route-after-refresh" }])
        );
        assert_eq!(
            saved.settings_config["codexRouting"]["catalogAliases"],
            json!({ "flash": "deepseek-v4-flash-live" })
        );
        assert_eq!(
            saved.settings_config["codexRouting"]["qwen"],
            json!({ "enabled": true })
        );
        assert_eq!(
            saved.settings_config["codexRouting"]["futureRoutingField"],
            json!({ "preserve": true })
        );
        assert_eq!(
            saved.settings_config["auth"]["apiKey"],
            json!("credential-must-survive")
        );
        assert_eq!(
            saved.settings_config["futureProviderField"],
            json!({ "preserve": true })
        );
    }
}

#[cfg(test)]
mod ensure_official_seed_tests {
    use crate::app_config::AppType;
    use crate::database::{
        Database, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, CODEX_OFFICIAL_PROVIDER_ID,
        GROKBUILD_OFFICIAL_PROVIDER_ID,
    };

    #[test]
    fn ensure_inserts_when_missing() {
        let db = Database::memory().expect("memory db");
        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(inserted, "should insert when missing");

        let provider = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("provider exists after ensure");

        assert_eq!(provider.id, CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID);
        assert_eq!(provider.name, "Claude Desktop Official");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.icon.as_deref(), Some("anthropic"));
        assert_eq!(provider.icon_color.as_deref(), Some("#D4915D"));
    }

    #[test]
    fn ensure_skips_when_present_and_preserves_customization() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");

        let mut renamed = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("seed present");
        renamed.name = "My Custom Backup".to_string();
        db.save_provider(AppType::ClaudeDesktop.as_str(), &renamed)
            .expect("save customization");

        let inserted = db
            .ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::ClaudeDesktop)
            .expect("ensure ok");
        assert!(!inserted, "should skip when present");

        let after = db
            .get_provider_by_id(
                CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
                AppType::ClaudeDesktop.as_str(),
            )
            .expect("query ok")
            .expect("still present");
        assert_eq!(
            after.name, "My Custom Backup",
            "customization must not be overwritten"
        );
    }

    #[test]
    fn ensure_recreates_codex_official_seed_after_deletion() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");
        db.delete_provider(AppType::Codex.as_str(), CODEX_OFFICIAL_PROVIDER_ID)
            .expect("delete Codex official");

        let inserted = db
            .ensure_official_seed_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
            .expect("ensure Codex official");
        assert!(inserted);
        let provider = db
            .get_provider_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex.as_str())
            .expect("query")
            .expect("Codex official restored");
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(provider.settings_config["auth"], serde_json::json!({}));
    }

    #[test]
    fn ensure_recreates_grokbuild_official_seed_after_deletion() {
        let db = Database::memory().expect("memory db");
        db.init_default_official_providers().expect("seed");
        db.delete_provider(AppType::GrokBuild.as_str(), GROKBUILD_OFFICIAL_PROVIDER_ID)
            .expect("delete Grok Build official");

        let inserted = db
            .ensure_official_seed_by_id(GROKBUILD_OFFICIAL_PROVIDER_ID, AppType::GrokBuild)
            .expect("ensure Grok Build official");
        assert!(inserted);
        let provider = db
            .get_provider_by_id(GROKBUILD_OFFICIAL_PROVIDER_ID, AppType::GrokBuild.as_str())
            .expect("query")
            .expect("Grok Build official restored");
        assert_eq!(provider.category.as_deref(), Some("official"));
        // 空 config：切换时不注入自定义模型表，Grok CLI 回落到自带 OAuth 登录
        assert_eq!(provider.settings_config["config"], serde_json::json!(""));
    }

    #[test]
    fn ensure_rejects_unknown_seed() {
        let db = Database::memory().expect("memory db");
        let result = db.ensure_official_seed_by_id("nonexistent-id", AppType::ClaudeDesktop);
        assert!(result.is_err(), "unknown seed id should be Err");
    }

    #[test]
    fn ensure_rejects_seed_app_type_mismatch() {
        let db = Database::memory().expect("memory db");
        let result =
            db.ensure_official_seed_by_id(CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID, AppType::Claude);
        assert!(result.is_err(), "(id, app_type) mismatch should be Err");
    }
}

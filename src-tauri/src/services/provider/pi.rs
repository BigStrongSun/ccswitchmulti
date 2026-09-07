use crate::error::AppError;
use crate::pi_config::{PiModelsStore, PiModelsWriteOutcome};
use crate::provider::{Provider, ProviderMeta};
use crate::store::AppState;
use indexmap::IndexMap;
use serde_json::Value;

const PI_APP: &str = "pi";

fn default_store() -> Result<PiModelsStore, AppError> {
    Ok(PiModelsStore::new(crate::pi_config::get_pi_models_path()?))
}

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    list_with_store(state, &default_store()?)
}

pub(super) fn add(
    state: &AppState,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    add_with_store(state, &default_store()?, provider, add_to_live)
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    provider: Provider,
) -> Result<bool, AppError> {
    let original_id = original_id.unwrap_or(&provider.id);
    if original_id != provider.id {
        return Err(AppError::InvalidInput(
            "Pi provider keys cannot be renamed".to_string(),
        ));
    }
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    update_with_store(state, &default_store()?, provider)
}

pub(super) fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    remove_with_store(state, &default_store()?, id)
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    delete_with_store(state, &default_store()?, id)
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = super::block_on_tauri_runtime(state.proxy_service.lock_switch_for_app(PI_APP));
    enable_with_store(state, &default_store()?, id)
}

pub(super) fn list_with_store(
    state: &AppState,
    store: &PiModelsStore,
) -> Result<IndexMap<String, Provider>, AppError> {
    let snapshot = store.read_snapshot()?;
    let saved = state.db.get_all_providers(PI_APP)?;

    for (id, config) in &snapshot.providers {
        let mut provider = saved.get(id).cloned().unwrap_or_else(|| {
            let name = native_provider_name(config).unwrap_or(id).to_string();
            let mut imported = Provider::with_id(id.clone(), name, config.clone(), None);
            imported.category = Some("custom".to_string());
            imported.icon = Some("pi".to_string());
            imported
        });
        if let Some(name) = native_provider_name(config) {
            provider.name = name.to_string();
        }
        provider.settings_config = config.clone();
        set_pi_content_version(&mut provider, &snapshot.content_version);
        state.db.save_provider(PI_APP, &provider)?;
    }

    // Database-only cards need the same current document version so a later
    // enable operation can prove that the list the user acted on is fresh.
    for (id, mut provider) in saved {
        if snapshot.providers.contains_key(&id) {
            continue;
        }
        set_pi_content_version(&mut provider, &snapshot.content_version);
        state.db.save_provider(PI_APP, &provider)?;
    }

    state.db.get_all_providers(PI_APP)
}

pub(super) fn add_with_store(
    state: &AppState,
    store: &PiModelsStore,
    mut provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    validate_provider(&provider)?;
    if state.db.get_provider_by_id(&provider.id, PI_APP)?.is_some() {
        return Err(AppError::InvalidInput(format!(
            "Pi provider '{}' already exists",
            provider.id
        )));
    }

    let expected_version = pi_content_version(&provider)?.to_string();
    let snapshot = store.read_snapshot()?;
    ensure_snapshot_version(&provider.id, &expected_version, &snapshot.content_version)?;
    if snapshot.providers.contains_key(&provider.id) {
        return Err(AppError::InvalidInput(format!(
            "Pi provider key '{}' already exists in models.json",
            provider.id
        )));
    }

    if !add_to_live {
        set_pi_content_version(&mut provider, &snapshot.content_version);
        state.db.save_provider(PI_APP, &provider)?;
        return Ok(true);
    }

    let outcome = store.put_provider(&provider.id, &provider.settings_config, &expected_version)?;
    set_pi_content_version(&mut provider, &outcome.content_version);
    if let Err(error) = state.db.save_provider(PI_APP, &provider) {
        rollback_insert(store, &provider.id, &outcome, error)?;
    }
    Ok(true)
}

pub(super) fn enable_with_store(
    state: &AppState,
    store: &PiModelsStore,
    id: &str,
) -> Result<(), AppError> {
    let mut provider = state
        .db
        .get_provider_by_id(id, PI_APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    validate_provider(&provider)?;
    let expected_version = pi_content_version(&provider)?.to_string();
    let snapshot = store.read_snapshot()?;
    ensure_snapshot_version(id, &expected_version, &snapshot.content_version)?;

    if let Some(native) = snapshot.providers.get(id) {
        provider.settings_config = native.clone();
        if let Some(name) = native_provider_name(native) {
            provider.name = name.to_string();
        }
        set_pi_content_version(&mut provider, &snapshot.content_version);
        state.db.save_provider(PI_APP, &provider)?;
        return Ok(());
    }

    let outcome = store.put_provider(id, &provider.settings_config, &expected_version)?;
    set_pi_content_version(&mut provider, &outcome.content_version);
    if let Err(error) = state.db.save_provider(PI_APP, &provider) {
        rollback_insert(store, id, &outcome, error)?;
    }
    Ok(())
}

pub(super) fn delete_with_store(
    state: &AppState,
    store: &PiModelsStore,
    id: &str,
) -> Result<(), AppError> {
    let Some(provider) = state.db.get_provider_by_id(id, PI_APP)? else {
        return Ok(());
    };
    let expected_version = pi_content_version(&provider)?.to_string();
    let snapshot = store.read_snapshot()?;
    ensure_snapshot_version(id, &expected_version, &snapshot.content_version)?;
    let previous_native = snapshot.providers.get(id).cloned();
    let outcome = store.remove_provider(id, &expected_version)?;

    if let Err(error) = state.db.delete_provider(PI_APP, id) {
        rollback_delete(store, id, previous_native.as_ref(), &outcome, error)?;
    }
    Ok(())
}

pub(super) fn update_with_store(
    state: &AppState,
    store: &PiModelsStore,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let previous = state
        .db
        .get_provider_by_id(&provider.id, PI_APP)?
        .ok_or_else(|| {
            AppError::InvalidInput(format!("Pi provider '{}' not found", provider.id))
        })?;
    validate_provider(&provider)?;
    let expected_version = pi_content_version(&provider)?.to_string();
    let snapshot = store.read_snapshot()?;
    ensure_snapshot_version(&provider.id, &expected_version, &snapshot.content_version)?;
    if !snapshot.providers.contains_key(&provider.id) {
        set_pi_content_version(&mut provider, &snapshot.content_version);
        state.db.save_provider(PI_APP, &provider)?;
        return Ok(true);
    }
    let outcome = store.put_provider(&provider.id, &provider.settings_config, &expected_version)?;
    set_pi_content_version(&mut provider, &outcome.content_version);

    if let Err(error) = state.db.save_provider(PI_APP, &provider) {
        rollback_provider_write(store, &provider, &previous, &outcome, error)?;
    }
    Ok(true)
}

pub(super) fn remove_with_store(
    state: &AppState,
    store: &PiModelsStore,
    id: &str,
) -> Result<(), AppError> {
    let mut provider = state
        .db
        .get_provider_by_id(id, PI_APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    let expected_version = pi_content_version(&provider)?.to_string();
    let outcome = store.remove_provider(id, &expected_version)?;
    set_pi_content_version(&mut provider, &outcome.content_version);

    if let Err(error) = state.db.save_provider(PI_APP, &provider) {
        if outcome.changed {
            match store.put_provider(id, &provider.settings_config, &outcome.content_version) {
                Ok(_) => return Err(error),
                Err(rollback) => {
                    return Err(AppError::Config(format!(
                        "failed to preserve Pi provider after removal: {error}; native rollback failed: {rollback}"
                    )))
                }
            }
        }
        return Err(error);
    }
    Ok(())
}

fn rollback_provider_write(
    store: &PiModelsStore,
    written: &Provider,
    previous: &Provider,
    outcome: &PiModelsWriteOutcome,
    database_error: AppError,
) -> Result<(), AppError> {
    if !outcome.changed {
        return Err(database_error);
    }
    match store.put_provider(
        &written.id,
        &previous.settings_config,
        &outcome.content_version,
    ) {
        Ok(_) => Err(database_error),
        Err(rollback) => Err(AppError::Config(format!(
            "failed to save Pi provider: {database_error}; native rollback failed: {rollback}"
        ))),
    }
}

fn rollback_insert(
    store: &PiModelsStore,
    id: &str,
    outcome: &PiModelsWriteOutcome,
    database_error: AppError,
) -> Result<(), AppError> {
    if !outcome.changed {
        return Err(database_error);
    }
    match store.remove_provider(id, &outcome.content_version) {
        Ok(_) => Err(database_error),
        Err(rollback) => Err(AppError::Config(format!(
            "failed to save Pi provider: {database_error}; native rollback failed: {rollback}"
        ))),
    }
}

fn rollback_delete(
    store: &PiModelsStore,
    id: &str,
    previous_native: Option<&Value>,
    outcome: &PiModelsWriteOutcome,
    database_error: AppError,
) -> Result<(), AppError> {
    let Some(previous_native) = previous_native else {
        return Err(database_error);
    };
    if !outcome.changed {
        return Err(database_error);
    }
    match store.put_provider(id, previous_native, &outcome.content_version) {
        Ok(_) => Err(database_error),
        Err(rollback) => Err(AppError::Config(format!(
            "failed to delete Pi provider: {database_error}; native rollback failed: {rollback}"
        ))),
    }
}

fn native_provider_name(config: &Value) -> Option<&str> {
    config
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
}

fn validate_provider(provider: &Provider) -> Result<(), AppError> {
    if provider.id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Pi provider key cannot be empty".to_string(),
        ));
    }
    if !provider.settings_config.is_object() {
        return Err(AppError::InvalidInput(
            "Pi provider configuration must be an object".to_string(),
        ));
    }
    Ok(())
}

fn pi_content_version(provider: &Provider) -> Result<&str, AppError> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.pi_models_content_version.as_deref())
        .ok_or_else(|| {
            AppError::Conflict(format!(
                "Pi provider '{}' has no models.json content version; refresh the provider list",
                provider.id
            ))
        })
}

fn ensure_snapshot_version(id: &str, expected: &str, actual: &str) -> Result<(), AppError> {
    if expected == actual {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "Pi provider '{id}' was loaded from a stale models.json snapshot; refresh the provider list"
        )))
    }
}

fn set_pi_content_version(provider: &mut Provider, content_version: &str) {
    provider.in_failover_queue = false;
    let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
    meta.live_config_managed = None;
    meta.pi_models_content_version = Some(content_version.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::pi_config::PiModelsStore;
    use crate::services::provider::ProviderService;
    use crate::store::AppState;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;

    fn state() -> AppState {
        AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ))
    }

    fn seed_models(path: &std::path::Path) {
        fs::write(
            path,
            serde_json::to_vec(&json!({
                "futureRoot": {"keep": true},
                "providers": {
                    "anthropic": {
                        "name": "Native override",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "anthropic-messages",
                        "models": [{"id": "claude-test"}],
                        "futureProviderField": {"keep": true}
                    },
                    "future-provider": {"futureOnly": 9}
                }
            }))
            .unwrap(),
        )
        .expect("seed Pi models");
    }

    fn input(id: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            "Test provider".to_string(),
            json!({
                "name": "Test provider",
                "baseUrl": "https://api.example.com/v1",
                "api": "openai-completions",
                "models": [{"id": "model-a"}]
            }),
            None,
        )
    }

    fn attach_current_version(provider: &mut Provider, store: &PiModelsStore) {
        let version = store.read_snapshot().unwrap().content_version;
        provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .pi_models_content_version = Some(version);
    }

    fn reject_pi_database_mutation(state: &AppState, operation: &str) {
        let (operation, row) = match operation {
            "INSERT" => ("INSERT", "NEW"),
            "UPDATE" => ("UPDATE", "NEW"),
            "DELETE" => ("DELETE", "OLD"),
            _ => panic!("unsupported trigger operation"),
        };
        state
            .db
            .conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TEMP TRIGGER reject_pi_provider_{operation} \
                 BEFORE {operation} ON providers \
                 WHEN {row}.app_type = 'pi' \
                 BEGIN SELECT RAISE(FAIL, 'forced Pi database failure'); END;"
            ))
            .expect("install Pi database failure trigger");
    }

    #[test]
    fn upstream_pi_provider_imports_every_explicit_node_with_one_content_version() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        seed_models(&path);
        let store = PiModelsStore::new(path);
        let state = state();

        let providers = list_with_store(&state, &store).expect("list Pi providers");

        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers["anthropic"].settings_config["futureProviderField"],
            json!({"keep": true})
        );
        assert_eq!(
            providers["future-provider"].settings_config,
            json!({"futureOnly": 9})
        );
        let versions = providers
            .values()
            .map(|provider| {
                provider
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.pi_models_content_version.as_deref())
                    .expect("Pi content version")
            })
            .collect::<Vec<_>>();
        assert!(versions[0].starts_with("sha256:"));
        assert!(versions.iter().all(|version| *version == versions[0]));
    }

    #[test]
    fn upstream_pi_provider_update_rejects_stale_db_snapshot_before_any_write() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        seed_models(&path);
        let store = PiModelsStore::new(path.clone());
        let state = state();
        let imported = list_with_store(&state, &store).expect("import Pi providers");
        let mut edited = imported["anthropic"].clone();
        edited.settings_config["name"] = json!("CCSM edit");

        let external = r#"{"providers":{"anthropic":{"name":"external"}},"external":true}"#;
        fs::write(&path, external).expect("external Pi edit");
        let error = update_with_store(&state, &store, edited)
            .expect_err("stale database snapshot must not overwrite Pi");

        assert!(matches!(error, crate::error::AppError::Conflict(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), external);
        let saved = state
            .db
            .get_provider_by_id("anthropic", "pi")
            .unwrap()
            .unwrap();
        assert_eq!(saved.settings_config["name"], json!("Native override"));
    }

    #[test]
    fn upstream_pi_provider_remove_preserves_latest_native_value_in_database() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        seed_models(&path);
        let store = PiModelsStore::new(path);
        let state = state();
        list_with_store(&state, &store).expect("import Pi providers");

        remove_with_store(&state, &store, "anthropic").expect("remove explicit provider");

        let native = store.read_snapshot().expect("read updated models");
        assert!(!native.providers.contains_key("anthropic"));
        assert!(native.providers.contains_key("future-provider"));
        let saved = state
            .db
            .get_provider_by_id("anthropic", "pi")
            .unwrap()
            .expect("database card remains");
        assert_eq!(saved.settings_config["name"], json!("Native override"));
        assert_eq!(
            saved.meta.and_then(|meta| meta.pi_models_content_version),
            Some(native.content_version)
        );
    }

    #[test]
    fn upstream_pi_provider_add_requires_caller_version_and_rolls_forward_both_stores() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let store = PiModelsStore::new(dir.path().join("models.json"));
        let state = state();
        let missing_version = input("new-provider");

        let error = add_with_store(&state, &store, missing_version, true)
            .expect_err("a live add without a caller snapshot must fail");
        assert!(matches!(error, crate::error::AppError::Conflict(_)));

        let mut provider = input("new-provider");
        attach_current_version(&mut provider, &store);
        add_with_store(&state, &store, provider, true).expect("add versioned Pi provider");

        let native = store.read_snapshot().expect("read native provider");
        assert!(native.providers.contains_key("new-provider"));
        let saved = state
            .db
            .get_provider_by_id("new-provider", PI_APP)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.meta.and_then(|meta| meta.pi_models_content_version),
            Some(native.content_version)
        );
    }

    #[test]
    fn upstream_pi_provider_enable_rejects_a_stale_database_only_card() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        fs::write(&path, r#"{"providers":{}}"#).expect("seed empty models");
        let store = PiModelsStore::new(path.clone());
        let state = state();
        let mut provider = input("disabled");
        attach_current_version(&mut provider, &store);
        add_with_store(&state, &store, provider, false).expect("save database-only provider");

        fs::write(&path, r#"{"providers":{},"external":true}"#).expect("external edit");
        let error = enable_with_store(&state, &store, "disabled")
            .expect_err("stale card must not enable over external edit");

        assert!(matches!(error, crate::error::AppError::Conflict(_)));
        assert!(!store
            .read_snapshot()
            .unwrap()
            .providers
            .contains_key("disabled"));
    }

    #[test]
    fn upstream_pi_provider_delete_removes_only_models_entry_and_database_card() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        let auth_path = dir.path().join("auth.json");
        let settings_path = dir.path().join("settings.json");
        fs::write(&auth_path, b"native auth").unwrap();
        fs::write(&settings_path, b"native defaults").unwrap();
        let store = PiModelsStore::new(path);
        let state = state();
        let mut provider = input("delete-me");
        attach_current_version(&mut provider, &store);
        add_with_store(&state, &store, provider, true).expect("add provider");

        delete_with_store(&state, &store, "delete-me").expect("delete provider");

        assert!(!store
            .read_snapshot()
            .unwrap()
            .providers
            .contains_key("delete-me"));
        assert!(state
            .db
            .get_provider_by_id("delete-me", PI_APP)
            .unwrap()
            .is_none());
        assert_eq!(fs::read(auth_path).unwrap(), b"native auth");
        assert_eq!(fs::read(settings_path).unwrap(), b"native defaults");
    }

    #[test]
    fn upstream_pi_provider_add_rolls_back_native_insert_when_database_save_fails() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let store = PiModelsStore::new(dir.path().join("models.json"));
        let state = state();
        let mut provider = input("rollback-add");
        attach_current_version(&mut provider, &store);
        reject_pi_database_mutation(&state, "INSERT");

        let error = add_with_store(&state, &store, provider, true)
            .expect_err("database failure must fail the add");

        assert!(matches!(error, AppError::Database(_)));
        assert!(!store
            .read_snapshot()
            .unwrap()
            .providers
            .contains_key("rollback-add"));
    }

    #[test]
    fn upstream_pi_provider_update_rolls_back_native_value_when_database_save_fails() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        seed_models(&path);
        let store = PiModelsStore::new(path);
        let state = state();
        let providers = list_with_store(&state, &store).expect("import providers");
        let mut edited = providers["anthropic"].clone();
        edited.settings_config["name"] = json!("must roll back");
        reject_pi_database_mutation(&state, "UPDATE");

        let error = update_with_store(&state, &store, edited)
            .expect_err("database failure must fail the update");

        assert!(matches!(error, AppError::Database(_)));
        assert_eq!(
            store.read_snapshot().unwrap().providers["anthropic"]["name"],
            json!("Native override")
        );
    }

    #[test]
    fn upstream_pi_provider_delete_restores_native_value_when_database_delete_fails() {
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let path = dir.path().join("models.json");
        seed_models(&path);
        let store = PiModelsStore::new(path);
        let state = state();
        list_with_store(&state, &store).expect("import providers");
        reject_pi_database_mutation(&state, "DELETE");

        let error = delete_with_store(&state, &store, "anthropic")
            .expect_err("database failure must fail the delete");

        assert!(matches!(error, AppError::Database(_)));
        assert_eq!(
            store.read_snapshot().unwrap().providers["anthropic"]["name"],
            json!("Native override")
        );
        assert!(state
            .db
            .get_provider_by_id("anthropic", PI_APP)
            .unwrap()
            .is_some());
    }

    #[test]
    #[serial_test::serial]
    fn upstream_pi_provider_service_dispatches_the_complete_native_lifecycle() {
        assert!(
            crate::settings::get_pi_override_dir().is_none(),
            "this test must never replace an explicit Pi directory from user settings"
        );
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let previous = std::env::var_os("PI_CODING_AGENT_DIR");
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", dir.path()) };

        let result = (|| {
            let store = PiModelsStore::new(dir.path().join("models.json"));
            let state = state();
            let mut provider = input("service-provider");
            attach_current_version(&mut provider, &store);

            ProviderService::add(&state, crate::app_config::AppType::Pi, provider, true)?;
            let mut listed = ProviderService::list(&state, crate::app_config::AppType::Pi)?;
            let mut edited = listed
                .swap_remove("service-provider")
                .expect("listed provider");
            edited.settings_config["name"] = json!("Edited through service");
            ProviderService::update(
                &state,
                crate::app_config::AppType::Pi,
                Some("service-provider"),
                edited,
            )?;

            ProviderService::remove_from_live_config(
                &state,
                crate::app_config::AppType::Pi,
                "service-provider",
            )?;
            assert!(!store
                .read_snapshot()?
                .providers
                .contains_key("service-provider"));
            let mut disabled = state
                .db
                .get_provider_by_id("service-provider", PI_APP)?
                .expect("database-only provider");
            disabled.settings_config["name"] = json!("Edited while disabled");
            ProviderService::update(
                &state,
                crate::app_config::AppType::Pi,
                Some("service-provider"),
                disabled,
            )?;
            assert!(
                !store
                    .read_snapshot()?
                    .providers
                    .contains_key("service-provider"),
                "editing a database-only Pi card must not enable it"
            );
            ProviderService::switch(&state, crate::app_config::AppType::Pi, "service-provider")?;
            assert_eq!(
                store.read_snapshot()?.providers["service-provider"]["name"],
                json!("Edited while disabled")
            );
            ProviderService::delete(&state, crate::app_config::AppType::Pi, "service-provider")?;
            assert!(!store
                .read_snapshot()?
                .providers
                .contains_key("service-provider"));
            assert!(state
                .db
                .get_provider_by_id("service-provider", PI_APP)?
                .is_none());
            Ok::<(), AppError>(())
        })();

        match previous {
            Some(value) => unsafe { std::env::set_var("PI_CODING_AGENT_DIR", value) },
            None => unsafe { std::env::remove_var("PI_CODING_AGENT_DIR") },
        }
        result.expect("Pi ProviderService lifecycle");
    }
}

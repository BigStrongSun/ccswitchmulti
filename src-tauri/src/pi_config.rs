//! Pi native `models.json` adapter.
//!
//! Pi continues to own `auth.json` and `settings.json`. This module only edits
//! explicit entries below `models.json.providers`, using caller-visible content
//! versions so a stale UI or database snapshot cannot overwrite native edits.

use crate::config::atomic_write_private;
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_PI_MODELS_BYTES: u64 = 1024 * 1024;
const MISSING_CONTENT_VERSION: &str = "missing";
static MODELS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PiModelsSnapshot {
    pub providers: IndexMap<String, Value>,
    pub content_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PiModelsWriteOutcome {
    pub changed: bool,
    pub content_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiNativeDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_dir: Option<String>,
}

pub(crate) struct PiModelsStore {
    path: PathBuf,
}

impl PiModelsStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn backup_path(&self) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("models.json");
        self.path
            .with_file_name(format!("{file_name}.cc-switch.bak"))
    }

    pub(crate) fn read_snapshot(&self) -> Result<PiModelsSnapshot, AppError> {
        let loaded = read_models_document(&self.path)?;
        Ok(PiModelsSnapshot {
            providers: providers(&loaded.document, &self.path)?
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            content_version: loaded.content_version,
        })
    }

    pub(crate) fn put_provider(
        &self,
        provider_key: &str,
        replacement: &Value,
        expected_content_version: &str,
    ) -> Result<PiModelsWriteOutcome, AppError> {
        validate_provider(provider_key, replacement)?;
        let _guard = self.lock_writes()?;
        let mut loaded = read_models_document(&self.path)?;
        ensure_content_version(
            &self.path,
            expected_content_version,
            &loaded.content_version,
        )?;

        let provider_map = providers_mut(&mut loaded.document, &self.path)?;
        if provider_map.get(provider_key) == Some(replacement) {
            return Ok(PiModelsWriteOutcome {
                changed: false,
                content_version: loaded.content_version,
            });
        }
        provider_map.insert(provider_key.to_string(), replacement.clone());

        self.persist_mutation(loaded)
    }

    pub(crate) fn remove_provider(
        &self,
        provider_key: &str,
        expected_content_version: &str,
    ) -> Result<PiModelsWriteOutcome, AppError> {
        let _guard = self.lock_writes()?;
        let mut loaded = read_models_document(&self.path)?;
        ensure_content_version(
            &self.path,
            expected_content_version,
            &loaded.content_version,
        )?;
        if !providers(&loaded.document, &self.path)?.contains_key(provider_key) {
            return Ok(PiModelsWriteOutcome {
                changed: false,
                content_version: loaded.content_version,
            });
        }
        providers_mut(&mut loaded.document, &self.path)?.remove(provider_key);
        self.persist_mutation(loaded)
    }

    fn persist_mutation(
        &self,
        loaded: LoadedModelsDocument,
    ) -> Result<PiModelsWriteOutcome, AppError> {
        let mut next = serde_json::to_vec_pretty(&loaded.document)
            .map_err(|source| AppError::JsonSerialize { source })?;
        next.push(b'\n');

        if let Some(original) = loaded.original_bytes.as_deref() {
            atomic_write_private(&self.backup_path(), original)?;
        }
        // The backup write can take long enough for Pi or an editor to publish
        // another version. Re-check immediately before the atomic replacement.
        let actual = current_content_version(&self.path)?;
        ensure_content_version(&self.path, &loaded.content_version, &actual)?;
        atomic_write_private(&self.path, &next)?;

        Ok(PiModelsWriteOutcome {
            changed: true,
            content_version: content_version(&next),
        })
    }

    fn lock_writes(&self) -> Result<MutexGuard<'_, ()>, AppError> {
        MODELS_FILE_LOCK
            .lock()
            .map_err(|error| AppError::Config(format!("Pi models write lock is poisoned: {error}")))
    }
}

pub(crate) fn get_pi_agent_dir() -> Result<PathBuf, AppError> {
    let path = crate::settings::get_pi_override_dir()
        .or_else(|| std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from))
        .unwrap_or_else(|| crate::config::get_home_dir().join(".pi").join("agent"));
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(format!(
            "Pi agent directory must be absolute: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(crate) fn get_pi_models_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("models.json"))
}

pub(crate) fn get_pi_settings_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("settings.json"))
}

pub(crate) fn read_pi_native_defaults() -> Result<PiNativeDefaults, AppError> {
    let path = get_pi_settings_path()?;
    if !path.exists() {
        return Ok(PiNativeDefaults::default());
    }
    let bytes = read_file_limited(&path)?;
    let source = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "Pi settings.json must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    json5::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "Pi settings.json is not valid JSON/JSONC ({}): {error}",
            path.display()
        ))
    })
}

pub(crate) fn provider_base_url(provider: &Value) -> Option<String> {
    provider
        .get("baseUrl")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .or_else(|| {
            provider
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| {
                    models.iter().find_map(|model| {
                        model
                            .get("baseUrl")
                            .and_then(Value::as_str)
                            .filter(|url| !url.trim().is_empty())
                    })
                })
        })
        .map(str::to_string)
}

struct LoadedModelsDocument {
    document: Value,
    original_bytes: Option<Vec<u8>>,
    content_version: String,
}

fn read_models_document(path: &Path) -> Result<LoadedModelsDocument, AppError> {
    if !path.exists() {
        return Ok(LoadedModelsDocument {
            document: Value::Object(Map::new()),
            original_bytes: None,
            content_version: MISSING_CONTENT_VERSION.to_string(),
        });
    }

    let bytes = read_file_limited(path)?;
    let source = String::from_utf8(bytes.clone()).map_err(|error| {
        AppError::Config(format!(
            "Pi models.json must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    let document = json5::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "Pi models.json is not valid JSON/JSONC ({}): {error}",
            path.display()
        ))
    })?;
    Ok(LoadedModelsDocument {
        document,
        original_bytes: Some(bytes.clone()),
        content_version: content_version(&bytes),
    })
}

fn read_file_limited(path: &Path) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_PI_MODELS_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Pi models.json exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PI_MODELS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_PI_MODELS_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Pi models.json exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn providers<'a>(document: &'a Value, path: &Path) -> Result<&'a Map<String, Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models.json root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("providers") {
        None => Ok(empty_providers()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "Pi models.json 'providers' must be an object: {}",
            path.display()
        ))),
    }
}

fn providers_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models.json root must be an object: {}",
            path.display()
        ))
    })?;
    root.entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::Config(format!(
                "Pi models.json 'providers' must be an object: {}",
                path.display()
            ))
        })
}

fn empty_providers() -> &'static Map<String, Value> {
    static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);
    &EMPTY
}

fn validate_provider(provider_key: &str, provider: &Value) -> Result<(), AppError> {
    if provider_key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Pi provider key cannot be empty".to_string(),
        ));
    }
    if !provider.is_object() {
        return Err(AppError::InvalidInput(
            "Pi provider configuration must be an object".to_string(),
        ));
    }
    Ok(())
}

fn current_content_version(path: &Path) -> Result<String, AppError> {
    match fs::metadata(path) {
        Ok(_) => Ok(content_version(&read_file_limited(path)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(MISSING_CONTENT_VERSION.to_string())
        }
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn ensure_content_version(path: &Path, expected: &str, actual: &str) -> Result<(), AppError> {
    if expected == actual {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "Pi models.json changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn content_version(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;

    fn provider(name: &str) -> Value {
        json!({
            "name": name,
            "baseUrl": "https://api.example.com/v1",
            "api": "openai-completions",
            "models": [{"id": "example-model"}],
            "futureProviderField": {"keep": true}
        })
    }

    #[test]
    fn upstream_pi_config_rejects_stale_content_version_without_touching_native_files() {
        let dir = tempfile::tempdir().expect("create Pi agent directory");
        let models_path = dir.path().join("models.json");
        let settings_path = dir.path().join("settings.json");
        let auth_path = dir.path().join("auth.json");
        let store = PiModelsStore::new(models_path.clone());
        fs::write(&models_path, r#"{"providers":{"managed":{"name":"old"}}}"#)
            .expect("seed models");
        fs::write(
            &settings_path,
            r#"{"defaultProvider":"managed","defaultModel":"native-default"}"#,
        )
        .expect("seed settings");
        fs::write(
            &auth_path,
            r#"{"managed":{"type":"api_key","key":"native"}}"#,
        )
        .expect("seed auth");

        let stale = store.read_snapshot().expect("read snapshot");
        let external = r#"{"providers":{"managed":{"name":"external"}},"external":true}"#;
        fs::write(&models_path, external).expect("simulate external Pi edit");

        let error = store
            .put_provider("managed", &provider("replacement"), &stale.content_version)
            .expect_err("stale snapshot must be rejected");

        assert!(matches!(error, crate::error::AppError::Conflict(_)));
        assert_eq!(fs::read_to_string(&models_path).unwrap(), external);
        assert_eq!(
            fs::read_to_string(&settings_path).unwrap(),
            r#"{"defaultProvider":"managed","defaultModel":"native-default"}"#
        );
        assert_eq!(
            fs::read_to_string(&auth_path).unwrap(),
            r#"{"managed":{"type":"api_key","key":"native"}}"#
        );
        assert!(!store.backup_path().exists());
    }

    #[test]
    fn upstream_pi_config_preserves_unknown_document_and_provider_fields() {
        let dir = tempfile::tempdir().expect("create Pi agent directory");
        let models_path = dir.path().join("models.json");
        let store = PiModelsStore::new(models_path.clone());
        fs::write(
            &models_path,
            r#"{
                "contentVersion": 17,
                "futureRoot": {"keep": [1, 2, 3]},
                "providers": {
                    "managed": {
                        "name": "old",
                        "futureProviderField": {"keep": true}
                    },
                    "native-other": {"oauth": "future", "unknown": 9}
                }
            }"#,
        )
        .expect("seed models");

        let snapshot = store.read_snapshot().expect("read snapshot");
        let mut replacement = snapshot.providers["managed"].clone();
        replacement["name"] = json!("new");
        let outcome = store
            .put_provider("managed", &replacement, &snapshot.content_version)
            .expect("update provider");

        assert!(outcome.changed);
        let written: Value = serde_json::from_slice(&fs::read(&models_path).unwrap()).unwrap();
        assert_eq!(written["contentVersion"], json!(17));
        assert_eq!(written["futureRoot"], json!({"keep": [1, 2, 3]}));
        assert_eq!(
            written["providers"]["managed"]["futureProviderField"],
            json!({"keep": true})
        );
        assert_eq!(
            written["providers"]["native-other"],
            json!({"oauth": "future", "unknown": 9})
        );
        assert_eq!(
            outcome.content_version,
            store.read_snapshot().unwrap().content_version
        );
    }

    #[test]
    fn upstream_pi_config_writes_exact_preimage_backup_before_mutation() {
        let dir = tempfile::tempdir().expect("create Pi agent directory");
        let models_path = dir.path().join("models.json");
        let store = PiModelsStore::new(models_path.clone());
        let original = b"{\r\n  // native comment\r\n  providers: {old: {future: true}}\r\n}\r\n";
        fs::write(&models_path, original).expect("seed models");

        let snapshot = store.read_snapshot().expect("read snapshot");
        store
            .put_provider("new", &provider("new"), &snapshot.content_version)
            .expect("insert provider");

        assert_eq!(fs::read(store.backup_path()).unwrap(), original);
    }

    #[test]
    fn upstream_pi_config_same_content_is_idempotent_and_does_not_rotate_backup() {
        let dir = tempfile::tempdir().expect("create Pi agent directory");
        let models_path = dir.path().join("models.json");
        let store = PiModelsStore::new(models_path.clone());
        fs::write(
            &models_path,
            serde_json::to_vec(&json!({"providers": {"managed": provider("same")}})).unwrap(),
        )
        .expect("seed models");
        fs::write(store.backup_path(), b"existing backup").expect("seed backup");

        let snapshot = store.read_snapshot().expect("read snapshot");
        let outcome = store
            .put_provider("managed", &provider("same"), &snapshot.content_version)
            .expect("idempotent update");

        assert!(!outcome.changed);
        assert_eq!(outcome.content_version, snapshot.content_version);
        assert_eq!(fs::read(store.backup_path()).unwrap(), b"existing backup");
    }

    #[test]
    fn upstream_pi_config_remove_is_targeted_versioned_and_idempotent() {
        let dir = tempfile::tempdir().expect("create Pi agent directory");
        let models_path = dir.path().join("models.json");
        let store = PiModelsStore::new(models_path.clone());
        fs::write(
            &models_path,
            serde_json::to_vec(&json!({
                "futureRoot": true,
                "providers": {
                    "managed": provider("managed"),
                    "native-other": {"oauth": "future", "unknown": 9}
                }
            }))
            .unwrap(),
        )
        .expect("seed models");

        let before = store.read_snapshot().expect("read snapshot");
        let removed = store
            .remove_provider("managed", &before.content_version)
            .expect("remove managed provider");
        assert!(removed.changed);

        let after = store.read_snapshot().expect("read updated snapshot");
        assert!(!after.providers.contains_key("managed"));
        assert_eq!(
            after.providers["native-other"],
            json!({"oauth": "future", "unknown": 9})
        );
        let document: Value = serde_json::from_slice(&fs::read(&models_path).unwrap()).unwrap();
        assert_eq!(document["futureRoot"], json!(true));

        fs::write(store.backup_path(), b"stable backup").expect("seed stable backup");
        let unchanged = store
            .remove_provider("managed", &after.content_version)
            .expect("repeat removal");
        assert!(!unchanged.changed);
        assert_eq!(fs::read(store.backup_path()).unwrap(), b"stable backup");
    }
}

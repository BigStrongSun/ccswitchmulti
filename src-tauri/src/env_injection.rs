//! Device-local environment-variable injection primitives.
//!
//! Ownership is deliberately separate from value equality. CCSwitchMulti only
//! owns a key after it inserted that key into an empty slot and recorded the
//! operation in its companion ledger. A value that already existed, even when
//! it is byte-for-byte equal to the requested value, remains user-owned.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value as TomlEditValue};

use crate::error::AppError;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvInjectionTarget {
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionTargets {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
}

impl Default for EnvInjectionTargets {
    fn default() -> Self {
        Self {
            claude: true,
            codex: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub targets: EnvInjectionTargets,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

impl EnvInjectionSettings {
    pub fn variables_for(&self, target: EnvInjectionTarget) -> BTreeMap<String, String> {
        let target_enabled = match target {
            EnvInjectionTarget::Claude => self.targets.claude,
            EnvInjectionTarget::Codex => self.targets.codex,
        };
        if !self.enabled || !target_enabled {
            return BTreeMap::new();
        }
        self.variables.clone()
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if let Some(key) = self.variables.keys().find(|key| !is_valid_env_key(key)) {
            return Err(AppError::Config(format!(
                "invalid environment variable name: {key:?}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "value")]
pub enum EnvInjectionOriginalValue {
    Absent,
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionOwnedEntry {
    pub managed_value: String,
    pub original: EnvInjectionOriginalValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionTargetReport {
    pub added_keys: Vec<String>,
    pub updated_keys: Vec<String>,
    pub removed_keys: Vec<String>,
    pub relinquished_keys: Vec<String>,
    pub conflicted_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvInjectionTargetSyncState {
    Disabled,
    Synced,
    Conflict,
    Pending,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionTargetSyncStatus {
    pub state: EnvInjectionTargetSyncState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relinquished_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicted_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_error: Option<String>,
}

impl EnvInjectionTargetSyncStatus {
    fn success(
        active: bool,
        ownership: &BTreeMap<String, EnvInjectionOwnedEntry>,
        report: EnvInjectionTargetReport,
    ) -> Self {
        let state = if !report.conflicted_keys.is_empty() {
            EnvInjectionTargetSyncState::Conflict
        } else if active {
            EnvInjectionTargetSyncState::Synced
        } else {
            EnvInjectionTargetSyncState::Disabled
        };
        Self {
            state,
            managed_keys: ownership.keys().cloned().collect(),
            added_keys: report.added_keys,
            updated_keys: report.updated_keys,
            removed_keys: report.removed_keys,
            relinquished_keys: report.relinquished_keys,
            conflicted_keys: report.conflicted_keys,
            error: None,
            rollback_error: None,
        }
    }

    fn failed(error: impl ToString, rollback_error: Option<String>) -> Self {
        Self {
            state: EnvInjectionTargetSyncState::Failed,
            managed_keys: Vec::new(),
            added_keys: Vec::new(),
            updated_keys: Vec::new(),
            removed_keys: Vec::new(),
            relinquished_keys: Vec::new(),
            conflicted_keys: Vec::new(),
            error: Some(error.to_string()),
            rollback_error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvInjectionSyncState {
    Disabled,
    Synced,
    Warning,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionSyncReport {
    pub state: EnvInjectionSyncState,
    pub claude: EnvInjectionTargetSyncStatus,
    pub codex: EnvInjectionTargetSyncStatus,
    pub codex_include_allowlist: bool,
}

impl EnvInjectionSyncReport {
    fn new(
        claude: EnvInjectionTargetSyncStatus,
        codex: EnvInjectionTargetSyncStatus,
        codex_include_allowlist: bool,
    ) -> Self {
        let statuses = [claude.state, codex.state];
        let failed = statuses
            .iter()
            .filter(|state| **state == EnvInjectionTargetSyncState::Failed)
            .count();
        let active = statuses
            .iter()
            .filter(|state| **state != EnvInjectionTargetSyncState::Disabled)
            .count();
        let has_conflict = statuses.contains(&EnvInjectionTargetSyncState::Conflict)
            || statuses.contains(&EnvInjectionTargetSyncState::Pending)
            || codex_include_allowlist;
        let state = if active == 0 {
            EnvInjectionSyncState::Disabled
        } else if failed == active {
            EnvInjectionSyncState::Failed
        } else if failed > 0 {
            EnvInjectionSyncState::Partial
        } else if has_conflict {
            EnvInjectionSyncState::Warning
        } else {
            EnvInjectionSyncState::Synced
        };
        Self {
            state,
            claude,
            codex,
            codex_include_allowlist,
        }
    }
}

fn default_ledger_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvInjectionOwnershipLedger {
    #[serde(default = "default_ledger_version")]
    version: u32,
    #[serde(default)]
    claude: BTreeMap<String, EnvInjectionOwnedEntry>,
    #[serde(default)]
    codex: BTreeMap<String, EnvInjectionOwnedEntry>,
}

impl Default for EnvInjectionOwnershipLedger {
    fn default() -> Self {
        Self {
            version: default_ledger_version(),
            claude: BTreeMap::new(),
            codex: BTreeMap::new(),
        }
    }
}

const OWNERSHIP_LEDGER_FILENAME: &str = "env-injection-ownership.json";

fn ownership_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ownership_ledger_path() -> PathBuf {
    crate::config::get_app_config_dir().join(OWNERSHIP_LEDGER_FILENAME)
}

fn read_ownership_ledger() -> Result<EnvInjectionOwnershipLedger, AppError> {
    let path = ownership_ledger_path();
    if !path.exists() {
        return Ok(EnvInjectionOwnershipLedger::default());
    }
    let ledger = crate::config::read_json_file::<EnvInjectionOwnershipLedger>(&path)?;
    if ledger.version != default_ledger_version() {
        return Err(AppError::Config(format!(
            "unsupported environment injection ownership ledger version: {}",
            ledger.version
        )));
    }
    Ok(ledger)
}

fn write_ownership_ledger(ledger: &EnvInjectionOwnershipLedger) -> Result<(), AppError> {
    crate::config::write_json_file(&ownership_ledger_path(), ledger)
}

fn restore_file_snapshot(
    path: &Path,
    before: Option<&[u8]>,
    expected_after: &[u8],
) -> Result<(), AppError> {
    let current = fs::read(path).map_err(|error| AppError::io(path, error))?;
    if current != expected_after {
        return Err(AppError::Config(format!(
            "environment injection rollback refused because {} changed concurrently",
            path.display()
        )));
    }
    match before {
        Some(bytes) => crate::config::atomic_write(path, bytes),
        None => fs::remove_file(path).map_err(|error| AppError::io(path, error)),
    }
}

pub fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && !key.contains('=')
        && !key.contains('\0')
        && !key.contains('\n')
        && !key.contains('\r')
}

fn reconcile_string_map<Read, Insert, Remove>(
    ownership: &mut BTreeMap<String, EnvInjectionOwnedEntry>,
    desired: &BTreeMap<String, String>,
    mut read: Read,
    mut insert: Insert,
    mut remove: Remove,
) -> EnvInjectionTargetReport
where
    Read: FnMut(&str) -> Option<String>,
    Insert: FnMut(&str, &str),
    Remove: FnMut(&str),
{
    let mut report = EnvInjectionTargetReport::default();

    for (key, owned) in ownership.clone() {
        if read(&key).as_deref() != Some(owned.managed_value.as_str()) {
            ownership.remove(&key);
            report.relinquished_keys.push(key);
            continue;
        }

        match desired.get(&key) {
            Some(next_value) if next_value != &owned.managed_value => {
                insert(&key, next_value);
                if let Some(entry) = ownership.get_mut(&key) {
                    entry.managed_value.clone_from(next_value);
                }
                report.updated_keys.push(key);
            }
            Some(_) => {}
            None => {
                match owned.original {
                    EnvInjectionOriginalValue::Absent => remove(&key),
                    EnvInjectionOriginalValue::String(original) => {
                        insert(&key, &original);
                    }
                }
                ownership.remove(&key);
                report.removed_keys.push(key);
            }
        }
    }

    for (key, value) in desired {
        if !is_valid_env_key(key) || ownership.contains_key(key) {
            continue;
        }
        if read(key).is_some() {
            report.conflicted_keys.push(key.clone());
            continue;
        }

        insert(key, value);
        ownership.insert(
            key.clone(),
            EnvInjectionOwnedEntry {
                managed_value: value.clone(),
                original: EnvInjectionOriginalValue::Absent,
            },
        );
        report.added_keys.push(key.clone());
    }

    report
}

pub fn reconcile_claude_settings(
    settings: &mut JsonValue,
    ownership: &mut BTreeMap<String, EnvInjectionOwnedEntry>,
    desired: &BTreeMap<String, String>,
) -> Result<EnvInjectionTargetReport, AppError> {
    let root = settings.as_object_mut().ok_or_else(|| {
        AppError::Config("Claude settings.json root must be an object".to_string())
    })?;
    if !root.contains_key("env") && (desired.is_empty() && ownership.is_empty()) {
        return Ok(EnvInjectionTargetReport::default());
    }
    let env = root
        .entry("env")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let env = env.as_object_mut().ok_or_else(|| {
        AppError::Config("Claude settings.json env must be an object".to_string())
    })?;

    let current = std::cell::RefCell::new(env);
    let report = reconcile_string_map(
        ownership,
        desired,
        |key| {
            current
                .borrow()
                .get(key)
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        },
        |key, value| {
            current
                .borrow_mut()
                .insert(key.to_string(), JsonValue::String(value.to_string()));
        },
        |key| {
            current.borrow_mut().remove(key);
        },
    );
    Ok(report)
}

fn policy_table_mut(doc: &mut DocumentMut) -> Result<&mut Table, AppError> {
    let item = doc
        .entry("shell_environment_policy")
        .or_insert(Item::Table(Table::new()));
    item.as_table_mut().ok_or_else(|| {
        AppError::Config("Codex config.toml shell_environment_policy must be a table".to_string())
    })
}

fn set_values(table: &Table) -> Result<BTreeMap<String, String>, AppError> {
    let Some(set) = table.get("set") else {
        return Ok(BTreeMap::new());
    };
    if let Some(values) = set.as_table() {
        return Ok(values
            .iter()
            .filter_map(|(key, item)| {
                item.as_str()
                    .map(|value| (key.to_string(), value.to_string()))
            })
            .collect());
    }
    if let Some(values) = set.as_inline_table() {
        return Ok(values
            .iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.to_string(), value.to_string()))
            })
            .collect());
    }
    Err(AppError::Config(
        "Codex config.toml shell_environment_policy.set must be a table".to_string(),
    ))
}

fn set_insert(table: &mut Table, key: &str, value: &str) -> Result<(), AppError> {
    if !table.contains_key("set") {
        table.insert(
            "set",
            Item::Value(TomlEditValue::InlineTable(InlineTable::new())),
        );
    }
    let set = table.get_mut("set").expect("set was inserted");
    if let Some(values) = set.as_table_mut() {
        values.insert(key, toml_edit::value(value));
        return Ok(());
    }
    if let Some(values) = set.as_inline_table_mut() {
        values.insert(key, TomlEditValue::from(value));
        return Ok(());
    }
    Err(AppError::Config(
        "Codex config.toml shell_environment_policy.set must be a table".to_string(),
    ))
}

fn set_remove(table: &mut Table, key: &str) -> Result<(), AppError> {
    let Some(set) = table.get_mut("set") else {
        return Ok(());
    };
    let empty = if let Some(values) = set.as_table_mut() {
        values.remove(key);
        values.is_empty()
    } else if let Some(values) = set.as_inline_table_mut() {
        values.remove(key);
        values.is_empty()
    } else {
        return Err(AppError::Config(
            "Codex config.toml shell_environment_policy.set must be a table".to_string(),
        ));
    };
    if empty {
        table.remove("set");
    }
    Ok(())
}

pub fn reconcile_codex_config_text(
    config_text: &str,
    ownership: &mut BTreeMap<String, EnvInjectionOwnedEntry>,
    desired: &BTreeMap<String, String>,
) -> Result<(String, EnvInjectionTargetReport), AppError> {
    let mut doc = config_text.parse::<DocumentMut>().map_err(|error| {
        AppError::Config(format!(
            "Codex config.toml parse failed during environment injection: {error}"
        ))
    })?;
    let table = policy_table_mut(&mut doc)?;
    let current_values = std::cell::RefCell::new(set_values(table)?);
    let mutations = std::cell::RefCell::new(BTreeMap::<String, Option<String>>::new());
    let report = reconcile_string_map(
        ownership,
        desired,
        |key| current_values.borrow().get(key).cloned(),
        |key, value| {
            current_values
                .borrow_mut()
                .insert(key.to_string(), value.to_string());
            mutations
                .borrow_mut()
                .insert(key.to_string(), Some(value.to_string()));
        },
        |key| {
            current_values.borrow_mut().remove(key);
            mutations.borrow_mut().insert(key.to_string(), None);
        },
    );

    for (key, value) in mutations.into_inner() {
        match value {
            Some(value) => set_insert(table, &key, &value)?,
            None => set_remove(table, &key)?,
        }
    }
    Ok((doc.to_string(), report))
}

fn codex_set_values_from_text(config_text: &str) -> Result<BTreeMap<String, String>, AppError> {
    if config_text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value = config_text.parse::<toml::Value>().map_err(|error| {
        AppError::Config(format!(
            "Codex config.toml parse failed during environment inspection: {error}"
        ))
    })?;
    Ok(value
        .get("shell_environment_policy")
        .and_then(|policy| policy.get("set"))
        .and_then(toml::Value::as_table)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default())
}

pub fn codex_env_policy_has_include_allowlist(config_text: &str) -> bool {
    config_text
        .parse::<toml::Value>()
        .ok()
        .and_then(|value| {
            value
                .get("shell_environment_policy")?
                .get("include_only")?
                .as_array()
                .map(|values| !values.is_empty())
        })
        .unwrap_or(false)
}

fn sync_claude_target(
    settings: &EnvInjectionSettings,
    ledger: &mut EnvInjectionOwnershipLedger,
) -> EnvInjectionTargetSyncStatus {
    let path = crate::config::get_claude_settings_path();
    sync_claude_target_at(settings, ledger, &path, write_ownership_ledger)
}

fn sync_claude_target_at<WriteLedger>(
    settings: &EnvInjectionSettings,
    ledger: &mut EnvInjectionOwnershipLedger,
    path: &Path,
    mut write_ledger: WriteLedger,
) -> EnvInjectionTargetSyncStatus
where
    WriteLedger: FnMut(&EnvInjectionOwnershipLedger) -> Result<(), AppError>,
{
    let desired = settings.variables_for(EnvInjectionTarget::Claude);
    if desired.is_empty() && ledger.claude.is_empty() {
        return EnvInjectionTargetSyncStatus::success(
            false,
            &ledger.claude,
            EnvInjectionTargetReport::default(),
        );
    }

    let before = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return EnvInjectionTargetSyncStatus::failed(AppError::io(path, error), None),
    };
    let mut live = match before.as_deref() {
        Some(bytes) => match serde_json::from_slice::<JsonValue>(bytes) {
            Ok(value) => value,
            Err(error) => {
                return EnvInjectionTargetSyncStatus::failed(
                    AppError::Config(format!(
                        "Claude settings.json parse failed during environment injection: {error}"
                    )),
                    None,
                )
            }
        },
        None => JsonValue::Object(JsonMap::new()),
    };
    let before_live = live.clone();
    let before_ownership = ledger.claude.clone();
    let mut next_ownership = before_ownership.clone();
    let report = match reconcile_claude_settings(&mut live, &mut next_ownership, &desired) {
        Ok(report) => report,
        Err(error) => return EnvInjectionTargetSyncStatus::failed(error, None),
    };

    let mut written_after = None;
    if live != before_live {
        match crate::config::write_json_file_with_contents(path, &live) {
            Ok(bytes) => written_after = Some(bytes),
            Err(error) => return EnvInjectionTargetSyncStatus::failed(error, None),
        }
    }

    if next_ownership != before_ownership {
        ledger.claude = next_ownership.clone();
        if let Err(error) = write_ledger(ledger) {
            ledger.claude = before_ownership;
            let rollback_error = written_after.as_deref().and_then(|after| {
                restore_file_snapshot(path, before.as_deref(), after)
                    .err()
                    .map(|error| error.to_string())
            });
            return EnvInjectionTargetSyncStatus::failed(error, rollback_error);
        }
    }

    EnvInjectionTargetSyncStatus::success(!desired.is_empty(), &ledger.claude, report)
}

fn sync_codex_target(
    settings: &EnvInjectionSettings,
    ledger: &mut EnvInjectionOwnershipLedger,
) -> EnvInjectionTargetSyncStatus {
    let desired = settings.variables_for(EnvInjectionTarget::Codex);
    if desired.is_empty() && ledger.codex.is_empty() {
        return EnvInjectionTargetSyncStatus::success(
            false,
            &ledger.codex,
            EnvInjectionTargetReport::default(),
        );
    }

    let before_ownership = ledger.codex.clone();
    let final_attempt = std::cell::RefCell::new(None);
    let written = match crate::codex_config::reconcile_codex_live_config_atomic(|live| {
        let mut next_ownership = before_ownership.clone();
        let (candidate, report) = reconcile_codex_config_text(live, &mut next_ownership, &desired)?;
        final_attempt.replace(Some((
            live.to_string(),
            candidate.clone(),
            next_ownership,
            report,
        )));
        Ok(candidate)
    }) {
        Ok(written) => written,
        Err(error) => return EnvInjectionTargetSyncStatus::failed(error, None),
    };
    let Some((before_live, _candidate, next_ownership, report)) = final_attempt.into_inner() else {
        return EnvInjectionTargetSyncStatus::failed(
            "Codex environment reconciliation produced no final attempt",
            None,
        );
    };

    if next_ownership != before_ownership {
        ledger.codex = next_ownership.clone();
        if let Err(error) = write_ownership_ledger(ledger) {
            ledger.codex = before_ownership;
            let rollback_error = crate::codex_config::reconcile_codex_live_config_atomic(|live| {
                if live != written {
                    return Err(AppError::Config(
                        "Codex environment injection rollback refused because config.toml changed concurrently"
                            .to_string(),
                    ));
                }
                Ok(before_live.clone())
            })
            .err()
            .map(|error| error.to_string());
            return EnvInjectionTargetSyncStatus::failed(error, rollback_error);
        }
    }

    EnvInjectionTargetSyncStatus::success(!desired.is_empty(), &ledger.codex, report)
}

fn failed_report(error: impl ToString) -> EnvInjectionSyncReport {
    let error = error.to_string();
    EnvInjectionSyncReport::new(
        EnvInjectionTargetSyncStatus::failed(&error, None),
        EnvInjectionTargetSyncStatus::failed(error, None),
        false,
    )
}

pub fn sync_to_live_configs(settings: &EnvInjectionSettings) -> EnvInjectionSyncReport {
    let _guard = match ownership_lock().lock() {
        Ok(guard) => guard,
        Err(error) => {
            return failed_report(format!("environment injection lock poisoned: {error}"))
        }
    };
    let mut ledger = match read_ownership_ledger() {
        Ok(ledger) => ledger,
        Err(error) => return failed_report(error),
    };

    let claude = sync_claude_target(settings, &mut ledger);
    let codex = sync_codex_target(settings, &mut ledger);
    let codex_include_allowlist = crate::codex_config::read_codex_config_text()
        .is_ok_and(|text| codex_env_policy_has_include_allowlist(&text));
    EnvInjectionSyncReport::new(claude, codex, codex_include_allowlist)
}

fn inspect_target(
    desired: &BTreeMap<String, String>,
    ownership: &BTreeMap<String, EnvInjectionOwnedEntry>,
    current: &BTreeMap<String, String>,
) -> EnvInjectionTargetSyncStatus {
    if desired.is_empty() && ownership.is_empty() {
        return EnvInjectionTargetSyncStatus::success(
            false,
            ownership,
            EnvInjectionTargetReport::default(),
        );
    }

    let mut managed_keys = Vec::new();
    let mut conflicted_keys = Vec::new();
    let mut pending_keys = Vec::new();
    for (key, desired_value) in desired {
        match (ownership.get(key), current.get(key)) {
            (Some(owned), Some(current_value))
                if owned.managed_value == *desired_value && current_value == desired_value =>
            {
                managed_keys.push(key.clone());
            }
            (None, Some(_)) | (Some(_), Some(_)) => conflicted_keys.push(key.clone()),
            (_, None) => pending_keys.push(key.clone()),
        }
    }
    for key in ownership.keys() {
        if (!desired.contains_key(key) || current.get(key) != Some(&ownership[key].managed_value))
            && !conflicted_keys.contains(key)
        {
            conflicted_keys.push(key.clone());
        }
    }
    let state = if !conflicted_keys.is_empty() {
        EnvInjectionTargetSyncState::Conflict
    } else if !pending_keys.is_empty() {
        EnvInjectionTargetSyncState::Pending
    } else if desired.is_empty() {
        EnvInjectionTargetSyncState::Disabled
    } else {
        EnvInjectionTargetSyncState::Synced
    };
    EnvInjectionTargetSyncStatus {
        state,
        managed_keys,
        added_keys: Vec::new(),
        updated_keys: Vec::new(),
        removed_keys: Vec::new(),
        relinquished_keys: Vec::new(),
        conflicted_keys,
        error: None,
        rollback_error: None,
    }
}

pub fn inspect_status(settings: &EnvInjectionSettings) -> EnvInjectionSyncReport {
    let _guard = match ownership_lock().lock() {
        Ok(guard) => guard,
        Err(error) => {
            return failed_report(format!("environment injection lock poisoned: {error}"))
        }
    };
    let ledger = match read_ownership_ledger() {
        Ok(ledger) => ledger,
        Err(error) => return failed_report(error),
    };

    let claude_values = match fs::read(crate::config::get_claude_settings_path()) {
        Ok(bytes) => match serde_json::from_slice::<JsonValue>(&bytes) {
            Ok(value) => value
                .get("env")
                .and_then(JsonValue::as_object)
                .map(|env| {
                    env.iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|value| (key.clone(), value.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(error) => {
                return failed_report(format!("Claude settings.json parse failed: {error}"))
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return failed_report(error),
    };
    let codex_text = match crate::codex_config::read_codex_config_text() {
        Ok(text) => text,
        Err(error) => return failed_report(error),
    };
    let codex_values = match codex_set_values_from_text(&codex_text) {
        Ok(values) => values,
        Err(error) => return failed_report(error),
    };
    let claude = inspect_target(
        &settings.variables_for(EnvInjectionTarget::Claude),
        &ledger.claude,
        &claude_values,
    );
    let codex = inspect_target(
        &settings.variables_for(EnvInjectionTarget::Codex),
        &ledger.codex,
        &codex_values,
    );
    EnvInjectionSyncReport::new(
        claude,
        codex,
        codex_env_policy_has_include_allowlist(&codex_text),
    )
}

pub fn inject_owned_into_claude_live(settings: &mut JsonValue) -> Result<(), AppError> {
    let desired_settings = crate::settings::get_settings().env_injection;
    let desired = desired_settings.variables_for(EnvInjectionTarget::Claude);
    if desired.is_empty() {
        return Ok(());
    }
    let ledger = read_ownership_ledger()?;
    let root = settings.as_object_mut().ok_or_else(|| {
        AppError::Config("Claude settings.json root must be an object".to_string())
    })?;
    let env = root
        .entry("env")
        .or_insert_with(|| JsonValue::Object(JsonMap::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::Config("Claude settings.json env must be an object".to_string())
        })?;
    for (key, owned) in ledger.claude {
        if desired.get(&key) == Some(&owned.managed_value) && !env.contains_key(&key) {
            env.insert(key, JsonValue::String(owned.managed_value));
        }
    }
    Ok(())
}

pub fn strip_owned_from_claude_live(settings: &mut JsonValue) -> Result<(), AppError> {
    let ledger = read_ownership_ledger()?;
    let Some(env) = settings.get_mut("env").and_then(JsonValue::as_object_mut) else {
        return Ok(());
    };
    for (key, owned) in ledger.claude {
        if env.get(&key).and_then(JsonValue::as_str) == Some(owned.managed_value.as_str()) {
            env.remove(&key);
        }
    }
    Ok(())
}

pub fn inject_owned_into_codex_live(config_text: &str) -> Result<String, AppError> {
    let desired_settings = crate::settings::get_settings().env_injection;
    let desired = desired_settings.variables_for(EnvInjectionTarget::Codex);
    if desired.is_empty() {
        return Ok(config_text.to_string());
    }
    let ledger = read_ownership_ledger()?;
    if ledger.codex.is_empty() {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text.parse::<DocumentMut>().map_err(|error| {
        AppError::Config(format!(
            "Codex config.toml parse failed while reapplying owned environment variables: {error}"
        ))
    })?;
    let table = policy_table_mut(&mut doc)?;
    let current = set_values(table)?;
    for (key, owned) in ledger.codex {
        if desired.get(&key) == Some(&owned.managed_value) && !current.contains_key(&key) {
            set_insert(table, &key, &owned.managed_value)?;
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        reconcile_claude_settings, reconcile_codex_config_text, sync_claude_target_at,
        EnvInjectionOriginalValue, EnvInjectionOwnedEntry, EnvInjectionOwnershipLedger,
        EnvInjectionSettings, EnvInjectionSyncReport, EnvInjectionSyncState,
        EnvInjectionTargetReport, EnvInjectionTargetSyncState, EnvInjectionTargetSyncStatus,
        EnvInjectionTargets,
    };
    use crate::error::AppError;
    use serde_json::Value as JsonValue;
    use std::collections::BTreeMap;

    fn desired(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn owned(value: &str) -> EnvInjectionOwnedEntry {
        EnvInjectionOwnedEntry {
            managed_value: value.to_string(),
            original: EnvInjectionOriginalValue::Absent,
        }
    }

    fn enabled_settings() -> EnvInjectionSettings {
        EnvInjectionSettings {
            enabled: true,
            targets: EnvInjectionTargets {
                claude: true,
                codex: false,
            },
            variables: desired(&[("TZ", "Asia/Shanghai")]),
        }
    }

    #[test]
    fn preexisting_equal_claude_value_is_not_claimed_or_removed() {
        let mut settings = serde_json::json!({"env": {"TZ": "Asia/Shanghai"}});
        let mut ownership = BTreeMap::new();

        let enabled = reconcile_claude_settings(
            &mut settings,
            &mut ownership,
            &desired(&[("TZ", "Asia/Shanghai")]),
        )
        .expect("reconcile equal pre-existing value");

        assert_eq!(settings["env"]["TZ"], "Asia/Shanghai");
        assert!(
            ownership.is_empty(),
            "an equal value is not ownership proof"
        );
        assert_eq!(enabled.conflicted_keys, vec!["TZ"]);

        let disabled = reconcile_claude_settings(&mut settings, &mut ownership, &BTreeMap::new())
            .expect("disable injection");
        assert_eq!(settings["env"]["TZ"], "Asia/Shanghai");
        assert!(disabled.removed_keys.is_empty());
    }

    #[test]
    fn user_changed_owned_claude_value_survives_disable_and_relinquishes_ownership() {
        let mut settings = serde_json::json!({"env": {"TZ": "UTC"}});
        let mut ownership = BTreeMap::from([("TZ".to_string(), owned("Asia/Shanghai"))]);

        let report = reconcile_claude_settings(&mut settings, &mut ownership, &BTreeMap::new())
            .expect("disable after user edit");

        assert_eq!(settings["env"]["TZ"], "UTC");
        assert!(ownership.is_empty());
        assert_eq!(report.relinquished_keys, vec!["TZ"]);
        assert!(report.removed_keys.is_empty());
    }

    #[test]
    fn owned_claude_value_can_be_updated_then_removed() {
        let mut settings = serde_json::json!({"env": {"TZ": "Asia/Shanghai"}});
        let mut ownership = BTreeMap::from([("TZ".to_string(), owned("Asia/Shanghai"))]);

        let updated =
            reconcile_claude_settings(&mut settings, &mut ownership, &desired(&[("TZ", "UTC")]))
                .expect("update owned value");
        assert_eq!(settings["env"]["TZ"], "UTC");
        assert_eq!(ownership["TZ"].managed_value, "UTC");
        assert_eq!(updated.updated_keys, vec!["TZ"]);

        let removed = reconcile_claude_settings(&mut settings, &mut ownership, &BTreeMap::new())
            .expect("remove owned value");
        assert!(settings["env"].get("TZ").is_none());
        assert!(ownership.is_empty());
        assert_eq!(removed.removed_keys, vec!["TZ"]);
    }

    #[test]
    fn codex_inline_set_preserves_unmanaged_values_and_adds_owned_key() {
        let base = concat!(
            "model = \"gpt-5\"\n",
            "[shell_environment_policy]\n",
            "set = { KEEP = \"user\" }\n",
        );
        let mut ownership = BTreeMap::new();

        let (next, report) = reconcile_codex_config_text(
            base,
            &mut ownership,
            &desired(&[("KEEP", "user"), ("TZ", "Asia/Shanghai")]),
        )
        .expect("reconcile inline set");

        let parsed = next.parse::<toml::Value>().expect("valid TOML");
        assert_eq!(
            parsed["shell_environment_policy"]["set"]["KEEP"].as_str(),
            Some("user")
        );
        assert_eq!(
            parsed["shell_environment_policy"]["set"]["TZ"].as_str(),
            Some("Asia/Shanghai")
        );
        assert!(!ownership.contains_key("KEEP"));
        assert_eq!(ownership["TZ"].managed_value, "Asia/Shanghai");
        assert_eq!(report.added_keys, vec!["TZ"]);
        assert_eq!(report.conflicted_keys, vec!["KEEP"]);
    }

    #[test]
    fn codex_table_set_removes_only_proven_owned_key() {
        let base = concat!(
            "[shell_environment_policy.set]\n",
            "TZ = \"Asia/Shanghai\"\n",
            "KEEP = \"user\"\n",
        );
        let mut ownership = BTreeMap::from([("TZ".to_string(), owned("Asia/Shanghai"))]);

        let (next, report) = reconcile_codex_config_text(base, &mut ownership, &BTreeMap::new())
            .expect("disable table-form injection");

        let parsed = next.parse::<toml::Value>().expect("valid TOML");
        assert!(parsed["shell_environment_policy"]["set"]
            .get("TZ")
            .is_none());
        assert_eq!(
            parsed["shell_environment_policy"]["set"]["KEEP"].as_str(),
            Some("user")
        );
        assert!(ownership.is_empty());
        assert_eq!(report.removed_keys, vec!["TZ"]);
    }

    #[test]
    fn persisted_sync_does_not_claim_an_equal_preexisting_claude_value() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(".claude").join("settings.json");
        crate::config::write_json_file(&path, &serde_json::json!({"env": {"TZ": "Asia/Shanghai"}}))
            .expect("seed Claude settings");
        let mut ledger = EnvInjectionOwnershipLedger::default();
        let ledger_writes = std::cell::Cell::new(0usize);

        let status = sync_claude_target_at(&enabled_settings(), &mut ledger, &path, |_| {
            ledger_writes.set(ledger_writes.get() + 1);
            Ok(())
        });

        assert_eq!(status.state, EnvInjectionTargetSyncState::Conflict);
        assert_eq!(status.conflicted_keys, vec!["TZ"]);
        assert!(ledger.claude.is_empty());
        assert_eq!(ledger_writes.get(), 0);

        let disabled =
            sync_claude_target_at(&EnvInjectionSettings::default(), &mut ledger, &path, |_| {
                Ok(())
            });
        assert_eq!(disabled.state, EnvInjectionTargetSyncState::Disabled);
        let live: JsonValue = crate::config::read_json_file(&path).expect("read Claude settings");
        assert_eq!(live["env"]["TZ"], "Asia/Shanghai");
    }

    #[test]
    fn ledger_write_failure_rolls_back_live_file_and_retry_succeeds() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(".claude").join("settings.json");
        let mut ledger = EnvInjectionOwnershipLedger::default();

        let failed = sync_claude_target_at(&enabled_settings(), &mut ledger, &path, |_| {
            Err(AppError::Config("ledger locked".to_string()))
        });

        assert_eq!(failed.state, EnvInjectionTargetSyncState::Failed);
        assert_eq!(failed.error.as_deref(), Some("配置错误: ledger locked"));
        assert!(failed.rollback_error.is_none());
        assert!(
            !path.exists(),
            "a newly created live file must be rolled back"
        );
        assert!(ledger.claude.is_empty());

        let persisted = std::cell::RefCell::new(None);
        let retried = sync_claude_target_at(&enabled_settings(), &mut ledger, &path, |next| {
            persisted.replace(Some(next.clone()));
            Ok(())
        });
        assert_eq!(retried.state, EnvInjectionTargetSyncState::Synced);
        assert_eq!(retried.added_keys, vec!["TZ"]);
        assert_eq!(
            persisted
                .borrow()
                .as_ref()
                .expect("ledger persisted")
                .claude["TZ"]
                .managed_value,
            "Asia/Shanghai"
        );
        let live: JsonValue = crate::config::read_json_file(&path).expect("read retried settings");
        assert_eq!(live["env"]["TZ"], "Asia/Shanghai");
    }

    #[test]
    fn aggregate_report_distinguishes_partial_sync_from_total_failure() {
        let synced = EnvInjectionTargetSyncStatus::success(
            true,
            &BTreeMap::new(),
            EnvInjectionTargetReport::default(),
        );
        let failed = EnvInjectionTargetSyncStatus::failed("Codex locked", None);

        let partial = EnvInjectionSyncReport::new(synced.clone(), failed.clone(), false);
        assert_eq!(partial.state, EnvInjectionSyncState::Partial);

        let total = EnvInjectionSyncReport::new(failed.clone(), failed, false);
        assert_eq!(total.state, EnvInjectionSyncState::Failed);
    }
}

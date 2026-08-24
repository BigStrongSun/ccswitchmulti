use crate::services::recovery_outcome::{self, RecoveryOutcome};

/// Return the most recent startup/configuration recovery result.
///
/// The result is persisted before the corresponding event is emitted, so this
/// command covers the case where the webview subscribed too late.
#[tauri::command]
pub fn get_last_recovery_outcome() -> Result<Option<RecoveryOutcome>, String> {
    recovery_outcome::get_last_recovery_outcome()
}

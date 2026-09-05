use crate::services::codex_plugin_registry::RepairableCodexPlugin;

/// Detect enabled Codex plugins whose cached manifest is not registered in the
/// personal marketplace. Detection is read-only and never enables a plugin.
#[tauri::command]
pub fn detect_codex_plugin_registration() -> Result<Vec<RepairableCodexPlugin>, String> {
    crate::services::codex_plugin_registry::detect_codex_plugin_registration()
}

/// Repair one user-confirmed Codex plugin marketplace entry.
#[tauri::command]
pub fn repair_codex_plugin_registration(
    plugin_id: String,
) -> Result<RepairableCodexPlugin, String> {
    crate::services::codex_plugin_registry::repair_codex_plugin_registration(&plugin_id)
}

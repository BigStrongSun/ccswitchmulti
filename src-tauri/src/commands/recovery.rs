use crate::services::recovery_outcome::{
    acknowledge_recovery_outcomes as acknowledge, get_pending_recovery_outcomes as get_pending,
    RecoveryOutcome,
};

#[tauri::command]
pub fn get_pending_recovery_outcomes() -> Result<Vec<RecoveryOutcome>, String> {
    get_pending()
}

#[tauri::command]
pub fn acknowledge_recovery_outcomes(generation: u64, ids: Vec<String>) -> Result<usize, String> {
    acknowledge(generation, &ids)
}

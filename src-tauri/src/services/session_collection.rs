//! Shared session-usage collection coordination.
//!
//! The sync kernel remains in `session_usage`; this module owns only scheduling,
//! status, and frontend notification so manual and periodic runs cannot diverge.

use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::{self, SessionSyncResult};
use chrono::Utc;
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};

pub const SESSION_COLLECTION_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCollectionStatus {
    pub revision: u64,
    pub phase: String,
    pub last_started_at: Option<i64>,
    pub last_completed_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub imported: u64,
    pub deferred: u64,
    pub errors_count: u32,
    pub last_error_summary: Option<String>,
    pub next_run_at: Option<i64>,
    pub interval_secs: u64,
}

impl Default for SessionCollectionStatus {
    fn default() -> Self {
        Self {
            revision: 0,
            phase: "not_started".to_string(),
            last_started_at: None,
            last_completed_at: None,
            last_success_at: None,
            imported: 0,
            deferred: 0,
            errors_count: 0,
            last_error_summary: None,
            next_run_at: None,
            interval_secs: SESSION_COLLECTION_INTERVAL_SECS,
        }
    }
}

impl SessionCollectionStatus {
    fn touch(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
    fn mark_started(&mut self, started_at: i64) {
        self.phase = "running".to_string();
        self.last_started_at = Some(started_at);
        self.touch();
    }
    fn mark_completed(
        &mut self,
        result: &SessionSyncResult,
        completed_at: i64,
        next_run_at: Option<Option<i64>>,
    ) {
        self.last_completed_at = Some(completed_at);
        self.imported = u64::from(result.imported);
        self.deferred = u64::from(result.deferred_files);
        self.errors_count = result.errors.len().min(u32::MAX as usize) as u32;
        if let Some(next_run_at) = next_run_at {
            self.next_run_at = next_run_at;
        }
        if result.errors.is_empty() {
            self.phase = "idle".to_string();
            self.last_success_at = Some(completed_at);
            self.last_error_summary = None;
        } else {
            self.phase = "degraded".to_string();
            self.last_error_summary = Some(format!("{} 个数据源采集失败", result.errors.len()));
        }
        self.touch();
    }
    fn mark_worker_failure(&mut self, completed_at: i64, next_run_at: Option<Option<i64>>) {
        self.last_completed_at = Some(completed_at);
        self.imported = 0;
        self.deferred = 0;
        self.errors_count = 1;
        self.last_error_summary = Some("采集工作线程未完成".to_string());
        if let Some(next_run_at) = next_run_at {
            self.next_run_at = next_run_at;
        }
        self.phase = "degraded".to_string();
        self.touch();
    }
}

fn status_store() -> &'static Mutex<SessionCollectionStatus> {
    static STATUS: OnceLock<Mutex<SessionCollectionStatus>> = OnceLock::new();
    STATUS.get_or_init(|| Mutex::new(SessionCollectionStatus::default()))
}
fn now_seconds() -> i64 {
    Utc::now().timestamp()
}
fn publish_status(mutator: impl FnOnce(&mut SessionCollectionStatus)) -> SessionCollectionStatus {
    let snapshot = {
        let mut status = status_store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mutator(&mut status);
        status.clone()
    };
    crate::usage_events::notify_session_collection_updated(&snapshot);
    snapshot
}
pub fn get_session_collection_status() -> SessionCollectionStatus {
    status_store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Run one collection pass. The existing global sync mutex is acquired before
/// status changes, so a queued manual run cannot incorrectly hide an active run.
pub async fn run_session_collection(
    db: Arc<Database>,
    backfill: bool,
    schedule_next: bool,
) -> Result<SessionSyncResult, AppError> {
    let _guard = session_usage::session_sync_mutex().lock().await;
    publish_status(|status| status.mark_started(now_seconds()));
    let task = tauri::async_runtime::spawn_blocking(move || {
        if backfill {
            if let Err(error) = db.backfill_missing_usage_costs() {
                log::warn!("Usage cost startup backfill failed: {error}");
            }
        }
        session_usage::sync_all_unlocked(&db)
    });
    match task.await {
        Ok(result) => {
            let completed_at = now_seconds();
            let next_run_at = schedule_next
                .then_some(Some(completed_at + SESSION_COLLECTION_INTERVAL_SECS as i64));
            publish_status(|status| status.mark_completed(&result, completed_at, next_run_at));
            Ok(result)
        }
        Err(error) => {
            let completed_at = now_seconds();
            let next_run_at = schedule_next
                .then_some(Some(completed_at + SESSION_COLLECTION_INTERVAL_SECS as i64));
            publish_status(|status| status.mark_worker_failure(completed_at, next_run_at));
            Err(AppError::Message(format!("会话用量同步任务失败: {error}")))
        }
    }
}

/// Starts one serial loop. A new delay begins only after a collection pass completes.
pub fn start_periodic_session_collection(db: Arc<Database>) {
    tauri::async_runtime::spawn(async move {
        let mut backfill = true;
        loop {
            if let Err(error) = run_session_collection(db.clone(), backfill, true).await {
                log::warn!("Session usage sync worker failed: {error}");
            }
            backfill = false;
            tokio::time::sleep(std::time::Duration::from_secs(
                SESSION_COLLECTION_INTERVAL_SECS,
            ))
            .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_empty_collection_records_completion_and_success() {
        let mut status = SessionCollectionStatus::default();
        status.mark_started(100);
        status.mark_completed(&SessionSyncResult::default(), 101, Some(Some(161)));

        assert_eq!(status.phase, "idle");
        assert_eq!(status.last_completed_at, Some(101));
        assert_eq!(status.last_success_at, Some(101));
        assert_eq!(status.imported, 0);
        assert_eq!(status.next_run_at, Some(161));
        assert_eq!(status.errors_count, 0);
    }

    #[test]
    fn failed_collection_exposes_only_safe_error_summary() {
        let mut status = SessionCollectionStatus::default();
        status.mark_started(100);
        let result = SessionSyncResult {
            errors: vec![
                "Codex 同步失败: C:\\Users\\alice\\.codex\\sessions\\secret.jsonl".to_string(),
            ],
            ..Default::default()
        };
        status.mark_completed(&result, 101, Some(Some(161)));

        assert_eq!(status.phase, "degraded");
        assert_eq!(status.errors_count, 1);
        assert_eq!(
            status.last_error_summary.as_deref(),
            Some("1 个数据源采集失败")
        );
        assert_eq!(status.last_success_at, None);
        assert!(!status
            .last_error_summary
            .as_deref()
            .unwrap_or_default()
            .contains("Users"));
    }

    #[test]
    fn manual_collection_keeps_the_periodic_deadline_and_prior_success() {
        let mut status = SessionCollectionStatus::default();
        status.mark_completed(&SessionSyncResult::default(), 100, Some(Some(160)));
        status.mark_started(120);
        let result = SessionSyncResult {
            errors: vec!["Codex 同步失败: fixture".to_string()],
            ..Default::default()
        };
        status.mark_completed(&result, 121, None);

        assert_eq!(status.next_run_at, Some(160));
        assert_eq!(status.last_success_at, Some(100));
        assert_eq!(status.phase, "degraded");
    }
}

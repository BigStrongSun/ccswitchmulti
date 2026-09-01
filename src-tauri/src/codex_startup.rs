use crate::codex_config_consistency::{CodexConfigConsistencyReport, CodexConfigConsistencyState};
use crate::store::AppState;

#[derive(Clone, Debug, Eq, PartialEq)]
enum CodexStartupConfigState {
    Ready,
    Repairable,
    Blocked(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodexStartupLaunchOutcome {
    Disabled,
    AlreadyRunning,
    Launched,
}

trait CodexStartupOperations {
    fn codex_is_running(&mut self) -> bool;
    async fn inspect_config(&mut self) -> Result<CodexStartupConfigState, String>;
    async fn repair_config(&mut self) -> Result<(), String>;
    async fn launch_codex(&mut self) -> Result<bool, String>;
}

async fn execute_codex_startup_launch<O: CodexStartupOperations>(
    operations: &mut O,
    enabled: bool,
    takeover_restore_ready: bool,
) -> Result<CodexStartupLaunchOutcome, String> {
    if !enabled {
        return Ok(CodexStartupLaunchOutcome::Disabled);
    }
    if !takeover_restore_ready {
        return Err("codex_startup_takeover_restore_not_ready".to_string());
    }
    if operations.codex_is_running() {
        return Ok(CodexStartupLaunchOutcome::AlreadyRunning);
    }

    let mut config_state = operations.inspect_config().await?;
    if config_state == CodexStartupConfigState::Repairable {
        operations.repair_config().await?;
        config_state = operations.inspect_config().await?;
    }
    if let CodexStartupConfigState::Blocked(reason) = config_state {
        return Err(format!("codex_startup_config_not_ready: {reason}"));
    }
    if config_state != CodexStartupConfigState::Ready {
        return Err("codex_startup_config_not_ready_after_repair".to_string());
    }

    if operations.launch_codex().await? {
        Ok(CodexStartupLaunchOutcome::Launched)
    } else {
        Ok(CodexStartupLaunchOutcome::AlreadyRunning)
    }
}

fn config_state_from_report(report: &CodexConfigConsistencyReport) -> CodexStartupConfigState {
    match report.state {
        CodexConfigConsistencyState::Consistent => CodexStartupConfigState::Ready,
        CodexConfigConsistencyState::ExternalDrift => CodexStartupConfigState::Repairable,
        CodexConfigConsistencyState::Unavailable if report.provider_id.is_some() => {
            CodexStartupConfigState::Repairable
        }
        CodexConfigConsistencyState::NotApplicable
            if matches!(
                report.reason.as_deref(),
                Some("proxy_takeover_active" | "no_current_provider")
            ) =>
        {
            CodexStartupConfigState::Ready
        }
        _ => CodexStartupConfigState::Blocked(
            report
                .reason
                .clone()
                .unwrap_or_else(|| format!("config_state_{:?}", report.state)),
        ),
    }
}

struct SystemCodexStartupOperations<'a> {
    state: &'a AppState,
}

impl CodexStartupOperations for SystemCodexStartupOperations<'_> {
    fn codex_is_running(&mut self) -> bool {
        crate::codex_desktop::is_codex_desktop_running()
    }

    async fn inspect_config(&mut self) -> Result<CodexStartupConfigState, String> {
        crate::codex_config_consistency::repair_current_profile_provider_before_inspection(
            self.state,
        )
        .map_err(String::from)?;
        let report = crate::codex_config_consistency::inspect(self.state).map_err(String::from)?;
        Ok(config_state_from_report(&report))
    }

    async fn repair_config(&mut self) -> Result<(), String> {
        crate::codex_config_consistency::reproject_current_ccsm_config(self.state)
            .await
            .map(|_| ())
            .map_err(String::from)
    }

    async fn launch_codex(&mut self) -> Result<bool, String> {
        crate::codex_desktop::launch_codex_desktop_with_ccswitch(true)
    }
}

pub(crate) async fn launch_after_startup_reconciliation(
    state: &AppState,
    takeover_restore_ready: bool,
) -> Result<CodexStartupLaunchOutcome, String> {
    let enabled = crate::settings::get_settings().launch_codex_desktop_with_ccswitch;
    let mut operations = SystemCodexStartupOperations { state };
    execute_codex_startup_launch(&mut operations, enabled, takeover_restore_ready).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn report(
        state: CodexConfigConsistencyState,
        provider_id: Option<&str>,
        reason: Option<&str>,
    ) -> CodexConfigConsistencyReport {
        CodexConfigConsistencyReport {
            state,
            provider_id: provider_id.map(str::to_string),
            expected_fingerprint: None,
            actual_fingerprint: None,
            changed_keys: Vec::new(),
            reason: reason.map(str::to_string),
            runtime_activation: crate::codex_config_consistency::CodexConfigRuntimeActivation {
                state:
                    crate::codex_config_consistency::CodexConfigRuntimeActivationState::NotRunning,
                app_server_started_at: None,
                config_modified_at: None,
                reason: None,
            },
        }
    }

    struct FakeStartupOperations {
        codex_running: bool,
        inspections: VecDeque<Result<CodexStartupConfigState, String>>,
        repair_result: Result<(), String>,
        launch_result: Result<bool, String>,
        events: Vec<&'static str>,
    }

    impl CodexStartupOperations for FakeStartupOperations {
        fn codex_is_running(&mut self) -> bool {
            self.events.push("detect_running");
            self.codex_running
        }

        async fn inspect_config(&mut self) -> Result<CodexStartupConfigState, String> {
            self.events.push("inspect");
            self.inspections
                .pop_front()
                .expect("test must provide an inspection result")
        }

        async fn repair_config(&mut self) -> Result<(), String> {
            self.events.push("repair");
            self.repair_result.clone()
        }

        async fn launch_codex(&mut self) -> Result<bool, String> {
            self.events.push("launch");
            self.launch_result.clone()
        }
    }

    fn operations_with(
        inspections: impl IntoIterator<Item = CodexStartupConfigState>,
    ) -> FakeStartupOperations {
        FakeStartupOperations {
            codex_running: false,
            inspections: inspections.into_iter().map(Ok).collect(),
            repair_result: Ok(()),
            launch_result: Ok(true),
            events: Vec::new(),
        }
    }

    #[tokio::test]
    async fn repairs_and_rechecks_config_before_launching_codex() {
        let mut operations = operations_with([
            CodexStartupConfigState::Repairable,
            CodexStartupConfigState::Ready,
        ]);

        let outcome = execute_codex_startup_launch(&mut operations, true, true)
            .await
            .expect("repairable config should be repaired before launch");

        assert_eq!(outcome, CodexStartupLaunchOutcome::Launched);
        assert_eq!(
            operations.events,
            ["detect_running", "inspect", "repair", "inspect", "launch"]
        );
    }

    #[tokio::test]
    async fn refuses_to_launch_when_repaired_config_is_still_not_ready() {
        let mut operations = operations_with([
            CodexStartupConfigState::Repairable,
            CodexStartupConfigState::Blocked("managed_projection_still_drifted".to_string()),
        ]);

        let error = execute_codex_startup_launch(&mut operations, true, true)
            .await
            .expect_err("unverified config must block Codex launch");

        assert!(error.contains("managed_projection_still_drifted"));
        assert_eq!(
            operations.events,
            ["detect_running", "inspect", "repair", "inspect"]
        );
    }

    #[tokio::test]
    async fn refuses_to_launch_when_codex_takeover_restore_failed() {
        let mut operations = operations_with([CodexStartupConfigState::Ready]);

        let error = execute_codex_startup_launch(&mut operations, true, false)
            .await
            .expect_err("failed takeover restore must block Codex launch");

        assert!(error.contains("takeover_restore_not_ready"));
        assert!(operations.events.is_empty());
    }

    #[tokio::test]
    async fn already_running_codex_is_not_reconfigured_or_relaunched() {
        let mut operations = operations_with([CodexStartupConfigState::Repairable]);
        operations.codex_running = true;

        let outcome = execute_codex_startup_launch(&mut operations, true, true)
            .await
            .expect("already-running Codex should be an idempotent no-op");

        assert_eq!(outcome, CodexStartupLaunchOutcome::AlreadyRunning);
        assert_eq!(operations.events, ["detect_running"]);
    }

    #[tokio::test]
    async fn disabled_startup_setting_does_not_touch_codex() {
        let mut operations = operations_with([CodexStartupConfigState::Ready]);

        let outcome = execute_codex_startup_launch(&mut operations, false, false)
            .await
            .expect("disabled startup is a successful no-op");

        assert_eq!(outcome, CodexStartupLaunchOutcome::Disabled);
        assert!(operations.events.is_empty());
    }

    #[test]
    fn consistent_and_active_takeover_reports_are_launch_ready() {
        assert_eq!(
            config_state_from_report(&report(
                CodexConfigConsistencyState::Consistent,
                Some("provider"),
                None,
            )),
            CodexStartupConfigState::Ready
        );
        assert_eq!(
            config_state_from_report(&report(
                CodexConfigConsistencyState::NotApplicable,
                Some("provider"),
                Some("proxy_takeover_active"),
            )),
            CodexStartupConfigState::Ready
        );
    }

    #[test]
    fn managed_drift_is_repairable_before_launch() {
        assert_eq!(
            config_state_from_report(&report(
                CodexConfigConsistencyState::ExternalDrift,
                Some("provider"),
                Some("live_config_changed"),
            )),
            CodexStartupConfigState::Repairable
        );
    }

    #[test]
    fn missing_current_provider_is_a_valid_unmanaged_startup() {
        assert_eq!(
            config_state_from_report(&report(
                CodexConfigConsistencyState::NotApplicable,
                None,
                Some("no_current_provider"),
            )),
            CodexStartupConfigState::Ready
        );
    }
}

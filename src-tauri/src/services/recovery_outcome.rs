#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    struct TempHome {
        _dir: TempDir,
        previous: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let previous = std::env::var("CC_SWITCH_TEST_HOME").ok();
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn recovery_outcome_serializes_stable_kind_and_fields() {
        let mut outcome = RecoveryOutcome::new(RecoveryOutcomeKind::ProviderOnlyRestored);
        outcome.app_type = Some("codex".to_string());
        outcome.kept_fields = vec!["desktop.max".to_string()];
        outcome.lost_fields = vec!["userTables".to_string()];
        outcome.next_step = Some("openLogs".to_string());

        let value: serde_json::Value = serde_json::to_value(&outcome).expect("serialize outcome");
        assert_eq!(value["kind"], "providerOnlyRestored");
        assert_eq!(value["appType"], "codex");
        assert_eq!(value["keptFields"][0], "desktop.max");
        assert_eq!(value["lostFields"][0], "userTables");
    }

    #[test]
    #[serial]
    fn recovery_outcome_persists_and_can_be_read_back() {
        let _home = TempHome::new();
        let mut outcome = RecoveryOutcome::for_app(
            RecoveryOutcomeKind::LivePreservedProviderRepaired,
            "codex",
        );
        outcome.kept_fields = vec!["desktop".to_string(), "plugins".to_string()];
        record_recovery_outcome(outcome.clone()).expect("record outcome");

        assert!(recovery_outcome_path().is_file());
        assert_eq!(
            get_last_recovery_outcome().expect("read outcome"),
            Some(outcome)
        );
    }

    #[test]
    #[serial]
    fn missing_or_corrupt_outcome_is_treated_as_no_result() {
        let _home = TempHome::new();
        assert_eq!(get_last_recovery_outcome().expect("missing outcome"), None);

        let path = recovery_outcome_path();
        std::fs::create_dir_all(path.parent().expect("outcome parent")).expect("create logs");
        std::fs::write(&path, b"not-json").expect("write corrupt outcome");
        assert_eq!(get_last_recovery_outcome().expect("corrupt outcome"), None);
    }

    #[test]
    #[serial]
    fn transient_startup_outcomes_are_conditionally_cleared() {
        let _home = TempHome::new();
        for kind in [
            RecoveryOutcomeKind::ActivePreviousInstance,
            RecoveryOutcomeKind::PlannedRestartOrUpdate,
        ] {
            record_recovery_outcome(RecoveryOutcome::new(kind)).expect("seed transient outcome");
            clear_transient_startup_outcome_if_not_active_or_planned()
                .expect("clear transient startup outcome");
            assert_eq!(
                get_last_recovery_outcome().expect("read cleared outcome"),
                None
            );
        }
    }

    #[test]
    #[serial]
    fn non_transient_recovery_outcome_is_not_cleared() {
        let _home = TempHome::new();
        let outcome = RecoveryOutcome::new(RecoveryOutcomeKind::ProviderOnlyRestored);
        record_recovery_outcome(outcome.clone()).expect("seed durable outcome");
        clear_transient_startup_outcome_if_not_active_or_planned()
            .expect("reconcile durable outcome");
        assert_eq!(
            get_last_recovery_outcome().expect("read durable outcome"),
            Some(outcome)
        );
    }
}

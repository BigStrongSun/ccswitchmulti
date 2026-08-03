//! Integration-test filesystem isolation must never target live user paths.

use std::path::PathBuf;

#[path = "support.rs"]
mod support;

use support::{ensure_test_home, reset_test_fs, test_mutex};

fn lock_test_fs() -> std::sync::MutexGuard<'static, ()> {
    test_mutex()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
#[test]
fn test_home_redirects_windows_localappdata() {
    let _guard = lock_test_fs();
    let home = ensure_test_home();
    let local_app_data = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA must be isolated for tests"),
    );

    assert_eq!(local_app_data, home.join("AppData").join("Local"));
}

#[test]
fn test_home_is_unique_to_the_test_process() {
    let _guard = lock_test_fs();
    let home = ensure_test_home();
    let expected_name = format!("cc-switch-test-home-{}", std::process::id());

    assert_eq!(
        home.file_name().and_then(|name| name.to_str()),
        Some(expected_name.as_str())
    );
}

#[cfg(windows)]
#[test]
fn reset_test_fs_removes_isolated_windows_appdata() {
    let _guard = lock_test_fs();
    let home = ensure_test_home();
    let sentinel = home
        .join("AppData")
        .join("Local")
        .join("Claude-3p")
        .join("configLibrary")
        .join("stale-profile.json");
    std::fs::create_dir_all(sentinel.parent().expect("sentinel parent"))
        .expect("create isolated AppData fixture");
    std::fs::write(&sentinel, b"stale fixture").expect("write isolated AppData fixture");

    reset_test_fs();

    assert!(!sentinel.exists(), "reset must remove isolated AppData");
}

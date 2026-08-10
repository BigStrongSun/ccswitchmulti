use crate::error::AppError;
#[cfg(not(target_os = "windows"))]
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// 获取 macOS 上的 .app bundle 路径
/// 将 `/path/to/CC Switch.app/Contents/MacOS/CC Switch` 转换为 `/path/to/CC Switch.app`
#[cfg(target_os = "macos")]
fn get_macos_app_bundle_path(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    let path_str = exe_path.to_string_lossy();
    // 查找 .app/Contents/MacOS/ 模式
    if let Some(app_pos) = path_str.find(".app/Contents/MacOS/") {
        let app_bundle_end = app_pos + 4; // ".app" 的结束位置
        Some(std::path::PathBuf::from(&path_str[..app_bundle_end]))
    } else {
        None
    }
}

/// 初始化 AutoLaunch 实例（macOS / Linux）
#[cfg(not(target_os = "windows"))]
fn get_auto_launch() -> Result<AutoLaunch, AppError> {
    let app_name = "CCSwitchMulti";
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;

    // macOS 需要使用 .app bundle 路径，否则 AppleScript login item 会打开终端
    #[cfg(target_os = "macos")]
    let app_path = get_macos_app_bundle_path(&exe_path).unwrap_or(exe_path);

    #[cfg(target_os = "linux")]
    let app_path = exe_path;

    // 使用 AutoLaunchBuilder 消除平台差异
    // macOS: 使用 AppleScript 方式（默认），需要 .app bundle 路径
    // Linux: 使用 XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(app_name)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

/// Windows 注册表自启实现。
///
/// `auto-launch 0.5.0` 写入 Run 值时不会给含空格的 exe 路径加引号，且禁用时
/// 不清理 Task Manager 的 `StartupApproved` 覆盖项。Windows 分支直接维护这两处
/// 注册表状态，并把真实的读写错误返回给调用方。
#[cfg(target_os = "windows")]
mod windows {
    use crate::error::AppError;
    use std::io::ErrorKind;
    use std::path::Path;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::{RegKey, RegValue};

    const RUN_REGKEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
    const STARTUP_APPROVED_REGKEY: &str =
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    const APP_NAME: &str = "CCSwitchMulti";
    const ENABLED_MARKER: [u8; 12] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    fn registry_error(context: &str, error: std::io::Error) -> AppError {
        AppError::Message(format!("{context}: {error}"))
    }

    fn current_exe() -> Result<std::path::PathBuf, AppError> {
        std::env::current_exe().map_err(|error| registry_error("无法获取应用路径", error))
    }

    fn quoted_exe_path(path: &Path) -> String {
        format!("\"{}\"", path.display())
    }

    pub(super) fn run_value_matches_exe(run_value: &str, exe: &Path) -> bool {
        let configured = run_value.trim().trim_matches('"');
        configured.eq_ignore_ascii_case(&exe.to_string_lossy())
    }

    pub(super) fn startup_approved_enabled(raw_value: Option<&[u8]>) -> bool {
        raw_value
            .and_then(|bytes| {
                (bytes.len() >= 8).then(|| bytes.iter().rev().take(8).all(|byte| *byte == 0))
            })
            .unwrap_or(true)
    }

    fn delete_value_if_present(
        key: &RegKey,
        value_name: &str,
        context: &str,
    ) -> Result<(), AppError> {
        match key.delete_value(value_name) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(registry_error(context, error)),
        }
    }

    pub(super) fn enable() -> Result<(), AppError> {
        let exe = current_exe()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run_key, _) = hkcu
            .create_subkey(RUN_REGKEY)
            .map_err(|error| registry_error("打开 Run 注册表键失败", error))?;
        run_key
            .set_value(APP_NAME, &quoted_exe_path(&exe))
            .map_err(|error| registry_error("写入自启 Run 值失败", error))?;

        let startup_approved_result = hkcu
            .create_subkey(STARTUP_APPROVED_REGKEY)
            .map_err(|error| registry_error("打开 StartupApproved 注册表键失败", error))
            .and_then(|(key, _)| {
                key.set_raw_value(
                    APP_NAME,
                    &RegValue {
                        vtype: winreg::enums::RegType::REG_BINARY,
                        bytes: ENABLED_MARKER.to_vec(),
                    },
                )
                .map_err(|error| registry_error("写入 StartupApproved 启用状态失败", error))
            });

        if let Err(error) = startup_approved_result {
            let _ = run_key.delete_value(APP_NAME);
            return Err(error);
        }

        log::info!("已启用开机自启");
        Ok(())
    }

    pub(super) fn disable() -> Result<(), AppError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.open_subkey_with_flags(RUN_REGKEY, KEY_SET_VALUE) {
            Ok(key) => delete_value_if_present(&key, APP_NAME, "删除自启 Run 值失败")?,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(registry_error("打开 Run 注册表键失败", error)),
        }

        match hkcu.open_subkey_with_flags(STARTUP_APPROVED_REGKEY, KEY_SET_VALUE) {
            Ok(key) => delete_value_if_present(&key, APP_NAME, "删除 StartupApproved 状态失败")?,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(registry_error("打开 StartupApproved 注册表键失败", error)),
        }

        log::info!("已禁用开机自启");
        Ok(())
    }

    pub(super) fn is_enabled() -> Result<bool, AppError> {
        let exe = current_exe()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = match hkcu.open_subkey_with_flags(RUN_REGKEY, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(registry_error("读取 Run 注册表键失败", error)),
        };
        let run_value = match run_key.get_value::<String, _>(APP_NAME) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(registry_error("读取自启 Run 值失败", error)),
        };
        if !run_value_matches_exe(&run_value, &exe) {
            return Ok(false);
        }

        let startup_key = match hkcu.open_subkey_with_flags(STARTUP_APPROVED_REGKEY, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(registry_error("读取 StartupApproved 注册表键失败", error)),
        };
        let raw_value = match startup_key.get_raw_value(APP_NAME) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(registry_error("读取 StartupApproved 状态失败", error)),
        };

        Ok(startup_approved_enabled(Some(&raw_value.bytes)))
    }
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        windows::enable()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .enable()
            .map_err(|e| AppError::Message(format!("启用开机自启失败: {e}")))?;
        log::info!("已启用开机自启");
        Ok(())
    }
}

/// 禁用开机自启
pub fn disable_auto_launch() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        windows::disable()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .disable()
            .map_err(|e| AppError::Message(format!("禁用开机自启失败: {e}")))?;
        log::info!("已禁用开机自启");
        Ok(())
    }
}

/// 检查是否已启用开机自启
pub fn is_auto_launch_enabled() -> Result<bool, AppError> {
    #[cfg(target_os = "windows")]
    {
        windows::is_enabled()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let auto_launch = get_auto_launch()?;
        auto_launch
            .is_enabled()
            .map_err(|e| AppError::Message(format!("检查开机自启状态失败: {e}")))
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_run_value_matches_current_exe_case_insensitively() {
        let exe = std::path::Path::new(r"C:\Program Files\CCSwitchMulti\cc-switch.exe");

        assert!(windows::run_value_matches_exe(
            r#""c:\PROGRAM FILES\CCSwitchMulti\CC-SWITCH.EXE""#,
            exe
        ));
        assert!(!windows::run_value_matches_exe(
            r#""C:\Program Files\OldCCSwitch\cc-switch.exe""#,
            exe
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_startup_approved_disabled_marker_overrides_run_value() {
        let enabled = [
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let disabled = [
            0x03, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        ];

        assert!(windows::startup_approved_enabled(None));
        assert!(windows::startup_approved_enabled(Some(&enabled)));
        assert!(!windows::startup_approved_enabled(Some(&disabled)));
    }

    #[test]
    fn startup_reconciliation_repairs_persisted_and_system_state_drift() {
        assert_eq!(auto_launch_reconciliation_action(true, false), Some(true));
        assert_eq!(auto_launch_reconciliation_action(false, true), Some(false));
        assert_eq!(auto_launch_reconciliation_action(true, true), None);
        assert_eq!(auto_launch_reconciliation_action(false, false), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_valid() {
        let exe_path = std::path::Path::new("/Applications/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from("/Applications/CC Switch.app"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_with_spaces() {
        let exe_path =
            std::path::Path::new("/Users/test/My Apps/CC Switch.app/Contents/MacOS/CC Switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(
            result,
            Some(std::path::PathBuf::from(
                "/Users/test/My Apps/CC Switch.app"
            ))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_not_in_bundle() {
        let exe_path = std::path::Path::new("/usr/local/bin/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_macos_app_bundle_path_dev_build() {
        // 开发环境下的路径通常不在 .app bundle 内
        let exe_path = std::path::Path::new("/Users/dev/project/target/debug/cc-switch");
        let result = get_macos_app_bundle_path(exe_path);
        assert_eq!(result, None);
    }
}

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
    // Linux: XDG autostart
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(app_name)
        .set_app_path(&app_path.to_string_lossy())
        .build()
        .map_err(|e| AppError::Message(format!("创建 AutoLaunch 失败: {e}")))?;

    Ok(auto_launch)
}

/// Windows 注册表自启实现。
///
/// auto-launch 0.5.0 的 Windows 实现有两处缺陷：
/// 1. enable() 写 Run 值时路径不加引号，安装在含空格路径（如 Program Files）时
///    登录按空格截断导致启动失败；
/// 2. disable() 只删 Run 值、不清除 StartupApproved 覆盖项，关过自启后残留
///    假启用状态。
/// 因此 Windows 分支直接用 winreg 操作注册表，绕开 crate。
#[cfg(target_os = "windows")]
mod windows {
    use crate::error::AppError;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::{RegKey, RegValue};

    const RUN_REGKEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
    const STARTUP_APPROVED_REGKEY: &str =
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    const APP_NAME: &str = "CCSwitchMulti";
    // 任务管理器 StartupApproved 的 enabled 标记（12 字节，首字节 0x02）
    const ENABLED_MARKER: [u8; 12] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    fn run_key() -> Result<RegKey, AppError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.create_subkey(RUN_REGKEY)
            .map(|(key, _)| key)
            .map_err(|e| AppError::Message(format!("打开 Run 注册表键失败: {e}")))
    }

    /// 当前 exe 路径，用双引号包裹，避免路径含空格时按空格截断。
    fn quoted_exe_path() -> Result<String, AppError> {
        let exe =
            std::env::current_exe().map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;
        Ok(format!("\"{}\"", exe.display()))
    }

    pub fn enable() -> Result<(), AppError> {
        let value = quoted_exe_path()?;
        run_key()?
            .set_value(APP_NAME, &value)
            .map_err(|e| AppError::Message(format!("写入自启注册表失败: {e}")))?;

        // 写入 enabled 标记，避免残留的 disabled 覆盖项阻止自启。
        if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            STARTUP_APPROVED_REGKEY,
            KEY_SET_VALUE,
        ) {
            let _ = hkcu.set_raw_value(
                APP_NAME,
                &RegValue {
                    vtype: winreg::enums::RegType::REG_BINARY,
                    bytes: ENABLED_MARKER.to_vec(),
                },
            );
        }

        log::info!("已启用开机自启");
        Ok(())
    }

    pub fn disable() -> Result<(), AppError> {
        let key = run_key()?;
        // Run 值可能已不存在，删除失败不视为错误。
        if let Err(e) = key.delete_value(APP_NAME) {
            log::debug!("删除自启 Run 值失败（可能不存在）: {e}");
        }

        // 同步清理 StartupApproved，避免残留 enabled 标记造成假启用状态。
        if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            STARTUP_APPROVED_REGKEY,
            KEY_SET_VALUE,
        ) {
            let _ = hkcu.delete_value(APP_NAME);
        }

        log::info!("已禁用开机自启");
        Ok(())
    }

    pub fn is_enabled() -> Result<bool, AppError> {
        let exe = std::env::current_exe()
            .map_err(|e| AppError::Message(format!("无法获取应用路径: {e}")))?;
        let exe_str = exe.display().to_string();
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_value_ok = hkcu
            .open_subkey_with_flags(RUN_REGKEY, KEY_READ)
            .and_then(|key| key.get_value::<String, _>(APP_NAME))
            .map(|value| {
                // 值为引号包裹的当前 exe 路径才视为有效自启，避免残留旧路径误报。
                value.trim().trim_matches('"') == exe_str
            })
            .unwrap_or(false);
        Ok(run_value_ok)
    }
}

/// 启用开机自启
pub fn enable_auto_launch() -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        windows::enable()?;
        return Ok(());
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
        windows::disable()?;
        return Ok(());
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
        return windows::is_enabled();
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

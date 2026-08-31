use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProcessIdentity {
    pub pid: u32,
    pub executable_path: String,
    pub started_at_ticks: u64,
}

impl ProcessIdentity {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.started_at_ticks == other.started_at_ticks
            && normalized_path(&self.executable_path) == normalized_path(&other.executable_path)
    }

    pub(crate) fn same_executable(&self, other: &Self) -> bool {
        executable_paths_match(&self.executable_path, &other.executable_path)
    }
}

fn executable_paths_match(left: &str, right: &str) -> bool {
    if normalized_path(left) == normalized_path(right) {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        if let (Some(left_identity), Some(right_identity)) =
            (windows_file_identity(left), windows_file_identity(right))
        {
            return left_identity == right_identity;
        }
    }

    false
}

pub(crate) fn current_process_identity() -> Option<ProcessIdentity> {
    process_identity(std::process::id())
}

pub(crate) fn current_executable_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(&path).ok().or(Some(path)))
}

pub(crate) fn executable_matches_current(path: &str) -> bool {
    current_executable_path()
        .is_some_and(|current| executable_paths_match(path, current.to_string_lossy().as_ref()))
}

pub(crate) fn executable_fingerprint(path: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    #[cfg(target_os = "windows")]
    {
        if let Some((volume, file_id)) = windows_file_identity(path) {
            hasher.update(b"windows-file:");
            hasher.update(volume.to_le_bytes());
            hasher.update(file_id);
            return format!("{:x}", hasher.finalize())[..16].to_string();
        }
    }
    hasher.update(normalized_path(path).as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

#[cfg(target_os = "windows")]
fn windows_file_identity(path: &str) -> Option<(u64, [u8; 16])> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    let file = std::fs::File::open(path).ok()?;
    let mut information = FILE_ID_INFO::default();
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            std::ptr::addr_of_mut!(information).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } != 0;
    succeeded.then_some((
        information.VolumeSerialNumber,
        information.FileId.Identifier,
    ))
}

pub(crate) fn config_scope_fingerprint() -> String {
    use sha2::{Digest, Sha256};

    let path = crate::config::get_app_config_dir();
    let normalized = std::fs::canonicalize(&path).unwrap_or(path);
    let mut hasher = Sha256::new();
    hasher.update(normalized_path(normalized.to_string_lossy().as_ref()).as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn normalized_path(path: &str) -> String {
    let mut normalized = Path::new(path)
        .components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
        normalized = format!(r"\\{rest}");
    } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        normalized = rest.to_string();
    }
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let result = (|| {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return None;
        }

        let mut path = vec![0u16; 32_768];
        let mut path_len = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut path_len) } == 0 {
            return None;
        }
        path.truncate(path_len as usize);
        let executable_path = String::from_utf16(&path).ok()?;
        let started_at_ticks =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        Some(ProcessIdentity {
            pid,
            executable_path,
            started_at_ticks,
        })
    })();

    unsafe { CloseHandle(handle) };
    result
}

#[cfg(target_os = "windows")]
pub(crate) fn tcp_listener_owner_pid(port: u16) -> Option<u32> {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    if port == 0 {
        return None;
    }
    let mut byte_len = 0u32;
    let first = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut byte_len,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if first != ERROR_INSUFFICIENT_BUFFER || byte_len < std::mem::size_of::<u32>() as u32 {
        return None;
    }

    let word_len = (byte_len as usize).div_ceil(std::mem::size_of::<u32>());
    let mut buffer = vec![0u32; word_len];
    let result = unsafe {
        GetExtendedTcpTable(
            buffer.as_mut_ptr().cast::<c_void>(),
            &mut byte_len,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != 0 {
        return None;
    }

    let count = buffer[0] as usize;
    let rows = unsafe { buffer.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>() };
    for index in 0..count {
        let row = unsafe { *rows.add(index) };
        if u16::from_be(row.dwLocalPort as u16) == port {
            return (row.dwOwningPid != 0).then_some(row.dwOwningPid);
        }
    }
    None
}

#[cfg(target_os = "windows")]
pub(crate) fn terminate_verified_process(expected: &ProcessIdentity) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };
    const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

    let observed = process_identity(expected.pid)
        .ok_or_else(|| format!("进程 {} 已退出或无法读取身份", expected.pid))?;
    if !expected.matches(&observed) {
        return Err(format!(
            "进程 {} 的身份在终止前发生变化，已拒绝操作",
            expected.pid
        ));
    }

    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | SYNCHRONIZE_ACCESS | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            expected.pid,
        )
    };
    if handle.is_null() {
        return Err(format!("无法打开待替换的旧 CCSM 进程 {}", expected.pid));
    }

    let result = (|| {
        let final_observed = process_identity(expected.pid)
            .ok_or_else(|| format!("进程 {} 已在终止前退出", expected.pid))?;
        if !expected.matches(&final_observed) {
            return Err(format!(
                "进程 {} 的身份在最终校验时发生变化，已拒绝操作",
                expected.pid
            ));
        }
        if unsafe { TerminateProcess(handle, 1) } == 0 {
            return Err(format!("无法终止待替换的旧 CCSM 进程 {}", expected.pid));
        }
        match unsafe { WaitForSingleObject(handle, 10_000) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(format!("等待旧 CCSM 进程 {} 退出超时", expected.pid)),
            status => Err(format!(
                "等待旧 CCSM 进程 {} 退出失败，状态码 {status}",
                expected.pid
            )),
        }
    })();

    unsafe { CloseHandle(handle) };
    result
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn terminate_verified_process(_expected: &ProcessIdentity) -> Result<(), String> {
    Err("当前平台尚不支持强制替换旧 CCSM 监听进程".to_string())
}

#[cfg(all(unix, not(target_os = "windows")))]
pub(crate) fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    let executable_path = std::fs::read_link(proc_dir.join("exe"))
        .ok()?
        .to_string_lossy()
        .to_string();
    let stat = std::fs::read_to_string(proc_dir.join("stat")).ok()?;
    let close = stat.rfind(')')?;
    let fields = stat
        .get(close + 2..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let started_at_ticks = fields.get(19)?.parse().ok()?;
    Some(ProcessIdentity {
        pid,
        executable_path,
        started_at_ticks,
    })
}

#[cfg(all(unix, not(target_os = "windows")))]
pub(crate) fn tcp_listener_owner_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn process_identity(_pid: u32) -> Option<ProcessIdentity> {
    None
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn tcp_listener_owner_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_requires_pid_executable_and_start_time_to_match() {
        let expected = ProcessIdentity {
            pid: 42,
            executable_path: r"C:\Apps\cc-switch.exe".to_string(),
            started_at_ticks: 100,
        };
        assert!(expected.matches(&expected));
        assert!(!expected.matches(&ProcessIdentity {
            started_at_ticks: 101,
            ..expected.clone()
        }));
        assert!(!expected.matches(&ProcessIdentity {
            executable_path: r"C:\Other\cc-switch.exe".to_string(),
            ..expected.clone()
        }));
        assert!(!expected.matches(&ProcessIdentity {
            pid: 43,
            ..expected.clone()
        }));
    }

    #[test]
    fn executable_fingerprint_is_case_insensitive_on_windows() {
        let upper = executable_fingerprint(r"C:\Apps\CC-SWITCH.EXE");
        let lower = executable_fingerprint(r"c:\apps\cc-switch.exe");
        if cfg!(windows) {
            assert_eq!(upper, lower);
        } else {
            assert_ne!(upper, lower);
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn executable_identity_matches_windows_virtualized_hardlink_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host_path = dir.path().join("cc-switch.exe");
        let projected_path = dir.path().join("cc-switch-projected.exe");
        std::fs::write(&host_path, b"same executable").expect("write host executable");
        std::fs::hard_link(&host_path, &projected_path).expect("create projected hard link");

        assert!(executable_paths_match(
            host_path.to_string_lossy().as_ref(),
            projected_path.to_string_lossy().as_ref(),
        ));
        assert_eq!(
            executable_fingerprint(host_path.to_string_lossy().as_ref()),
            executable_fingerprint(projected_path.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn current_process_has_queryable_identity() {
        let identity = current_process_identity().expect("current process identity");
        assert_eq!(identity.pid, std::process::id());
        assert!(identity.started_at_ticks > 0);
        assert!(!identity.executable_path.trim().is_empty());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn tcp_listener_owner_resolves_to_the_current_process() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();

        assert_eq!(tcp_listener_owner_pid(port), Some(std::process::id()));
    }
}

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

/// Why a listener's owning process identity could not be read.
///
/// Windows reports "this PID does not exist" and "this PID belongs to another
/// account or is otherwise protected" with different Win32 codes. The port
/// ownership guard needs that distinction: a stale/dead PID and an unreadable
/// foreign process are both fail-closed, but they mean very different things to
/// the user reading the log or the toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessIdentityError {
    NotFound,
    AccessDenied,
    Unavailable(u32),
}

impl ProcessIdentityError {
    #[cfg(target_os = "windows")]
    pub(crate) fn from_win32(code: u32) -> Self {
        const ERROR_ACCESS_DENIED: u32 = 5;
        const ERROR_INVALID_PARAMETER: u32 = 87;
        match code {
            ERROR_ACCESS_DENIED => Self::AccessDenied,
            ERROR_INVALID_PARAMETER => Self::NotFound,
            other => Self::Unavailable(other),
        }
    }

    /// Human readable, Chinese-first detail used in logs and toasts.
    pub(crate) fn detail(self) -> String {
        match self {
            Self::NotFound => "进程已不存在（PID 已失效或已被回收，Win32 87）".to_string(),
            Self::AccessDenied => {
                "拒绝访问（Win32 5：该进程属于其它账户或受系统保护，当前用户无法读取）".to_string()
            }
            Self::Unavailable(code) => format!("读取进程身份失败（Win32 {code}）"),
        }
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

/// One TCP endpoint row for a local port, regardless of socket state.
///
/// The ownership guard only ever looked at `LISTEN` rows through
/// `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_LISTENER)`. That silently hides the
/// two other ways a `bind` can fail: a residual socket whose owner already
/// exited, and a socket held for the port that is not in `LISTEN` at all. These
/// rows make the failure explainable instead of "identity could not be
/// verified".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TcpPortRow {
    pub family: &'static str,
    pub state: &'static str,
    pub local_address: String,
    pub remote_address: String,
    pub pid: u32,
}

#[cfg(target_os = "windows")]
fn tcp_state_label(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RCVD",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}

/// Read every local TCP endpoint row that uses `port` (IPv4 + IPv6, all states).
#[cfg(target_os = "windows")]
pub(crate) fn tcp_port_rows(port: u16) -> Vec<TcpPortRow> {
    let mut rows = Vec::new();
    collect_tcp4_port_rows(port, &mut rows);
    collect_tcp6_port_rows(port, &mut rows);
    rows
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn tcp_port_rows(port: u16) -> Vec<TcpPortRow> {
    let _ = port;
    Vec::new()
}

#[cfg(target_os = "windows")]
fn collect_tcp4_port_rows(port: u16, rows: &mut Vec<TcpPortRow>) {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    let Some(buffer) = query_tcp_table(AF_INET as u32, TCP_TABLE_OWNER_PID_ALL) else {
        return;
    };
    let count = table_entry_count::<MIB_TCPROW_OWNER_PID>(&buffer);
    let entries = unsafe { buffer.as_ptr().add(1).cast::<MIB_TCPROW_OWNER_PID>() };
    for index in 0..count {
        let row = unsafe { *entries.add(index) };
        let local_port = u16::from_be(row.dwLocalPort as u16);
        if local_port != port {
            continue;
        }
        let local_address = std::net::Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
        let remote_address = std::net::Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes());
        let remote_port = u16::from_be(row.dwRemotePort as u16);
        rows.push(TcpPortRow {
            family: "ipv4",
            state: tcp_state_label(row.dwState),
            local_address: format!("{local_address}:{local_port}"),
            remote_address: format!("{remote_address}:{remote_port}"),
            pid: row.dwOwningPid,
        });
    }
}

#[cfg(target_os = "windows")]
fn collect_tcp6_port_rows(port: u16, rows: &mut Vec<TcpPortRow>) {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCP6ROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET6;

    let Some(buffer) = query_tcp_table(AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL) else {
        return;
    };
    let count = table_entry_count::<MIB_TCP6ROW_OWNER_PID>(&buffer);
    let entries = unsafe { buffer.as_ptr().add(1).cast::<MIB_TCP6ROW_OWNER_PID>() };
    for index in 0..count {
        let row = unsafe { *entries.add(index) };
        let local_port = u16::from_be(row.dwLocalPort as u16);
        if local_port != port {
            continue;
        }
        let local_address = std::net::Ipv6Addr::from(row.ucLocalAddr);
        let remote_address = std::net::Ipv6Addr::from(row.ucRemoteAddr);
        let remote_port = u16::from_be(row.dwRemotePort as u16);
        rows.push(TcpPortRow {
            family: "ipv6",
            state: tcp_state_label(row.dwState),
            local_address: format!("[{local_address}]:{local_port}"),
            remote_address: format!("[{remote_address}]:{remote_port}"),
            pid: row.dwOwningPid,
        });
    }
}

#[cfg(target_os = "windows")]
fn table_entry_count<Row>(buffer: &[u32]) -> usize {
    let Some(header) = buffer.first() else {
        return 0;
    };
    let capacity = (buffer.len().saturating_sub(1) * std::mem::size_of::<u32>())
        / std::mem::size_of::<Row>().max(1);
    (*header as usize).min(capacity)
}

#[cfg(target_os = "windows")]
fn query_tcp_table(address_family: u32, table_class: i32) -> Option<Vec<u32>> {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::NetworkManagement::IpHelper::GetExtendedTcpTable;

    let mut byte_len = 0u32;
    let first = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut byte_len,
            0,
            address_family,
            table_class,
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
            address_family,
            table_class,
            0,
        )
    };
    if result != 0 {
        return None;
    }
    Some(buffer)
}

/// Build the diagnosis text for a port that could not be bound.
///
/// `annotate` returns a short suffix for an owning PID (resolved executable path
/// or the reason the identity is unreadable), which keeps this formatter
/// testable without touching the live process table.
fn summarize_port_rows(port: u16, rows: &[TcpPortRow], annotate: impl Fn(u32) -> String) -> String {
    const MAX_ROWS: usize = 6;
    if rows.is_empty() {
        return format!(
            "端口 {port} 占用诊断：TCP 表里没有该端口的任何条目（既无 LISTEN，也无残留连接）"
        );
    }

    let mut described = Vec::new();
    for row in rows.iter().take(MAX_ROWS) {
        let identity = if row.pid == 0 {
            " 归属内核（PID 0）".to_string()
        } else {
            annotate(row.pid)
        };
        described.push(format!(
            "{} {} {} pid={}{}",
            row.state, row.family, row.local_address, row.pid, identity
        ));
    }
    if rows.len() > MAX_ROWS {
        described.push(format!("…还有 {} 条", rows.len() - MAX_ROWS));
    }
    format!("端口 {port} 占用诊断：{}", described.join("；"))
}

/// Human readable diagnosis of everything the TCP table knows about `port`.
pub(crate) fn describe_port_blockers(port: u16) -> String {
    let rows = tcp_port_rows(port);
    summarize_port_rows(port, &rows, |pid| match process_identity_result(pid) {
        Ok(identity) => format!(" path={}", identity.executable_path),
        Err(error) => format!(" 身份不可读（{}）", error.detail()),
    })
}

/// A live process whose parent is `parent_pid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildProcess {
    pub pid: u32,
    pub name: String,
    pub executable_path: String,
}

/// Whether this image belongs to this product and may therefore be terminated
/// when it is the real holder of a stale listener socket.
///
/// Locally reproduced failure mode: a listening socket can outlive the process
/// that created it when another live process holds a duplicated copy of the
/// handle. The TCP table then keeps naming the dead creator as the owner, so the
/// ownership guard can neither verify nor terminate it. Only this product own
/// helper images (WebView2 host, sidecars, the app itself) may be ended to
/// release such a listener; everything else stays fail-closed.
pub(crate) fn is_product_helper_image(
    name: &str,
    executable_path: &str,
    install_dir: Option<&str>,
) -> bool {
    const HELPER_IMAGES: [&str; 4] = [
        "msedgewebview2.exe",
        "ccsm.exe",
        "codex-history-repairer.exe",
        "cc-switch.exe",
    ];
    let lower_name = name.trim().to_ascii_lowercase();
    if HELPER_IMAGES.contains(&lower_name.as_str()) {
        return true;
    }
    let normalized = normalized_path(executable_path);
    if normalized.is_empty() {
        return false;
    }
    if normalized.contains(r"\microsoft\edgewebview\application\") {
        return true;
    }
    if let Some(dir) = install_dir {
        let dir = normalized_path(dir);
        if !dir.is_empty() && normalized.starts_with(&dir) {
            return true;
        }
    }
    false
}

/// Live child processes of `parent_pid`.
#[cfg(target_os = "windows")]
pub(crate) fn child_processes_of(parent_pid: u32) -> Vec<ChildProcess> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut result = Vec::new();
    if parent_pid == 0 {
        return result;
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return result;
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32ParentProcessID == parent_pid && entry.th32ProcessID != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            result.push(ChildProcess {
                pid: entry.th32ProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..end]),
                executable_path: process_identity(entry.th32ProcessID)
                    .map(|identity| identity.executable_path)
                    .unwrap_or_default(),
            });
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    result
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn child_processes_of(_parent_pid: u32) -> Vec<ChildProcess> {
    Vec::new()
}

/// 判断 PID 当前是否真实存在（用于区分“监听者已死、socket 句柄仍被别人持有”的残留行）。
///
/// 不能只看 OpenProcess 的错误码：进程刚被终止时可能返回 ERROR_GEN_FAILURE(31)，
/// 而不是 ERROR_INVALID_PARAMETER(87)；因此这里直接查进程快照。
#[cfg(target_os = "windows")]
pub(crate) fn process_exists(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    if pid == 0 {
        return false;
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return true;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut found = false;
    while has_entry {
        if entry.th32ProcessID == pid {
            found = true;
            break;
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    found
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn process_exists(pid: u32) -> bool {
    process_identity(pid).is_some()
}

/// 该端口的所有 TCP 行是否都属于同一个 PID（即占用者是“我们自己”而不是外部程序）。
pub(crate) fn port_rows_all_owned_by(port: u16, pid: u32) -> bool {
    let rows = tcp_port_rows(port);
    !rows.is_empty() && rows.iter().all(|row| row.pid == pid)
}

/// 让监听 socket 句柄不可被后续创建的子进程继承。
///
/// 实测过：应用被强杀后，15721 的 LISTEN 行会以“已死的 PID”继续存在，说明句柄被
/// 子进程（WebView2 等）持有；显式清掉继承位可以避免这个残留。
#[cfg(target_os = "windows")]
pub(crate) fn harden_socket_handle_not_inheritable(raw_socket: usize) {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    if raw_socket == 0 || raw_socket == usize::MAX {
        return;
    }
    let handle = raw_socket as windows_sys::Win32::Foundation::HANDLE;
    unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
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
            return hex::encode(hasher.finalize())[..16].to_string();
        }
    }
    hasher.update(normalized_path(path).as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
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
    hex::encode(hasher.finalize())[..16].to_string()
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
pub(crate) fn process_identity_result(pid: u32) -> Result<ProcessIdentity, ProcessIdentityError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return Err(ProcessIdentityError::NotFound);
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Err(ProcessIdentityError::from_win32(unsafe { GetLastError() }));
    }

    let result = (|| {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(ProcessIdentityError::from_win32(unsafe { GetLastError() }));
        }

        let mut path = vec![0u16; 32_768];
        let mut path_len = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut path_len) } == 0 {
            return Err(ProcessIdentityError::from_win32(unsafe { GetLastError() }));
        }
        path.truncate(path_len as usize);
        let executable_path =
            String::from_utf16(&path).map_err(|_| ProcessIdentityError::Unavailable(0))?;
        let started_at_ticks =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        Ok(ProcessIdentity {
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

#[cfg(target_os = "macos")]
pub(crate) fn process_identity_result(pid: u32) -> Result<ProcessIdentity, ProcessIdentityError> {
    use libc::{errno, proc_pidinfo, proc_pidpath, EACCES, EPERM, PROC_PIDTBSDINFO};

    if pid == 0 {
        return Err(ProcessIdentityError::NotFound);
    }

    let mut path = [0u8; 4096];
    let path_len = unsafe {
        proc_pidpath(
            pid as libc::c_int,
            path.as_mut_ptr().cast(),
            path.len() as u32,
        )
    };
    if path_len <= 0 {
        // proc_pidpath 对受保护/其它账户的进程会以 EPERM/EACCES 失败：进程存在但身份不可读，
        // 必须与“进程已不存在”区分（端口归属守卫对两者都 fail-closed，但对用户含义不同）。
        let code = unsafe { errno() };
        return Err(if code == EPERM || code == EACCES {
            ProcessIdentityError::AccessDenied
        } else {
            ProcessIdentityError::NotFound
        });
    }
    let executable_path = std::str::from_utf8(&path[..path_len as usize])
        .map_err(|_| ProcessIdentityError::Unavailable(0))?
        .to_string();

    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let info_size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes_written = unsafe {
        proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            info_size as libc::c_int,
        )
    };
    if bytes_written != info_size as libc::c_int {
        return Err(ProcessIdentityError::Unavailable(0));
    }
    let info = unsafe { info.assume_init() };
    let started_at_ticks = info
        .pbi_start_tvsec
        .saturating_mul(1_000_000)
        .saturating_add(info.pbi_start_tvusec);
    if started_at_ticks == 0 {
        return Err(ProcessIdentityError::Unavailable(0));
    }

    Ok(ProcessIdentity {
        pid,
        executable_path,
        started_at_ticks,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn process_identity_result(pid: u32) -> Result<ProcessIdentity, ProcessIdentityError> {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    let executable_path = std::fs::read_link(proc_dir.join("exe"))
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ProcessIdentityError::NotFound,
            std::io::ErrorKind::PermissionDenied => ProcessIdentityError::AccessDenied,
            _ => ProcessIdentityError::Unavailable(error.raw_os_error().unwrap_or_default() as u32),
        })?
        .to_string_lossy()
        .to_string();
    let stat = std::fs::read_to_string(proc_dir.join("stat"))
        .map_err(|_| ProcessIdentityError::Unavailable(0))?;
    let close = stat
        .rfind(')')
        .ok_or(ProcessIdentityError::Unavailable(0))?;
    let fields = stat
        .get(close + 2..)
        .ok_or(ProcessIdentityError::Unavailable(0))?
        .split_whitespace()
        .collect::<Vec<_>>();
    let started_at_ticks = fields
        .get(19)
        .and_then(|value| value.parse().ok())
        .ok_or(ProcessIdentityError::Unavailable(0))?;
    Ok(ProcessIdentity {
        pid,
        executable_path,
        started_at_ticks,
    })
}

#[cfg(unix)]
pub(crate) fn tcp_listener_owner_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn process_identity_result(_pid: u32) -> Result<ProcessIdentity, ProcessIdentityError> {
    Err(ProcessIdentityError::NotFound)
}

pub(crate) fn process_identity(pid: u32) -> Option<ProcessIdentity> {
    process_identity_result(pid).ok()
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

    #[test]
    #[cfg(target_os = "windows")]
    fn tcp_state_labels_cover_the_states_the_guard_reports() {
        assert_eq!(tcp_state_label(2), "LISTEN");
        assert_eq!(tcp_state_label(5), "ESTABLISHED");
        assert_eq!(tcp_state_label(11), "TIME_WAIT");
        assert_eq!(tcp_state_label(99), "UNKNOWN");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn process_identity_error_details_distinguish_missing_from_foreign() {
        assert!(ProcessIdentityError::from_win32(5)
            .detail()
            .contains("其它账户"));
        assert!(ProcessIdentityError::from_win32(87)
            .detail()
            .contains("已不存在"));
        assert!(ProcessIdentityError::from_win32(1234)
            .detail()
            .contains("1234"));
    }

    #[test]
    fn port_blocker_summary_names_owner_state_and_read_failure() {
        let rows = vec![
            TcpPortRow {
                family: "ipv4",
                state: "LISTEN",
                local_address: "127.0.0.1:15721".to_string(),
                remote_address: "0.0.0.0:0".to_string(),
                pid: 4321,
            },
            TcpPortRow {
                family: "ipv4",
                state: "TIME_WAIT",
                local_address: "127.0.0.1:15721".to_string(),
                remote_address: "127.0.0.1:50000".to_string(),
                pid: 0,
            },
        ];

        let summary = summarize_port_rows(15721, &rows, |pid| {
            format!(" 身份不可读（测试态，PID {pid}）")
        });

        assert!(summary.contains("LISTEN ipv4 127.0.0.1:15721 pid=4321"));
        assert!(summary.contains("TIME_WAIT ipv4 127.0.0.1:15721 pid=0 归属内核（PID 0）"));
        assert!(!summary.contains("没有任何条目"));
    }

    #[test]
    fn port_blocker_summary_reports_a_port_without_any_tcp_row() {
        let summary = summarize_port_rows(15721, &[], |_| String::new());
        assert!(summary.contains("TCP 表里没有该端口的任何条目"));
    }

    #[test]
    fn port_blocker_summary_is_bounded() {
        let rows = (0..9)
            .map(|index| TcpPortRow {
                family: "ipv4",
                state: "ESTABLISHED",
                local_address: "127.0.0.1:15721".to_string(),
                remote_address: format!("127.0.0.1:{}", 50_000 + index),
                pid: 1000 + index,
            })
            .collect::<Vec<_>>();

        let summary = summarize_port_rows(15721, &rows, |_| String::new());
        assert!(summary.contains("…还有 3 条"));
        assert_eq!(summary.matches("ESTABLISHED").count(), 6);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn process_exists_matches_reality() {
        assert!(process_exists(std::process::id()));
        assert!(!process_exists(0));
        assert!(!process_exists(0xFFFF_FFF0));
    }

    #[test]
    fn port_rows_are_attributed_to_the_listening_process() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().expect("addr").port();
        assert!(port_rows_all_owned_by(port, std::process::id()));
        assert!(!port_rows_all_owned_by(
            port,
            std::process::id().wrapping_add(1)
        ));
        let free = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
        let free_port = free.local_addr().expect("addr").port();
        drop(free);
        assert!(!port_rows_all_owned_by(free_port, std::process::id()));
    }
    #[test]
    fn product_helper_allowlist_is_narrow() {
        assert!(is_product_helper_image("msedgewebview2.exe", "", None));
        assert!(is_product_helper_image("CC-SWITCH.EXE", "", None));
        assert!(is_product_helper_image(
            "msedgewebview2.exe",
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\1.2.3\msedgewebview2.exe",
            None
        ));
        assert!(is_product_helper_image(
            "other.exe",
            r"C:\Users\test\AppData\Local\CCSwitchMulti\other.exe",
            Some(r"C:\Users\test\AppData\Local\CCSwitchMulti")
        ));
        assert!(!is_product_helper_image(
            "cmd.exe",
            r"C:\Windows\System32\cmd.exe",
            Some(r"C:\Users\test\AppData\Local\CCSwitchMulti")
        ));
        assert!(!is_product_helper_image(
            "chrome.exe",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            None
        ));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn child_processes_of_reports_a_live_child() {
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping", "-n", "11", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn child");
        let children = child_processes_of(std::process::id());
        let found = children.iter().any(|entry| entry.pid == child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            found,
            "spawned child must be reported as a child of this process: {children:?}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn live_port_rows_describe_a_listening_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let pid = std::process::id();

        let rows = tcp_port_rows(port);
        assert!(
            rows.iter()
                .any(|row| row.state == "LISTEN" && row.pid == pid),
            "expected a LISTEN row for pid {pid}, got {rows:?}"
        );
        let summary = describe_port_blockers(port);
        assert!(summary.contains("LISTEN"), "{summary}");
        assert!(summary.contains(&format!("pid={pid}")), "{summary}");
    }
}

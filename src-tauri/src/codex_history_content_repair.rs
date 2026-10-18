//! Explicit offline recovery of legacy Router agent-message plaintext envelopes.
//! Preview plans contain only metadata; applying always requires a stopped Codex.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

type RepairResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Change {
    line: usize,
    byte_offset: usize,
    item_id: String,
    part_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Target {
    relative_path: String,
    bytes: usize,
    after_bytes: usize,
    compressed: bool,
    before_sha256: String,
    after_sha256: String,
    changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u8,
    id: String,
    codex_home: String,
    scanned_files: usize,
    compressed_files: usize,
    targets: Vec<Target>,
}

struct Scan {
    targets: Vec<Target>,
    inventory: BTreeMap<String, String>,
    compressed: usize,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn io_error(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}: {error}")
}

fn no_links(path: &Path) -> RepairResult<()> {
    let mut component_path = PathBuf::new();
    for part in path.components() {
        component_path.push(part);
        if matches!(part, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&component_path)
            .map_err(|e| io_error("Cannot inspect recovery path", e))?;
        let linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            linked || metadata.file_attributes() & 0x400 != 0
        };
        if linked {
            return Err("Linked or reparse-point recovery paths are not supported".into());
        }
    }
    Ok(())
}

fn canonical_home(home: &Path) -> RepairResult<PathBuf> {
    let home = if home.is_absolute() {
        home.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("Cannot locate current directory", e))?
            .join(home)
    };
    no_links(&home)?;
    if !home.is_dir() {
        return Err("Codex home must be an existing directory".into());
    }
    fs::canonicalize(home).map_err(|e| io_error("Cannot resolve Codex home", e))
}

fn contained_path(home: &Path, relative: &str) -> RepairResult<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.as_os_str().is_empty()
    {
        return Err("Invalid history relative path".into());
    }
    let path = home.join(relative);
    no_links(&path)?;
    // Canonicalize the parent so an already exclusively opened target need not
    // be reopened by Windows' path resolver.
    let parent = fs::canonicalize(path.parent().ok_or("Missing history parent")?)
        .map_err(|e| io_error("Cannot resolve history parent", e))?;
    if !parent.starts_with(home) {
        return Err("History path escaped Codex home".into());
    }
    Ok(path)
}

fn opaque(value: &str) -> bool {
    if value.len() < 64 || !value.is_ascii() {
        return false;
    }
    if value.starts_with("gAAAAA") {
        return true;
    }
    // Python's validated altchars decoder accepts mixed alphabets and ignores
    // unused trailing bits. Keep those ambiguous values opaque as well.
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    let decoder = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let normalized = value
        .trim_end_matches('=')
        .replace('-', "+")
        .replace('_', "/");
    decoder
        .decode(normalized)
        .is_ok_and(|bytes| bytes.len() >= 32)
}

fn repair_rollout(data: &[u8]) -> RepairResult<(Vec<u8>, Vec<Change>)> {
    let lines: Vec<&[u8]> = data.split_inclusive(|byte| *byte == b'\n').collect();
    let Some(header_line) = lines
        .iter()
        .find(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
    else {
        return Ok((data.to_vec(), vec![]));
    };
    let header: Value =
        serde_json::from_slice(header_line).map_err(|_| "Invalid history header JSON")?;
    if header["type"] != "session_meta"
        || !matches!(
            header["payload"]["model_provider"].as_str(),
            Some("codex_model_router" | "codex_model_router_v2" | "cc_switch_codex_router")
        )
    {
        return Ok((data.to_vec(), vec![]));
    }
    let mut changes = Vec::new();
    let mut result = Vec::with_capacity(data.len());
    for (index, line) in lines.into_iter().enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            result.extend_from_slice(line);
            continue;
        }
        let record: Value =
            serde_json::from_slice(line).map_err(|_| "Invalid Router history JSON")?;
        if record["type"] != "response_item" || record["payload"]["type"] != "agent_message" {
            result.extend_from_slice(line);
            continue;
        }
        let item_id = record["payload"]["id"]
            .as_str()
            .filter(|id| id.starts_with("amsg_"))
            .ok_or("Agent message identity cannot be established")?;
        let Some(parts) = record["payload"]["content"].as_array() else {
            return Err("Unknown agent-message content format".into());
        };
        let mut expected = record.clone();
        let mut updated = line.to_vec();
        for (part_index, part) in parts.iter().enumerate() {
            if part["type"] != "encrypted_content" {
                continue;
            }
            let Some(value) = part["encrypted_content"].as_str() else {
                continue;
            };
            if value.is_empty() || opaque(value) {
                continue;
            }
            if part
                .as_object()
                .is_none_or(|fields| fields.len() != 2 || !fields.contains_key("encrypted_content"))
            {
                return Err(
                    "Unknown fields in a plaintext envelope; manual review required".into(),
                );
            }
            let old = serde_json::to_vec(part).map_err(|_| "Cannot encode plaintext envelope")?;
            let mut replacement = serde_json::Map::new();
            replacement.insert("type".into(), Value::String("input_text".into()));
            replacement.insert("text".into(), Value::String(value.into()));
            let replacement = Value::Object(replacement);
            let new =
                serde_json::to_vec(&replacement).map_err(|_| "Cannot encode recovered envelope")?;
            let offsets: Vec<_> = updated
                .windows(old.len())
                .enumerate()
                .filter_map(|(at, bytes)| (bytes == old).then_some(at))
                .collect();
            if offsets.len() != 1 || new.len() > old.len() {
                return Err("Cannot identify one exact fixed-length replacement".into());
            }
            let at = offsets[0];
            updated[at..at + old.len()].fill(b' ');
            updated[at..at + new.len()].copy_from_slice(&new);
            expected["payload"]["content"][part_index] = replacement;
            changes.push(Change {
                line: index + 1,
                byte_offset: result.len(),
                item_id: item_id.into(),
                part_index,
            });
        }
        let decoded: Value =
            serde_json::from_slice(&updated).map_err(|_| "Recovered history is invalid")?;
        if updated.len() != line.len() || decoded != expected {
            return Err("History content or byte boundaries changed unexpectedly".into());
        }
        result.extend_from_slice(&updated);
    }
    if result.len() != data.len() {
        return Err("History byte length changed".into());
    }
    Ok((result, changes))
}

fn repair_stored(data: &[u8], compressed: bool) -> RepairResult<(Vec<u8>, Vec<Change>)> {
    if !compressed {
        return repair_rollout(data);
    }
    let decoded = zstd::stream::decode_all(data).map_err(|_| "Cannot decompress history")?;
    let (repaired, changes) = repair_rollout(&decoded)?;
    if changes.is_empty() {
        return Ok((data.to_vec(), changes));
    }
    let encoded = zstd::stream::encode_all(repaired.as_slice(), 3)
        .map_err(|_| "Cannot recompress history")?;
    if zstd::stream::decode_all(encoded.as_slice())
        .map_err(|_| "Compressed history verification failed")?
        != repaired
    {
        return Err("Compressed history roundtrip failed".into());
    }
    Ok((encoded, changes))
}

fn history_paths(home: &Path) -> RepairResult<Vec<String>> {
    fn visit(home: &Path, directory: &Path, paths: &mut Vec<String>) -> RepairResult<()> {
        no_links(directory)?;
        for entry in fs::read_dir(directory).map_err(|e| io_error("Cannot list history", e))? {
            let entry = entry.map_err(|e| io_error("Cannot list history entry", e))?;
            let path = entry.path();
            no_links(&path)?;
            let kind = entry
                .file_type()
                .map_err(|e| io_error("Cannot inspect history entry", e))?;
            if kind.is_dir() {
                visit(home, &path, paths)?;
            } else if kind.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
            {
                let relative = path
                    .strip_prefix(home)
                    .map_err(|_| "History path escaped Codex home")?
                    .to_str()
                    .ok_or("History path is not UTF-8")?
                    .to_owned();
                contained_path(home, &relative)?;
                paths.push(relative);
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    for folder in ["sessions", "archived_sessions"] {
        let path = home.join(folder);
        if path
            .try_exists()
            .map_err(|e| io_error("Cannot inspect history folder", e))?
        {
            visit(home, &path, &mut paths)?;
        }
    }
    paths.sort();
    Ok(paths)
}

fn scan(home: &Path, locked: &BTreeMap<String, Vec<u8>>) -> RepairResult<Scan> {
    let mut result = Scan {
        targets: Vec::new(),
        inventory: BTreeMap::new(),
        compressed: 0,
    };
    for relative in history_paths(home)? {
        let path = contained_path(home, &relative)?;
        let bytes = match locked.get(&relative) {
            Some(bytes) => bytes.clone(),
            None => fs::read(path).map_err(|e| io_error("Cannot read history", e))?,
        };
        let compressed = relative.ends_with(".jsonl.zst");
        if compressed {
            result.compressed += 1;
        }
        let (after, changes) = repair_stored(&bytes, compressed)?;
        let before_sha256 = digest(&bytes);
        result
            .inventory
            .insert(relative.clone(), before_sha256.clone());
        if !changes.is_empty() {
            result.targets.push(Target {
                relative_path: relative,
                bytes: bytes.len(),
                after_bytes: after.len(),
                compressed,
                before_sha256,
                after_sha256: digest(&after),
                changes,
            });
        }
    }
    Ok(result)
}

fn read_native_config(home: &Path) -> RepairResult<(String, bool)> {
    let path = home.join("config.toml");
    let text = if path
        .try_exists()
        .map_err(|e| io_error("Cannot inspect Codex config", e))?
    {
        no_links(&path)?;
        fs::read_to_string(path).map_err(|e| io_error("Cannot read Codex config", e))?
    } else {
        String::new()
    };
    let document = text
        .trim_start_matches('\u{feff}')
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "Cannot parse Codex config")?;
    let native_fields = |table: &dyn toml_edit::TableLike| {
        table
            .get("model_provider")
            .is_none_or(|item| item.as_str() == Some("openai"))
            && !table.contains_key("openai_base_url")
            && !table.contains_key("model_catalog_json")
            && !table.contains_key("base_url")
            && table
                .get("model_providers")
                .and_then(|item| item.get("openai"))
                .is_none_or(|item| item.get("base_url").is_none())
    };
    let selected_native = match document.get("profile") {
        None => true,
        Some(selected) => selected
            .as_str()
            .and_then(|name| document.get("profiles")?.get(name)?.as_table_like())
            .is_some_and(native_fields),
    };
    let native = native_fields(document.as_table()) && selected_native;
    Ok((text, native))
}

fn outcome(
    plan: &Plan,
    token: &str,
    path: &Path,
    native: bool,
    applied: bool,
    backup: Option<&Path>,
) -> ContentRepairOutcome {
    ContentRepairOutcome { dry_run: !applied, codex_home: plan.codex_home.clone(), preview_token: Some(token.into()),
        plan_path: Some(path.to_string_lossy().into_owned()), backup_dir: backup.map(|path| path.to_string_lossy().into_owned()),
        scanned_files: plan.scanned_files, affected_files: plan.targets.len(), compressed_files: plan.compressed_files,
        repaired_parts: plan.targets.iter().map(|target| target.changes.len()).sum(), native_config: native,
        requires_codex_exit: !applied && !plan.targets.is_empty(), stage: if applied {"completed"} else {"preview"}.into(),
        message: if applied { "History content recovery completed and readback verified." } else if !native {
            "Preview complete. Switch to native official Codex configuration and fully exit Codex before applying."
        } else { "Preview complete. Fully exit Codex before applying this saved plan." }.into() }
}

fn prepare_directory(path: &Path) -> RepairResult<()> {
    // Check every existing ancestor before creating the final directory.
    let mut ancestor = path;
    while !ancestor
        .try_exists()
        .map_err(|e| io_error("Cannot inspect recovery storage", e))?
    {
        ancestor = ancestor.parent().ok_or("Invalid recovery storage")?;
    }
    no_links(ancestor)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| io_error("Cannot create recovery storage", e))?;
    no_links(path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentRepairOutcome {
    pub dry_run: bool,
    pub codex_home: String,
    pub preview_token: Option<String>,
    pub plan_path: Option<String>,
    pub backup_dir: Option<String>,
    pub scanned_files: usize,
    pub affected_files: usize,
    pub compressed_files: usize,
    pub repaired_parts: usize,
    pub native_config: bool,
    pub requires_codex_exit: bool,
    pub stage: String,
    pub message: String,
}

pub fn preview(codex_home: Option<String>) -> Result<ContentRepairOutcome, String> {
    let home = codex_home
        .map(std::path::PathBuf::from)
        .unwrap_or_else(crate::codex_config::get_codex_config_dir);
    preview_at(
        &home,
        &crate::config::get_app_config_dir().join("history-content-repair"),
    )
}

pub fn apply(preview_token: String) -> Result<ContentRepairOutcome, String> {
    apply_at(
        &preview_token,
        &crate::config::get_app_config_dir().join("history-content-repair"),
        &SystemRuntime,
    )
}

trait Runtime {
    fn ensure_stopped(&self) -> RepairResult<()>;
    fn before_backup(&self) -> RepairResult<()> {
        Ok(())
    }
    fn before_receipt(&self, _backup: &Path) -> RepairResult<()> {
        Ok(())
    }
    fn write_target(
        &self,
        _index: usize,
        _restoring: bool,
        file: &mut File,
        bytes: &[u8],
    ) -> RepairResult<()> {
        write_and_sync(file, bytes)
    }
    fn verify_target(
        &self,
        _index: usize,
        _restoring: bool,
        actual: &[u8],
        expected: &[u8],
    ) -> RepairResult<()> {
        if actual == expected {
            Ok(())
        } else {
            Err("History byte readback failed".into())
        }
    }
}

struct SystemRuntime;
impl Runtime for SystemRuntime {
    fn ensure_stopped(&self) -> RepairResult<()> {
        ensure_codex_stopped()
    }
}

#[cfg(windows)]
fn ensure_codex_stopped() -> RepairResult<()> {
    use std::os::windows::process::CommandExt;
    // Never inspect arbitrary command-line substrings: an external agent or
    // this inspection command can legitimately mention Codex by name.
    let script = r#"$ErrorActionPreference='Stop'; try {
      $all = @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop);
      if ($all.Count -eq 0) { throw 'Empty process inventory' }
      $busy = @($all | Where-Object {
        $name = [System.IO.Path]::GetFileNameWithoutExtension($_.Name).ToLowerInvariant();
        ($name -in @('codex','chatgpt','codex-app-server','chatgpt-app-server')) -or
        (($name -eq 'app-server') -and ($_.ExecutablePath -match '(?i)[\\/](codex|chatgpt)[\\/]'))
      });
      if ($busy.Count -gt 0) { [Console]::Out.Write('busy') } else { [Console]::Out.Write('clear') }
    } catch { [Console]::Error.Write('Process inspection failed'); exit 2 }"#;
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|_| "Cannot inspect Codex processes; apply refused")?;
    if !output.status.success() {
        return Err("Cannot inspect Codex processes; apply refused".into());
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "clear" => Ok(()),
        "busy" => Err("Codex or ChatGPT is still running. Fully exit it before applying.".into()),
        _ => Err("Unrecognized Codex process inspection result; apply refused".into()),
    }
}

#[cfg(not(windows))]
fn ensure_codex_stopped() -> RepairResult<()> {
    fn busy_name(path: &str) -> bool {
        let name = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "codex" | "chatgpt" | "codex-app-server" | "chatgpt-app-server"
        ) || (name == "app-server"
            && path
                .to_ascii_lowercase()
                .split('/')
                .any(|part| matches!(part, "codex" | "chatgpt")))
    }
    #[cfg(target_os = "linux")]
    {
        for entry in
            fs::read_dir("/proc").map_err(|_| "Cannot inspect Codex processes; apply refused")?
        {
            let entry = entry.map_err(|_| "Cannot inspect process inventory")?;
            if !entry
                .file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                continue;
            }
            let comm = match fs::read_to_string(entry.path().join("comm")) {
                Ok(comm) => comm,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("Cannot inspect process identity; apply refused".into()),
            };
            if busy_name(comm.trim()) {
                return Err(
                    "Codex or ChatGPT is still running. Fully exit it before applying.".into(),
                );
            }
            if comm.trim() == "app-server" {
                let executable = fs::read_link(entry.path().join("exe"))
                    .map_err(|_| "Cannot inspect app-server identity; apply refused")?;
                if busy_name(&executable.to_string_lossy()) {
                    return Err("Codex app-server is still running".into());
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-axo", "comm="])
            .output()
            .map_err(|_| "Cannot inspect Codex processes; apply refused")?;
        if !output.status.success() || output.stdout.is_empty() {
            return Err("Cannot inspect Codex processes; apply refused".into());
        }
        if String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| busy_name(line.trim()))
        {
            return Err("Codex or ChatGPT is still running. Fully exit it before applying.".into());
        }
        Ok(())
    }
}

fn exclusive_file(path: &Path) -> RepairResult<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x00200000); // OPEN_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|e| {
        io_error(
            "History is busy or inaccessible; close Codex before applying",
            e,
        )
    })?;
    #[cfg(not(windows))]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("History is busy; close Codex before applying".into());
        }
    }
    let metadata = file
        .metadata()
        .map_err(|e| io_error("Cannot inspect opened history", e))?;
    if !metadata.is_file() {
        return Err("History target is not a regular file".into());
    }
    ensure_single_link(&file)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Opened history is a reparse point".into());
        }
    }
    Ok(file)
}

fn ensure_single_link(file: &File) -> RepairResult<()> {
    #[cfg(windows)]
    let links = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) }
            == 0
        {
            return Err(io_error(
                "Cannot inspect history hard links",
                std::io::Error::last_os_error(),
            ));
        }
        unsafe { information.assume_init() }.nNumberOfLinks as u64
    };
    #[cfg(unix)]
    let links = {
        use std::os::unix::fs::MetadataExt;
        file.metadata()
            .map_err(|e| io_error("Cannot inspect history hard links", e))?
            .nlink()
    };
    if links != 1 {
        return Err("History targets with hard links are not supported".into());
    }
    Ok(())
}

fn read_opened(file: &mut File) -> RepairResult<Vec<u8>> {
    file.seek(SeekFrom::Start(0))
        .map_err(|e| io_error("Cannot seek history", e))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| io_error("Cannot read opened history", e))?;
    Ok(bytes)
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> RepairResult<()> {
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(bytes))
        .and_then(|_| file.set_len(bytes.len() as u64))
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|e| io_error("Cannot persist history bytes", e))
}

struct OpenedTarget {
    file: File,
    before: Vec<u8>,
    after: Vec<u8>,
}

fn validate_opened_identity(
    home: &Path,
    target: &Target,
    opened: &OpenedTarget,
) -> RepairResult<()> {
    ensure_single_link(&opened.file)?;
    let path = contained_path(home, &target.relative_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let on_path = fs::symlink_metadata(path)
            .map_err(|e| io_error("Cannot recheck history identity", e))?;
        let on_handle = opened
            .file
            .metadata()
            .map_err(|e| io_error("Cannot recheck opened history identity", e))?;
        if on_path.dev() != on_handle.dev() || on_path.ino() != on_handle.ino() {
            return Err("History path was replaced while opening the batch".into());
        }
    }
    #[cfg(windows)]
    let _ = (path, opened); // share_mode(0) prevents target replacement.
    Ok(())
}

fn read_plan(token: &str, storage: &Path) -> RepairResult<(Plan, PathBuf)> {
    if token.len() != 101
        || token.as_bytes()[36] != b'-'
        || !token.as_bytes()[37..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err("Invalid preview token; a saved preview is required".into());
    }
    let id = &token[..36];
    if uuid::Uuid::parse_str(id)
        .map_err(|_| "Invalid preview token")?
        .to_string()
        != id
    {
        return Err("Invalid preview token".into());
    }
    let path = storage.join("plans").join(format!("{token}.json"));
    no_links(&path)?;
    let raw = fs::read(&path).map_err(|e| io_error("Cannot read saved preview plan", e))?;
    if digest(&raw) != token[37..] {
        return Err("Saved preview digest does not match token".into());
    }
    let plan: Plan = serde_json::from_slice(&raw).map_err(|_| "Invalid saved preview plan")?;
    if plan.version != 1 || plan.id != id {
        return Err("Unsupported or mismatched saved preview".into());
    }
    if plan.scanned_files < plan.targets.len()
        || plan.compressed_files
            != plan
                .targets
                .iter()
                .filter(|target| target.compressed)
                .count()
    {
        return Err("Invalid saved preview counts".into());
    }
    let mut unique = std::collections::BTreeSet::new();
    for target in &plan.targets {
        let first = Path::new(&target.relative_path).components().next();
        if !matches!(first, Some(Component::Normal(name)) if name == "sessions" || name == "archived_sessions")
            || !target.relative_path.ends_with(if target.compressed {
                ".jsonl.zst"
            } else {
                ".jsonl"
            })
            || target.changes.is_empty()
            || !unique.insert(&target.relative_path)
        {
            return Err("Invalid saved history target".into());
        }
    }
    Ok((plan, path))
}

fn save_verified(path: &Path, bytes: &[u8]) -> RepairResult<()> {
    prepare_directory(path.parent().ok_or("Missing backup parent")?)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| io_error("Cannot create backup file", e))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| io_error("Cannot persist backup file", e))?;
    drop(file);
    if fs::read(path).map_err(|e| io_error("Cannot verify backup file", e))? != bytes {
        return Err("Backup readback does not match original bytes".into());
    }
    Ok(())
}

fn make_backup(
    home: &Path,
    backup: &Path,
    inventory: &BTreeMap<String, String>,
    locked: &BTreeMap<String, Vec<u8>>,
    config: &str,
) -> RepairResult<()> {
    prepare_directory(backup)?;
    let mut entries = Vec::new();
    for (relative, expected_digest) in inventory {
        let path = contained_path(home, relative)?;
        let bytes = match locked.get(relative) {
            Some(bytes) => bytes.clone(),
            None => fs::read(path)
                .map_err(|e| io_error("Cannot read history for complete backup", e))?,
        };
        if digest(&bytes) != *expected_digest {
            return Err("History changed during backup; create a fresh preview".into());
        }
        save_verified(&backup.join("codex").join(relative), &bytes)?;
        entries.push(serde_json::json!({"relative_path":relative,"bytes":bytes.len(),"sha256":expected_digest}));
    }
    let index = home.join("session_index.jsonl");
    if index
        .try_exists()
        .map_err(|e| io_error("Cannot inspect session index", e))?
    {
        let bytes = fs::read(contained_path(home, "session_index.jsonl")?)
            .map_err(|e| io_error("Cannot read session index", e))?;
        save_verified(&backup.join("codex/session_index.jsonl"), &bytes)?;
        entries.push(serde_json::json!({"relative_path":"session_index.jsonl","bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    if let Some(database) = crate::codex_state_db::resolve_active_codex_state_db_path(
        home,
        config.trim_start_matches('\u{feff}'),
    ) {
        no_links(&database)?;
        let source = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| io_error("Cannot open active Codex SQLite for backup", e))?;
        source
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| io_error("Cannot set SQLite backup timeout", e))?;
        let destination = backup.join("sqlite/state_5.sqlite");
        prepare_directory(destination.parent().ok_or("Missing SQLite backup parent")?)?;
        let mut copy = rusqlite::Connection::open(&destination)
            .map_err(|e| io_error("Cannot create SQLite snapshot", e))?;
        {
            let snapshot = rusqlite::backup::Backup::new(&source, &mut copy)
                .map_err(|e| io_error("Cannot start SQLite backup", e))?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                use rusqlite::backup::StepResult;
                match snapshot
                    .step(128)
                    .map_err(|e| io_error("Cannot complete SQLite backup", e))?
                {
                    StepResult::Done => break,
                    StepResult::More => {}
                    StepResult::Busy | StepResult::Locked => {
                        std::thread::sleep(std::time::Duration::from_millis(25))
                    }
                    _ => return Err("Unknown SQLite backup state".into()),
                }
                if std::time::Instant::now() >= deadline {
                    return Err("SQLite backup timed out before any history write".into());
                }
            }
        }
        let integrity: String = copy
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| io_error("Cannot verify SQLite snapshot", e))?;
        if integrity != "ok" {
            return Err("SQLite snapshot integrity check failed".into());
        }
        drop(copy);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o600))
                .map_err(|e| io_error("Cannot secure SQLite snapshot permissions", e))?;
        }
        let bytes =
            fs::read(&destination).map_err(|e| io_error("Cannot read SQLite snapshot", e))?;
        entries.push(serde_json::json!({"relative_path":"sqlite/state_5.sqlite","source_path":database,"bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    let manifest = serde_json::to_vec_pretty(
        &serde_json::json!({"version":1,"codex_home":home,"files":entries}),
    )
    .map_err(|_| "Cannot serialize backup manifest")?;
    save_verified(&backup.join("manifest.json"), &manifest)
}

fn apply_at(
    token: &str,
    storage: &Path,
    runtime: &impl Runtime,
) -> RepairResult<ContentRepairOutcome> {
    let (plan, plan_path) = read_plan(token, storage)?;
    let home = canonical_home(Path::new(&plan.codex_home))?;
    if home.to_str() != Some(plan.codex_home.as_str()) {
        return Err("Saved Codex home no longer resolves identically".into());
    }
    let (config, native) = read_native_config(&home)?;
    if !native {
        return Err("Recovery requires native official Codex configuration without a catalog or base URL override".into());
    }
    runtime.ensure_stopped()?;
    let current = scan(&home, &BTreeMap::new())?;
    if current.targets != plan.targets {
        return Err("History changed after preview; create a fresh preview".into());
    }
    let mut opened = Vec::new();
    let mut locked = BTreeMap::new();
    for target in &plan.targets {
        let path = contained_path(&home, &target.relative_path)?;
        let mut file = exclusive_file(&path)?;
        let before = read_opened(&mut file)?;
        let (after, changes) = repair_stored(&before, target.compressed)?;
        if before.len() != target.bytes
            || after.len() != target.after_bytes
            || digest(&before) != target.before_sha256
            || digest(&after) != target.after_sha256
            || changes != target.changes
        {
            return Err("Repair plan is stale; create a fresh preview".into());
        }
        locked.insert(target.relative_path.clone(), before.clone());
        opened.push(OpenedTarget {
            file,
            before,
            after,
        });
    }
    // A newly added affected file invalidates the plan even after every target
    // has been exclusively opened. Unrelated native additions remain harmless.
    let current = scan(&home, &locked)?;
    if current.targets != plan.targets {
        return Err("History targets changed while opening the batch".into());
    }
    runtime.ensure_stopped()?;
    if read_native_config(&home)? != (config.clone(), true) {
        return Err("Codex config changed while opening history".into());
    }
    let backup = storage
        .join("backups")
        .join(format!("{}-{}", plan.id, uuid::Uuid::new_v4()));
    runtime.before_backup()?;
    make_backup(&home, &backup, &current.inventory, &locked, &config).map_err(|e| {
        format!(
            "Backup failed before any history write: {e}. Backup location: {}",
            backup.display()
        )
    })?;
    // Keep the pinned original plan inside the snapshot so it remains usable
    // for audit even after old preview files are cleaned up.
    no_links(&plan_path)?;
    let raw_plan =
        fs::read(&plan_path).map_err(|e| io_error("Cannot reread pinned recovery plan", e))?;
    if digest(&raw_plan) != token[37..] {
        return Err(format!(
            "Saved preview changed during backup. Backup: {}",
            backup.display()
        ));
    }
    save_verified(&backup.join("repair-plan.json"), &raw_plan)?;
    for (target, opened) in plan.targets.iter().zip(&mut opened) {
        validate_opened_identity(&home, target, opened)?;
        if read_opened(&mut opened.file)? != opened.before
            || fs::read(backup.join("codex").join(&target.relative_path))
                .map_err(|e| io_error("Cannot verify exact original backup", e))?
                != opened.before
        {
            return Err(format!(
                "Original or backup changed before writing. Backup: {}",
                backup.display()
            ));
        }
    }
    if scan(&home, &locked)?.targets != plan.targets {
        return Err(format!(
            "History targets changed during backup. Backup: {}",
            backup.display()
        ));
    }
    runtime.ensure_stopped()?;
    if read_native_config(&home)? != (config, true) {
        return Err("Codex config changed during backup; no history written".into());
    }
    let mut result = outcome(&plan, token, &plan_path, true, true, Some(&backup));
    result.scanned_files = current.inventory.len();
    let pending_receipt = backup.join(".repair-result.pending.json");
    let transaction = (|| -> RepairResult<()> {
        for (index, (target, opened)) in plan.targets.iter().zip(&mut opened).enumerate() {
            validate_opened_identity(&home, target, opened)?;
            runtime.write_target(index, false, &mut opened.file, &opened.after)?;
            let actual = read_opened(&mut opened.file)?;
            runtime.verify_target(index, false, &actual, &opened.after)?;
            let (again, changes) = repair_stored(&actual, target.compressed)?;
            if again != actual || !changes.is_empty() {
                return Err("Recovered history is not idempotent".into());
            }
            validate_opened_identity(&home, target, opened)?;
        }
        runtime.before_receipt(&backup)?;
        let receipt =
            serde_json::to_vec_pretty(&result).map_err(|_| "Cannot serialize recovery receipt")?;
        save_verified(&pending_receipt, &receipt)?;
        fs::rename(&pending_receipt, backup.join("repair-result.json"))
            .map_err(|e| io_error("Cannot finalize recovery receipt", e))?;
        Ok(())
    })();
    if let Err(original) = transaction {
        let mut failures = Vec::new();
        // Include the file whose write failed and all later files: a partial
        // write or sync error must never prevent the remaining restorations.
        for (index, opened) in opened.iter_mut().enumerate() {
            let restoration = (|| {
                runtime.write_target(index, true, &mut opened.file, &opened.before)?;
                let actual = read_opened(&mut opened.file)?;
                runtime.verify_target(index, true, &actual, &opened.before)
            })();
            if let Err(error) = restoration {
                failures.push(format!("target {}: {error}", index + 1));
            }
        }
        if pending_receipt.exists() {
            if let Err(error) = fs::remove_file(&pending_receipt) {
                failures.push(format!("pending receipt cleanup: {error}"));
            }
        }
        return Err(if failures.is_empty() {
            format!(
                "Recovery failed; every original was restored and verified: {original}. Backup: {}",
                backup.display()
            )
        } else {
            format!("Recovery failed: {original}. Some restorations could not be verified: {}. Exact backups remain at {}", failures.join("; "), backup.display())
        });
    }
    Ok(result)
}

fn preview_at(home: &Path, storage: &Path) -> Result<ContentRepairOutcome, String> {
    let home = canonical_home(home)?;
    let (_, native) = read_native_config(&home)?;
    let current = scan(&home, &BTreeMap::new())?;
    let plan = Plan {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        codex_home: home.to_str().ok_or("Codex home is not UTF-8")?.into(),
        scanned_files: current.inventory.len(),
        compressed_files: current
            .targets
            .iter()
            .filter(|target| target.compressed)
            .count(),
        targets: current.targets,
    };
    let raw = serde_json::to_vec_pretty(&plan).map_err(|_| "Cannot serialize recovery plan")?;
    let token = format!("{}-{}", plan.id, digest(&raw));
    let plans = storage.join("plans");
    prepare_directory(&plans)?;
    let path = plans.join(format!("{token}.json"));
    save_verified(&path, &raw)?;
    Ok(outcome(&plan, &token, &path, native, false, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn fixture(provider: &str, text: &str) -> Vec<u8> {
        let records = [
            json!({"type":"session_meta","payload":{"id":"thread","model_provider":provider,"history_mode":"paginated"}}),
            json!({"type":"response_item","payload":{"type":"agent_message","id":"amsg_test","author":"/root/worker","content":[{"type":"encrypted_content","encrypted_content":text}]}}),
            json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"gAAAAAopaque","summary":[]}}),
        ];
        records
            .iter()
            .flat_map(|record| {
                let mut line = serde_json::to_vec(record).unwrap();
                line.push(b'\n');
                line
            })
            .collect()
    }

    fn home_fixture(root: &Path) -> std::path::PathBuf {
        let home = root.join("home");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::write(
            home.join("config.toml"),
            format!(
                "model_provider = \"openai\"\nsqlite_home = '''{}'''\n",
                home.join("isolated-sqlite").display()
            ),
        )
        .unwrap();
        fs::write(
            home.join("sessions/rollout.jsonl"),
            fixture("codex_model_router_v2", "完整结果保留。"),
        )
        .unwrap();
        home
    }

    #[test]
    fn preview_counts_plaintext_without_copying_body_to_plan() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let before = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
        let result = preview_at(&home, &root.path().join("storage")).unwrap();
        assert!(result.dry_run);
        assert_eq!(
            (
                result.scanned_files,
                result.affected_files,
                result.repaired_parts
            ),
            (1, 1, 1)
        );
        assert_eq!(
            fs::read(home.join("sessions/rollout.jsonl")).unwrap(),
            before
        );
        let plan = fs::read_to_string(result.plan_path.unwrap()).unwrap();
        assert!(!plan.contains("完整结果保留"));
    }

    struct Stopped;
    impl Runtime for Stopped {
        fn ensure_stopped(&self) -> RepairResult<()> {
            Ok(())
        }
    }

    #[test]
    fn apply_saved_plan_preserves_offsets_and_makes_complete_backup() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        fs::create_dir_all(home.join("archived_sessions/subtask")).unwrap();
        let archived = fixture("openai", "原生记录完整保留。");
        fs::write(
            home.join("archived_sessions/subtask/native.jsonl"),
            &archived,
        )
        .unwrap();
        fs::write(home.join("session_index.jsonl"), b"index sentinel\n").unwrap();
        let before = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
        let config_before = fs::read(home.join("config.toml")).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        let result = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped,
        )
        .unwrap();
        assert!(!result.dry_run);
        assert_eq!(result.repaired_parts, 1);
        let after = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
        assert_eq!(
            before
                .split_inclusive(|b| *b == b'\n')
                .map(<[u8]>::len)
                .collect::<Vec<_>>(),
            after
                .split_inclusive(|b| *b == b'\n')
                .map(<[u8]>::len)
                .collect::<Vec<_>>()
        );
        let message: Value =
            serde_json::from_slice(after.split_inclusive(|b| *b == b'\n').nth(1).unwrap()).unwrap();
        assert_eq!(
            message["payload"]["content"][0],
            json!({"type":"input_text","text":"完整结果保留。"})
        );
        let backup = PathBuf::from(result.backup_dir.unwrap());
        assert_eq!(
            fs::read(backup.join("codex/sessions/rollout.jsonl")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(backup.join("codex/archived_sessions/subtask/native.jsonl")).unwrap(),
            archived
        );
        assert_eq!(
            fs::read(backup.join("codex/session_index.jsonl")).unwrap(),
            b"index sentinel\n"
        );
        assert_eq!(fs::read(home.join("config.toml")).unwrap(), config_before);
        assert!(backup.join("repair-result.json").is_file());
        assert_eq!(
            fs::read(backup.join("repair-plan.json")).unwrap(),
            fs::read(result.plan_path.unwrap()).unwrap()
        );
    }

    #[test]
    fn preview_preserves_ambiguous_base64_and_native_records() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let values = [
            format!("{}-", "+".repeat(63)),
            format!("{}B", "A".repeat(65)),
            format!("gAAAAA{}", "A".repeat(160)),
        ];
        let mut originals = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let path = home.join(format!("sessions/opaque-{index}.jsonl"));
            let original = fixture("codex_model_router_v2", value);
            fs::write(&path, &original).unwrap();
            originals.push((path, original));
        }
        let native = fixture("openai", "原生文本不改动");
        fs::write(home.join("sessions/rollout.jsonl"), &native).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        assert_eq!((preview.affected_files, preview.repaired_parts), (0, 0));
        let result = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped,
        )
        .unwrap();
        assert_eq!(result.affected_files, 0);
        for (path, original) in originals {
            assert_eq!(fs::read(path).unwrap(), original);
        }
        assert_eq!(
            fs::read(home.join("sessions/rollout.jsonl")).unwrap(),
            native
        );
    }

    #[test]
    fn compressed_and_plain_history_preserve_full_text_and_line_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        fs::create_dir_all(home.join("archived_sessions/workers")).unwrap();
        let decoded = fixture(
            "cc_switch_codex_router",
            "ASCII full answer.\r\n路径 C:\\work 与中文正文都保留。",
        );
        let decoded = String::from_utf8(decoded)
            .unwrap()
            .replace('\n', "\r\n")
            .into_bytes();
        let original = zstd::stream::encode_all(decoded.as_slice(), 3).unwrap();
        let compressed_path = home.join("archived_sessions/workers/worker.jsonl.zst");
        fs::write(&compressed_path, &original).unwrap();
        let native = zstd::stream::encode_all(fixture("openai", "native").as_slice(), 3).unwrap();
        fs::write(home.join("sessions/native.jsonl.zst"), native).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        assert_eq!(
            (
                preview.scanned_files,
                preview.affected_files,
                preview.compressed_files,
                preview.repaired_parts
            ),
            (3, 2, 1, 2)
        );
        let applied = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped,
        )
        .unwrap();
        let stored_after = fs::read(&compressed_path).unwrap();
        let after = zstd::stream::decode_all(stored_after.as_slice()).unwrap();
        assert_eq!(
            decoded
                .split_inclusive(|b| *b == b'\n')
                .map(<[u8]>::len)
                .collect::<Vec<_>>(),
            after
                .split_inclusive(|b| *b == b'\n')
                .map(<[u8]>::len)
                .collect::<Vec<_>>()
        );
        let record: Value =
            serde_json::from_slice(after.split_inclusive(|b| *b == b'\n').nth(1).unwrap()).unwrap();
        assert_eq!(
            record["payload"]["content"][0]["text"],
            "ASCII full answer.\r\n路径 C:\\work 与中文正文都保留。"
        );
        assert_eq!(
            fs::read(
                PathBuf::from(applied.backup_dir.unwrap())
                    .join("codex/archived_sessions/workers/worker.jsonl.zst")
            )
            .unwrap(),
            original
        );
        assert_eq!(preview_at(&home, &storage).unwrap().repaired_parts, 0);
    }

    #[test]
    fn unknown_fields_missing_identity_and_noncompact_duplicates_fail_closed() {
        for mode in ["fields", "identity", "duplicate", "pretty"] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            let path = home.join("sessions/rollout.jsonl");
            let before = fs::read(&path).unwrap();
            let mut records: Vec<Value> = before
                .split(|b| *b == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect();
            match mode {
                "fields" => records[1]["payload"]["content"][0]["future"] = json!(true),
                "identity" => records[1]["payload"]["id"] = json!("message_unknown"),
                "duplicate" => {
                    let same = records[1]["payload"]["content"][0].clone();
                    records[1]["payload"]["content"]
                        .as_array_mut()
                        .unwrap()
                        .push(same);
                }
                "pretty" => {}
                _ => unreachable!(),
            }
            let mut original: Vec<u8> = records
                .iter()
                .flat_map(|record| {
                    let mut line = serde_json::to_vec(record).unwrap();
                    line.push(b'\n');
                    line
                })
                .collect();
            if mode == "pretty" {
                original = String::from_utf8(original)
                    .unwrap()
                    .replace(
                        "\"type\":\"encrypted_content\"",
                        "\"type\": \"encrypted_content\"",
                    )
                    .into_bytes();
            }
            fs::write(&path, &original).unwrap();
            assert!(
                preview_at(&home, &root.path().join("storage")).is_err(),
                "{mode}"
            );
            assert_eq!(fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn stale_plan_or_new_target_refuses_the_entire_batch_but_native_addition_is_backed_up() {
        for mode in ["changed", "new_target", "native"] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            let path = home.join("sessions/rollout.jsonl");
            let original = fs::read(&path).unwrap();
            let storage = root.path().join("storage");
            let preview = preview_at(&home, &storage).unwrap();
            match mode {
                "changed" => fs::write(
                    &path,
                    fixture("codex_model_router_v2", "预览之后正文改变了。"),
                )
                .unwrap(),
                "new_target" => fs::write(
                    home.join("sessions/new.jsonl"),
                    fixture("codex_model_router", "新增待恢复正文。"),
                )
                .unwrap(),
                "native" => fs::write(
                    home.join("sessions/new.jsonl"),
                    fixture("openai", "新增原生记录。"),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            let before_apply = fs::read(&path).unwrap();
            let result = apply_at(
                preview.preview_token.as_deref().unwrap(),
                &storage,
                &Stopped,
            );
            if mode == "native" {
                let result = result.unwrap();
                assert_eq!(result.scanned_files, 2);
                assert_eq!(
                    fs::read(
                        PathBuf::from(result.backup_dir.unwrap())
                            .join("codex/sessions/rollout.jsonl")
                    )
                    .unwrap(),
                    original
                );
            } else {
                assert!(result.unwrap_err().contains("changed after preview"));
                assert_eq!(fs::read(path).unwrap(), before_apply);
                assert!(!storage.join("backups").exists());
            }
        }
    }

    #[test]
    fn invalid_token_or_modified_plan_is_rejected_without_writes() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        let original = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
        for token in ["../config.toml", "", "C:\\somewhere\\plan.json"] {
            assert!(apply_at(token, &storage, &Stopped)
                .unwrap_err()
                .contains("Invalid preview token"));
        }
        let path = PathBuf::from(preview.plan_path.unwrap());
        let mut raw = fs::read(&path).unwrap();
        raw.push(b' ');
        fs::write(&path, raw).unwrap();
        assert!(apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped
        )
        .unwrap_err()
        .contains("digest"));
        assert_eq!(
            fs::read(home.join("sessions/rollout.jsonl")).unwrap(),
            original
        );
    }

    struct FaultRuntime {
        stopped_error: Option<&'static str>,
        fail_write: Option<usize>,
        fail_readback: Option<usize>,
        fail_restore: Vec<usize>,
        fail_receipt: bool,
        restored: std::sync::Mutex<Vec<usize>>,
    }
    impl Default for FaultRuntime {
        fn default() -> Self {
            Self {
                stopped_error: None,
                fail_write: None,
                fail_readback: None,
                fail_restore: vec![],
                fail_receipt: false,
                restored: std::sync::Mutex::new(vec![]),
            }
        }
    }
    impl Runtime for FaultRuntime {
        fn ensure_stopped(&self) -> RepairResult<()> {
            self.stopped_error.map_or(Ok(()), |error| Err(error.into()))
        }
        fn before_receipt(&self, backup: &Path) -> RepairResult<()> {
            if self.fail_receipt {
                fs::create_dir(backup.join("repair-result.json")).unwrap();
            }
            Ok(())
        }
        fn write_target(
            &self,
            index: usize,
            restoring: bool,
            file: &mut File,
            bytes: &[u8],
        ) -> RepairResult<()> {
            if restoring {
                self.restored.lock().unwrap().push(index);
                write_and_sync(file, bytes)?;
                if self.fail_restore.contains(&index) {
                    return Err("Injected persistent sync failure".into());
                }
            } else if self.fail_write == Some(index) {
                file.seek(SeekFrom::Start(0)).unwrap();
                file.write_all(&bytes[..8]).unwrap();
                file.sync_all().unwrap();
                return Err("Injected partial write failure".into());
            } else {
                write_and_sync(file, bytes)?;
            }
            Ok(())
        }
        fn verify_target(
            &self,
            index: usize,
            restoring: bool,
            actual: &[u8],
            expected: &[u8],
        ) -> RepairResult<()> {
            if !restoring && self.fail_readback == Some(index) {
                return Err("Injected readback failure".into());
            }
            if actual != expected {
                return Err("Byte readback mismatch".into());
            }
            Ok(())
        }
    }

    #[test]
    fn process_failures_busy_file_and_backup_failure_never_write_any_target() {
        for mode in [
            "process_busy",
            "process_probe_failed",
            "busy_file",
            "backup",
        ] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            let second = home.join("sessions/second.jsonl");
            fs::write(&second, fixture("codex_model_router", "第二个文件完整保留")).unwrap();
            let paths = [home.join("sessions/rollout.jsonl"), second];
            let before: Vec<_> = paths.iter().map(|path| fs::read(path).unwrap()).collect();
            let storage = root.path().join("storage");
            let preview = preview_at(&home, &storage).unwrap();
            let held = (mode == "busy_file").then(|| exclusive_file(&paths[1]).unwrap());
            let mut runtime = FaultRuntime::default();
            if mode == "process_busy" {
                runtime.stopped_error = Some("Codex is still running");
            }
            if mode == "process_probe_failed" {
                runtime.stopped_error = Some("Process inspection failed");
            }
            if mode == "backup" {
                fs::write(storage.join("backups"), b"block directory").unwrap();
            }
            assert!(
                apply_at(
                    preview.preview_token.as_deref().unwrap(),
                    &storage,
                    &runtime
                )
                .is_err(),
                "{mode}"
            );
            drop(held);
            for (path, expected) in paths.iter().zip(before) {
                assert_eq!(fs::read(path).unwrap(), expected, "{mode}");
            }
        }
    }

    #[test]
    fn partial_write_and_readback_failures_restore_every_file_and_keep_exact_backups() {
        for mode in ["partial_write", "readback", "rollback_failure"] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            for index in 1..3 {
                fs::write(
                    home.join(format!("sessions/rollout-{index}.jsonl")),
                    fixture("codex_model_router", &format!("完整历史 {index} 保留")),
                )
                .unwrap();
            }
            let paths = history_paths(&canonical_home(&home).unwrap()).unwrap();
            let before: Vec<_> = paths
                .iter()
                .map(|path| fs::read(home.join(path)).unwrap())
                .collect();
            let storage = root.path().join("storage");
            let preview = preview_at(&home, &storage).unwrap();
            let runtime = FaultRuntime {
                fail_write: (mode != "readback").then_some(1),
                fail_readback: (mode == "readback").then_some(1),
                fail_restore: if mode == "rollback_failure" {
                    vec![0, 1]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            let error = apply_at(
                preview.preview_token.as_deref().unwrap(),
                &storage,
                &runtime,
            )
            .unwrap_err();
            assert_eq!(*runtime.restored.lock().unwrap(), vec![0, 1, 2], "{mode}");
            if mode == "rollback_failure" {
                assert!(
                    error.contains("target 1")
                        && error.contains("target 2")
                        && error.contains("Exact backups remain")
                );
            } else {
                assert!(error.contains("every original was restored and verified"));
            }
            let backup = fs::read_dir(storage.join("backups"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            for (relative, original) in paths.iter().zip(before) {
                assert_eq!(fs::read(home.join(relative)).unwrap(), original);
                assert_eq!(
                    fs::read(backup.join("codex").join(relative)).unwrap(),
                    original
                );
            }
            assert!(!backup.join("repair-result.json").exists());
        }
    }

    #[test]
    fn external_sqlite_home_is_snapshotted_without_mutating_database_config_or_auth() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let sqlite_home = root.path().join("external-state");
        fs::create_dir_all(&sqlite_home).unwrap();
        let database = sqlite_home.join("state_5.sqlite");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('active external state');").unwrap();
        drop(connection);
        fs::write(
            home.join("state_5.sqlite"),
            b"inactive database must not be opened",
        )
        .unwrap();
        let config = format!(
            "\u{feff}model_provider = \"openai\"\nsqlite_home = '''{}'''\n",
            sqlite_home.display()
        );
        fs::write(home.join("config.toml"), config.as_bytes()).unwrap();
        fs::write(home.join("auth.json"), b"synthetic private auth sentinel").unwrap();
        let original_database = fs::read(&database).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        let result = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped,
        )
        .unwrap();
        let backup = PathBuf::from(result.backup_dir.unwrap());
        let snapshot = rusqlite::Connection::open(backup.join("sqlite/state_5.sqlite")).unwrap();
        assert_eq!(
            snapshot
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "active external state"
        );
        assert_eq!(fs::read(database).unwrap(), original_database);
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            config.as_bytes()
        );
        assert_eq!(
            fs::read(home.join("auth.json")).unwrap(),
            b"synthetic private auth sentinel"
        );
        assert!(!backup.join("codex/auth.json").exists() && !backup.join("auth.json").exists());
    }

    #[test]
    fn router_or_native_endpoint_overrides_refuse_apply_without_modifying_history() {
        for config in [
            "model_provider = 'codex_model_router'",
            "openai_base_url = 'http://127.0.0.1'",
            "model_catalog_json = '/tmp/catalog.json'",
            "[model_providers.openai]\nbase_url = 'http://127.0.0.1'",
        ] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            fs::write(home.join("config.toml"), config).unwrap();
            let path = home.join("sessions/rollout.jsonl");
            let before = fs::read(&path).unwrap();
            let storage = root.path().join("storage");
            let preview = preview_at(&home, &storage).unwrap();
            assert!(!preview.native_config);
            assert!(apply_at(
                preview.preview_token.as_deref().unwrap(),
                &storage,
                &Stopped
            )
            .unwrap_err()
            .contains("native official"));
            assert_eq!(fs::read(path).unwrap(), before);
            assert_eq!(
                fs::read_to_string(home.join("config.toml")).unwrap(),
                config
            );
        }
    }

    #[test]
    fn selected_profile_must_resolve_to_native_official_configuration() {
        for (profile, native) in [
            ("model_provider = 'codex_model_router'", false),
            ("openai_base_url = 'http://127.0.0.1'", false),
            ("model_catalog_json = '/tmp/catalog.json'", false),
            (
                "[profiles.selected.model_providers.openai]\nbase_url = 'http://127.0.0.1'",
                false,
            ),
            (
                "model_provider = 'openai'\nmodel = 'native-test-model'",
                true,
            ),
            ("", false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let home = home_fixture(root.path());
            let text = if profile.is_empty() {
                "model_provider = 'openai'\nprofile = 'missing'\n".into()
            } else {
                format!("model_provider = 'openai'\nprofile = 'selected'\n[profiles.selected]\n{profile}\n")
            };
            let text = format!(
                "sqlite_home = '''{}'''\n{text}",
                home.join("isolated-sqlite").display()
            );
            fs::write(home.join("config.toml"), &text).unwrap();
            let before = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
            let storage = root.path().join("storage");
            let preview = preview_at(&home, &storage).unwrap();
            assert_eq!(preview.native_config, native, "{profile}");
            let result = apply_at(
                preview.preview_token.as_deref().unwrap(),
                &storage,
                &Stopped,
            );
            if native {
                assert_eq!(result.unwrap().repaired_parts, 1);
            } else {
                assert!(result.unwrap_err().contains("native official"));
                assert_eq!(
                    fs::read(home.join("sessions/rollout.jsonl")).unwrap(),
                    before
                );
            }
            assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), text);
        }
    }

    #[test]
    fn failed_receipt_finalization_rolls_back_without_a_success_receipt() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let original = fs::read(home.join("sessions/rollout.jsonl")).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        let runtime = FaultRuntime {
            fail_receipt: true,
            ..Default::default()
        };
        let error = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &runtime,
        )
        .unwrap_err();
        assert!(
            error.contains("finalize recovery receipt")
                && error.contains("every original was restored")
        );
        assert_eq!(
            fs::read(home.join("sessions/rollout.jsonl")).unwrap(),
            original
        );
        let backup = fs::read_dir(storage.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(!backup.join("repair-result.json").is_file());
        assert!(!backup.join(".repair-result.pending.json").exists());
    }

    #[test]
    fn hardlinked_history_is_rejected_before_any_write() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let path = home.join("sessions/rollout.jsonl");
        let original = fs::read(&path).unwrap();
        let outside = root.path().join("external-hardlink.jsonl");
        fs::hard_link(&path, &outside).unwrap();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        assert!(apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped
        )
        .unwrap_err()
        .contains("hard link"));
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read(outside).unwrap(), original);
        assert!(!storage.join("backups").exists());
    }

    #[test]
    fn linked_history_directory_is_rejected_without_following_it() {
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let original = fixture("codex_model_router", "外部目录不可跟随或改动");
        fs::write(outside.join("external.jsonl"), &original).unwrap();
        let linked = home.join("sessions").join("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("cmd.exe")
                .args(["/C", "mklink", "/J"])
                .arg(&linked)
                .arg(&outside)
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "Cannot create synthetic junction: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert!(preview_at(&home, &root.path().join("storage"))
            .unwrap_err()
            .contains("Linked or reparse"));
        assert_eq!(fs::read(outside.join("external.jsonl")).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_artifacts_have_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let home = home_fixture(root.path());
        let original_mode = fs::metadata(home.join("sessions/rollout.jsonl"))
            .unwrap()
            .permissions()
            .mode();
        let storage = root.path().join("storage");
        let preview = preview_at(&home, &storage).unwrap();
        assert_eq!(
            fs::metadata(&storage).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(preview.plan_path.as_ref().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let applied = apply_at(
            preview.preview_token.as_deref().unwrap(),
            &storage,
            &Stopped,
        )
        .unwrap();
        let backup = PathBuf::from(applied.backup_dir.unwrap());
        assert_eq!(
            fs::metadata(backup.join("codex/sessions/rollout.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(home.join("sessions/rollout.jsonl"))
                .unwrap()
                .permissions()
                .mode(),
            original_mode
        );
    }
}

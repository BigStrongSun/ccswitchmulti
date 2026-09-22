//! Bounded Codex rollout discovery.
//!
//! This module deliberately owns no timer, database, parser, or notification.
//! The caller supplies the current UTC timestamp on each existing collection
//! tick and decides how to parse the returned candidate paths.

use crate::error::AppError;
use chrono::{DateTime, Datelike, Duration, Utc};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// The full walk is a repair/coverage pass, not the normal 60-second path.
pub const DEFAULT_FULL_RESCAN_SECS: i64 = 15 * 60;
const RECENT_ACTIVE_DAYS: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodexDiscoveryRootKind {
    Sessions,
    ArchivedSessions,
}

#[derive(Debug, Clone)]
pub struct CodexDiscoveryRoot {
    pub kind: CodexDiscoveryRootKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct CodexDiscoveryBatch {
    pub paths: Vec<PathBuf>,
    /// Whether this batch came from a full repair walk (loggable by the
    /// caller); the schedule itself is owned by `CodexDiscoveryState`.
    pub full_scan: bool,
}

#[derive(Debug, Clone)]
pub struct CodexDiscoveryState {
    roots: Vec<CodexDiscoveryRoot>,
    known_paths: HashMap<CodexDiscoveryRootKind, BTreeSet<PathBuf>>,
    full_rescan_secs: i64,
    next_full_scan_at: Option<i64>,
}

impl CodexDiscoveryState {
    pub fn new(roots: Vec<CodexDiscoveryRoot>) -> Self {
        Self::with_full_rescan_secs(roots, DEFAULT_FULL_RESCAN_SECS)
    }

    pub fn with_full_rescan_secs(roots: Vec<CodexDiscoveryRoot>, full_rescan_secs: i64) -> Self {
        Self {
            roots,
            known_paths: HashMap::new(),
            full_rescan_secs: full_rescan_secs.max(1),
            next_full_scan_at: None,
        }
    }

    /// Returns a deterministic, root-contained candidate set. The first pass
    /// and periodic repair passes walk all roots; normal passes inspect known
    /// files and only the current/previous active date directories.
    pub fn discover_at(&mut self, now: i64) -> Result<CodexDiscoveryBatch, AppError> {
        let full_scan = self
            .next_full_scan_at
            .is_none_or(|deadline| now >= deadline);
        let mut candidates = BTreeSet::new();

        for root in &self.roots {
            let root_path = canonical_existing_directory(&root.path)?;
            let found = if full_scan {
                collect_jsonl_recursive(&root_path)?
            } else {
                self.collect_bounded_candidates(root.kind, &root_path, now)?
            };
            for path in found {
                candidates.insert(path);
            }
        }

        if full_scan {
            self.known_paths.clear();
            for root in &self.roots {
                let root_path = canonical_existing_directory(&root.path)?;
                let paths = candidates
                    .iter()
                    .filter(|path| is_path_within_root(path, &root_path))
                    .cloned()
                    .collect();
                self.known_paths.insert(root.kind, paths);
            }
            self.next_full_scan_at = Some(now.saturating_add(self.full_rescan_secs));
        } else {
            for root in &self.roots {
                let root_path = canonical_existing_directory(&root.path)?;
                let known = self.known_paths.entry(root.kind).or_default();
                *known = known
                    .iter()
                    .filter_map(|path| fs::canonicalize(path).ok())
                    .filter(|path| path.is_file() && is_path_within_root(path, &root_path))
                    .collect();
                known.extend(
                    candidates
                        .iter()
                        .filter(|path| is_path_within_root(path, &root_path))
                        .cloned(),
                );
            }
        }

        Ok(CodexDiscoveryBatch {
            paths: candidates.into_iter().collect(),
            full_scan,
        })
    }

    fn collect_bounded_candidates(
        &self,
        kind: CodexDiscoveryRootKind,
        root: &Path,
        now: i64,
    ) -> Result<BTreeSet<PathBuf>, AppError> {
        let mut paths: BTreeSet<PathBuf> = self
            .known_paths
            .get(&kind)
            .into_iter()
            .flatten()
            .filter_map(|path| fs::canonicalize(path).ok())
            .filter(|path| path.is_file() && is_path_within_root(path, root))
            .collect();

        // Active rollouts are date partitioned; a shallow recent-date walk
        // catches new files without revisiting historic directories each tick.
        if kind == CodexDiscoveryRootKind::Sessions {
            for offset in 0..=RECENT_ACTIVE_DAYS {
                let day = DateTime::<Utc>::from_timestamp(now, 0)
                    .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
                    - Duration::days(offset);
                let recent = root
                    .join(format!("{:04}", day.year()))
                    .join(format!("{:02}", day.month()))
                    .join(format!("{:02}", day.day()));
                if let Ok(canonical_recent) = fs::canonicalize(&recent) {
                    if canonical_recent.is_dir() && is_path_within_root(&canonical_recent, root) {
                        paths.extend(collect_jsonl_shallow(&canonical_recent, root)?);
                    }
                }
            }
        }
        Ok(paths)
    }
}

fn canonical_existing_directory(path: &Path) -> Result<PathBuf, AppError> {
    if !path.is_dir() {
        return Ok(path.to_path_buf());
    }
    fs::canonicalize(path)
        .map_err(|error| AppError::Config(format!("无法规范化 Codex 会话根目录: {error}")))
}

fn is_jsonl(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("jsonl")
}

fn is_path_within_root(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok()
}

fn collect_jsonl_shallow(dir: &Path, root: &Path) -> Result<BTreeSet<PathBuf>, AppError> {
    let mut paths = BTreeSet::new();
    let entries = fs::read_dir(dir)
        .map_err(|error| AppError::Config(format!("读取 Codex 会话目录失败: {error}")))?;
    for entry in entries {
        let path = entry
            .map_err(|error| AppError::Config(format!("读取 Codex 会话目录项失败: {error}")))?
            .path();
        if path.is_file() && is_jsonl(&path) {
            if let Ok(canonical) = fs::canonicalize(&path) {
                if is_path_within_root(&canonical, root) {
                    paths.insert(canonical);
                }
            }
        }
    }
    Ok(paths)
}

fn collect_jsonl_recursive(root: &Path) -> Result<BTreeSet<PathBuf>, AppError> {
    let mut paths = BTreeSet::new();
    if !root.is_dir() {
        return Ok(paths);
    }
    let root = fs::canonicalize(root)
        .map_err(|error| AppError::Config(format!("无法规范化 Codex 会话根目录: {error}")))?;
    let mut pending = vec![root.clone()];
    let mut visited_dirs = BTreeSet::new();
    while let Some(dir) = pending.pop() {
        let canonical_dir = match fs::canonicalize(&dir) {
            Ok(path) if path.is_dir() && is_path_within_root(&path, &root) => path,
            _ => continue,
        };
        if !visited_dirs.insert(canonical_dir.clone()) {
            continue;
        }
        let entries = fs::read_dir(&canonical_dir)
            .map_err(|error| AppError::Config(format!("读取 Codex 会话目录失败: {error}")))?;
        for entry in entries {
            let path = entry
                .map_err(|error| AppError::Config(format!("读取 Codex 会话目录项失败: {error}")))?
                .path();
            if path.is_dir() {
                if let Ok(canonical) = fs::canonicalize(&path) {
                    if canonical.is_dir() && is_path_within_root(&canonical, &root) {
                        pending.push(canonical);
                    }
                }
            } else if path.is_file() && is_jsonl(&path) {
                if let Ok(canonical) = fs::canonicalize(&path) {
                    if is_path_within_root(&canonical, &root) {
                        paths.insert(canonical);
                    }
                }
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccsm-discovery-{name}-{nonce}"));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn write_fixture(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("fixture directory");
        fs::write(path, b"{}\n").expect("fixture file");
    }

    #[test]
    fn first_pass_finds_old_active_and_archived_files_then_tick_stays_bounded() {
        let root = fixture_root("bootstrap");
        let active = root.join("sessions");
        let archived = root.join("archived_sessions");
        let old = active.join("2020/01/01/old.jsonl");
        let archive = archived.join("archive.jsonl");
        write_fixture(&old);
        write_fixture(&archive);
        let mut state = CodexDiscoveryState::with_full_rescan_secs(
            vec![
                CodexDiscoveryRoot {
                    kind: CodexDiscoveryRootKind::Sessions,
                    path: active.clone(),
                },
                CodexDiscoveryRoot {
                    kind: CodexDiscoveryRootKind::ArchivedSessions,
                    path: archived.clone(),
                },
            ],
            900,
        );

        let bootstrap = state.discover_at(1_700_000_000).expect("bootstrap");
        let tick = state.discover_at(1_700_000_060).expect("tick");

        assert!(bootstrap.full_scan);
        assert!(bootstrap
            .paths
            .iter()
            .any(|path| path.ends_with("old.jsonl")));
        assert!(bootstrap
            .paths
            .iter()
            .any(|path| path.ends_with("archive.jsonl")));
        assert!(!tick.full_scan);
        assert!(tick.paths.iter().any(|path| path.ends_with("old.jsonl")));
        assert!(tick
            .paths
            .iter()
            .any(|path| path.ends_with("archive.jsonl")));
        fs::remove_dir_all(root).expect("cleanup fixture");
    }

    #[test]
    fn full_rescan_eventually_finds_a_new_file_in_an_old_directory() {
        let root = fixture_root("old-dir");
        let active = root.join("sessions");
        let old = active.join("2020/01/01/late.jsonl");
        let mut state = CodexDiscoveryState::with_full_rescan_secs(
            vec![CodexDiscoveryRoot {
                kind: CodexDiscoveryRootKind::Sessions,
                path: active.clone(),
            }],
            120,
        );

        state.discover_at(1_700_000_000).expect("bootstrap");
        write_fixture(&old);
        let bounded = state.discover_at(1_700_000_060).expect("bounded tick");
        let repaired = state.discover_at(1_700_000_120).expect("repair pass");

        assert!(!bounded
            .paths
            .iter()
            .any(|path| path.ends_with("late.jsonl")));
        assert!(repaired.full_scan);
        assert!(repaired
            .paths
            .iter()
            .any(|path| path.ends_with("late.jsonl")));
        fs::remove_dir_all(root).expect("cleanup fixture");
    }

    #[test]
    fn recent_active_directory_is_discovered_without_full_walk() {
        let root = fixture_root("recent");
        let active = root.join("sessions");
        let now = 1_700_000_000;
        let day = DateTime::<Utc>::from_timestamp(now, 0).expect("time");
        let late = active
            .join(format!("{:04}", day.year()))
            .join(format!("{:02}", day.month()))
            .join(format!("{:02}", day.day()))
            .join("late.jsonl");
        let mut state = CodexDiscoveryState::with_full_rescan_secs(
            vec![CodexDiscoveryRoot {
                kind: CodexDiscoveryRootKind::Sessions,
                path: active.clone(),
            }],
            900,
        );

        state.discover_at(now).expect("bootstrap");
        write_fixture(&late);
        let tick = state.discover_at(now + 60).expect("bounded tick");

        assert!(!tick.full_scan);
        assert!(tick.paths.iter().any(|path| path.ends_with("late.jsonl")));
        fs::remove_dir_all(root).expect("cleanup fixture");
    }

    #[test]
    fn canonical_external_path_is_rejected_by_root_isolation() {
        let root = fixture_root("root-isolation");
        let external = fixture_root("external");
        let root_canonical = fs::canonicalize(&root).expect("canonical root");
        let external_file = external.join("outside.jsonl");
        write_fixture(&external_file);
        let external_canonical = fs::canonicalize(&external_file).expect("canonical external");

        assert!(!is_path_within_root(&external_canonical, &root_canonical));
        fs::remove_dir_all(root).expect("cleanup root fixture");
        fs::remove_dir_all(external).expect("cleanup external fixture");
    }
}

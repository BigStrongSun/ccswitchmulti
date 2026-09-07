//! Pi-native instruction files and slash-command templates.

use crate::config::atomic_write;
use crate::error::AppError;
use crate::pi_config::get_pi_agent_dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_PROMPT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TEMPLATE_SLUG_BYTES: usize = 128;
const MISSING_REVISION: &str = "missing";
static PROMPT_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PiAgentsFileSnapshot {
    pub content: Option<String>,
    pub revision: String,
}

/// Holds the process lock across AGENTS.md read/compare/write and a caller's
/// associated database transaction.
pub(crate) struct PiAgentsFileGuard {
    _guard: MutexGuard<'static, ()>,
    path: PathBuf,
}

impl PiAgentsFileGuard {
    pub(crate) fn acquire() -> Result<Self, AppError> {
        Ok(Self {
            _guard: lock_prompt_files()?,
            path: get_pi_agent_dir()?.join("AGENTS.md"),
        })
    }

    pub(crate) fn read(&self) -> Result<PiAgentsFileSnapshot, AppError> {
        let (content, revision) = match fs::File::open(&self.path) {
            Ok(file) => {
                let bytes = read_open_file_limited(file, &self.path, "Pi AGENTS.md")?;
                let revision = revision(&bytes);
                let content = String::from_utf8(bytes).map_err(|error| {
                    AppError::InvalidInput(format!(
                        "Pi AGENTS.md must be UTF-8 ({}): {error}",
                        self.path.display()
                    ))
                })?;
                (Some(content), revision)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (None, MISSING_REVISION.to_string())
            }
            Err(error) => return Err(AppError::io(&self.path, error)),
        };
        Ok(PiAgentsFileSnapshot { content, revision })
    }

    pub(crate) fn replace(&self, expected_revision: &str, content: &str) -> Result<(), AppError> {
        validate_content_size(content, "Pi AGENTS.md")?;
        ensure_revision(&self.path, expected_revision, "Pi AGENTS.md")?;
        atomic_write(&self.path, content.as_bytes())
    }

    pub(crate) fn delete(&self, expected_revision: &str) -> Result<(), AppError> {
        ensure_revision(&self.path, expected_revision, "Pi AGENTS.md")?;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(&self.path, error)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiPromptFileKind {
    SystemOverride,
    SystemAppend,
}

impl PiPromptFileKind {
    fn filename(self) -> &'static str {
        match self {
            Self::SystemOverride => "SYSTEM.md",
            Self::SystemAppend => "APPEND_SYSTEM.md",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiPromptFileSnapshot {
    pub exists: bool,
    pub revision: String,
    pub content: String,
}

pub struct PiPromptFileService;

impl PiPromptFileService {
    pub fn read(kind: PiPromptFileKind) -> Result<PiPromptFileSnapshot, AppError> {
        let _guard = lock_prompt_files()?;
        read_prompt_file(&get_pi_agent_dir()?, kind)
    }

    pub fn replace(
        kind: PiPromptFileKind,
        expected_revision: &str,
        content: &str,
    ) -> Result<PiPromptFileSnapshot, AppError> {
        validate_instruction_content(content)?;
        let _guard = lock_prompt_files()?;
        let root = get_pi_agent_dir()?;
        let path = root.join(kind.filename());
        ensure_revision(&path, expected_revision, "Pi prompt file")?;
        atomic_write(&path, content.as_bytes())?;
        read_prompt_file(&root, kind)
    }

    pub fn delete(kind: PiPromptFileKind, expected_revision: &str) -> Result<bool, AppError> {
        let _guard = lock_prompt_files()?;
        let path = get_pi_agent_dir()?.join(kind.filename());
        ensure_revision(&path, expected_revision, "Pi prompt file")?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::io(&path, error)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiPromptTemplate {
    pub slug: String,
    pub content: String,
    pub revision: String,
}

pub struct PiPromptTemplateService;

impl PiPromptTemplateService {
    pub fn list() -> Result<Vec<PiPromptTemplate>, AppError> {
        let _guard = lock_prompt_files()?;
        let dir = get_pi_agent_dir()?.join("prompts");
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(AppError::io(&dir, error)),
        };
        let mut templates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| AppError::io(&dir, error))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let Some(slug) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_template_slug(slug).is_err() {
                continue;
            }
            let bytes = read_limited(&path, "Pi prompt template")?;
            let revision = revision(&bytes);
            let content = String::from_utf8(bytes).map_err(|error| {
                AppError::InvalidInput(format!(
                    "Pi prompt template must be UTF-8 ({}): {error}",
                    path.display()
                ))
            })?;
            templates.push(PiPromptTemplate {
                slug: slug.to_string(),
                content,
                revision,
            });
        }
        templates.sort_by(|left, right| left.slug.cmp(&right.slug));
        Ok(templates)
    }

    pub fn upsert(
        slug: &str,
        original_slug: Option<&str>,
        expected_revision: &str,
        content: &str,
    ) -> Result<PiPromptTemplate, AppError> {
        validate_template_slug(slug)?;
        if let Some(original_slug) = original_slug {
            validate_template_slug(original_slug)?;
        }
        validate_content_size(content, "Pi prompt template")?;
        let _guard = lock_prompt_files()?;
        let dir = get_pi_agent_dir()?.join("prompts");
        let path = template_path(&dir, slug);

        if let Some(original_slug) = original_slug.filter(|value| *value != slug) {
            let original_path = template_path(&dir, original_slug);
            ensure_revision(&original_path, expected_revision, "Pi prompt template")?;
            ensure_revision(&path, MISSING_REVISION, "Pi prompt template")?;
            fs::rename(&original_path, &path)
                .map_err(|error| AppError::io(&original_path, error))?;
            if let Err(write_error) = atomic_write(&path, content.as_bytes()) {
                if let Err(rollback_error) = fs::rename(&path, &original_path) {
                    return Err(AppError::Message(format!(
                        "Pi prompt template save failed ({write_error}); rename rollback also failed: {rollback_error}"
                    )));
                }
                return Err(write_error);
            }
        } else {
            ensure_revision(&path, expected_revision, "Pi prompt template")?;
            atomic_write(&path, content.as_bytes())?;
        }

        Ok(PiPromptTemplate {
            slug: slug.to_string(),
            content: content.to_string(),
            revision: revision(content.as_bytes()),
        })
    }

    pub fn delete(slug: &str, expected_revision: &str) -> Result<bool, AppError> {
        validate_template_slug(slug)?;
        let _guard = lock_prompt_files()?;
        let path = template_path(&get_pi_agent_dir()?.join("prompts"), slug);
        ensure_revision(&path, expected_revision, "Pi prompt template")?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::io(&path, error)),
        }
    }
}

fn lock_prompt_files() -> Result<MutexGuard<'static, ()>, AppError> {
    PROMPT_FILE_LOCK
        .lock()
        .map_err(|error| AppError::Config(format!("Pi prompt file lock is poisoned: {error}")))
}

fn read_prompt_file(root: &Path, kind: PiPromptFileKind) -> Result<PiPromptFileSnapshot, AppError> {
    let path = root.join(kind.filename());
    let (exists, content, revision) = match fs::File::open(&path) {
        Ok(file) => {
            let bytes = read_open_file_limited(file, &path, "Pi prompt file")?;
            let revision = revision(&bytes);
            let content = String::from_utf8(bytes).map_err(|error| {
                AppError::InvalidInput(format!(
                    "Pi prompt file must be UTF-8 ({}): {error}",
                    path.display()
                ))
            })?;
            (true, content, revision)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (false, String::new(), MISSING_REVISION.to_string())
        }
        Err(error) => return Err(AppError::io(&path, error)),
    };
    Ok(PiPromptFileSnapshot {
        exists,
        revision,
        content,
    })
}

fn ensure_revision(path: &Path, expected: &str, label: &str) -> Result<(), AppError> {
    let actual = match fs::File::open(path) {
        Ok(file) => revision(&read_open_file_limited(file, path, label)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => MISSING_REVISION.to_string(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual == expected {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "{label} changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn read_limited(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    read_open_file_limited(file, path, label)
}

fn read_open_file_limited(file: fs::File, path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_PROMPT_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PROMPT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_PROMPT_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn validate_instruction_content(content: &str) -> Result<(), AppError> {
    if content.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Pi instruction cannot be blank; remove the file to deactivate it".to_string(),
        ));
    }
    validate_content_size(content, "Pi instruction")
}

fn validate_content_size(content: &str, label: &str) -> Result<(), AppError> {
    if content.len() as u64 > MAX_PROMPT_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} exceeds the 1 MiB limit"
        )));
    }
    Ok(())
}

fn revision(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn template_path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.md"))
}

fn validate_template_slug(slug: &str) -> Result<(), AppError> {
    let basename = slug.split('.').next().unwrap_or_default();
    let windows_reserved = matches!(
        basename.to_ascii_lowercase().as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    );
    let valid = !slug.is_empty()
        && slug.len() <= MAX_TEMPLATE_SLUG_BYTES
        && !slug.starts_with('.')
        && !slug.ends_with('.')
        && !windows_reserved
        && slug.chars().all(|character| {
            !character.is_control()
                && !character.is_whitespace()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        });
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidInput(
            "Pi prompt-template name must be one portable slash-command token".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_pi_dir<T>(test: impl FnOnce(&Path) -> T) -> T {
        assert!(crate::settings::get_pi_override_dir().is_none());
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let previous = std::env::var_os("PI_CODING_AGENT_DIR");
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", dir.path()) };
        let result = test(dir.path());
        match previous {
            Some(value) => unsafe { std::env::set_var("PI_CODING_AGENT_DIR", value) },
            None => unsafe { std::env::remove_var("PI_CODING_AGENT_DIR") },
        }
        result
    }

    #[test]
    #[serial]
    fn fixed_instruction_files_require_nonblank_versioned_content() {
        with_pi_dir(|_| {
            let missing = PiPromptFileService::read(PiPromptFileKind::SystemAppend)
                .expect("read missing file");
            assert_eq!(missing.revision, "missing");
            assert!(PiPromptFileService::replace(
                PiPromptFileKind::SystemAppend,
                &missing.revision,
                "  "
            )
            .is_err());

            let saved = PiPromptFileService::replace(
                PiPromptFileKind::SystemAppend,
                &missing.revision,
                "append instructions",
            )
            .expect("save instruction file");
            assert!(saved.revision.starts_with("sha256:"));
            assert!(matches!(
                PiPromptFileService::delete(PiPromptFileKind::SystemAppend, "missing"),
                Err(AppError::Conflict(_))
            ));
            assert!(
                PiPromptFileService::delete(PiPromptFileKind::SystemAppend, &saved.revision)
                    .expect("delete instruction file")
            );
        });
    }

    #[test]
    #[serial]
    fn empty_template_round_trips_through_a_versioned_rename() {
        with_pi_dir(|dir| {
            let created = PiPromptTemplateService::upsert("draft", None, "missing", "")
                .expect("create empty template");
            assert!(created.revision.starts_with("sha256:"));
            let renamed = PiPromptTemplateService::upsert(
                "review.v2",
                Some("draft"),
                &created.revision,
                "updated",
            )
            .expect("rename template");
            assert_eq!(
                PiPromptTemplateService::list().expect("list templates"),
                vec![renamed]
            );
            assert!(!dir.join("prompts").join("draft.md").exists());
            for invalid in ["CON", "lpt1.txt", "with space", "a/b", ".hidden"] {
                assert!(
                    PiPromptTemplateService::upsert(invalid, None, "missing", "body").is_err(),
                    "{invalid}"
                );
            }
        });
    }

    #[test]
    #[serial]
    fn oversized_native_prompt_is_rejected_before_loading() {
        with_pi_dir(|dir| {
            let path = dir.join("prompts").join("oversized.md");
            fs::create_dir_all(path.parent().expect("prompt directory"))
                .expect("create prompt directory");
            let file = fs::File::create(&path).expect("create prompt file");
            file.set_len(MAX_PROMPT_FILE_BYTES + 1)
                .expect("make sparse oversized prompt");
            let error = PiPromptTemplateService::list().expect_err("reject oversized prompt");
            assert!(error.to_string().contains("1 MiB limit"));
        });
    }
}

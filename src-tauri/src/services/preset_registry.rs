//! 可更新 Provider 预设注册表 —— 传输 + 配置 + manifest 校验地基。
//!
//! 本模块是 WebDAV 同步与预设注册表联调的可验证地基（见
//! `docs/superpowers/plans/2026-08-23-webdav-preset-registry-integration.md`）：
//! - 预设源（[`PresetSource`]）可跑在用户已有的 WebDAV/S3 上，复用 [`super::webdav`] 原语。
//! - manifest 必须通过 size / SHA-256 / 过期 / 版本不回退 / Ed25519 签名校验才接受。
//! - 安全红线：没有受信源 + 签名验证前，不得做裸 URL 下载更新（`pinned-key` 源必须带签名）。
//!
//! 本模块只做“获取 + 校验”，不落地应用预设、不写 DB；应用与三方合并是 P1 后续工作。

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::AppError;

/// 支持的 manifest schema 版本。
pub const SUPPORTED_PRESET_SCHEMA_VERSION: u32 = 1;
/// 预设 manifest 下载上限（1MB，与同步 manifest 上限一致）。
pub const MAX_PRESET_MANIFEST_BYTES: usize = 1024 * 1024;

/// 预设源传输类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetSourceKind {
    /// 复用 [`super::webdav`] 原语的 WebDAV/S3 兼容存储。
    WebDav,
    /// 直接 HTTPS 拉取（仍需签名校验）。
    Https,
}

/// 信任分层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresetTrust {
    /// 固定公钥 + Ed25519 签名，全部校验通过才接受。
    PinnedKey,
    /// 本地导入 / 用户显式信任，跳过签名但保留 hash/过期/版本校验。
    Local,
}

/// 单个预设源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetSource {
    pub id: String,
    pub kind: PresetSourceKind,
    pub trust: PresetTrust,
    #[serde(default)]
    pub enabled: bool,
    /// WebDAV/HTTPS 基础地址。
    pub base_url: String,
    /// WebDAV 用户名（可空）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    /// WebDAV 密码（可空；明文不落盘由上层负责）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    /// 远程根目录（WebDAV）。
    #[serde(default)]
    pub remote_root: String,
    /// 设备 profile（WebDAV）。
    #[serde(default)]
    pub profile: String,
    /// 固定公钥（base64，Ed25519）。`pinned-key` 源必填。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub public_key: String,
    /// 最近一次检查时间（unix 秒）。
    #[serde(default)]
    pub last_checked_at: i64,
    /// 最近一次接受的版本（用于版本不回退校验）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_accepted_version: String,
}

impl PresetSource {
    /// WebDAV 预设 manifest 的远程路径段：`{remote_root}/presets/{profile}/manifest.json`。
    pub fn manifest_segments(&self) -> Vec<String> {
        let mut segs: Vec<String> = super::webdav::path_segments(&self.remote_root)
            .map(|s| s.to_string())
            .collect();
        segs.push("presets".to_string());
        let profile = self.profile.trim();
        if !profile.is_empty() {
            segs.push(profile.to_string());
        }
        segs.push("manifest.json".to_string());
        segs
    }

    /// Resolve a signed manifest target under this source's preset root.
    /// Targets are always relative paths; an absolute URL or traversal would
    /// let an untrusted manifest escape the configured WebDAV namespace.
    pub fn target_segments(&self, target: &str) -> Result<Vec<String>, AppError> {
        let target = target.trim();
        if target.is_empty()
            || target.starts_with('/')
            || target.contains("://")
            || target.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(AppError::localized(
                "preset.target.invalid",
                "预设 manifest 的 target 必须是预设目录内的相对路径",
                "Preset manifest target must be a relative path inside the preset directory.",
            ));
        }

        let mut segs: Vec<String> = super::webdav::path_segments(&self.remote_root)
            .map(|s| s.to_string())
            .collect();
        segs.push("presets".to_string());
        if !self.profile.trim().is_empty() {
            segs.push(self.profile.trim().to_string());
        }
        segs.extend(
            target
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_string),
        );
        Ok(segs)
    }
}

fn default_schema_version() -> u32 {
    SUPPORTED_PRESET_SCHEMA_VERSION
}

/// 设备级预设注册表配置（存 `settings.json`，不跨设备同步）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetRegistrySettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub sources: Vec<PresetSource>,
}

impl Default for PresetRegistrySettings {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            sources: Vec::new(),
        }
    }
}

impl PresetRegistrySettings {
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// 归一化：去掉空白、丢弃空源，避免空配置落盘。
    pub fn normalize(&mut self) {
        for s in &mut self.sources {
            s.base_url = s.base_url.trim().to_string();
            s.username = s.username.trim().to_string();
            s.remote_root = s.remote_root.trim().to_string();
            s.profile = s.profile.trim().to_string();
            s.public_key = s.public_key.trim().to_string();
        }
        self.sources
            .retain(|s| !s.id.trim().is_empty() && !s.base_url.is_empty());
    }
}

/// 签名 manifest（发布端离线签名，客户端固定公钥验证）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetManifest {
    pub schema_version: u32,
    pub version: String,
    /// 发布时间（unix 秒）。
    pub published_at: i64,
    /// 过期时间（unix 秒）。
    pub expires_at: i64,
    /// 目标预设包 URL（相对或绝对）。
    pub target: String,
    /// 目标包 SHA-256（hex，小写）。
    pub sha256: String,
    /// 目标包字节数。
    pub size: u64,
    /// Ed25519 签名（base64），对 [`PresetManifest::signed_payload`] 签名。
    pub signature: String,
    /// 变更摘要。
    #[serde(default)]
    pub changelog: String,
}

impl PresetManifest {
    /// 参与签名的规范化载荷（不含 `signature` 字段）。
    ///
    /// 采用固定字段顺序的换行拼接，保证发布端与客户端对同一 manifest 得到同一字节串。
    pub fn signed_payload(&self) -> String {
        format!(
            "ccsm-preset/v{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.schema_version,
            self.version,
            self.published_at,
            self.expires_at,
            self.target,
            self.sha256,
            self.size,
            self.changelog
        )
    }
}

/// 校验拒绝原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestRejectReason {
    UnsupportedSchema { version: u32 },
    Expired { expires_at: i64, now: i64 },
    VersionRollback { current: String, incoming: String },
    SizeMismatch { expected: u64, actual: u64 },
    HashMismatch { expected: String, actual: String },
    MissingSignature,
    BadPublicKey,
    BadSignature,
}

impl std::fmt::Display for ManifestRejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { version } => write!(f, "不支持的 schema 版本 {version}"),
            Self::Expired { expires_at, now } => {
                write!(f, "已过期 (expires_at={expires_at}, now={now})")
            }
            Self::VersionRollback { current, incoming } => {
                write!(f, "版本回退 (current={current}, incoming={incoming})")
            }
            Self::SizeMismatch { expected, actual } => {
                write!(f, "大小不匹配 (expected={expected}, actual={actual})")
            }
            Self::HashMismatch { expected, actual } => {
                write!(f, "SHA-256 不匹配 (expected={expected}, actual={actual})")
            }
            Self::MissingSignature => write!(f, "pinned-key 源缺少签名"),
            Self::BadPublicKey => write!(f, "公钥缺失或非法"),
            Self::BadSignature => write!(f, "签名无效"),
        }
    }
}

/// 校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestVerdict {
    Accepted,
    Rejected(ManifestRejectReason),
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 解析版本为数值分量序列（如 `2026.08.23` -> `[2026, 8, 23]`）。
fn parse_version(v: &str) -> Option<Vec<u64>> {
    let mut out = Vec::new();
    for part in v.trim().split('.') {
        out.push(part.parse::<u64>().ok()?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `a <= b` 的版本比较；无法解析为纯数值分量时退化为字符串比较。
fn version_le(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Some(x), Some(y)) => x <= y,
        _ => a.cmp(b) != std::cmp::Ordering::Greater,
    }
}

/// `incoming` 相对 `current` 是否为回退（严格小于）。
fn is_rollback(current: &str, incoming: &str) -> bool {
    match (parse_version(current), parse_version(incoming)) {
        (Some(x), Some(y)) => y < x,
        _ => incoming.cmp(current) == std::cmp::Ordering::Less,
    }
}

/// `incoming` 是否严格新于 `current`（用于“是否有更新”判断）。
pub fn is_newer(current: &str, incoming: &str) -> bool {
    match (parse_version(current), parse_version(incoming)) {
        (Some(x), Some(y)) => y > x,
        _ => incoming.cmp(current) == std::cmp::Ordering::Greater,
    }
}

fn decode_public_key(b64: &str) -> Result<VerifyingKey, ()> {
    let bytes = BASE64.decode(b64.as_bytes()).map_err(|_| ())?;
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| ())?;
    VerifyingKey::from_bytes(&arr).map_err(|_| ())
}

/// 校验 manifest。纯函数，便于测试。
///
/// - `target_bytes` 为 `Some` 时校验 size + SHA-256；为 `None` 时跳过（仅校验元数据 + 签名）。
/// - `pinned-key` 源必须带有效签名；`local` 源跳过签名但保留其余校验。
pub fn validate_manifest(
    manifest: &PresetManifest,
    trust: PresetTrust,
    public_key_b64: &str,
    last_accepted_version: &str,
    now_unix: i64,
    target_bytes: Option<&[u8]>,
) -> ManifestVerdict {
    if manifest.schema_version != SUPPORTED_PRESET_SCHEMA_VERSION {
        return ManifestVerdict::Rejected(ManifestRejectReason::UnsupportedSchema {
            version: manifest.schema_version,
        });
    }
    if manifest.expires_at <= now_unix {
        return ManifestVerdict::Rejected(ManifestRejectReason::Expired {
            expires_at: manifest.expires_at,
            now: now_unix,
        });
    }
    if !last_accepted_version.is_empty() && is_rollback(last_accepted_version, &manifest.version) {
        return ManifestVerdict::Rejected(ManifestRejectReason::VersionRollback {
            current: last_accepted_version.to_string(),
            incoming: manifest.version.clone(),
        });
    }
    if let Some(bytes) = target_bytes {
        let actual_len = bytes.len() as u64;
        if actual_len != manifest.size {
            return ManifestVerdict::Rejected(ManifestRejectReason::SizeMismatch {
                expected: manifest.size,
                actual: actual_len,
            });
        }
        let actual = sha256_hex(bytes);
        if !actual.eq_ignore_ascii_case(&manifest.sha256) {
            return ManifestVerdict::Rejected(ManifestRejectReason::HashMismatch {
                expected: manifest.sha256.clone(),
                actual,
            });
        }
    }
    match trust {
        PresetTrust::Local => ManifestVerdict::Accepted,
        PresetTrust::PinnedKey => {
            if manifest.signature.trim().is_empty() {
                return ManifestVerdict::Rejected(ManifestRejectReason::MissingSignature);
            }
            let Ok(key) = decode_public_key(public_key_b64) else {
                return ManifestVerdict::Rejected(ManifestRejectReason::BadPublicKey);
            };
            let sig_bytes = match BASE64.decode(manifest.signature.as_bytes()) {
                Ok(b) => b,
                Err(_) => return ManifestVerdict::Rejected(ManifestRejectReason::BadSignature),
            };
            let Ok(sig) = Signature::from_slice(&sig_bytes) else {
                return ManifestVerdict::Rejected(ManifestRejectReason::BadSignature);
            };
            match key.verify(manifest.signed_payload().as_bytes(), &sig) {
                Ok(()) => ManifestVerdict::Accepted,
                Err(_) => ManifestVerdict::Rejected(ManifestRejectReason::BadSignature),
            }
        }
    }
}

/// 从 WebDAV 源拉取并校验 manifest。
///
/// 复用 [`super::webdav`] 原语；下载后走 [`validate_manifest`]。本函数不落地应用预设。
pub async fn fetch_preset_manifest_from_webdav(
    source: &PresetSource,
    now_unix: i64,
) -> Result<PresetManifest, AppError> {
    let auth = super::webdav::auth_from_credentials(&source.username, &source.password);
    let segments = source.manifest_segments();
    let url = super::webdav::build_remote_url(&source.base_url, &segments)?;
    let Some((bytes, _etag)) =
        super::webdav::get_bytes(&url, &auth, MAX_PRESET_MANIFEST_BYTES).await?
    else {
        return Err(AppError::localized(
            "preset.manifest.missing",
            "预设 manifest 不存在",
            "Preset manifest not found",
        ));
    };
    let manifest: PresetManifest = serde_json::from_slice(&bytes).map_err(|e| AppError::Json {
        path: "manifest.json".to_string(),
        source: e,
    })?;
    match validate_manifest(
        &manifest,
        source.trust,
        &source.public_key,
        &source.last_accepted_version,
        now_unix,
        None,
    ) {
        ManifestVerdict::Accepted => Ok(manifest),
        ManifestVerdict::Rejected(reason) => Err(AppError::localized(
            "preset.manifest.rejected",
            format!("预设 manifest 被拒绝: {reason}"),
            format!("Preset manifest rejected: {reason}"),
        )),
    }
}

/// Download the catalog payload announced by a previously accepted manifest.
/// It is revalidated with the exact bytes, so a manifest may not be replayed
/// for a different bundle after its metadata was accepted.
pub async fn download_preset_bundle_from_webdav(
    source: &PresetSource,
    manifest: &PresetManifest,
    now_unix: i64,
) -> Result<Vec<u8>, AppError> {
    let auth = super::webdav::auth_from_credentials(&source.username, &source.password);
    let segments = source.target_segments(&manifest.target)?;
    let url = super::webdav::build_remote_url(&source.base_url, &segments)?;
    let max_bytes = usize::try_from(crate::services::preset_catalog::MAX_PRESET_TABLE_BYTES)
        .unwrap_or(usize::MAX);
    let Some((bytes, _etag)) = super::webdav::get_bytes(&url, &auth, max_bytes).await? else {
        return Err(AppError::localized(
            "preset.bundle.missing",
            "预设 manifest 指向的包不存在",
            "Preset bundle referenced by the manifest was not found.",
        ));
    };
    match validate_manifest(
        manifest,
        source.trust,
        &source.public_key,
        &source.last_accepted_version,
        now_unix,
        Some(&bytes),
    ) {
        ManifestVerdict::Accepted => Ok(bytes),
        ManifestVerdict::Rejected(reason) => Err(AppError::localized(
            "preset.bundle.rejected",
            format!("预设包被拒绝: {reason}"),
            format!("Preset bundle was rejected: {reason}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// 固定测试密钥（确定性，避免依赖随机数）。
    fn test_keypair() -> (SigningKey, String) {
        let seed = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        let pub_b64 = BASE64.encode(signing_key.verifying_key().to_bytes());
        (signing_key, pub_b64)
    }

    fn sign(manifest: &PresetManifest, signing_key: &SigningKey) -> String {
        let sig = signing_key.sign(manifest.signed_payload().as_bytes());
        BASE64.encode(sig.to_bytes())
    }

    fn base_manifest() -> PresetManifest {
        PresetManifest {
            schema_version: SUPPORTED_PRESET_SCHEMA_VERSION,
            version: "2026.08.23".to_string(),
            published_at: 1_700_000_000,
            expires_at: 2_000_000_000,
            target: "preset-2026.08.23.json".to_string(),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            size: 0,
            signature: String::new(),
            changelog: "initial".to_string(),
        }
    }

    const NOW: i64 = 1_750_000_000;

    #[test]
    fn accepts_valid_signed_manifest() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        m.signature = sign(&m, &sk);
        let verdict = validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, None);
        assert_eq!(verdict, ManifestVerdict::Accepted);
    }

    #[test]
    fn rejects_bad_signature() {
        let (sk, _pub_b64) = test_keypair();
        let (other_sk, other_pub) = {
            let seed = [7u8; 32];
            let k = SigningKey::from_bytes(&seed);
            let pub_b64 = BASE64.encode(k.verifying_key().to_bytes());
            (k, pub_b64)
        };
        let mut m = base_manifest();
        // 用 A 的公钥声明，但用 B 的私钥签名 -> 应拒绝。
        m.signature = sign(&m, &other_sk);
        let _ = &sk;
        let verdict = validate_manifest(&m, PresetTrust::PinnedKey, &other_pub, "", NOW, None);
        // 这里公钥与签名匹配，应接受；改为用不匹配公钥验证。
        assert_eq!(verdict, ManifestVerdict::Accepted);

        // 真正的不匹配：公钥用 A，签名用 B。
        let (_, pub_a) = test_keypair();
        let verdict2 = validate_manifest(&m, PresetTrust::PinnedKey, &pub_a, "", NOW, None);
        assert_eq!(
            verdict2,
            ManifestVerdict::Rejected(ManifestRejectReason::BadSignature)
        );
    }

    #[test]
    fn rejects_missing_signature_for_pinned_key() {
        let (_sk, pub_b64) = test_keypair();
        let m = base_manifest(); // signature 为空
        let verdict = validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, None);
        assert_eq!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::MissingSignature)
        );
    }

    #[test]
    fn rejects_expired() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        m.expires_at = NOW - 1;
        m.signature = sign(&m, &sk);
        let verdict = validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, None);
        assert!(matches!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::Expired { .. })
        ));
    }

    #[test]
    fn rejects_version_rollback() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        m.version = "2026.08.01".to_string(); // 低于已接受的 2026.08.23
        m.signature = sign(&m, &sk);
        let verdict = validate_manifest(
            &m,
            PresetTrust::PinnedKey,
            &pub_b64,
            "2026.08.23",
            NOW,
            None,
        );
        assert!(matches!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::VersionRollback { .. })
        ));
    }

    #[test]
    fn accepts_newer_version() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        m.version = "2026.09.01".to_string();
        m.signature = sign(&m, &sk);
        let verdict = validate_manifest(
            &m,
            PresetTrust::PinnedKey,
            &pub_b64,
            "2026.08.23",
            NOW,
            None,
        );
        assert_eq!(verdict, ManifestVerdict::Accepted);
    }

    #[test]
    fn rejects_hash_mismatch() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        let payload = b"real preset bytes";
        m.size = payload.len() as u64;
        m.sha256 = "1111111111111111111111111111111111111111111111111111111111111111".to_string();
        m.signature = sign(&m, &sk);
        let verdict =
            validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, Some(payload));
        assert!(matches!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::HashMismatch { .. })
        ));
    }

    #[test]
    fn accepts_matching_hash_and_size() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        let payload = b"real preset bytes";
        m.size = payload.len() as u64;
        m.sha256 = sha256_hex(payload);
        m.signature = sign(&m, &sk);
        let verdict =
            validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, Some(payload));
        assert_eq!(verdict, ManifestVerdict::Accepted);
    }

    #[test]
    fn rejects_size_mismatch() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        let payload = b"real preset bytes";
        m.size = 999_999;
        m.sha256 = sha256_hex(payload);
        m.signature = sign(&m, &sk);
        let verdict =
            validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, Some(payload));
        assert!(matches!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::SizeMismatch { .. })
        ));
    }

    #[test]
    fn local_trust_skips_signature_but_keeps_expiry() {
        let m = base_manifest(); // 无签名
        let verdict = validate_manifest(&m, PresetTrust::Local, "", "", NOW, None);
        assert_eq!(verdict, ManifestVerdict::Accepted);

        let mut expired = base_manifest();
        expired.expires_at = NOW - 1;
        let verdict2 = validate_manifest(&expired, PresetTrust::Local, "", "", NOW, None);
        assert!(matches!(
            verdict2,
            ManifestVerdict::Rejected(ManifestRejectReason::Expired { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_schema() {
        let (sk, pub_b64) = test_keypair();
        let mut m = base_manifest();
        m.schema_version = 99;
        m.signature = sign(&m, &sk);
        let verdict = validate_manifest(&m, PresetTrust::PinnedKey, &pub_b64, "", NOW, None);
        assert!(matches!(
            verdict,
            ManifestVerdict::Rejected(ManifestRejectReason::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn manifest_segments_layout() {
        let src = PresetSource {
            id: "ccsm-official".to_string(),
            kind: PresetSourceKind::WebDav,
            trust: PresetTrust::PinnedKey,
            enabled: true,
            base_url: "https://dav.example.com".to_string(),
            username: String::new(),
            password: String::new(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            public_key: String::new(),
            last_checked_at: 0,
            last_accepted_version: String::new(),
        };
        let segs = src.manifest_segments();
        assert_eq!(
            segs,
            vec!["cc-switch-sync", "presets", "default", "manifest.json"]
        );
    }

    #[test]
    fn target_segments_stay_inside_the_configured_preset_root() {
        let source = PresetSource {
            id: "ccsm-official".to_string(),
            kind: PresetSourceKind::WebDav,
            trust: PresetTrust::Local,
            enabled: true,
            base_url: "https://dav.example.com".to_string(),
            username: String::new(),
            password: String::new(),
            remote_root: "catalog".to_string(),
            profile: "stable".to_string(),
            public_key: String::new(),
            last_checked_at: 0,
            last_accepted_version: String::new(),
        };

        assert_eq!(
            source.target_segments("bundles/preset-table.json").unwrap(),
            vec![
                "catalog",
                "presets",
                "stable",
                "bundles",
                "preset-table.json"
            ]
        );
        assert!(source.target_segments("../other.json").is_err());
        assert!(source
            .target_segments("https://example.com/other.json")
            .is_err());
    }

    #[test]
    fn version_compare_handles_leading_zeros() {
        // 2026.08.23 与 2026.8.23 视为同一版本（前导零不影响数值比较）。
        assert!(!is_rollback("2026.8.23", "2026.08.23"));
        assert!(is_rollback("2026.08.23", "2026.08.01"));
        assert!(!is_rollback("2026.08.23", "2026.09.01"));
        assert!(version_le("2026.08.23", "2026.08.23"));
    }
}

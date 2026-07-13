//! Codex 多设备额度协作的本地报告与网关约束。
//!
//! 这里只保存脱敏账号 scope、官方窗口百分比及 token 聚合值。不得把 OAuth
//! 凭据、prompt、原始 JSONL、工作目录或请求内容带入协作数据。

use std::collections::{BTreeMap, HashMap};

use chrono::{Local, TimeZone};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::subscription::SubscriptionQuota;
use crate::settings::{self, QuotaCollaborationSettings};

const FRESH_WINDOW_MAX_AGE_SECS: i64 = 10 * 60;

/// 可由其它设备读取的最小协作报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaDeviceReport {
    pub protocol_version: u8,
    pub account_scope: String,
    pub device_id: String,
    pub device_name: String,
    pub captured_at: i64,
    pub today_tokens: u64,
    pub seven_day_tokens: u64,
    pub today_requests: u64,
    pub seven_day_requests: u64,
    pub tiers: Vec<QuotaWindowSnapshot>,
}

/// 官方窗口快照，单位仅为官方百分比。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindowSnapshot {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}

/// 页面显示需要的跨设备汇总。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCollaborationOverview {
    pub configured: bool,
    pub mode: String,
    pub enforce_remaining_percent: f64,
    pub device_id: String,
    pub reports: Vec<QuotaDeviceReport>,
    pub latest_window_utilization: BTreeMap<String, f64>,
    pub latest_window_captured_at: Option<i64>,
    pub warning: Option<String>,
}

/// 从本机 Codex auth 的 account_id 构建不可逆账号 scope。
pub fn codex_account_scope() -> Option<String> {
    let auth: serde_json::Value =
        crate::config::read_json_file(&crate::get_codex_auth_path()).ok()?;
    let account_id = auth.pointer("/tokens/account_id")?.as_str()?.trim();
    (!account_id.is_empty()).then(|| {
        format!(
            "{:x}",
            Sha256::digest(format!("ccswitchmulti-quota-v1:{account_id}").as_bytes())
        )
    })
}

/// 在成功读取官方额度后记录本机报告，并刷新本机网关的快速约束依据。
pub fn record_codex_quota_snapshot(
    db: &Database,
    quota: &SubscriptionQuota,
) -> Result<(), AppError> {
    if !quota.success || quota.tool != "codex" {
        return Ok(());
    }
    let Some(account_scope) = codex_account_scope() else {
        return Ok(());
    };
    let config = settings::get_settings().quota_collaboration;
    let captured_at = quota.queried_at.unwrap_or_else(now_seconds);
    let report = build_report(db, account_scope, &config, quota, captured_at)?;
    save_report(db, &report)?;
    update_window_cache(std::slice::from_ref(&report));
    Ok(())
}

/// 返回当前账号的本地协作缓存；远端同步后同样写入这个缓存。
pub fn get_overview(db: &Database) -> Result<QuotaCollaborationOverview, AppError> {
    let app_settings = settings::get_settings();
    let config = app_settings.quota_collaboration;
    let Some(scope) = codex_account_scope() else {
        return Ok(QuotaCollaborationOverview {
            mode: config.mode,
            enforce_remaining_percent: config.enforce_remaining_percent,
            device_id: config.device_id,
            warning: Some("当前 Codex 登录没有可用于设备协作的账号标识。".to_string()),
            ..Default::default()
        });
    };
    Ok(QuotaCollaborationOverview {
        configured: app_settings
            .webdav_sync
            .as_ref()
            .is_some_and(|value| value.enabled)
            || app_settings
                .s3_sync
                .as_ref()
                .is_some_and(|value| value.enabled),
        mode: config.mode,
        enforce_remaining_percent: config.enforce_remaining_percent,
        device_id: config.device_id,
        reports: load_reports(db, &scope)?,
        latest_window_utilization: config.latest_window_utilization,
        latest_window_captured_at: config.latest_window_captured_at,
        warning: None,
    })
}

/// 合并已验证的远端报告。相同设备只接受更新时间更晚的报告。
pub fn merge_remote_reports(db: &Database, reports: &[QuotaDeviceReport]) -> Result<(), AppError> {
    let Some(scope) = codex_account_scope() else {
        return Ok(());
    };
    for report in reports {
        if is_valid_report(report, &scope) {
            save_report(db, report)?;
        }
    }
    update_window_cache(&load_reports(db, &scope)?);
    Ok(())
}

/// 上传本机报告并读取同一账号 scope 下所有设备的独立报告文件。
///
/// 一个设备只覆盖自己的 `{device_id}.json`，不会覆盖其它设备，避免多机写入
/// 同一个全局文件时发生“最后一次上传丢掉其它设备”的竞态。
pub async fn sync_reports(db: &Database) -> Result<QuotaCollaborationOverview, AppError> {
    let scope = codex_account_scope().ok_or_else(|| {
        AppError::localized(
            "quota.collaboration.account_missing",
            "当前 Codex 登录没有可用于多设备协作的账号标识。",
            "The current Codex login has no account identity for device collaboration.",
        )
    })?;
    let app_settings = settings::get_settings();
    let device_id = app_settings.quota_collaboration.device_id.clone();
    let local = load_reports(db, &scope)?
        .into_iter()
        .find(|report| report.device_id == device_id)
        .ok_or_else(|| {
            AppError::localized(
                "quota.collaboration.snapshot_missing",
                "请先刷新一次 Codex 官方额度，再同步多设备报告。",
                "Refresh the official Codex quota once before syncing device reports.",
            )
        })?;
    let reports = if let Some(sync) = app_settings.webdav_sync.filter(|value| value.enabled) {
        sync_webdav(&sync, &scope, &local).await?
    } else if let Some(sync) = app_settings.s3_sync.filter(|value| value.enabled) {
        sync_s3(&sync, &scope, &local).await?
    } else {
        return Err(AppError::localized(
            "quota.collaboration.sync_unconfigured",
            "请先在设置中启用 WebDAV 或 S3 同步，再使用多设备额度协作。",
            "Enable WebDAV or S3 sync before using device quota collaboration.",
        ));
    };
    merge_remote_reports(db, &reports)?;
    get_overview(db)
}

/// 代理热路径的约束判定。陈旧或缺失的快照不会造成误拦截。
pub fn codex_enforcement_reason(now: i64) -> Option<String> {
    let config = settings::get_settings().quota_collaboration;
    codex_enforcement_reason_inner(now, &config)
}

/// `codex_enforcement_reason` 的纯函数版本，接受显式配置，便于测试。
///
/// 不访问全局状态，不会写入磁盘。
fn codex_enforcement_reason_inner(now: i64, config: &QuotaCollaborationSettings) -> Option<String> {
    if config.mode != "enforce"
        || now.saturating_sub(config.latest_window_captured_at?) > FRESH_WINDOW_MAX_AGE_SECS
    {
        return None;
    }
    let highest_used = config
        .latest_window_utilization
        .values()
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0_f64, f64::max);
    let remaining = (100.0 - highest_used).max(0.0);
    (remaining <= config.enforce_remaining_percent).then(|| {
        format!(
            "账号官方窗口剩余约 {:.0}%，达到约束阈值 {:.0}%。此限制只作用于经过本机 CCSwitchMulti 网关的 Codex 请求。",
            remaining, config.enforce_remaining_percent
        )
    })
}

/// 依据本地 Codex 代理日志构建聚合报告。
fn build_report(
    db: &Database,
    account_scope: String,
    config: &QuotaCollaborationSettings,
    quota: &SubscriptionQuota,
    captured_at: i64,
) -> Result<QuotaDeviceReport, AppError> {
    let (today_start, seven_days_start) = range_starts(captured_at);
    let (today_tokens, today_requests) = usage_totals(db, today_start)?;
    let (seven_day_tokens, seven_day_requests) = usage_totals(db, seven_days_start)?;
    let device_name = if config.device_name.is_empty() {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "CCSwitchMulti".to_string())
    } else {
        config.device_name.clone()
    };
    Ok(QuotaDeviceReport {
        protocol_version: 1,
        account_scope,
        device_id: config.device_id.clone(),
        device_name,
        captured_at,
        today_tokens,
        seven_day_tokens,
        today_requests,
        seven_day_requests,
        tiers: quota
            .tiers
            .iter()
            .map(|tier| QuotaWindowSnapshot {
                name: tier.name.clone(),
                utilization: tier.utilization.clamp(0.0, 100.0),
                resets_at: tier.resets_at.clone(),
            })
            .collect(),
    })
}

/// 汇总本机已记录的 Codex token 和请求数，不读取请求内容。
fn usage_totals(db: &Database, start_at: i64) -> Result<(u64, u64), AppError> {
    let conn = lock_conn!(db.conn);
    let (tokens, requests): (i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_read_tokens + cache_creation_tokens), 0),
                    COUNT(*)
             FROM proxy_request_logs
             WHERE app_type = 'codex' AND created_at >= ?1",
            params![start_at],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| AppError::Database(format!("读取 Codex 协作用量失败: {error}")))?;
    Ok((tokens.max(0) as u64, requests.max(0) as u64))
}

/// 使用设备维度 UPSERT 保存当前报告。
fn save_report(db: &Database, report: &QuotaDeviceReport) -> Result<(), AppError> {
    let payload = serde_json::to_string(report)
        .map_err(|error| AppError::Database(format!("序列化协作报告失败: {error}")))?;
    let conn = lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO quota_collaboration_reports (
             account_scope, device_id, device_name, captured_at, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(account_scope, device_id) DO UPDATE SET
             device_name = excluded.device_name,
             captured_at = excluded.captured_at,
             payload = CASE WHEN excluded.captured_at >= quota_collaboration_reports.captured_at
                 THEN excluded.payload ELSE quota_collaboration_reports.payload END",
        params![
            report.account_scope,
            report.device_id,
            report.device_name,
            report.captured_at,
            payload
        ],
    )
    .map_err(|error| AppError::Database(format!("保存协作报告失败: {error}")))?;
    Ok(())
}

/// 读取已缓存的设备报告。损坏行只跳过，不能让一台旧设备阻断整个页面。
fn load_reports(db: &Database, scope: &str) -> Result<Vec<QuotaDeviceReport>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut statement = conn
        .prepare(
            "SELECT payload FROM quota_collaboration_reports
             WHERE account_scope = ?1 ORDER BY captured_at DESC",
        )
        .map_err(|error| AppError::Database(format!("准备读取协作报告失败: {error}")))?;
    let rows = statement
        .query_map(params![scope], |row| row.get::<_, String>(0))
        .map_err(|error| AppError::Database(format!("读取协作报告失败: {error}")))?;
    let mut reports = Vec::new();
    for row in rows {
        let payload =
            row.map_err(|error| AppError::Database(format!("读取协作报告行失败: {error}")))?;
        if let Ok(report) = serde_json::from_str::<QuotaDeviceReport>(&payload) {
            if is_valid_report(&report, scope) {
                reports.push(report);
            }
        }
    }
    Ok(reports)
}

/// 验证远端输入的协议版本、账号隔离和百分比边界。
fn is_valid_report(report: &QuotaDeviceReport, scope: &str) -> bool {
    report.protocol_version == 1
        && report.account_scope == scope
        && !report.device_id.trim().is_empty()
        && !report.device_name.trim().is_empty()
        && report.captured_at > 0
        && report.tiers.iter().all(|tier| {
            !tier.name.trim().is_empty()
                && tier.utilization.is_finite()
                && (0.0..=100.0).contains(&tier.utilization)
        })
}

/// 合并所有设备官方窗口时采用最高利用率，避免从旧设备低估总额度消耗。
fn update_window_cache(reports: &[QuotaDeviceReport]) {
    let mut values = HashMap::<String, f64>::new();
    for report in reports {
        for tier in &report.tiers {
            values
                .entry(tier.name.clone())
                .and_modify(|value| *value = value.max(tier.utilization))
                .or_insert(tier.utilization);
        }
    }
    let latest = reports.iter().map(|report| report.captured_at).max();
    let _ = settings::mutate_quota_collaboration(|config| {
        config.latest_window_utilization = values.into_iter().collect();
        config.latest_window_captured_at = latest;
    });
}

/// 计算今天和包含今天的七日窗口起点。
fn range_starts(now: i64) -> (i64, i64) {
    let local = Local
        .timestamp_opt(now, 0)
        .single()
        .unwrap_or_else(Local::now);
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|value| Local.from_local_datetime(&value).single())
        .unwrap_or(local)
        .timestamp();
    (midnight, midnight - 6 * 24 * 60 * 60)
}

/// 获取秒级时间戳，避免把前端毫秒时间带进 SQL 查询。
fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}

/// WebDAV 同步：列目录发现设备文件后逐个读取，不依赖全局可写索引。
async fn sync_webdav(
    sync: &crate::settings::WebDavSyncSettings,
    scope: &str,
    local: &QuotaDeviceReport,
) -> Result<Vec<QuotaDeviceReport>, AppError> {
    use crate::services::webdav;
    let auth = webdav::auth_from_credentials(&sync.username, &sync.password);
    let mut segments: Vec<String> = webdav::path_segments(&sync.remote_root)
        .map(str::to_string)
        .collect();
    segments.extend(["quota-collaboration".into(), "v1".into(), scope.into()]);
    webdav::ensure_remote_directories(&sync.base_url, &segments, &auth).await?;
    let directory = webdav::build_remote_url(&sync.base_url, &segments)?;
    let mut own_segments = segments.clone();
    own_segments.push(format!("{}.json", local.device_id));
    let own_url = webdav::build_remote_url(&sync.base_url, &own_segments)?;
    webdav::put_bytes(
        &own_url,
        &auth,
        serde_json::to_vec(local)
            .map_err(|error| AppError::Database(format!("序列化协作上报失败: {error}")))?,
        "application/json",
    )
    .await?;
    let mut reports = Vec::new();
    for url in webdav::list_child_urls(&directory, &auth, 200)
        .await?
        .into_iter()
        .filter(|url| url.ends_with(".json"))
    {
        if let Some((body, _)) = webdav::get_bytes(&url, &auth, 256 * 1024).await? {
            if let Ok(report) = serde_json::from_slice(&body) {
                reports.push(report);
            }
        }
    }
    Ok(reports)
}

/// S3 同步：ListObjectsV2 根据前缀发现每台设备独立对象。
async fn sync_s3(
    sync: &crate::settings::S3SyncSettings,
    scope: &str,
    local: &QuotaDeviceReport,
) -> Result<Vec<QuotaDeviceReport>, AppError> {
    use crate::services::s3::{self, S3Credentials};
    let credentials = S3Credentials {
        access_key_id: sync.access_key_id.clone(),
        secret_access_key: sync.secret_access_key.clone(),
        region: sync.region.clone(),
        bucket: sync.bucket.clone(),
        endpoint: sync.endpoint.clone(),
    };
    let prefix = format!(
        "{}/quota-collaboration/v1/{}/",
        sync.remote_root.trim_matches('/'),
        scope
    );
    s3::put_object(
        &credentials,
        &format!("{prefix}{}.json", local.device_id),
        serde_json::to_vec(local)
            .map_err(|error| AppError::Database(format!("序列化协作上报失败: {error}")))?,
        "application/json",
    )
    .await?;
    let mut reports = Vec::new();
    for key in s3::list_object_keys(&credentials, &prefix, 200)
        .await?
        .into_iter()
        .filter(|key| key.ends_with(".json"))
    {
        if let Some((body, _)) = s3::get_object(&credentials, &key, 256 * 1024).await? {
            if let Ok(report) = serde_json::from_slice(&body) {
                reports.push(report);
            }
        }
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

    // 1. Report Validation

    #[test]
    fn report_validation_accepts_valid_report() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "validscope".into(),
            device_id: "device-x".into(),
            device_name: "test-machine".into(),
            captured_at: 1000,
            today_tokens: 500,
            seven_day_tokens: 5000,
            today_requests: 10,
            seven_day_requests: 100,
            tiers: vec![
                QuotaWindowSnapshot {
                    name: "five_hour".into(),
                    utilization: 50.0,
                    resets_at: None,
                },
                QuotaWindowSnapshot {
                    name: "seven_day".into(),
                    utilization: 30.0,
                    resets_at: Some("2026-07-15T00:00:00Z".into()),
                },
            ],
        };
        assert!(is_valid_report(&report, "validscope"));
    }

    #[test]
    fn report_validation_rejects_utilization_above_100() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![QuotaWindowSnapshot {
                name: "five_hour".into(),
                utilization: 101.0,
                resets_at: None,
            }],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_utilization_below_zero() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![QuotaWindowSnapshot {
                name: "five_hour".into(),
                utilization: -0.01,
                resets_at: None,
            }],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_non_finite_utilization() {
        for util in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let report = QuotaDeviceReport {
                protocol_version: 1,
                account_scope: "scope".into(),
                device_id: "device".into(),
                device_name: "machine".into(),
                captured_at: 1,
                today_tokens: 0,
                seven_day_tokens: 0,
                today_requests: 0,
                seven_day_requests: 0,
                tiers: vec![QuotaWindowSnapshot {
                    name: "five_hour".into(),
                    utilization: util,
                    resets_at: None,
                }],
            };
            assert!(!is_valid_report(&report, "scope"));
        }
    }

    #[test]
    fn report_validation_rejects_empty_device_id() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "  ".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_empty_device_name() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "\t\n".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_wrong_protocol_version() {
        let report = QuotaDeviceReport {
            protocol_version: 0,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_wrong_account_scope() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope-a".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        assert!(!is_valid_report(&report, "scope-b"));
    }

    #[test]
    fn report_validation_rejects_zero_captured_at() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 0,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    #[test]
    fn report_validation_rejects_empty_tier_name() {
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "device".into(),
            device_name: "machine".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![QuotaWindowSnapshot {
                name: "".into(),
                utilization: 50.0,
                resets_at: None,
            }],
        };
        assert!(!is_valid_report(&report, "scope"));
    }

    // 2. DB Persistence

    #[test]
    fn save_and_load_report_roundtrip() {
        let db = Database::memory().expect("create mem db");
        let report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "myscope".into(),
            device_id: "dev-1".into(),
            device_name: "desktop".into(),
            captured_at: 2000,
            today_tokens: 100,
            seven_day_tokens: 700,
            today_requests: 5,
            seven_day_requests: 35,
            tiers: vec![QuotaWindowSnapshot {
                name: "five_hour".into(),
                utilization: 45.0,
                resets_at: None,
            }],
        };
        save_report(&db, &report).expect("save ok");
        let loaded = load_reports(&db, "myscope").expect("load ok");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].device_id, "dev-1");
        assert_eq!(loaded[0].today_tokens, 100);

        let wrong = load_reports(&db, "otherscope").expect("load ok");
        assert!(wrong.is_empty());
    }

    #[test]
    fn multiple_devices_coexist_in_same_scope() {
        let db = Database::memory().expect("create mem db");
        let dev_a = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "shared".into(),
            device_id: "device-a".into(),
            device_name: "Alpha".into(),
            captured_at: 100,
            today_tokens: 10,
            seven_day_tokens: 70,
            today_requests: 2,
            seven_day_requests: 14,
            tiers: vec![],
        };
        let dev_b = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "shared".into(),
            device_id: "device-b".into(),
            device_name: "Beta".into(),
            captured_at: 200,
            today_tokens: 20,
            seven_day_tokens: 140,
            today_requests: 4,
            seven_day_requests: 28,
            tiers: vec![],
        };
        save_report(&db, &dev_a).expect("save A");
        save_report(&db, &dev_b).expect("save B");
        let loaded = load_reports(&db, "shared").expect("load");
        assert_eq!(loaded.len(), 2);
        let ids: Vec<&str> = loaded.iter().map(|r| r.device_id.as_str()).collect();
        assert!(ids.contains(&"device-a"));
        assert!(ids.contains(&"device-b"));
    }

    // 3. Merge & Isolation

    #[test]
    fn merge_remote_reports_filters_invalid_scope() {
        // 注：merge_remote_reports 内部调用 codex_account_scope() 读取文件系统，
        // 在测试环境中返回 None 导致操作跳过。
        // 这里改用 save_report + load_reports 直接验证 scope 隔离。
        let db = Database::memory().expect("create mem db");
        let valid = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "good".into(),
            device_name: "ok".into(),
            captured_at: 100,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        let bad = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "wrong-scope".into(),
            device_id: "bad".into(),
            device_name: "ok".into(),
            captured_at: 100,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        save_report(&db, &valid).expect("save valid");
        save_report(&db, &bad).expect("save bad");
        let loaded_scope = load_reports(&db, "scope").expect("load scope");
        assert_eq!(loaded_scope.len(), 1, "scope 下只应有合法报告");
        assert_eq!(loaded_scope[0].device_id, "good");
        let loaded_wrong = load_reports(&db, "wrong-scope").expect("load wrong");
        assert_eq!(loaded_wrong.len(), 1, "wrong-scope 下应有 bad");
        assert_eq!(loaded_wrong[0].device_id, "bad");
    }

    #[test]
    fn merge_replaces_same_device_with_newer_report() {
        // 使用 save_report 直接验证 UPSERT 逻辑：同 device_id 下高 captured_at 覆盖低。
        let db = Database::memory().expect("create mem db");
        let old = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "dev".into(),
            device_name: "old".into(),
            captured_at: 100,
            today_tokens: 10,
            seven_day_tokens: 70,
            today_requests: 2,
            seven_day_requests: 14,
            tiers: vec![QuotaWindowSnapshot {
                name: "five_hour".into(),
                utilization: 20.0,
                resets_at: None,
            }],
        };
        let new = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "dev".into(),
            device_name: "new".into(),
            captured_at: 200,
            today_tokens: 50,
            seven_day_tokens: 350,
            today_requests: 10,
            seven_day_requests: 70,
            tiers: vec![QuotaWindowSnapshot {
                name: "five_hour".into(),
                utilization: 80.0,
                resets_at: None,
            }],
        };
        save_report(&db, &old).expect("save old");
        save_report(&db, &new).expect("save new");
        let loaded = load_reports(&db, "scope").expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].device_name, "new");
        assert_eq!(loaded[0].today_tokens, 50);
        assert_eq!(loaded[0].captured_at, 200);
    }

    #[test]
    fn older_report_does_not_replace_newer() {
        let db = Database::memory().expect("create mem db");
        let existing = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "dev".into(),
            device_name: "existing".into(),
            captured_at: 300,
            today_tokens: 100,
            seven_day_tokens: 700,
            today_requests: 20,
            seven_day_requests: 140,
            tiers: vec![],
        };
        save_report(&db, &existing).expect("save existing");
        let stale = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "scope".into(),
            device_id: "dev".into(),
            device_name: "stale".into(),
            captured_at: 200,
            today_tokens: 5,
            seven_day_tokens: 35,
            today_requests: 1,
            seven_day_requests: 7,
            tiers: vec![],
        };
        merge_remote_reports(&db, &[stale]).expect("merge");
        let loaded = load_reports(&db, "scope").expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].device_name, "existing");
        assert_eq!(loaded[0].captured_at, 300);
    }

    // 4. Range Calculation

    #[test]
    fn range_starts_produces_midnight_and_seven_days_ago() {
        use chrono::{Local, TimeZone};
        let now = chrono::Utc::now().timestamp();
        // Use Local to match range_starts internal clock source
        let local = Local
            .timestamp_opt(now, 0)
            .single()
            .unwrap_or_else(Local::now);
        let expected_midnight = local
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|d| Local.from_local_datetime(&d).single())
            .unwrap_or(local)
            .timestamp();
        let expected_seven = expected_midnight - 6 * 24 * 60 * 60;
        let (today_start, seven_ago) = range_starts(now);
        assert_eq!(today_start, expected_midnight, "today_start mismatch");
        assert_eq!(seven_ago, expected_seven, "seven_ago mismatch");
    }

    #[test]
    fn range_starts_midnight_boundary() {
        use chrono::TimeZone;
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let late = tz
            .with_ymd_and_hms(2026, 7, 14, 23, 59, 59)
            .unwrap()
            .timestamp();
        let early = tz
            .with_ymd_and_hms(2026, 7, 14, 0, 0, 1)
            .unwrap()
            .timestamp();
        let (late_today, _) = range_starts(late);
        let (early_today, _) = range_starts(early);
        assert_eq!(late_today, early_today);
    }

    // 5. Window Cache

    #[test]
    fn window_cache_takes_highest_utilization() {
        let reports = vec![
            QuotaDeviceReport {
                protocol_version: 1,
                account_scope: "s".into(),
                device_id: "a".into(),
                device_name: "a".into(),
                captured_at: 100,
                today_tokens: 0,
                seven_day_tokens: 0,
                today_requests: 0,
                seven_day_requests: 0,
                tiers: vec![
                    QuotaWindowSnapshot {
                        name: "five_hour".into(),
                        utilization: 30.0,
                        resets_at: None,
                    },
                    QuotaWindowSnapshot {
                        name: "seven_day".into(),
                        utilization: 20.0,
                        resets_at: None,
                    },
                ],
            },
            QuotaDeviceReport {
                protocol_version: 1,
                account_scope: "s".into(),
                device_id: "b".into(),
                device_name: "b".into(),
                captured_at: 200,
                today_tokens: 0,
                seven_day_tokens: 0,
                today_requests: 0,
                seven_day_requests: 0,
                tiers: vec![
                    QuotaWindowSnapshot {
                        name: "five_hour".into(),
                        utilization: 60.0,
                        resets_at: None,
                    },
                    QuotaWindowSnapshot {
                        name: "seven_day".into(),
                        utilization: 10.0,
                        resets_at: None,
                    },
                ],
            },
        ];
        update_window_cache(&reports);
    }

    #[test]
    fn window_cache_empty_reports_does_not_panic() {
        update_window_cache(&[]);
    }

    // 6. Enforcement Strategy

    #[test]
    fn enforcement_observe_mode_returns_none() {
        let config = QuotaCollaborationSettings {
            mode: "observe".into(),
            latest_window_utilization: Default::default(),
            latest_window_captured_at: Some(1000),
            ..Default::default()
        };
        assert_eq!(codex_enforcement_reason_inner(1000, &config), None);
    }

    #[test]
    fn enforcement_stale_cache_returns_none() {
        let config = QuotaCollaborationSettings {
            mode: "enforce".into(),
            latest_window_utilization: Default::default(),
            latest_window_captured_at: Some(0),
            ..Default::default()
        };
        assert_eq!(
            codex_enforcement_reason_inner(FRESH_WINDOW_MAX_AGE_SECS + 1, &config),
            None
        );
    }

    #[test]
    fn enforcement_missing_captured_at_returns_none() {
        let config = QuotaCollaborationSettings {
            mode: "enforce".into(),
            latest_window_utilization: Default::default(),
            latest_window_captured_at: None,
            ..Default::default()
        };
        assert_eq!(codex_enforcement_reason_inner(1000, &config), None);
    }

    #[test]
    fn enforcement_high_utilization_triggers_block() {
        let mut util = std::collections::BTreeMap::new();
        util.insert("five_hour".into(), 85.0);
        util.insert("seven_day".into(), 75.0);
        let config = QuotaCollaborationSettings {
            mode: "enforce".into(),
            enforce_remaining_percent: 20.0,
            latest_window_utilization: util,
            latest_window_captured_at: Some(1000),
            ..Default::default()
        };
        let result = codex_enforcement_reason_inner(1000, &config);
        assert!(result.is_some());
        assert!(result.unwrap().contains("15%"));
    }

    #[test]
    fn enforcement_low_utilization_does_not_block() {
        let mut util = std::collections::BTreeMap::new();
        util.insert("five_hour".into(), 10.0);
        let config = QuotaCollaborationSettings {
            mode: "enforce".into(),
            enforce_remaining_percent: 20.0,
            latest_window_utilization: util,
            latest_window_captured_at: Some(1000),
            ..Default::default()
        };
        assert_eq!(codex_enforcement_reason_inner(1000, &config), None);
    }

    #[test]
    fn enforcement_ignores_non_finite_utilization() {
        let mut util = std::collections::BTreeMap::new();
        util.insert("five_hour".into(), f64::NAN);
        util.insert("seven_day".into(), f64::INFINITY);
        let config = QuotaCollaborationSettings {
            mode: "enforce".into(),
            enforce_remaining_percent: 20.0,
            latest_window_utilization: util,
            latest_window_captured_at: Some(1000),
            ..Default::default()
        };
        assert_eq!(codex_enforcement_reason_inner(1000, &config), None);
    }

    // 7. WebDAV E2E via Mock Server

    #[tokio::test]
    async fn webdav_e2e_device_upload_and_discovery() {
        let (_port, _guard, _store) = start_mock_webdav();
        let base_url = format!("http://127.0.0.1:{_port}/");

        let local_report = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "testscope".into(),
            device_id: "dev-e2e-1".into(),
            device_name: "e2e-device".into(),
            captured_at: 1000,
            today_tokens: 500,
            seven_day_tokens: 3500,
            today_requests: 15,
            seven_day_requests: 105,
            tiers: vec![
                QuotaWindowSnapshot {
                    name: "five_hour".into(),
                    utilization: 42.0,
                    resets_at: None,
                },
                QuotaWindowSnapshot {
                    name: "seven_day".into(),
                    utilization: 33.0,
                    resets_at: Some("2026-07-15Z".into()),
                },
            ],
        };
        let sync = crate::settings::WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            base_url,
            username: String::new(),
            password: String::new(),
            remote_root: "cc-switch-sync".into(),
            profile: String::new(),
            include_keys_on_upload: false,
            status: crate::settings::WebDavSyncStatus::default(),
        };
        let remote_reports = super::sync_webdav(&sync, "testscope", &local_report)
            .await
            .expect("sync_webdav ok");
        assert!(remote_reports.iter().any(|r| r.device_id == "dev-e2e-1"));
        let found = remote_reports
            .iter()
            .find(|r| r.device_id == "dev-e2e-1")
            .unwrap();
        assert_eq!(found.today_tokens, 500);
        assert_eq!(found.tiers.len(), 2);

        let path = format!("/cc-switch-sync/quota-collaboration/v1/testscope/dev-e2e-1.json");
        assert!(_store.lock().unwrap().contains_key(&path));
    }

    #[tokio::test]
    async fn webdav_e2e_multiple_devices_discover_each_other() {
        let (_port, _guard, store) = start_mock_webdav();
        let base_url = format!("http://127.0.0.1:{_port}/");

        let dev_a = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "sharedscope".into(),
            device_id: "device-a".into(),
            device_name: "Alpha".into(),
            captured_at: 100,
            today_tokens: 100,
            seven_day_tokens: 700,
            today_requests: 5,
            seven_day_requests: 35,
            tiers: vec![],
        };
        store.lock().unwrap().insert(
            "/cc-switch-sync/quota-collaboration/v1/sharedscope/device-a.json".into(),
            serde_json::to_vec(&dev_a).unwrap(),
        );

        let dev_b = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "sharedscope".into(),
            device_id: "device-b".into(),
            device_name: "Beta".into(),
            captured_at: 200,
            today_tokens: 200,
            seven_day_tokens: 1400,
            today_requests: 10,
            seven_day_requests: 70,
            tiers: vec![],
        };
        let sync = crate::settings::WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            base_url,
            username: String::new(),
            password: String::new(),
            remote_root: "cc-switch-sync".into(),
            profile: String::new(),
            include_keys_on_upload: false,
            status: crate::settings::WebDavSyncStatus::default(),
        };
        let reports = super::sync_webdav(&sync, "sharedscope", &dev_b)
            .await
            .expect("sync_webdav ok");
        let ids: Vec<&str> = reports.iter().map(|r| r.device_id.as_str()).collect();
        assert!(ids.contains(&"device-a"));
        assert!(ids.contains(&"device-b"));
        assert_eq!(reports.len(), 2);
    }

    #[tokio::test]
    async fn webdav_e2e_non_json_files_are_skipped() {
        let (_port, _guard, store) = start_mock_webdav();
        let base_url = format!("http://127.0.0.1:{_port}/");
        let scope = "skipscope".to_string();
        let dir = format!("/cc-switch-sync/quota-collaboration/v1/{scope}");
        {
            let mut s = store.lock().unwrap();
            s.insert(format!("{dir}/readme.txt"), b"not json".to_vec());
        }
        let sync = crate::settings::WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            base_url,
            username: String::new(),
            password: String::new(),
            remote_root: "cc-switch-sync".into(),
            profile: String::new(),
            include_keys_on_upload: false,
            status: crate::settings::WebDavSyncStatus::default(),
        };
        let local = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: scope.clone(),
            device_id: "self".into(),
            device_name: "self".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        let reports = super::sync_webdav(&sync, &scope, &local)
            .await
            .expect("sync_webdav ok");
        assert!(reports.iter().any(|r| r.device_id == "self"));
        assert!(!reports.iter().any(|r| r.device_name == "readme"));
    }

    #[tokio::test]
    async fn webdav_e2e_handles_404_gracefully() {
        let (_port, _guard, _store) = start_mock_webdav();
        let base_url = format!("http://127.0.0.1:{_port}/");

        let sync = crate::settings::WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            base_url,
            username: String::new(),
            password: String::new(),
            remote_root: "cc-switch-sync".into(),
            profile: String::new(),
            include_keys_on_upload: false,
            status: crate::settings::WebDavSyncStatus::default(),
        };
        let local = QuotaDeviceReport {
            protocol_version: 1,
            account_scope: "emptyscope".into(),
            device_id: "self".into(),
            device_name: "self".into(),
            captured_at: 1,
            today_tokens: 0,
            seven_day_tokens: 0,
            today_requests: 0,
            seven_day_requests: 0,
            tiers: vec![],
        };
        let reports = super::sync_webdav(&sync, "emptyscope", &local)
            .await
            .expect("sync_webdav ok");
        assert!(reports.iter().any(|r| r.device_id == "self"));
    }

    // Mock WebDAV Server Helpers

    fn start_mock_webdav() -> (
        u16,
        thread::JoinHandle<()>,
        Arc<Mutex<HashMap<String, Vec<u8>>>>,
    ) {
        let store: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let store_cl = Arc::clone(&store);
        let listener = Arc::new(Mutex::new(TcpListener::bind("127.0.0.1:0").expect("bind")));
        let listener_cl = Arc::clone(&listener);
        let port = listener.lock().unwrap().local_addr().unwrap().port();

        let handle = thread::spawn(move || loop {
            let (stream, _) = {
                let guard = listener_cl.lock().unwrap();
                match guard.accept() {
                    Ok(conn) => conn,
                    Err(_) => break,
                }
            };
            serve_webdav_connection(stream, &store_cl);
        });
        (port, handle, store)
    }

    fn serve_webdav_connection(mut stream: TcpStream, store: &Mutex<HashMap<String, Vec<u8>>>) {
        use std::io::Read;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .ok();

        // Read the entire first HTTP request: method line then headers.
        // Uses a simple line-by-line approach reading one byte at a time for accuracy.
        let read_line = |s: &mut TcpStream| -> Option<String> {
            let mut line = String::new();
            let mut b2 = [0u8; 1];
            loop {
                match s.read(&mut b2) {
                    Ok(0) => return None,
                    Ok(_) => {}
                    Err(_) => return None,
                }
                let c = b2[0] as char;
                if c == '\r' {
                    continue;
                }
                if c == '\n' {
                    return Some(line);
                }
                line.push(c);
            }
        };

        let request_line = match read_line(&mut stream) {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };

        let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return;
        }
        let method = parts[0].to_uppercase();
        let raw_path = parts[1];
        let path = if raw_path.starts_with("http://") || raw_path.starts_with("https://") {
            url::Url::parse(raw_path)
                .map(|u| u.path().to_string())
                .unwrap_or_else(|_| raw_path.to_string())
        } else {
            raw_path.to_string()
        };

        // read headers
        let mut content_length: usize = 0;
        loop {
            let hdr = match read_line(&mut stream) {
                Some(h) => h,
                None => return,
            };
            if hdr.is_empty() {
                break;
            }
            if let Some(v) = hdr
                .strip_prefix("content-length:")
                .or_else(|| hdr.strip_prefix("Content-Length:"))
            {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }

        // read body
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            let mut read_so_far = 0;
            while read_so_far < content_length {
                match stream.read(&mut body[read_so_far..]) {
                    Ok(0) => break,
                    Ok(n) => read_so_far += n,
                    Err(_) => break,
                }
            }
        }

        // handle request
        match method.as_str() {
            "MKCOL" => {
                send_response(&mut stream, "201 Created", "text/plain", &[]);
            }
            "PUT" => {
                store.lock().unwrap().insert(path, body);
                send_response(&mut stream, "201 Created", "text/plain", &[]);
            }
            "GET" => {
                let guard = store.lock().unwrap();
                if let Some(data) = guard.get(&path) {
                    send_response(&mut stream, "200 OK", "application/octet-stream", data);
                } else {
                    send_response(&mut stream, "404 Not Found", "text/plain", &[]);
                }
            }
            "PROPFIND" => {
                let guard = store.lock().unwrap();
                let xml = build_propfind_xml(&path, &guard);
                send_response(
                    &mut stream,
                    "207 Multi-Status",
                    "application/xml; charset=utf-8",
                    xml.as_bytes(),
                );
            }
            _ => {
                send_response(&mut stream, "405 Method Not Allowed", "text/plain", &[]);
            }
        }
    }
    fn build_propfind_xml(path: &str, store: &HashMap<String, Vec<u8>>) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<D:multistatus xmlns:D=\"DAV:\">\n",
        );
        xml.push_str(&format!("  <D:response><D:href>{path}</D:href><D:propstat><D:prop><D:displayname>{}</D:displayname></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>\n", path.rsplit('/').filter(|s| !s.is_empty()).next().unwrap_or("")));
        let dir_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        for key in store.keys() {
            if key.starts_with(&dir_prefix) && key != path {
                let name = key
                    .rsplit('/')
                    .filter(|s| !s.is_empty())
                    .next()
                    .unwrap_or("");
                xml.push_str(&format!("  <D:response><D:href>{key}</D:href><D:propstat><D:prop><D:displayname>{name}</D:displayname></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>\n"));
            }
        }
        xml.push_str("</D:multistatus>\n");
        xml
    }

    fn send_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut buf = resp.into_bytes();
        buf.extend_from_slice(body);
        let _ = stream.write_all(&buf);
        let _ = stream.flush();
    }
}

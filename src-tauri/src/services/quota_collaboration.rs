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

    #[test]
    fn report_validation_rejects_out_of_range_utilization() {
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
}

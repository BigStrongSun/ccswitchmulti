//! Schema 定义和迁移
//!
//! 负责数据库表结构的创建和版本迁移。

use super::{lock_conn, Database, SCHEMA_VERSION};
use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Serialize)]
struct LegacySkillMigrationRow {
    directory: String,
    app_type: String,
}

impl Database {
    /// 创建所有数据库表
    pub(crate) fn create_tables(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::create_tables_on_conn(&conn)
    }

    /// 在指定连接上创建表（供迁移和测试使用）
    pub(crate) fn create_tables_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 1. Providers 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                is_current BOOLEAN NOT NULL DEFAULT 0,
                in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 2. Provider Endpoints 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS provider_endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider_id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                url TEXT NOT NULL,
                added_at INTEGER,
                FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 3. MCP Servers 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mcp_servers (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, server_config TEXT NOT NULL,
            description TEXT, homepage TEXT, docs TEXT, tags TEXT NOT NULL DEFAULT '[]',
            enabled_claude BOOLEAN NOT NULL DEFAULT 0, enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0, enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 4. Prompts 表
        conn.execute("CREATE TABLE IF NOT EXISTS prompts (
            id TEXT NOT NULL, app_type TEXT NOT NULL, name TEXT NOT NULL, content TEXT NOT NULL,
            description TEXT, enabled BOOLEAN NOT NULL DEFAULT 1, created_at INTEGER, updated_at INTEGER,
            PRIMARY KEY (id, app_type)
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 5. Skills 表（v3.10.0+ 统一结构）
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            directory TEXT NOT NULL,
            repo_owner TEXT,
            repo_name TEXT,
            repo_branch TEXT DEFAULT 'main',
            readme_url TEXT,
            enabled_claude BOOLEAN NOT NULL DEFAULT 0,
            enabled_codex BOOLEAN NOT NULL DEFAULT 0,
            enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
            enabled_grokbuild BOOLEAN NOT NULL DEFAULT 0,
            enabled_opencode BOOLEAN NOT NULL DEFAULT 0,
            enabled_hermes BOOLEAN NOT NULL DEFAULT 0,
            installed_at INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT,
            updated_at INTEGER NOT NULL DEFAULT 0
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 6. Skill Repos 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_repos (
            owner TEXT NOT NULL, name TEXT NOT NULL, branch TEXT NOT NULL DEFAULT 'main',
            enabled BOOLEAN NOT NULL DEFAULT 1, PRIMARY KEY (owner, name)
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 7. Settings 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 8. Proxy Config 表（三行结构，app_type 主键）
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_config (
            app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
            proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 初始化三行数据（每应用不同默认值）
        //
        // 兼容旧数据库：
        // - 老版本 proxy_config 是单例表（没有 app_type 列），此时不能执行三行 seed insert；
        // - 旧表会在 apply_schema_migrations() 中迁移为三行结构后再插入。
        if Self::has_column(conn, "proxy_config", "app_type")? {
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('claude', 6, 90, 180, 600, 8, 3, 90, 0.7, 15)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('codex', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('gemini', 5, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "INSERT OR IGNORE INTO proxy_config (app_type, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests)
                VALUES ('grokbuild', 3, 60, 120, 600, 4, 2, 60, 0.6, 10)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        // 9. Provider Health 表
        conn.execute("CREATE TABLE IF NOT EXISTS provider_health (
            provider_id TEXT NOT NULL, app_type TEXT NOT NULL, is_healthy INTEGER NOT NULL DEFAULT 1,
            consecutive_failures INTEGER NOT NULL DEFAULT 0, last_success_at TEXT, last_failure_at TEXT,
            last_error TEXT, updated_at TEXT NOT NULL,
            PRIMARY KEY (provider_id, app_type),
            FOREIGN KEY (provider_id, app_type) REFERENCES providers(id, app_type) ON DELETE CASCADE
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        // 10. Proxy Request Logs 表
        // pricing_model = 写入时实际用于计价的模型名（pricing_model_source 解析结果），
        // 回填按它重算；NULL 表示 v11 之前的历史行，'' 表示未计价的错误行。
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_request_logs (
            request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
            request_model TEXT,
            pricing_model TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            input_token_semantics INTEGER NOT NULL DEFAULT 0,
            input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
            cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
            total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
            provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
            cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL,
            data_source TEXT NOT NULL DEFAULT 'proxy'
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("CREATE INDEX IF NOT EXISTS idx_request_logs_provider ON proxy_request_logs(provider_id, app_type)", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON proxy_request_logs(created_at)", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_model ON proxy_request_logs(model)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_session ON proxy_request_logs(session_id)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_status ON proxy_request_logs(status_code)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Self::create_request_logs_usage_indexes_if_supported(conn)?;

        // 11. Model Pricing 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
            model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
            cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
            cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 12. Stream Check Logs 表
        conn.execute("CREATE TABLE IF NOT EXISTS stream_check_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, provider_id TEXT NOT NULL, provider_name TEXT NOT NULL,
            app_type TEXT NOT NULL, status TEXT NOT NULL, success INTEGER NOT NULL, message TEXT NOT NULL,
            response_time_ms INTEGER, http_status INTEGER, model_used TEXT,
            retry_count INTEGER DEFAULT 0, tested_at INTEGER NOT NULL
        )", []).map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_stream_check_logs_provider
             ON stream_check_logs(app_type, provider_id, tested_at DESC)",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Self::create_protocol_compatibility_tables(conn)?;
        Self::create_protocol_probe_observation_tables(conn)?;
        Self::create_reasoning_manual_override_tables(conn)?;

        // 注意：circuit_breaker_config 已合并到 proxy_config 表中

        // 16. Proxy Live Backup 表 (Live 配置备份)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proxy_live_backup (
            app_type TEXT PRIMARY KEY, original_config TEXT NOT NULL, backed_up_at TEXT NOT NULL
        )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 17. Usage Daily Rollups 表 (日聚合统计)
        // request_model 保留路由接管的「客户端别名 → 真实模型」映射维度，
        // pricing_model 保留写入时的计价基准（request 计价模式下与 model 分叉），
        // 否则明细被 prune 后接管计费不可审计；历史行迁移时填 ''（未知）。
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
                date TEXT NOT NULL,
                app_type TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                request_model TEXT NOT NULL DEFAULT '',
                pricing_model TEXT NOT NULL DEFAULT '',
                request_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                input_token_semantics INTEGER NOT NULL DEFAULT 0,
                total_cost_usd TEXT NOT NULL DEFAULT '0',
                avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 18. Session Log Sync 表 (会话日志同步状态)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_log_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 19. 多设备额度协作只缓存每设备最新的脱敏聚合报告。
        conn.execute(
            "CREATE TABLE IF NOT EXISTS quota_collaboration_reports (
                account_scope TEXT NOT NULL,
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                captured_at INTEGER NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY (account_scope, device_id)
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 20. 项目配置快照。与 v11 -> v12 迁移同时创建，兼容两条开发线
        // 都曾使用 schema v12 的数据库。
        conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                sort_order INTEGER,
                created_at INTEGER,
                updated_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        if conn
            .execute(
                "INSERT OR REPLACE INTO settings (key, value)
                 SELECT 'current_profile_id_claude', value FROM settings
                 WHERE key = 'current_profile_id'",
                [],
            )
            .is_ok()
        {
            let _ = conn.execute("DELETE FROM settings WHERE key = 'current_profile_id'", []);
        }

        // 尝试添加 live_takeover_active 列到 proxy_config 表
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN live_takeover_active INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // 尝试添加基础配置列到 proxy_config 表（兼容 v3.9.0-2 升级）
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN proxy_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN listen_address TEXT NOT NULL DEFAULT '127.0.0.1'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN listen_port INTEGER NOT NULL DEFAULT 15721",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN enable_logging INTEGER NOT NULL DEFAULT 1",
            [],
        );

        // 尝试添加超时配置列到 proxy_config 表
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN streaming_idle_timeout INTEGER NOT NULL DEFAULT 120",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE proxy_config ADD COLUMN non_streaming_timeout INTEGER NOT NULL DEFAULT 600",
            [],
        );

        // 兼容：若旧版 proxy_config 仍为单例结构（无 app_type），则在启动时直接转换为三行结构
        // 说明：user_version=2 时不会再触发 v1->v2 迁移，但新代码查询依赖 app_type 列。
        if Self::table_exists(conn, "proxy_config")?
            && !Self::has_column(conn, "proxy_config", "app_type")?
        {
            Self::migrate_proxy_config_to_per_app(conn)?;
        }

        // 确保 in_failover_queue 列存在（对于已存在的 v2 数据库）
        Self::add_column_if_missing(
            conn,
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 删除旧的 failover_queue 表（如果存在）
        let _ = conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", []);
        let _ = conn.execute("DROP TABLE IF EXISTS failover_queue", []);

        // 为故障转移队列创建索引（基于 providers 表）
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        );

        Ok(())
    }

    /// 应用 Schema 迁移
    pub(crate) fn apply_schema_migrations(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::apply_schema_migrations_on_conn(&conn)
    }

    /// 在指定连接上应用 Schema 迁移
    pub(crate) fn apply_schema_migrations_on_conn(conn: &Connection) -> Result<(), AppError> {
        conn.execute("SAVEPOINT schema_migration;", [])
            .map_err(|e| AppError::Database(format!("开启迁移 savepoint 失败: {e}")))?;

        let mut version = Self::get_user_version(conn)?;

        if version > SCHEMA_VERSION {
            conn.execute("ROLLBACK TO schema_migration;", []).ok();
            conn.execute("RELEASE schema_migration;", []).ok();
            return Err(AppError::Database(format!(
                "数据库版本过新（{version}），当前应用仅支持 {SCHEMA_VERSION}，请升级应用后再尝试。"
            )));
        }

        let result = (|| {
            while version < SCHEMA_VERSION {
                match version {
                    0 => {
                        log::info!("检测到 user_version=0，迁移到 1（补齐缺失列并设置版本）");
                        Self::migrate_v0_to_v1(conn)?;
                        Self::set_user_version(conn, 1)?;
                    }
                    1 => {
                        log::info!(
                            "迁移数据库从 v1 到 v2（添加使用统计表和完整字段，重构 skills 表）"
                        );
                        Self::migrate_v1_to_v2(conn)?;
                        Self::set_user_version(conn, 2)?;
                    }
                    2 => {
                        log::info!("迁移数据库从 v2 到 v3（Skills 统一管理架构）");
                        Self::migrate_v2_to_v3(conn)?;
                        Self::set_user_version(conn, 3)?;
                    }
                    3 => {
                        log::info!("迁移数据库从 v3 到 v4（OpenCode 支持）");
                        Self::migrate_v3_to_v4(conn)?;
                        Self::set_user_version(conn, 4)?;
                    }
                    4 => {
                        log::info!("迁移数据库从 v4 到 v5（计费模式支持）");
                        Self::migrate_v4_to_v5(conn)?;
                        Self::set_user_version(conn, 5)?;
                    }
                    5 => {
                        log::info!("迁移数据库从 v5 到 v6（使用量聚合表 + Copilot 模板类型统一）");
                        Self::migrate_v5_to_v6(conn)?;
                        Self::set_user_version(conn, 6)?;
                    }
                    6 => {
                        log::info!("迁移数据库从 v6 到 v7（Skills 更新检测支持）");
                        Self::migrate_v6_to_v7(conn)?;
                        Self::set_user_version(conn, 7)?;
                    }
                    7 => {
                        log::info!("迁移数据库从 v7 到 v8（会话日志使用追踪 + 修正模型定价）");
                        Self::migrate_v7_to_v8(conn)?;
                        Self::set_user_version(conn, 8)?;
                    }
                    8 => {
                        log::info!("迁移数据库从 v8 到 v9（全面补充模型定价）");
                        Self::migrate_v8_to_v9(conn)?;
                        Self::set_user_version(conn, 9)?;
                    }
                    9 => {
                        log::info!("迁移数据库从 v9 到 v10（添加 Hermes Agent 支持）");
                        Self::migrate_v9_to_v10(conn)?;
                        Self::set_user_version(conn, 10)?;
                    }
                    10 => {
                        log::info!("迁移数据库从 v10 到 v11（usage_daily_rollups 保留 request_model 维度）");
                        Self::migrate_v10_to_v11(conn)?;
                        Self::set_user_version(conn, 11)?;
                    }
                    11 => {
                        log::info!("迁移数据库从 v11 到 v12（添加多设备额度协作报告缓存）");
                        Self::migrate_v11_to_v12(conn)?;
                        Self::set_user_version(conn, 12)?;
                    }
                    12 => {
                        log::info!("迁移数据库从 v12 到 v13（记录输入 token 缓存语义）");
                        Self::migrate_v12_to_v13(conn)?;
                        Self::set_user_version(conn, 13)?;
                    }
                    13 => {
                        log::info!("迁移数据库从 v13 到 v14（添加 Grok Build 代理配置）");
                        Self::migrate_v13_to_v14(conn)?;
                        Self::set_user_version(conn, 14)?;
                    }
                    14 => {
                        log::info!("迁移数据库从 v14 到 v15（Skills/MCP 添加 Grok Build 支持）");
                        Self::migrate_v14_to_v15(conn)?;
                        Self::set_user_version(conn, 15)?;
                    }
                    15 => {
                        log::info!("迁移数据库从 v15 到 v16（重建 Codex 会话用量）");
                        Self::migrate_v15_to_v16(conn)?;
                        Self::set_user_version(conn, 16)?;
                    }
                    16 => {
                        log::info!("迁移数据库从 v16 到 v17（添加协议兼容性探测档案）");
                        Self::migrate_v16_to_v17(conn)?;
                        Self::set_user_version(conn, 17)?;
                    }
                    17 => {
                        log::info!("迁移数据库从 v17 到 v18（添加 Codex reasoning 手工覆盖）");
                        Self::migrate_v17_to_v18(conn)?;
                        Self::set_user_version(conn, 18)?;
                    }
                    18 => {
                        log::info!("迁移数据库从 v18 到 v19（添加分协议探测证据）");
                        Self::migrate_v18_to_v19(conn)?;
                        Self::set_user_version(conn, 19)?;
                    }
                    _ => {
                        return Err(AppError::Database(format!(
                            "未知的数据库版本 {version}，无法迁移到 {SCHEMA_VERSION}"
                        )));
                    }
                }
                version = Self::get_user_version(conn)?;
            }
            Ok(())
        })();

        match result {
            Ok(_) => {
                conn.execute("RELEASE schema_migration;", [])
                    .map_err(|e| AppError::Database(format!("提交迁移 savepoint 失败: {e}")))?;
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK TO schema_migration;", []).ok();
                conn.execute("RELEASE schema_migration;", []).ok();
                Err(e)
            }
        }
    }

    /// v0 -> v1 迁移：补齐所有缺失列
    fn migrate_v0_to_v1(conn: &Connection) -> Result<(), AppError> {
        // providers 表
        Self::add_column_if_missing(conn, "providers", "category", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "sort_index", "INTEGER")?;
        Self::add_column_if_missing(conn, "providers", "notes", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "icon_color", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "meta", "TEXT NOT NULL DEFAULT '{}'")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "is_current",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // provider_endpoints 表
        Self::add_column_if_missing(conn, "provider_endpoints", "added_at", "INTEGER")?;

        // mcp_servers 表
        Self::add_column_if_missing(conn, "mcp_servers", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "homepage", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "docs", "TEXT")?;
        Self::add_column_if_missing(conn, "mcp_servers", "tags", "TEXT NOT NULL DEFAULT '[]'")?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_codex",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_gemini",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // prompts 表
        Self::add_column_if_missing(conn, "prompts", "description", "TEXT")?;
        Self::add_column_if_missing(conn, "prompts", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        Self::add_column_if_missing(conn, "prompts", "created_at", "INTEGER")?;
        Self::add_column_if_missing(conn, "prompts", "updated_at", "INTEGER")?;

        // skills 表
        Self::add_column_if_missing(conn, "skills", "installed_at", "INTEGER NOT NULL DEFAULT 0")?;

        // skill_repos 表
        Self::add_column_if_missing(
            conn,
            "skill_repos",
            "branch",
            "TEXT NOT NULL DEFAULT 'main'",
        )?;
        Self::add_column_if_missing(conn, "skill_repos", "enabled", "BOOLEAN NOT NULL DEFAULT 1")?;
        // 注意: skills_path 字段已被移除，因为现在支持全仓库递归扫描

        Ok(())
    }

    /// v1 -> v2 迁移：添加使用统计表和完整字段，重构 skills 表
    fn migrate_v1_to_v2(conn: &Connection) -> Result<(), AppError> {
        // providers 表字段
        Self::add_column_if_missing(
            conn,
            "providers",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "providers", "limit_daily_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "limit_monthly_usd", "TEXT")?;
        Self::add_column_if_missing(conn, "providers", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "providers",
            "in_failover_queue",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 添加代理超时配置字段
        if Self::table_exists(conn, "proxy_config")? {
            // 兼容旧版本缺失的基础字段
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "proxy_enabled",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_address",
                "TEXT NOT NULL DEFAULT '127.0.0.1'",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "listen_port",
                "INTEGER NOT NULL DEFAULT 15721",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "enable_logging",
                "INTEGER NOT NULL DEFAULT 1",
            )?;

            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_first_byte_timeout",
                "INTEGER NOT NULL DEFAULT 60",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "streaming_idle_timeout",
                "INTEGER NOT NULL DEFAULT 120",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "non_streaming_timeout",
                "INTEGER NOT NULL DEFAULT 600",
            )?;
        }

        // 删除旧的 failover_queue 表（如果存在）
        conn.execute("DROP INDEX IF EXISTS idx_failover_queue_order", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 索引失败: {e}")))?;
        conn.execute("DROP TABLE IF EXISTS failover_queue", [])
            .map_err(|e| AppError::Database(format!("删除 failover_queue 表失败: {e}")))?;

        // 创建 failover 索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_providers_failover
             ON providers(app_type, in_failover_queue, sort_index)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 failover 索引失败: {e}")))?;

        // proxy_request_logs 表
        conn.execute("CREATE TABLE IF NOT EXISTS proxy_request_logs (
            request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
            request_model TEXT,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            input_token_semantics INTEGER NOT NULL DEFAULT 0,
            input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
            cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
            total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
            provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
            cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL
        )", [])?;

        // 为已存在的表添加新字段
        Self::add_column_if_missing(conn, "proxy_request_logs", "provider_type", "TEXT")?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "is_streaming",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(
            conn,
            "proxy_request_logs",
            "cost_multiplier",
            "TEXT NOT NULL DEFAULT '1.0'",
        )?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "first_token_ms", "INTEGER")?;
        Self::add_column_if_missing(conn, "proxy_request_logs", "duration_ms", "INTEGER")?;

        // model_pricing 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
            model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
            cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
            cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
        )",
            [],
        )?;

        // 清空并重新插入模型定价
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;

        // 重构 skills 表（添加 app_type 字段）
        Self::migrate_skills_table(conn)?;

        // 重构 proxy_config 为三行结构（每应用独立配置）
        Self::migrate_proxy_config_to_per_app(conn)?;

        Ok(())
    }

    /// 将 proxy_config 迁移为三行结构（每应用独立配置）
    fn migrate_proxy_config_to_per_app(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新表结构（幂等性）
        if !Self::table_exists(conn, "proxy_config")? {
            // 表不存在，跳过迁移（新安装）
            return Ok(());
        }

        if Self::has_column(conn, "proxy_config", "app_type")? {
            // 已经是三行结构，跳过迁移
            log::info!("proxy_config 已经是三行结构，跳过迁移");
            return Ok(());
        }

        // 读取旧配置
        let old_config = conn
            .query_row(
                "SELECT listen_address, listen_port, max_retries, enable_logging,
                    streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout
             FROM proxy_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i32>(3)?,
                        row.get::<_, i32>(4).unwrap_or(30),
                        row.get::<_, i32>(5).unwrap_or(60),
                        row.get::<_, i32>(6).unwrap_or(300),
                    ))
                },
            )
            .unwrap_or_else(|_| ("127.0.0.1".to_string(), 5000, 3, 1, 30, 60, 300));

        let old_cb = conn.query_row(
            "SELECT failure_threshold, success_threshold, timeout_seconds, error_rate_threshold, min_requests
             FROM circuit_breaker_config WHERE id = 1", [],
            |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?, row.get::<_, i64>(2)?,
                      row.get::<_, f64>(3)?, row.get::<_, i32>(4)?))
        ).unwrap_or((5, 2, 60, 0.5, 10));

        let get_bool = |key: &str| -> bool {
            conn.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| {
                r.get::<_, String>(0)
            })
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        };

        let apps = [
            (
                "claude",
                get_bool("proxy_takeover_claude"),
                get_bool("auto_failover_enabled_claude"),
                6,
                45,
                90,
                8,
                3,
                90,
                0.6,
                15,
            ),
            (
                "codex",
                get_bool("proxy_takeover_codex"),
                get_bool("auto_failover_enabled_codex"),
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "gemini",
                get_bool("proxy_takeover_gemini"),
                get_bool("auto_failover_enabled_gemini"),
                5,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
            (
                "grokbuild",
                false,
                false,
                3,
                old_config.4,
                old_config.5,
                old_cb.0,
                old_cb.1,
                old_cb.2,
                old_cb.3,
                old_cb.4,
            ),
        ];

        // 创建新表
        conn.execute("DROP TABLE IF EXISTS proxy_config_new", [])?;
        conn.execute("CREATE TABLE proxy_config_new (
            app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
            proxy_enabled INTEGER NOT NULL DEFAULT 0, listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
            listen_port INTEGER NOT NULL DEFAULT 15721, enable_logging INTEGER NOT NULL DEFAULT 1,
            enabled INTEGER NOT NULL DEFAULT 0, auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3, streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
            streaming_idle_timeout INTEGER NOT NULL DEFAULT 120, non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
            circuit_failure_threshold INTEGER NOT NULL DEFAULT 4, circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
            circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60, circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
            circuit_min_requests INTEGER NOT NULL DEFAULT 10,
            default_cost_multiplier TEXT NOT NULL DEFAULT '1',
            pricing_model_source TEXT NOT NULL DEFAULT 'response',
            live_takeover_active INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')), updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )", [])?;

        // 插入三行配置
        for (app, takeover, failover, retries, fb, idle, cb_f, cb_s, cb_t, cb_r, cb_m) in apps {
            conn.execute(
                "INSERT INTO proxy_config_new (app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                 enabled, auto_failover_enabled, max_retries, streaming_first_byte_timeout, streaming_idle_timeout,
                 non_streaming_timeout, circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                 circuit_error_rate_threshold, circuit_min_requests)
                 VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![app, old_config.0, old_config.1, old_config.3,
                    if takeover { 1 } else { 0 }, if failover { 1 } else { 0 },
                    retries, fb, idle, old_config.6, cb_f, cb_s, cb_t, cb_r, cb_m]
            ).map_err(|e| AppError::Database(format!("插入 {app} 配置失败: {e}")))?;
        }

        // 替换表并清理
        conn.execute("DROP TABLE IF EXISTS proxy_config", [])?;
        conn.execute("ALTER TABLE proxy_config_new RENAME TO proxy_config", [])?;
        conn.execute("DROP TABLE IF EXISTS circuit_breaker_config", [])?;
        conn.execute("DELETE FROM settings WHERE key LIKE 'proxy_takeover_%'", [])?;
        conn.execute(
            "DELETE FROM settings WHERE key LIKE 'auto_failover_enabled_%'",
            [],
        )?;

        log::info!("proxy_config 已迁移为三行结构");
        Ok(())
    }

    /// 迁移 skills 表：从单 key 主键改为 (directory, app_type) 复合主键
    fn migrate_skills_table(conn: &Connection) -> Result<(), AppError> {
        // v3 结构（统一管理架构）已经是更高版本的 skills 表：
        // - 主键为 id
        // - 包含 enabled_claude / enabled_codex / enabled_gemini 等列
        // 在这种情况下，不应再执行 v1 -> v2 的迁移逻辑，否则会因列不匹配而失败。
        if Self::has_column(conn, "skills", "enabled_claude")?
            || Self::has_column(conn, "skills", "id")?
        {
            log::info!("skills 表已经是 v3 结构，跳过 v1 -> v2 迁移");
            return Ok(());
        }

        // 检查是否已经是新表结构
        if Self::has_column(conn, "skills", "app_type")? {
            log::info!("skills 表已经包含 app_type 字段，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表...");

        // 1. 重命名旧表
        conn.execute("ALTER TABLE skills RENAME TO skills_old", [])
            .map_err(|e| AppError::Database(format!("重命名旧 skills 表失败: {e}")))?;

        // 2. 创建新表
        conn.execute(
            "CREATE TABLE skills (
                directory TEXT NOT NULL,
                app_type TEXT NOT NULL,
                installed BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (directory, app_type)
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        // 3. 迁移数据：解析 key 格式（如 "claude:my-skill" 或 "codex:foo"）
        //    旧数据如果没有前缀，默认为 claude
        let mut stmt = conn
            .prepare("SELECT key, installed, installed_at FROM skills_old")
            .map_err(|e| AppError::Database(format!("查询旧 skills 数据失败: {e}")))?;

        let old_skills: Vec<(String, bool, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 数据失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 数据失败: {e}")))?;

        let count = old_skills.len();

        for (key, installed, installed_at) in old_skills {
            // 解析 key: "app:directory" 或 "directory"（默认 claude）
            let (app_type, directory) = if let Some(idx) = key.find(':') {
                let (app, dir) = key.split_at(idx);
                (app.to_string(), dir[1..].to_string()) // 跳过冒号
            } else {
                ("claude".to_string(), key.clone())
            };

            conn.execute(
                "INSERT INTO skills (directory, app_type, installed, installed_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![directory, app_type, installed, installed_at],
            )
            .map_err(|e| {
                AppError::Database(format!("迁移 skill {key} 到新表失败: {e}"))
            })?;
        }

        // 4. 删除旧表
        conn.execute("DROP TABLE skills_old", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        log::info!("skills 表迁移完成，共迁移 {count} 条记录");
        Ok(())
    }

    /// v2 -> v3 迁移：Skills 统一管理架构
    ///
    /// 将 skills 表从 (directory, app_type) 复合主键结构迁移到统一的 id 主键结构，
    /// 支持三应用启用标志（enabled_claude, enabled_codex, enabled_gemini）。
    ///
    /// 迁移策略：
    /// 1. 旧数据库只存储安装记录，真正的 skill 文件在文件系统
    /// 2. 直接重建新表结构，后续由 SkillService 在首次启动时扫描文件系统重建数据
    fn migrate_v2_to_v3(conn: &Connection) -> Result<(), AppError> {
        // 检查是否已经是新结构（通过检查是否有 enabled_claude 列）
        if Self::has_column(conn, "skills", "enabled_claude")? {
            log::info!("skills 表已经是 v3 结构，跳过迁移");
            return Ok(());
        }

        log::info!("开始迁移 skills 表到 v3 结构（统一管理架构）...");

        // 1. 备份旧数据（用于日志和后续启动迁移）
        let old_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
            .unwrap_or(0);
        log::info!("旧 skills 表有 {old_count} 条记录");

        let mut stmt = conn
            .prepare(
                "SELECT directory, app_type FROM skills
                 WHERE installed = 1",
            )
            .map_err(|e| AppError::Database(format!("查询旧 skills 快照失败: {e}")))?;
        let snapshot_rows: Vec<LegacySkillMigrationRow> = stmt
            .query_map([], |row| {
                Ok(LegacySkillMigrationRow {
                    directory: row.get(0)?,
                    app_type: row.get(1)?,
                })
            })
            .map_err(|e| AppError::Database(format!("读取旧 skills 快照失败: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("解析旧 skills 快照失败: {e}")))?;
        let snapshot_json = serde_json::to_string(&snapshot_rows)
            .map_err(|e| AppError::Database(format!("序列化旧 skills 快照失败: {e}")))?;

        // 标记：需要在启动后从文件系统扫描并重建 Skills 数据
        // 说明：v3 结构将 Skills 的 SSOT 迁移到 ~/.cc-switch/skills/，
        // 旧表只存“安装记录”，无法直接无损迁移到新结构，因此改为启动后扫描 app 目录导入。
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_pending', 'true')",
            [],
        );
        let _ = conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('skills_ssot_migration_snapshot', ?1)",
            [snapshot_json],
        );

        // 2. 删除旧表
        conn.execute("DROP TABLE IF EXISTS skills", [])
            .map_err(|e| AppError::Database(format!("删除旧 skills 表失败: {e}")))?;

        // 3. 创建新表
        conn.execute(
            "CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                directory TEXT NOT NULL,
                repo_owner TEXT,
                repo_name TEXT,
                repo_branch TEXT DEFAULT 'main',
                readme_url TEXT,
                enabled_claude BOOLEAN NOT NULL DEFAULT 0,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0,
                enabled_gemini BOOLEAN NOT NULL DEFAULT 0,
                installed_at INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建新 skills 表失败: {e}")))?;

        log::info!(
            "skills 表已迁移到 v3 结构。\n\
             注意：旧的安装记录已清除，首次启动时将自动扫描文件系统重建数据。"
        );

        Ok(())
    }

    /// v3 -> v4 迁移：添加 OpenCode 支持
    ///
    /// 为 mcp_servers 和 skills 表添加 enabled_opencode 列。
    fn migrate_v3_to_v4(conn: &Connection) -> Result<(), AppError> {
        // 为 mcp_servers 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // 为 skills 表添加 enabled_opencode 列
        Self::add_column_if_missing(
            conn,
            "skills",
            "enabled_opencode",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        log::info!("v3 -> v4 迁移完成：已添加 OpenCode 支持");
        Ok(())
    }

    /// v4 -> v5 迁移：新增计费模式配置与请求模型字段
    fn migrate_v4_to_v5(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_config")? {
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "default_cost_multiplier",
                "TEXT NOT NULL DEFAULT '1'",
            )?;
            Self::add_column_if_missing(
                conn,
                "proxy_config",
                "pricing_model_source",
                "TEXT NOT NULL DEFAULT 'response'",
            )?;
        }
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "request_model", "TEXT")?;
        }

        log::info!("v4 -> v5 迁移完成：已添加计费模式与请求模型字段");
        Ok(())
    }

    /// v5 -> v6 迁移：添加使用量日聚合表 + 统一 Copilot 模板类型
    fn migrate_v5_to_v6(conn: &Connection) -> Result<(), AppError> {
        // 1. 添加使用量日聚合表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage_daily_rollups (
                date TEXT NOT NULL,
                app_type TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                request_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                total_cost_usd TEXT NOT NULL DEFAULT '0',
                avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, app_type, provider_id, model)
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 usage_daily_rollups 表失败: {e}")))?;

        // 2. 统一 Copilot 模板类型为 github_copilot
        let mut stmt = conn
            .prepare("SELECT id, app_type, meta FROM providers")
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut updates = Vec::new();
        for row in rows {
            let (id, app_type, meta_str) = row.map_err(|e| AppError::Database(e.to_string()))?;

            if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&meta_str) {
                let mut updated = false;

                if let Some(usage_script) = meta.get_mut("usage_script") {
                    if let Some(template_type) = usage_script.get_mut("template_type") {
                        if template_type == "copilot" {
                            *template_type =
                                serde_json::Value::String("github_copilot".to_string());
                            updated = true;
                        }
                    }
                }

                if updated {
                    let new_meta_str = serde_json::to_string(&meta)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    updates.push((id, app_type, new_meta_str));
                }
            }
        }

        for (id, app_type, new_meta) in updates {
            conn.execute(
                "UPDATE providers SET meta = ?1 WHERE id = ?2 AND app_type = ?3",
                params![new_meta, id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        log::info!("v5 -> v6 迁移完成：已添加使用量日聚合表，统一 copilot 模板类型");
        Ok(())
    }

    /// v6 -> v7: Skills 更新检测支持（content_hash + updated_at）
    fn migrate_v6_to_v7(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(conn, "skills", "content_hash", "TEXT")?;
            Self::add_column_if_missing(
                conn,
                "skills",
                "updated_at",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        log::info!("v6 -> v7 迁移完成：已添加 content_hash 和 updated_at 列");
        Ok(())
    }

    /// v7 -> v8: 会话日志使用追踪（无代理模式统计支持）
    fn migrate_v7_to_v8(conn: &Connection) -> Result<(), AppError> {
        // 1. 为 proxy_request_logs 添加 data_source 列，区分数据来源
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "data_source",
                "TEXT NOT NULL DEFAULT 'proxy'",
            )?;
            Self::create_request_logs_usage_indexes_if_supported(conn)?;
        }

        // 2. 创建会话日志同步状态表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_log_sync (
                file_path TEXT PRIMARY KEY,
                last_modified INTEGER NOT NULL,
                last_line_offset INTEGER NOT NULL DEFAULT 0,
                last_synced_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 session_log_sync 表失败: {e}")))?;

        // 3. 修正国产模型定价：之前误将 CNY 值存为 USD 字段，统一转换为 USD
        if Self::table_exists(conn, "model_pricing")? {
            let pricing_fixes: &[(&str, &str, &str, &str, &str)] = &[
                ("deepseek-v3.2", "0.28", "0.42", "0.028", "0"),
                ("deepseek-v3.1", "0.55", "1.67", "0.055", "0"),
                ("deepseek-v3", "0.28", "1.11", "0.028", "0"),
                ("doubao-seed-code", "0.17", "1.11", "0.02", "0"),
                ("kimi-k2-thinking", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-0905", "0.55", "2.20", "0.10", "0"),
                ("kimi-k2-turbo", "1.11", "8.06", "0.14", "0"),
                ("minimax-m2.1", "0.27", "0.95", "0.03", "0"),
                ("minimax-m2.1-lightning", "0.27", "2.33", "0.03", "0"),
                ("minimax-m2", "0.27", "0.95", "0.03", "0"),
                ("glm-4.7", "0.39", "1.75", "0.04", "0"),
                ("glm-4.6", "0.28", "1.11", "0.03", "0"),
                ("mimo-v2-flash", "0.09", "0.29", "0.009", "0"),
            ];
            for (model_id, input, output, cache_read, cache_creation) in pricing_fixes {
                conn.execute(
                    "UPDATE model_pricing SET
                        input_cost_per_million = ?2,
                        output_cost_per_million = ?3,
                        cache_read_cost_per_million = ?4,
                        cache_creation_cost_per_million = ?5
                     WHERE model_id = ?1",
                    rusqlite::params![model_id, input, output, cache_read, cache_creation],
                )
                .map_err(|e| AppError::Database(format!("更新模型 {model_id} 定价失败: {e}")))?;
            }
        }

        log::info!("v7 -> v8 迁移完成：data_source 列、session_log_sync 表、修正 13 个模型定价");
        Ok(())
    }

    /// v8 → v9: 全面补充模型定价（清空 + 重新 seed）
    fn migrate_v8_to_v9(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS model_pricing (
                model_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
                input_cost_per_million TEXT NOT NULL, output_cost_per_million TEXT NOT NULL,
                cache_read_cost_per_million TEXT NOT NULL DEFAULT '0',
                cache_creation_cost_per_million TEXT NOT NULL DEFAULT '0'
            )",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建 model_pricing 表失败: {e}")))?;
        conn.execute("DELETE FROM model_pricing", [])
            .map_err(|e| AppError::Database(format!("清空模型定价失败: {e}")))?;
        Self::seed_model_pricing(conn)?;
        log::info!("v8 -> v9 迁移完成：已刷新全部模型定价数据");
        Ok(())
    }

    /// v9 -> v10 迁移：添加 Hermes Agent 支持
    fn migrate_v9_to_v10(conn: &Connection) -> Result<(), AppError> {
        Self::add_column_if_missing(
            conn,
            "mcp_servers",
            "enabled_hermes",
            "BOOLEAN NOT NULL DEFAULT 0",
        )?;

        // skills table may not exist in databases migrated from very old versions
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_hermes",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }

        log::info!("v9 -> v10 迁移完成：已添加 Hermes Agent 支持");
        Ok(())
    }

    /// v10 -> v11：usage_daily_rollups 增加 request_model 维度（进入主键），
    /// proxy_request_logs 增加 pricing_model 列（写入时的计价基准，回填依据）。
    ///
    /// 路由接管下 model（真实上游模型）≠ request_model（客户端别名），
    /// 旧 rollup 只按 model 聚合，明细 prune 后映射关系永久丢失、计费不可审计。
    /// SQLite 改主键必须重建表；历史行的 request_model 已不可知，填 ''。
    fn migrate_v10_to_v11(conn: &Connection) -> Result<(), AppError> {
        // proxy_request_logs.pricing_model：NULL = v11 前的历史行（回填走
        // model → 占位符回退 request_model 的旧逻辑），'' = 未计价的错误行
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(conn, "proxy_request_logs", "pricing_model", "TEXT")?;
        }

        if !Self::table_exists(conn, "usage_daily_rollups")? {
            log::info!("v10 -> v11：usage_daily_rollups 不存在，跳过重建");
            return Ok(());
        }

        conn.execute_batch(
            "ALTER TABLE usage_daily_rollups RENAME TO usage_daily_rollups_v10;
             CREATE TABLE usage_daily_rollups (
                 date TEXT NOT NULL,
                 app_type TEXT NOT NULL,
                 provider_id TEXT NOT NULL,
                 model TEXT NOT NULL,
                 request_model TEXT NOT NULL DEFAULT '',
                 pricing_model TEXT NOT NULL DEFAULT '',
                 request_count INTEGER NOT NULL DEFAULT 0,
                 success_count INTEGER NOT NULL DEFAULT 0,
                 input_tokens INTEGER NOT NULL DEFAULT 0,
                 output_tokens INTEGER NOT NULL DEFAULT 0,
                 cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                 cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                 total_cost_usd TEXT NOT NULL DEFAULT '0',
                 avg_latency_ms INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
             );
             INSERT INTO usage_daily_rollups
                 (date, app_type, provider_id, model, request_model, pricing_model,
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms)
             SELECT date, app_type, provider_id, model, '', '',
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
             FROM usage_daily_rollups_v10;
             DROP TABLE usage_daily_rollups_v10;",
        )
        .map_err(|e| {
            AppError::Database(format!("v10 -> v11 重建 usage_daily_rollups 失败: {e}"))
        })?;

        log::info!(
            "v10 -> v11 迁移完成：usage_daily_rollups 已保留 request_model/pricing_model 维度"
        );
        Ok(())
    }

    /// v11 -> v12：创建协作缓存和项目快照表。
    fn migrate_v11_to_v12(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS quota_collaboration_reports (
                account_scope TEXT NOT NULL,
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                captured_at INTEGER NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY (account_scope, device_id)
            )",
            [],
        )
        .map_err(|error| AppError::Database(format!("v11 -> v12 创建协作表失败: {error}")))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                payload TEXT NOT NULL,
                sort_order INTEGER,
                created_at INTEGER,
                updated_at INTEGER
            )",
            [],
        )
        .map_err(|error| AppError::Database(format!("v11 -> v12 创建 profiles 表失败: {error}")))?;
        Ok(())
    }

    /// v12 -> v13：记录 input_tokens 是否包含缓存写入。
    ///
    /// 默认 0 表示旧版或未知语义。该迁移保持字段兼容，使曾被上游 v3.17.0
    /// 升级为 v13 的数据库可由 CCSwitchMulti 安全打开。
    fn migrate_v12_to_v13(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "proxy_request_logs")? {
            Self::add_column_if_missing(
                conn,
                "proxy_request_logs",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "usage_daily_rollups")? {
            Self::add_column_if_missing(
                conn,
                "usage_daily_rollups",
                "input_token_semantics",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v13 -> v14: allow Grok Build to own an independent proxy configuration row.
    fn migrate_v13_to_v14(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_config")? {
            return Ok(());
        }

        conn.execute("DROP TABLE IF EXISTS proxy_config_v14", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "CREATE TABLE proxy_config_v14 (
                app_type TEXT PRIMARY KEY CHECK (app_type IN ('claude','codex','gemini','grokbuild')),
                proxy_enabled INTEGER NOT NULL DEFAULT 0,
                listen_address TEXT NOT NULL DEFAULT '127.0.0.1',
                listen_port INTEGER NOT NULL DEFAULT 15721,
                enable_logging INTEGER NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 0,
                auto_failover_enabled INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 3,
                streaming_first_byte_timeout INTEGER NOT NULL DEFAULT 60,
                streaming_idle_timeout INTEGER NOT NULL DEFAULT 120,
                non_streaming_timeout INTEGER NOT NULL DEFAULT 600,
                circuit_failure_threshold INTEGER NOT NULL DEFAULT 4,
                circuit_success_threshold INTEGER NOT NULL DEFAULT 2,
                circuit_timeout_seconds INTEGER NOT NULL DEFAULT 60,
                circuit_error_rate_threshold REAL NOT NULL DEFAULT 0.6,
                circuit_min_requests INTEGER NOT NULL DEFAULT 10,
                default_cost_multiplier TEXT NOT NULL DEFAULT '1',
                pricing_model_source TEXT NOT NULL DEFAULT 'response',
                live_takeover_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        let copied_columns = [
            ("app_type", "'claude'"),
            ("proxy_enabled", "0"),
            ("listen_address", "'127.0.0.1'"),
            ("listen_port", "15721"),
            ("enable_logging", "1"),
            ("enabled", "0"),
            ("auto_failover_enabled", "0"),
            ("max_retries", "3"),
            ("streaming_first_byte_timeout", "60"),
            ("streaming_idle_timeout", "120"),
            ("non_streaming_timeout", "600"),
            ("circuit_failure_threshold", "4"),
            ("circuit_success_threshold", "2"),
            ("circuit_timeout_seconds", "60"),
            ("circuit_error_rate_threshold", "0.6"),
            ("circuit_min_requests", "10"),
            ("default_cost_multiplier", "'1'"),
            ("pricing_model_source", "'response'"),
            ("live_takeover_active", "0"),
            ("created_at", "datetime('now')"),
            ("updated_at", "datetime('now')"),
        ]
        .into_iter()
        .map(|(column, fallback)| {
            Self::has_column(conn, "proxy_config", column).map(|exists| {
                if exists {
                    format!("\"{column}\"")
                } else {
                    fallback.into()
                }
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

        let copy_sql = format!(
            "INSERT INTO proxy_config_v14 (
                app_type, proxy_enabled, listen_address, listen_port, enable_logging,
                enabled, auto_failover_enabled, max_retries,
                streaming_first_byte_timeout, streaming_idle_timeout, non_streaming_timeout,
                circuit_failure_threshold, circuit_success_threshold, circuit_timeout_seconds,
                circuit_error_rate_threshold, circuit_min_requests,
                default_cost_multiplier, pricing_model_source, live_takeover_active,
                created_at, updated_at
            )
            SELECT {copied_columns} FROM proxy_config"
        );
        conn.execute(&copy_sql, [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        conn.execute("DROP TABLE proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("ALTER TABLE proxy_config_v14 RENAME TO proxy_config", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT OR IGNORE INTO proxy_config (app_type) VALUES ('grokbuild')",
            [],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// v14 -> v15: persist Grok Build enablement for unified Skills and MCP.
    fn migrate_v14_to_v15(conn: &Connection) -> Result<(), AppError> {
        if Self::table_exists(conn, "mcp_servers")? {
            Self::add_column_if_missing(
                conn,
                "mcp_servers",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        if Self::table_exists(conn, "skills")? {
            Self::add_column_if_missing(
                conn,
                "skills",
                "enabled_grokbuild",
                "BOOLEAN NOT NULL DEFAULT 0",
            )?;
        }
        Ok(())
    }

    /// v15 -> v16: remove Codex session rows and cursors so startup sync can
    /// rebuild them with fork-history alignment. Must stay connection-level:
    /// schema migration already owns the Database connection mutex.
    fn migrate_v15_to_v16(conn: &Connection) -> Result<(), AppError> {
        let codex_dir = crate::codex_config::get_codex_config_dir();
        crate::services::session_usage_codex::reset_codex_usage_on_conn(conn, &codex_dir)
    }

    fn migrate_v16_to_v17(conn: &Connection) -> Result<(), AppError> {
        Self::create_protocol_compatibility_tables(conn)
    }

    fn migrate_v17_to_v18(conn: &Connection) -> Result<(), AppError> {
        Self::create_reasoning_manual_override_tables(conn)
    }

    fn migrate_v18_to_v19(conn: &Connection) -> Result<(), AppError> {
        Self::create_protocol_probe_observation_tables(conn)
    }

    fn create_protocol_compatibility_tables(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS protocol_compatibility_profiles (
                target_key TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                route_id TEXT,
                public_model TEXT NOT NULL,
                upstream_model TEXT NOT NULL,
                transport TEXT NOT NULL,
                endpoint_fingerprint TEXT NOT NULL,
                authentication_kind TEXT NOT NULL,
                readiness TEXT NOT NULL,
                profile_json TEXT NOT NULL,
                tested_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_protocol_compatibility_provider
             ON protocol_compatibility_profiles(provider_id, route_id, public_model, tested_at DESC);
             CREATE INDEX IF NOT EXISTS idx_protocol_compatibility_expiry
             ON protocol_compatibility_profiles(expires_at);",
        )
        .map_err(|error| {
            AppError::Database(format!("创建 protocol_compatibility_profiles 失败: {error}"))
        })
    }

    fn create_protocol_probe_observation_tables(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS protocol_probe_observations (
                target_key TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                route_id TEXT,
                public_model TEXT NOT NULL,
                upstream_model TEXT NOT NULL,
                transport TEXT NOT NULL,
                endpoint_fingerprint TEXT NOT NULL,
                authentication_kind TEXT NOT NULL,
                readiness TEXT NOT NULL,
                observation_json TEXT NOT NULL,
                tested_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_protocol_probe_observations_provider
             ON protocol_probe_observations(provider_id, route_id, public_model, tested_at DESC);
             CREATE INDEX IF NOT EXISTS idx_protocol_probe_observations_expiry
             ON protocol_probe_observations(expires_at);",
        )
        .map_err(|error| {
            AppError::Database(format!("创建 protocol_probe_observations 失败: {error}"))
        })
    }

    fn create_reasoning_manual_override_tables(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS codex_reasoning_manual_overrides (
                target_key TEXT PRIMARY KEY,
                target_json TEXT NOT NULL,
                revision INTEGER NOT NULL,
                override_json TEXT,
                reason TEXT NOT NULL,
                updated_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_codex_reasoning_manual_overrides_updated
             ON codex_reasoning_manual_overrides(updated_at DESC);",
        )
        .map_err(|error| {
            AppError::Database(format!(
                "创建 codex_reasoning_manual_overrides 失败: {error}"
            ))
        })
    }

    /// 插入默认模型定价数据
    /// 格式: (model_id, display_name, input, output, cache_read, cache_creation)
    /// 注意: model_id 使用短横线格式（如 claude-haiku-4-5），与 API 返回的模型名称标准化后一致
    fn seed_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_data = [
            // Claude Fable 5（Opus 之上的新档）
            (
                "claude-fable-5",
                "Claude Fable 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            (
                "claude-mythos-5",
                "Claude Mythos 5",
                "10",
                "50",
                "1.00",
                "12.50",
            ),
            // Claude Opus 5（与 Opus 4.8 同价位；fast mode $10/$50 不入表）
            ("claude-opus-5", "Claude Opus 5", "5", "25", "0.50", "6.25"),
            // Claude 4.8 系列
            (
                "claude-opus-4-8",
                "Claude Opus 4.8",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude Sonnet 5（list 价，与 Sonnet 4.6 一致；促销 $2/$10 至 2026-08-31 不入表）
            (
                "claude-sonnet-5",
                "Claude Sonnet 5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.7 系列
            (
                "claude-opus-4-7",
                "Claude Opus 4.7",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            // Claude 4.6 系列（裸 id 行覆盖无日期后缀的日志变体，与 dated 行同价）
            (
                "claude-opus-4-6",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-opus-4-6-20260206",
                "Claude Opus 4.6",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-6-20260217",
                "Claude Sonnet 4.6",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 4.5 系列
            (
                "claude-opus-4-5-20251101",
                "Claude Opus 4.5",
                "5",
                "25",
                "0.50",
                "6.25",
            ),
            (
                "claude-sonnet-4-5-20250929",
                "Claude Sonnet 4.5",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            (
                "claude-haiku-4-5-20251001",
                "Claude Haiku 4.5",
                "1",
                "5",
                "0.10",
                "1.25",
            ),
            // Claude 4 系列 (Legacy Models)
            (
                "claude-opus-4-20250514",
                "Claude Opus 4",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-opus-4-1-20250805",
                "Claude Opus 4.1",
                "15",
                "75",
                "1.50",
                "18.75",
            ),
            (
                "claude-sonnet-4-20250514",
                "Claude Sonnet 4",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // Claude 3.5 系列
            (
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku",
                "0.80",
                "4",
                "0.08",
                "1",
            ),
            (
                "claude-3-5-sonnet-20241022",
                "Claude 3.5 Sonnet",
                "3",
                "15",
                "0.30",
                "3.75",
            ),
            // GPT-5.6 系列：OpenAI API 标准短上下文价（2026-07-13）。
            // cache creation 是官方 pricing 页面单列的 cache writes，不与普通
            // input 混算，避免带 prompt cache 的请求少算费用。
            ("gpt-5.6-sol", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            // 2026-07-30 OpenAI 降价：luna -80%、terra -20%，sol 不变（Fast mode 2× 价不入表）
            ("gpt-5.6-terra", "GPT-5.6 Terra", "2", "12", "0.20", "2.50"),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
            ),
            ("gpt-5.6", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-low", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-medium", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-high", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-xhigh", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            ("gpt-5.6-minimal", "GPT-5.6 Sol", "5", "30", "0.50", "6.25"),
            // GPT-5.5 系列
            ("gpt-5.5", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-low", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-medium", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-high", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-xhigh", "GPT-5.5", "5", "30", "0.50", "0"),
            ("gpt-5.5-minimal", "GPT-5.5", "5", "30", "0.50", "0"),
            // GPT-5.4 系列
            ("gpt-5.4", "GPT-5.4", "2.50", "15", "0.25", "0"),
            ("gpt-5.4-mini", "GPT-5.4 Mini", "0.75", "4.50", "0.075", "0"),
            ("gpt-5.4-nano", "GPT-5.4 Nano", "0.20", "1.25", "0.02", "0"),
            // GPT-5.2 系列
            ("gpt-5.2", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-low", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-medium", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-high", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-xhigh", "GPT-5.2", "1.75", "14", "0.175", "0"),
            ("gpt-5.2-codex", "GPT-5.2 Codex", "1.75", "14", "0.175", "0"),
            (
                "gpt-5.2-codex-low",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-medium",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-high",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.2-codex-xhigh",
                "GPT-5.2 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            // GPT-5.3 Codex 系列
            ("gpt-5.3-codex", "GPT-5.3 Codex", "1.75", "14", "0.175", "0"),
            // 官方价格页未单列 Spark；它是本地 Codex 目录中的 gpt-5.3-codex
            // 变体，暂按公开 gpt-5.3-codex 标准价计费，避免统计显示为 $0。
            (
                "gpt-5.3-codex-spark",
                "GPT-5.3 Codex Spark",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-spark",
                "GPT-5.3 Codex Spark",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-low",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-medium",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-high",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            (
                "gpt-5.3-codex-xhigh",
                "GPT-5.3 Codex",
                "1.75",
                "14",
                "0.175",
                "0",
            ),
            // GPT-5.1 系列
            ("gpt-5.1", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-low", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-medium", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-high", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-minimal", "GPT-5.1", "1.25", "10", "0.125", "0"),
            ("gpt-5.1-codex", "GPT-5.1 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5.1-codex-mini",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-high",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5.1-codex-max-xhigh",
                "GPT-5.1 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            // GPT-5 系列
            ("gpt-5", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-low", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-medium", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-high", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-minimal", "GPT-5", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            ("gpt-5-codex-low", "GPT-5 Codex", "1.25", "10", "0.125", "0"),
            (
                "gpt-5-codex-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-medium",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gpt-5-codex-mini-high",
                "GPT-5 Codex",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            // OpenAI Reasoning 系列
            ("o3", "OpenAI o3", "2", "8", "0.50", "0"),
            ("o4-mini", "OpenAI o4-mini", "1.10", "4.40", "0.275", "0"),
            // GPT-4.1 系列
            ("gpt-4.1", "GPT-4.1", "2", "8", "0.50", "0"),
            ("gpt-4.1-mini", "GPT-4.1 Mini", "0.40", "1.60", "0.10", "0"),
            ("gpt-4.1-nano", "GPT-4.1 Nano", "0.10", "0.40", "0.025", "0"),
            // Gemini 3.6 系列
            (
                "gemini-3.6-flash",
                "Gemini 3.6 Flash",
                "1.50",
                "7.50",
                "0.15",
                "0",
            ),
            // Gemini 3.5 系列
            (
                "gemini-3.5-flash",
                "Gemini 3.5 Flash",
                "1.50",
                "9.00",
                "0.15",
                "0",
            ),
            (
                "gemini-3.5-flash-lite",
                "Gemini 3.5 Flash Lite",
                "0.30",
                "2.50",
                "0.03",
                "0",
            ),
            // Gemini 3.1 系列
            (
                "gemini-3.1-pro-preview",
                "Gemini 3.1 Pro Preview",
                "2",
                "12",
                "0.20",
                "0",
            ),
            (
                "gemini-3.1-flash-lite",
                "Gemini 3.1 Flash Lite",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            (
                "gemini-3.1-flash-lite-preview",
                "Gemini 3.1 Flash Lite Preview",
                "0.25",
                "1.50",
                "0.025",
                "0",
            ),
            // Gemini 3 系列
            (
                "gemini-3-pro-preview",
                "Gemini 3 Pro Preview",
                "2",
                "12",
                "0.2",
                "0",
            ),
            (
                "gemini-3-flash-preview",
                "Gemini 3 Flash Preview",
                "0.5",
                "3",
                "0.05",
                "0",
            ),
            // Gemini 2.5 系列
            (
                "gemini-2.5-pro",
                "Gemini 2.5 Pro",
                "1.25",
                "10",
                "0.125",
                "0",
            ),
            (
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                "0.3",
                "2.5",
                "0.03",
                "0",
            ),
            (
                "gemini-2.5-flash-lite",
                "Gemini 2.5 Flash Lite",
                "0.10",
                "0.40",
                "0.01",
                "0",
            ),
            // Gemini 2.0 系列
            (
                "gemini-2.0-flash",
                "Gemini 2.0 Flash",
                "0.10",
                "0.40",
                "0.025",
                "0",
            ),
            // StepFun 系列
            (
                "step-3.7-flash",
                "Step 3.7 Flash",
                "0.19",
                "1.13",
                "0.04",
                "0",
            ),
            (
                "step-3.5-flash",
                "Step 3.5 Flash",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            (
                "step-3.5-flash-2603",
                "Step 3.5 Flash 2603",
                "0.10",
                "0.30",
                "0.02",
                "0",
            ),
            // ====== 国产模型 (USD/1M tokens) ======
            // Doubao (字节跳动)
            // Seed 2.1 系列（2026-06 火山引擎官方 list 价，CNY 按 ~7.14 折算）：
            //   pro   输入 6 元 / 输出 30 元 / 命中 1.2 元
            //   turbo 输入 3 元 / 输出 15 元 / 命中 0.6 元
            // 「缓存存储 0.017 元/M/小时」是按时长计费的存储费，与本表 cache_creation（按 token 写入价）口径不同，置 0。
            (
                "doubao-seed-2-1-pro",
                "Doubao Seed 2.1 Pro",
                "0.84",
                "4.2",
                "0.17",
                "0",
            ),
            (
                "doubao-seed-2-1-turbo",
                "Doubao Seed 2.1 Turbo",
                "0.42",
                "2.1",
                "0.08",
                "0",
            ),
            (
                "doubao-seed-code",
                "Doubao Seed Code",
                "0.17",
                "1.11",
                "0.02",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
            ),
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
            ),
            // DeepSeek 系列
            (
                "deepseek-v3.2",
                "DeepSeek V3.2",
                "0.28",
                "0.42",
                "0.028",
                "0",
            ),
            (
                "deepseek-v3.1",
                "DeepSeek V3.1",
                "0.55",
                "1.67",
                "0.055",
                "0",
            ),
            ("deepseek-v3", "DeepSeek V3", "0.28", "1.11", "0.028", "0"),
            // deepseek-chat / deepseek-reasoner 自 2026-07 起为 V4 Flash 的 legacy 别名（同价）
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            // DeepSeek V4 系列（官方 CNY 按 1 USD ≈ 7.14 折算）
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.14",
                "0.28",
                "0.0028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.435",
                "0.87",
                "0.003625",
                "0",
            ),
            // Kimi (月之暗面)
            (
                "kimi-k2-thinking",
                "Kimi K2 Thinking",
                "0.55",
                "2.20",
                "0.10",
                "0",
            ),
            ("kimi-k2-0905", "Kimi K2", "0.55", "2.20", "0.10", "0"),
            (
                "kimi-k2-turbo",
                "Kimi K2 Turbo",
                "1.11",
                "8.06",
                "0.14",
                "0",
            ),
            ("kimi-k2.5", "Kimi K2.5", "0.60", "3.00", "0.10", "0"),
            ("kimi-k2.6", "Kimi K2.6", "0.95", "4.00", "0.16", "0"),
            (
                "kimi-k2.7-code",
                "Kimi K2.7 Code",
                "0.95",
                "4.00",
                "0.19",
                "0",
            ),
            // HighSpeed 加速档=本体 2 倍价（Kimi 官方一贯模式，同 K2 Turbo）
            (
                "kimi-k2.7-code-highspeed",
                "Kimi K2.7 Code HighSpeed",
                "1.90",
                "8.00",
                "0.38",
                "0",
            ),
            ("kimi-k3", "Kimi K3", "3.00", "15.00", "0.30", "0"),
            // Kimi For Coding 套餐里 K3 的裸名（无 kimi- 前缀），同标准 list 价
            ("k3", "Kimi K3", "3.00", "15.00", "0.30", "0"),
            // 腾讯混元 (Tencent Hunyuan)（官方 CNY 1/4/0.25 按 1 USD ≈ 7.14 折算；Hy3 阶梯计价取最低档）
            ("hunyuan-hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            ("hy3", "Hunyuan Hy3", "0.14", "0.56", "0.035", "0"),
            // MiniMax 系列
            ("minimax-m2.1", "MiniMax M2.1", "0.27", "0.95", "0.03", "0"),
            (
                "minimax-m2.1-lightning",
                "MiniMax M2.1 Lightning",
                "0.27",
                "2.33",
                "0.03",
                "0",
            ),
            ("minimax-m2", "MiniMax M2", "0.27", "0.95", "0.03", "0"),
            ("minimax-m2.5", "MiniMax M2.5", "0.15", "0.95", "0.03", "0"),
            (
                "minimax-m2.5-lightning",
                "MiniMax M2.5 Lightning",
                "0.30",
                "2.40",
                "0.03",
                "0",
            ),
            (
                "minimax-m2.7",
                "MiniMax M2.7",
                "0.30",
                "1.20",
                "0.06",
                "0.375",
            ),
            (
                "minimax-m2.7-highspeed",
                "MiniMax M2.7 Highspeed",
                "0.60",
                "2.40",
                "0.06",
                "0.375",
            ),
            ("minimax-m3", "MiniMax M3", "0.30", "1.20", "0.06", "0"),
            // GLM (智谱)
            ("glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0"),
            ("glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0"),
            ("glm-5", "GLM-5", "1", "3.2", "0.2", "0"),
            ("glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0"),
            ("glm-5.2", "GLM-5.2", "1.4", "4.4", "0.26", "0"),
            ("glm-5-turbo", "GLM-5-Turbo", "1.2", "4", "0.24", "0"),
            ("glm-5v-turbo", "GLM-5V-Turbo", "1.2", "4", "0.24", "0"),
            // MiMo (小米)
            (
                "mimo-v2-flash",
                "MiMo V2 Flash",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            ("mimo-v2-pro", "MiMo V2 Pro", "0.435", "0.87", "0.0036", "0"),
            ("mimo-v2.5", "MiMo V2.5", "0.14", "0.29", "0.0028", "0"),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
            ),
            // Qwen 系列 (阿里巴巴)
            ("qwen3.8-max", "Qwen3.8 Max", "2", "6", "0.25", "2.50"),
            ("qwen3.7-max", "Qwen3.7 Max", "2.50", "7.50", "0.25", "0"),
            ("qwen3.7-plus", "Qwen3.7 Plus", "0.40", "1.60", "0.08", "0"),
            // Qwen3.6 27B：OpenRouter 公开 API 实时价（2026-07-13）。
            // `qwen3.6` 是 CCSwitchMulti 现有 vLLM 路由别名；自托管实际 GPU
            // 成本不能从 token 单价推导，故这里表示同模型的公开 API 参考成本。
            (
                "qwen3.6",
                "Qwen3.6 27B (OpenRouter Reference)",
                "0.285",
                "2.40",
                "0.15",
                "0",
            ),
            ("qwen3.6-27b", "Qwen3.6 27B", "0.285", "2.40", "0.15", "0"),
            ("qwen3.6-plus", "Qwen3.6 Plus", "0.50", "3", "0.10", "0"),
            (
                "qwen3.6-flash",
                "Qwen3.6 Flash",
                "0.1875",
                "1.125",
                "0.0375",
                "0",
            ),
            ("qwen3.5-plus", "Qwen3.5 Plus", "0.26", "1.56", "0.052", "0"),
            ("qwen3-max", "Qwen3 Max", "0.78", "3.90", "0", "0"),
            (
                "qwen3-235b-a22b",
                "Qwen3 235B-A22B",
                "0.70",
                "8.40",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
            ),
            (
                "qwen3-coder-480b",
                "Qwen3 Coder 480B",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-480b-a35b-instruct",
                "Qwen3 Coder 480B-A35B Instruct",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
            ),
            (
                "qwen3-coder-next",
                "Qwen3 Coder Next",
                "0.12",
                "0.75",
                "0",
                "0",
            ),
            ("qwq-plus", "QwQ Plus", "0.80", "2.40", "0", "0"),
            ("qwq-32b", "QwQ 32B", "0.20", "0.60", "0", "0"),
            ("qwen3-32b", "Qwen3 32B", "0.16", "0.64", "0", "0"),
            // Grok 系列 (xAI)
            ("grok-4.5", "Grok 4.5", "2", "6", "0.50", "0"),
            // Grok CLI 官方 OAuth 态 modelUsage 上报的内部别名。定价由
            // costUsdTicks（1 tick = 1e-10 USD）双轮实测反推：input/output 与
            // grok-4.5 同为 2/6，cache read 实际按 0.30 计（非 API 挂牌的 0.50）
            ("grok-4.5-build", "Grok 4.5 Build", "2", "6", "0.30", "0"),
            ("grok-4.3", "Grok 4.3", "1.25", "2.50", "0.20", "0"),
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
            ),
            (
                "grok-4-1-fast-reasoning",
                "Grok 4.1 Fast Reasoning",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            (
                "grok-4-1-fast-non-reasoning",
                "Grok 4.1 Fast",
                "0.20",
                "0.50",
                "0.05",
                "0",
            ),
            ("grok-4", "Grok 4", "3", "15", "0.75", "0"),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
            ),
            ("grok-build-0.1", "Grok Build 0.1", "1", "2", "0.20", "0"),
            ("grok-3", "Grok 3", "3", "15", "0.75", "0"),
            ("grok-3-mini", "Grok 3 Mini", "0.25", "0.50", "0.075", "0"),
            // Mistral 系列
            (
                "mistral-medium-3.5",
                "Mistral Medium 3.5",
                "1.50",
                "7.50",
                "0",
                "0",
            ),
            (
                "mistral-small-4",
                "Mistral Small 4",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "devstral-small-2-2512",
                "Devstral Small 2",
                "0.10",
                "0.30",
                "0.01",
                "0",
            ),
            (
                "magistral-small",
                "Magistral Small",
                "0.50",
                "1.50",
                "0",
                "0",
            ),
            ("codestral-2508", "Codestral", "0.30", "0.90", "0.03", "0"),
            (
                "devstral-small-1.1",
                "Devstral Small 1.1",
                "0.07",
                "0.28",
                "0.01",
                "0",
            ),
            ("devstral-2-2512", "Devstral 2", "0.40", "2", "0.04", "0"),
            (
                "devstral-medium",
                "Devstral Medium",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-large-3-2512",
                "Mistral Large 3",
                "0.50",
                "1.50",
                "0.05",
                "0",
            ),
            (
                "mistral-medium-3.1",
                "Mistral Medium 3.1",
                "0.40",
                "2",
                "0.04",
                "0",
            ),
            (
                "mistral-small-3.2-24b",
                "Mistral Small 3.2",
                "0.075",
                "0.20",
                "0.01",
                "0",
            ),
            ("magistral-medium", "Magistral Medium", "2", "5", "0", "0"),
            // Cohere 系列
            ("command-a", "Cohere Command A", "2.50", "10", "0", "0"),
            (
                "command-r-plus",
                "Cohere Command R+",
                "2.50",
                "10",
                "0",
                "0",
            ),
            ("command-r", "Cohere Command R", "0.15", "0.60", "0", "0"),
            // OpenAI 补充
            ("o3-pro", "OpenAI o3-pro", "20", "80", "0", "0"),
            ("o3-mini", "OpenAI o3-mini", "0.55", "2.20", "0.55", "0"),
            ("o1", "OpenAI o1", "15", "60", "7.50", "0"),
            ("o1-mini", "OpenAI o1-mini", "0.55", "2.20", "0.55", "0"),
            ("codex-mini", "Codex Mini", "0.75", "3", "0.025", "0"),
            ("gpt-5-mini", "GPT-5 Mini", "0.25", "2", "0.025", "0"),
            ("gpt-5-nano", "GPT-5 Nano", "0.05", "0.40", "0.005", "0"),
        ];

        let mut stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| AppError::Database(format!("准备模型定价语句失败: {e}")))?;
        for (model_id, display_name, input, output, cache_read, cache_creation) in pricing_data {
            stmt.execute(rusqlite::params![
                model_id,
                display_name,
                input,
                output,
                cache_read,
                cache_creation
            ])
            .map_err(|e| AppError::Database(format!("插入模型定价失败: {e}")))?;
        }

        log::info!("已插入 {} 条默认模型定价数据", pricing_data.len());
        Ok(())
    }

    fn repair_current_model_pricing(conn: &Connection) -> Result<(), AppError> {
        let pricing_fixes = [
            // 2026-07-30 OpenAI GPT-5.6 降价：luna -80%、terra -20%（sol 不变）。
            // 每档两条守卫：主守卫匹配 ≥v3.19（已跑过 07-12 cache_write 修正），
            // 0 态守卫匹配 <v3.19 直升用户（cache_write 仍为旧 seed 的 0）
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
                "1",
                "6",
                "0.10",
                "1.25",
            ),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "0.20",
                "1.20",
                "0.02",
                "0.25",
                "1",
                "6",
                "0.10",
                "0",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2",
                "12",
                "0.20",
                "2.50",
                "2.50",
                "15",
                "0.25",
                "3.125",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2",
                "12",
                "0.20",
                "2.50",
                "2.50",
                "15",
                "0.25",
                "0",
            ),
            // 2026-07-31 models.dev 审计核价：DeepSeek V4 发布后 chat/reasoner 降为 V4 Flash
            // 别名价；MiniMax M3 官方 standard 档 0.3/1.2（旧值疑似录了加速档）
            (
                "deepseek-chat",
                "DeepSeek Chat",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.27",
                "1.10",
                "0.07",
                "0",
            ),
            (
                "deepseek-reasoner",
                "DeepSeek Reasoner",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.55",
                "2.19",
                "0.14",
                "0",
            ),
            (
                "minimax-m3",
                "MiniMax M3",
                "0.30",
                "1.20",
                "0.06",
                "0",
                "0.60",
                "2.40",
                "0.12",
                "0",
            ),
            // 2026-07-12 GPT-5.6 家族 cache write=1.25× 输入价（OpenAI 5.6 起的新规），
            // 修正早期 seed 的 0 值；只匹配未被用户改过的行
            (
                "gpt-5.6-sol",
                "GPT-5.6 Sol",
                "5",
                "30",
                "0.50",
                "6.25",
                "5",
                "30",
                "0.50",
                "0",
            ),
            (
                "gpt-5.6-terra",
                "GPT-5.6 Terra",
                "2.50",
                "15",
                "0.25",
                "3.125",
                "2.50",
                "15",
                "0.25",
                "0",
            ),
            (
                "gpt-5.6-luna",
                "GPT-5.6 Luna",
                "1",
                "6",
                "0.10",
                "1.25",
                "1",
                "6",
                "0.10",
                "0",
            ),
            // 2026-06-10 全量核价（厂商官方 list 价；CNY 按 ~7.14 折算）
            // GLM 4.6/4.7：旧值是中转/OpenRouter 折扣价，统一到 Z.ai 官方（与 glm-5/5.1 一致）
            (
                "glm-4.7", "GLM-4.7", "0.6", "2.2", "0.11", "0", "0.39", "1.75", "0.04", "0",
            ),
            (
                "glm-4.6", "GLM-4.6", "0.6", "2.2", "0.11", "0", "0.28", "1.11", "0.03", "0",
            ),
            // Grok 4.20：xAI 已降价 2/6 → 1.25/2.50
            (
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            (
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20",
                "1.25",
                "2.50",
                "0.20",
                "0",
                "2",
                "6",
                "0.20",
                "0",
            ),
            // Kimi K2.5 官方 output 3.00
            (
                "kimi-k2.5",
                "Kimi K2.5",
                "0.60",
                "3.00",
                "0.10",
                "0",
                "0.60",
                "2.50",
                "0.10",
                "0",
            ),
            // MiniMax M2.5 input 0.15
            (
                "minimax-m2.5",
                "MiniMax M2.5",
                "0.15",
                "0.95",
                "0.03",
                "0",
                "0.12",
                "0.95",
                "0.03",
                "0",
            ),
            // Mistral Devstral 2 output 0.90 → 2（与同表 devstral-medium 一致）
            (
                "devstral-2-2512",
                "Devstral 2",
                "0.40",
                "2",
                "0.04",
                "0",
                "0.40",
                "0.90",
                "0.04",
                "0",
            ),
            // Doubao Seed 2.0：lite 旧价贵 3-4 倍 + 全系补 cache 命中价
            (
                "doubao-seed-2-0-lite",
                "Doubao Seed 2.0 Lite",
                "0.08",
                "0.50",
                "0.017",
                "0",
                "0.25",
                "2",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-pro",
                "Doubao Seed 2.0 Pro",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code",
                "Doubao Seed 2.0 Code",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-code-preview-latest",
                "Doubao Seed 2.0 Code Preview",
                "0.47",
                "2.37",
                "0.09",
                "0",
                "0.47",
                "2.37",
                "0",
                "0",
            ),
            (
                "doubao-seed-2-0-mini",
                "Doubao Seed 2.0 Mini",
                "0.03",
                "0.31",
                "0.0056",
                "0",
                "0.03",
                "0.31",
                "0",
                "0",
            ),
            // MiMo：5/27 永久降价，旧值是旧价
            (
                "mimo-v2-pro",
                "MiMo V2 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            (
                "mimo-v2.5",
                "MiMo V2.5",
                "0.14",
                "0.29",
                "0.0028",
                "0",
                "0.09",
                "0.29",
                "0.009",
                "0",
            ),
            (
                "mimo-v2.5-pro",
                "MiMo V2.5 Pro",
                "0.435",
                "0.87",
                "0.0036",
                "0",
                "1",
                "3",
                "0",
                "0",
            ),
            // Qwen：官方 context cache 输入价按标准输入价的 20% 计。
            // Qwen3.6 Plus 已从旧版 $0.325/$1.95 更新为当前国际部署短上下文价。
            (
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                "0.50",
                "3",
                "0.10",
                "0",
                "0.325",
                "1.95",
                "0.065",
                "0",
            ),
            (
                "qwen3.5-plus",
                "Qwen3.5 Plus",
                "0.26",
                "1.56",
                "0.052",
                "0",
                "0.26",
                "1.56",
                "0",
                "0",
            ),
            (
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                "0.65",
                "3.25",
                "0.13",
                "0",
                "0.65",
                "3.25",
                "0",
                "0",
            ),
            (
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                "0.195",
                "0.975",
                "0.039",
                "0",
                "0.195",
                "0.975",
                "0",
                "0",
            ),
            (
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                "0.14",
                "0.28",
                "0.0028",
                "0",
                "0.14",
                "0.28",
                "0.028",
                "0",
            ),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.435",
                "0.87",
                "0.003625",
                "0",
                "1.68",
                "3.36",
                "0.14",
                "0",
            ),
            (
                "glm-5", "GLM-5", "1", "3.2", "0.2", "0", "0.72", "2.30", "0", "0",
            ),
            (
                "glm-5.1", "GLM-5.1", "1.4", "4.4", "0.26", "0", "0.95", "3.15", "0", "0",
            ),
            (
                "grok-code-fast-1",
                "Grok Build 0.1 (Code Fast Alias)",
                "1",
                "2",
                "0.20",
                "0",
                "0.20",
                "1.50",
                "0.02",
                "0",
            ),
        ];

        for (
            model_id,
            display_name,
            input,
            output,
            cache_read,
            cache_creation,
            old_input,
            old_output,
            old_cache_read,
            old_cache_creation,
        ) in pricing_fixes
        {
            conn.execute(
                "UPDATE model_pricing SET
                    display_name = ?2,
                    input_cost_per_million = ?3,
                    output_cost_per_million = ?4,
                    cache_read_cost_per_million = ?5,
                    cache_creation_cost_per_million = ?6
                 WHERE model_id = ?1
                   AND input_cost_per_million = ?7
                   AND output_cost_per_million = ?8
                   AND cache_read_cost_per_million = ?9
                   AND cache_creation_cost_per_million = ?10",
                rusqlite::params![
                    model_id,
                    display_name,
                    input,
                    output,
                    cache_read,
                    cache_creation,
                    old_input,
                    old_output,
                    old_cache_read,
                    old_cache_creation
                ],
            )
            .map_err(|e| AppError::Database(format!("修复模型 {model_id} 定价失败: {e}")))?;
        }

        Ok(())
    }

    /// 确保模型定价表具备默认数据
    pub fn ensure_model_pricing_seeded(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        Self::ensure_model_pricing_seeded_on_conn(&conn)
    }

    fn ensure_model_pricing_seeded_on_conn(conn: &Connection) -> Result<(), AppError> {
        // 每次启动都执行 INSERT OR IGNORE，增量追加新模型；仅修复仍等于旧内置值的定价。
        Self::seed_model_pricing(conn)?;
        Self::repair_current_model_pricing(conn)
    }

    /// 将旧版 DeepSeek Codex 配置升级为 V4 Flash 原生 Responses，同时保留 Pro 的 Chat 兼容路由。
    ///
    /// DeepSeek 官方 Responses API 当前仅支持 `deepseek-v4-flash`；`deepseek-v4-pro`
    /// 要到 2026-08 才支持 Codex，因此不能把包含 Pro 的整条 legacy route 直接改成
    /// `openai_responses`。这里只处理官方 `api.deepseek.com`，幂等且不触碰其它供应商。
    pub(crate) fn repair_deepseek_native_responses_on_conn(
        conn: &Connection,
    ) -> Result<(), AppError> {
        fn route_target_provider_id(route: &serde_json::Value) -> Option<&str> {
            [
                route.get("targetProviderId"),
                route.get("target_provider_id"),
                route.get("providerId"),
                route.get("provider_id"),
                route.pointer("/upstream/targetProviderId"),
                route.pointer("/upstream/target_provider_id"),
                route.pointer("/upstream/providerId"),
                route.pointer("/upstream/provider_id"),
            ]
            .into_iter()
            .flatten()
            .find_map(serde_json::Value::as_str)
        }

        fn route_matches_model(route: &serde_json::Value, model: &str) -> bool {
            let exact = route
                .pointer("/match/models")
                .or_else(|| route.pointer("/modelSelection/models"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|models| {
                    models
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .any(|candidate| candidate.eq_ignore_ascii_case(model))
                });
            let model_lower = model.to_ascii_lowercase();
            let prefix = route
                .pointer("/match/prefixes")
                .or_else(|| route.get("matchPrefixes"))
                .or_else(|| route.get("match_prefixes"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|prefixes| {
                    prefixes
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|prefix| !prefix.is_empty())
                        .any(|prefix| model_lower.starts_with(&prefix.to_ascii_lowercase()))
                });
            let include_allows = route
                .pointer("/modelSelection/models")
                .and_then(serde_json::Value::as_array)
                .map(|models| {
                    models
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .any(|candidate| candidate.eq_ignore_ascii_case(model))
                })
                .unwrap_or(true);
            exact || (prefix && include_allows)
        }

        fn canonicalize_deepseek_route(
            route: &mut serde_json::Value,
            model: &str,
            api_format: &str,
        ) -> bool {
            let mut changed = false;
            let next_models = serde_json::json!([model]);
            if route.pointer("/match/models") != Some(&next_models) {
                route["match"]["models"] = next_models;
                changed = true;
            }
            let next_prefixes = serde_json::json!([model]);
            if route.pointer("/match/prefixes") != Some(&next_prefixes) {
                route["match"]["prefixes"] = next_prefixes;
                changed = true;
            }

            if route.get("upstream").is_some() {
                if route
                    .pointer("/upstream/apiFormat")
                    .and_then(serde_json::Value::as_str)
                    != Some(api_format)
                {
                    route["upstream"]["apiFormat"] = serde_json::json!(api_format);
                    changed = true;
                }
                if let Some(model_map) = route
                    .pointer_mut("/upstream/modelMap")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    let before = model_map.len();
                    model_map.retain(|key, _| key == model);
                    changed |= model_map.len() != before;
                }
            } else if route.get("apiFormat").and_then(serde_json::Value::as_str) != Some(api_format)
            {
                route["apiFormat"] = serde_json::json!(api_format);
                changed = true;
            }

            changed
        }

        let mut stmt = conn
            .prepare("SELECT id, name, meta, settings_config FROM providers WHERE app_type='codex'")
            .map_err(|e| AppError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut providers = Vec::new();
        for row in rows {
            let (id, name, meta, settings_config) =
                row.map_err(|e| AppError::Database(e.to_string()))?;
            providers.push((id, name, meta, settings_config));
        }

        let official_deepseek_ids: Vec<String> = providers
            .iter()
            .filter_map(|(id, name, _, settings)| {
                let haystack = format!("{id} {name} {settings}").to_lowercase();
                (haystack.contains("deepseek") && haystack.contains("api.deepseek.com"))
                    .then(|| id.clone())
            })
            .collect();
        let official_deepseek_models = providers
            .iter()
            .filter(|(id, _, _, _)| official_deepseek_ids.contains(id))
            .map(|(id, _, _, settings)| {
                let lower = settings.to_lowercase();
                (
                    id.clone(),
                    (
                        lower.contains("deepseek-v4-flash"),
                        lower.contains("deepseek-v4-pro"),
                    ),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();

        let mut updates = Vec::new();
        for (id, name, meta_str, settings_str) in providers {
            let haystack = format!("{id} {name} {settings_str}").to_lowercase();
            let references_official_target = official_deepseek_ids
                .iter()
                .any(|target_id| settings_str.contains(target_id));
            if !haystack.contains("deepseek")
                || (!haystack.contains("api.deepseek.com") && !references_official_target)
            {
                continue;
            }

            let mut meta = serde_json::from_str::<serde_json::Value>(&meta_str)
                .unwrap_or_else(|_| serde_json::json!({}));
            let mut settings = serde_json::from_str::<serde_json::Value>(&settings_str)
                .unwrap_or_else(|_| serde_json::json!({}));
            let mut changed = false;
            let has_flash = settings_str.to_lowercase().contains("deepseek-v4-flash");

            if has_flash {
                for key in ["apiFormat", "api_format"] {
                    if meta.get(key).and_then(serde_json::Value::as_str) == Some("openai_chat") {
                        meta[key] = serde_json::json!("openai_responses");
                        changed = true;
                    }
                    if settings.get(key).and_then(serde_json::Value::as_str) == Some("openai_chat")
                    {
                        settings[key] = serde_json::json!("openai_responses");
                        changed = true;
                    }
                }
            }

            if let Some(routes) = settings
                .pointer_mut("/codexRouting/routes")
                .and_then(serde_json::Value::as_array_mut)
            {
                let mut additions = Vec::new();
                let mut has_pro_chat_route = routes.iter().any(|existing| {
                    existing
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|id| id.contains("-pro-chat"))
                });
                for route in routes.iter_mut() {
                    let route_text = route.to_string().to_lowercase();
                    if !route_text.contains("deepseek") {
                        continue;
                    }
                    let target_models = route_target_provider_id(route)
                        .and_then(|target_id| official_deepseek_models.get(target_id))
                        .copied();

                    // A route that references a dedicated official DeepSeek provider must be
                    // repaired from that provider's own catalog, not from the MultiRouter's
                    // aggregate settings JSON. The old aggregate `has_flash` check saw Flash in
                    // a sibling route and repeatedly rewrote a separate Pro/Chat route into the
                    // Flash/Responses window on every startup.
                    match target_models {
                        Some((false, true)) => {
                            changed |= canonicalize_deepseek_route(
                                route,
                                "deepseek-v4-pro",
                                "openai_chat",
                            );
                            continue;
                        }
                        Some((true, false)) => {
                            changed |= canonicalize_deepseek_route(
                                route,
                                "deepseek-v4-flash",
                                "openai_responses",
                            );
                            continue;
                        }
                        Some((true, true))
                            if route_matches_model(route, "deepseek-v4-pro")
                                && !route_matches_model(route, "deepseek-v4-flash") =>
                        {
                            changed |= canonicalize_deepseek_route(
                                route,
                                "deepseek-v4-pro",
                                "openai_chat",
                            );
                            continue;
                        }
                        _ => {}
                    }

                    let old_api = route
                        .pointer("/upstream/apiFormat")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| route.get("apiFormat").and_then(serde_json::Value::as_str));
                    if old_api != Some("openai_chat") {
                        continue;
                    }

                    let covers_pro = route_text.contains("deepseek-v4-pro")
                        || route_text.contains("\"deepseek\"")
                        || route_text.contains("\"deepseek-\"");
                    let route_has_flash = target_models
                        .map(|(target_has_flash, _)| target_has_flash)
                        .unwrap_or(has_flash);
                    if route_has_flash {
                        if let Some(upstream) = route.get_mut("upstream") {
                            upstream["apiFormat"] = serde_json::json!("openai_responses");
                        } else {
                            route["apiFormat"] = serde_json::json!("openai_responses");
                        }
                        if route.get("match").is_none() {
                            route["match"] = serde_json::json!({});
                        }
                        if let Some(models) = route
                            .get_mut("match")
                            .and_then(|match_obj| match_obj.get_mut("models"))
                            .and_then(serde_json::Value::as_array_mut)
                        {
                            models.clear();
                            models.push(serde_json::json!("deepseek-v4-flash"));
                        } else if let Some(match_obj) = route
                            .get_mut("match")
                            .and_then(serde_json::Value::as_object_mut)
                        {
                            match_obj.insert(
                                "models".to_string(),
                                serde_json::json!(["deepseek-v4-flash"]),
                            );
                        }
                        if let Some(prefixes) = route
                            .get_mut("match")
                            .and_then(|match_obj| match_obj.get_mut("prefixes"))
                            .and_then(serde_json::Value::as_array_mut)
                        {
                            prefixes.clear();
                            prefixes.push(serde_json::json!("deepseek-v4-flash"));
                        } else if let Some(match_obj) = route
                            .get_mut("match")
                            .and_then(serde_json::Value::as_object_mut)
                        {
                            match_obj.insert(
                                "prefixes".to_string(),
                                serde_json::json!(["deepseek-v4-flash"]),
                            );
                        }
                        changed = true;
                    }

                    if covers_pro && route_has_flash && !has_pro_chat_route {
                        let route_id = route
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("router-codex-deepseek");
                        let label = route
                            .get("label")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("DeepSeek");
                        let mut pro_route = route.clone();
                        pro_route["id"] = serde_json::json!(format!("{route_id}-pro-chat"));
                        pro_route["label"] = serde_json::json!(format!("{label} V4 Pro Chat"));
                        if pro_route.get("match").is_none() {
                            pro_route["match"] = serde_json::json!({});
                        }
                        if let Some(match_obj) = pro_route
                            .get_mut("match")
                            .and_then(serde_json::Value::as_object_mut)
                        {
                            match_obj.insert(
                                "models".to_string(),
                                serde_json::json!(["deepseek-v4-pro"]),
                            );
                            match_obj.insert(
                                "prefixes".to_string(),
                                serde_json::json!(["deepseek-v4-pro"]),
                            );
                        }
                        if let Some(upstream) = pro_route.get_mut("upstream") {
                            upstream["apiFormat"] = serde_json::json!("openai_chat");
                            if let Some(model_map) = upstream
                                .get_mut("modelMap")
                                .and_then(serde_json::Value::as_object_mut)
                            {
                                model_map.retain(|key, _| key == "deepseek-v4-pro");
                            }
                        } else {
                            pro_route["apiFormat"] = serde_json::json!("openai_chat");
                        }
                        additions.push(pro_route);
                        has_pro_chat_route = true;
                    }
                }
                if !additions.is_empty() {
                    routes.extend(additions);
                    changed = true;
                }
            }

            if changed {
                updates.push((
                    id,
                    serde_json::to_string(&meta).map_err(|e| AppError::Database(e.to_string()))?,
                    serde_json::to_string(&settings)
                        .map_err(|e| AppError::Database(e.to_string()))?,
                ));
            }
        }

        for (id, new_meta, new_settings) in updates {
            conn.execute(
                "UPDATE providers SET meta = ?1, settings_config = ?2 WHERE id = ?3 AND app_type = 'codex'",
                params![new_meta, new_settings, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    // --- 辅助方法 ---

    pub(crate) fn get_user_version(conn: &Connection) -> Result<i32, AppError> {
        conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|e| AppError::Database(format!("读取 user_version 失败: {e}")))
    }

    pub(crate) fn set_user_version(conn: &Connection, version: i32) -> Result<(), AppError> {
        if version < 0 {
            return Err(AppError::Database("user_version 不能为负数".to_string()));
        }
        let sql = format!("PRAGMA user_version = {version};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("写入 user_version 失败: {e}")))?;
        Ok(())
    }

    fn create_request_logs_usage_indexes_if_supported(conn: &Connection) -> Result<(), AppError> {
        if !Self::table_exists(conn, "proxy_request_logs")? {
            return Ok(());
        }

        let has_app_type = Self::has_column(conn, "proxy_request_logs", "app_type")?;
        let has_created_at = Self::has_column(conn, "proxy_request_logs", "created_at")?;
        if has_app_type && has_created_at {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_request_logs_app_created_at
                 ON proxy_request_logs(app_type, created_at DESC)",
                [],
            )
            .map_err(|e| AppError::Database(format!("创建使用量应用时间索引失败: {e}")))?;
        }

        let required_columns = [
            "app_type",
            "data_source",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "created_at",
            "cache_creation_tokens",
        ];
        for column in required_columns {
            if !Self::has_column(conn, "proxy_request_logs", column)? {
                return Ok(());
            }
        }

        conn.execute("DROP INDEX IF EXISTS idx_request_logs_dedup_lookup", [])
            .map_err(|e| AppError::Database(format!("删除旧使用量去重索引失败: {e}")))?;

        // 查询层为了兼容历史 NULL data_source 行，会使用
        // COALESCE(data_source, 'proxy')。普通 data_source 索引无法匹配该表达式，
        // 会让跨源去重子查询退化成大量扫描；表达式索引让 SQLite 能按同一表达式查找。
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_dedup_lookup_expr
             ON proxy_request_logs(app_type, COALESCE(data_source, 'proxy'), input_tokens,
                                   output_tokens, cache_read_tokens, created_at,
                                   cache_creation_tokens)",
            [],
        )
        .map_err(|e| AppError::Database(format!("创建使用量去重表达式索引失败: {e}")))?;
        Ok(())
    }

    fn validate_identifier(s: &str, kind: &str) -> Result<(), AppError> {
        if s.is_empty() {
            return Err(AppError::Database(format!("{kind} 不能为空")));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AppError::Database(format!(
                "非法{kind}: {s}，仅允许字母、数字和下划线"
            )));
        }
        Ok(())
    }

    pub(crate) fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|e| AppError::Database(format!("读取表名失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表名失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(0)
                .map_err(|e| AppError::Database(format!("解析表名失败: {e}")))?;
            if name.eq_ignore_ascii_case(table) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn has_column(
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        let sql = format!("PRAGMA table_info(\"{table}\");");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("读取表结构失败: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| AppError::Database(format!("查询表结构失败: {e}")))?;
        while let Some(row) = rows.next().map_err(|e| AppError::Database(e.to_string()))? {
            let name: String = row
                .get(1)
                .map_err(|e| AppError::Database(format!("读取列名失败: {e}")))?;
            if name.eq_ignore_ascii_case(column) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<bool, AppError> {
        Self::validate_identifier(table, "表名")?;
        Self::validate_identifier(column, "列名")?;

        if !Self::table_exists(conn, table)? {
            return Err(AppError::Database(format!(
                "表 {table} 不存在，无法添加列 {column}"
            )));
        }
        if Self::has_column(conn, table, column)? {
            return Ok(false);
        }

        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};");
        conn.execute(&sql, [])
            .map_err(|e| AppError::Database(format!("为表 {table} 添加列 {column} 失败: {e}")))?;
        log::info!("已为表 {table} 添加缺失列 {column}");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 验证 v12 数据库升级后具备完整 v13 列，并且重复执行保持幂等。
    #[test]
    fn migrate_v12_to_v13_adds_input_token_semantics_columns() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE proxy_request_logs (request_id TEXT PRIMARY KEY)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE usage_daily_rollups (date TEXT PRIMARY KEY)",
            [],
        )?;
        Database::set_user_version(&conn, 12)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::has_column(
            &conn,
            "proxy_request_logs",
            "input_token_semantics"
        )?);
        assert!(Database::has_column(
            &conn,
            "usage_daily_rollups",
            "input_token_semantics"
        )?);

        Ok(())
    }

    #[test]
    fn migrate_v13_to_v14_adds_grokbuild_proxy_row_and_preserves_values() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn)?;
        conn.execute("DELETE FROM proxy_config WHERE app_type = 'grokbuild'", [])?;
        conn.execute(
            "UPDATE proxy_config SET enabled = 1, max_retries = 9 WHERE app_type = 'codex'",
            [],
        )?;
        Database::set_user_version(&conn, 13)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        let grok_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_config WHERE app_type = 'grokbuild'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(grok_rows, 1);
        let codex_values: (i64, i64) = conn.query_row(
            "SELECT enabled, max_retries FROM proxy_config WHERE app_type = 'codex'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(codex_values, (1, 9));

        Ok(())
    }

    #[test]
    fn migrate_v14_to_v15_adds_grokbuild_skill_and_mcp_flags() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE mcp_servers (
                id TEXT PRIMARY KEY,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0
            );
            CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                enabled_codex BOOLEAN NOT NULL DEFAULT 0
            );",
        )?;
        conn.execute(
            "INSERT INTO mcp_servers (id, enabled_codex) VALUES ('mcp-1', 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO skills (id, enabled_codex) VALUES ('skill-1', 1)",
            [],
        )?;
        Database::set_user_version(&conn, 14)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        assert!(Database::has_column(
            &conn,
            "mcp_servers",
            "enabled_grokbuild"
        )?);
        assert!(Database::has_column(&conn, "skills", "enabled_grokbuild")?);
        let mcp_values: (i64, i64) = conn.query_row(
            "SELECT enabled_codex, enabled_grokbuild FROM mcp_servers WHERE id = 'mcp-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let skill_values: (i64, i64) = conn.query_row(
            "SELECT enabled_codex, enabled_grokbuild FROM skills WHERE id = 'skill-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(mcp_values, (1, 0));
        assert_eq!(skill_values, (1, 0));

        Ok(())
    }

    #[test]
    fn migrate_v15_to_v16_resets_only_codex_session_usage() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn)?;
        conn.execute_batch(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens,
                output_tokens, cache_read_tokens, latency_ms, status_code,
                created_at, data_source
             ) VALUES
                ('codex-row', '_codex_session', 'codex', 'gpt', 1, 1, 0, 0, 200, 1, 'codex_session'),
                ('gemini-row', '_gemini_session', 'gemini', 'gemini', 1, 1, 0, 0, 200, 1, 'gemini_session');
             INSERT INTO usage_daily_rollups (date, app_type, provider_id, model)
             VALUES
                ('2026-07-10', 'codex', '_codex_session', 'gpt'),
                ('2026-07-10', 'gemini', '_gemini_session', 'gemini');
             INSERT INTO session_log_sync
                (file_path, last_modified, last_line_offset, last_synced_at)
             VALUES
                ('/old/sessions/rollout-old-00000000-0000-4000-8000-000000000001.jsonl', 1, 1, 1),
                ('/gemini/tmp/session-123.json', 1, 1, 1);",
        )?;
        Database::set_user_version(&conn, 15)?;

        Database::apply_schema_migrations_on_conn(&conn)?;

        assert_eq!(Database::get_user_version(&conn)?, SCHEMA_VERSION);
        let counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'gemini_session'),
                (SELECT COUNT(*) FROM usage_daily_rollups WHERE provider_id = '_codex_session'),
                (SELECT COUNT(*) FROM session_log_sync)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(counts, (0, 1, 0, 1));
        Ok(())
    }

    #[test]
    fn repair_deepseek_native_responses_splits_legacy_router_idempotently() -> Result<(), AppError>
    {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn)?;

        let deepseek_settings = json!({
            "auth": {"OPENAI_API_KEY": "sk-test"},
            "modelCatalog": {
                "models": [
                    {"model": "deepseek-v4-flash"},
                    {"model": "deepseek-v4-pro"}
                ]
            },
            "config": "model = \"deepseek-v4-flash\"\n[model_providers.custom]\nbase_url = \"https://api.deepseek.com\"\nwire_api = \"responses\""
        });
        let router_settings = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "router-codex-deepseek",
                    "label": "DeepSeek",
                    "enabled": true,
                    "targetProviderId": "codex-deepseek",
                    "match": {
                        "models": ["deepseek-v4-flash", "deepseek-v4-pro"],
                        "prefixes": ["deepseek"]
                    },
                    "upstream": {
                        "apiFormat": "openai_chat",
                        "auth": {"source": "provider_config"}
                    },
                    "capabilities": {
                        "codexCache": {
                            "cacheMode": "deepseek_context_cache",
                            "usageFields": [
                                "usage.prompt_cache_hit_tokens",
                                "usage.prompt_cache_miss_tokens"
                            ]
                        }
                    }
                }]
            }
        });

        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES ('codex-deepseek', 'codex', 'DeepSeek', ?1, ?2)",
            params![
                deepseek_settings.to_string(),
                json!({"apiFormat": "openai_chat"}).to_string()
            ],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES ('codex-multirouter', 'codex', 'GPT Router', ?1, ?2)",
            params![router_settings.to_string(), "{}".to_string()],
        )?;

        Database::repair_deepseek_native_responses_on_conn(&conn)?;

        let (settings_text,): (String,) = conn.query_row(
            "SELECT settings_config FROM providers WHERE id='codex-multirouter'",
            [],
            |row| Ok((row.get(0)?,)),
        )?;
        let settings: serde_json::Value =
            serde_json::from_str(&settings_text).map_err(|e| AppError::Database(e.to_string()))?;
        let routes = settings
            .pointer("/codexRouting/routes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| AppError::Config("missing routes".to_string()))?;

        assert_eq!(routes.len(), 2);
        assert_eq!(
            routes[0]
                .pointer("/upstream/apiFormat")
                .and_then(serde_json::Value::as_str),
            Some("openai_responses")
        );
        assert_eq!(
            routes[0].pointer("/match/models"),
            Some(&json!(["deepseek-v4-flash"]))
        );
        assert_eq!(
            routes[0].pointer("/match/prefixes"),
            Some(&json!(["deepseek-v4-flash"]))
        );
        assert_eq!(
            routes[1]
                .pointer("/upstream/apiFormat")
                .and_then(serde_json::Value::as_str),
            Some("openai_chat")
        );
        assert_eq!(
            routes[1].pointer("/match/models"),
            Some(&json!(["deepseek-v4-pro"]))
        );
        assert_eq!(
            routes[1].pointer("/match/prefixes"),
            Some(&json!(["deepseek-v4-pro"]))
        );
        let (deepseek_meta_text,): (String,) = conn.query_row(
            "SELECT meta FROM providers WHERE id='codex-deepseek'",
            [],
            |row| Ok((row.get(0)?,)),
        )?;
        let deepseek_meta: serde_json::Value = serde_json::from_str(&deepseek_meta_text)
            .map_err(|e| AppError::Database(e.to_string()))?;
        assert_eq!(
            deepseek_meta
                .get("apiFormat")
                .and_then(serde_json::Value::as_str),
            Some("openai_responses")
        );

        Database::repair_deepseek_native_responses_on_conn(&conn)?;
        let (settings_text_after,): (String,) = conn.query_row(
            "SELECT settings_config FROM providers WHERE id='codex-multirouter'",
            [],
            |row| Ok((row.get(0)?,)),
        )?;
        let settings_after: serde_json::Value = serde_json::from_str(&settings_text_after)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let routes_after = settings_after
            .pointer("/codexRouting/routes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| AppError::Config("missing routes".to_string()))?;
        assert_eq!(routes_after.len(), 2);

        Ok(())
    }

    #[test]
    fn repair_deepseek_native_responses_keeps_separate_pro_provider_on_chat() -> Result<(), AppError>
    {
        let conn = Connection::open_in_memory()?;
        Database::create_tables_on_conn(&conn)?;

        let flash_settings = json!({
            "auth": {"OPENAI_API_KEY": "sk-test"},
            "base_url": "https://api.deepseek.com",
            "apiFormat": "openai_responses",
            "modelCatalog": {"models": [{"model": "deepseek-v4-flash"}]}
        });
        let pro_settings = json!({
            "auth": {"OPENAI_API_KEY": "sk-test"},
            "base_url": "https://api.deepseek.com",
            "apiFormat": "openai_chat",
            "modelCatalog": {"models": [{"model": "deepseek-v4-pro"}]}
        });
        // This is the live corruption seen in the user report: the route still targets the
        // dedicated Pro/Chat provider, but an earlier startup repair rewrote its protocol and
        // match window to Flash/Responses.
        let router_settings = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "router-deepseek-flash",
                        "label": "DeepSeek Flash",
                        "enabled": true,
                        "targetProviderId": "deepseek-flash",
                        "match": {
                            "models": ["deepseek-v4-flash"],
                            "prefixes": ["deepseek-v4-flash"]
                        },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": {"source": "provider_config"}
                        }
                    },
                    {
                        "id": "router-deepseek-pro",
                        "label": "DeepSeek Pro Chat",
                        "enabled": true,
                        "targetProviderId": "deepseek-pro",
                        "match": {
                            "models": ["deepseek-v4-flash"],
                            "prefixes": ["deepseek-v4-flash"]
                        },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": {"source": "provider_config"}
                        }
                    }
                ]
            }
        });

        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES ('deepseek-flash', 'codex', 'DeepSeek Responses', ?1, ?2)",
            params![
                flash_settings.to_string(),
                json!({"apiFormat": "openai_responses"}).to_string()
            ],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES ('deepseek-pro', 'codex', 'DeepSeek Chat', ?1, ?2)",
            params![
                pro_settings.to_string(),
                json!({"apiFormat": "openai_chat", "apiFormatSource": "manual"}).to_string()
            ],
        )?;
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES ('codex-multirouter', 'codex', 'Router', ?1, '{}')",
            params![router_settings.to_string()],
        )?;

        Database::repair_deepseek_native_responses_on_conn(&conn)?;

        let settings_text: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id='codex-multirouter' AND app_type='codex'",
            [],
            |row| row.get(0),
        )?;
        let settings: serde_json::Value =
            serde_json::from_str(&settings_text).map_err(|e| AppError::Database(e.to_string()))?;
        let routes = settings
            .pointer("/codexRouting/routes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| AppError::Config("missing routes".to_string()))?;
        let pro_route = routes
            .iter()
            .find(|route| route.get("targetProviderId") == Some(&json!("deepseek-pro")))
            .ok_or_else(|| AppError::Config("missing Pro route".to_string()))?;

        assert_eq!(
            pro_route.pointer("/upstream/apiFormat"),
            Some(&json!("openai_chat"))
        );
        assert_eq!(
            pro_route.pointer("/match/models"),
            Some(&json!(["deepseek-v4-pro"]))
        );
        assert_eq!(
            pro_route.pointer("/match/prefixes"),
            Some(&json!(["deepseek-v4-pro"]))
        );

        Database::repair_deepseek_native_responses_on_conn(&conn)?;
        let settings_text_after: String = conn.query_row(
            "SELECT settings_config FROM providers WHERE id='codex-multirouter' AND app_type='codex'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(settings_text_after, settings_text);

        Ok(())
    }
}

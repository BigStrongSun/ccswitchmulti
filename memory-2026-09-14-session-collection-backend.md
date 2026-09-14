# Session collection backend contract

- Periodic and manual session usage collection must go through one coordinator and the existing `session_sync_mutex`; schedule the next automatic pass only after the current pass completes.
- `SessionCollectionStatus` is non-sensitive process state. It starts as `not_started`, has monotonic `revision`, uses Unix seconds, keeps the previous successful timestamp on errors, and emits `session-collection-updated` independently from `usage-log-recorded`.
- `get_codex_subagent_usage_stats` is a CCSM database-only read: `codex_usage_sessions` supplies structured subagent and parent metadata, while `proxy_request_logs` rows restricted to `data_source='codex_session'` provide usage. Do not reopen Codex history SQLite or rollout JSONL in this query, and never mix proxy traffic.
- `last_seen_at` is an ingestion clock, not event time. Date ranges are established by usage facts; metadata `first_activity_at`/`last_activity_at` are only conservative missing/unknown evidence. Parent direct usage remains `unknown_may_overlap` until a durable attribution-provenance boundary proves legacy rows cannot contain child usage.

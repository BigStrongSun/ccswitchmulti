# Codex traffic frontend semantics

- The MultiRouter status traffic page has two non-additive layers: the request table is only the current API's latest 50 `request_log` samples, while the session panel is bounded local Codex history plus `codex_session` evidence.
- Router diagnostic events are evidence for routing but are not request usage or latency; do not feed them into request/token/average-latency aggregates.
- `CodexSubagentUsageStats` must mark missing local evidence explicitly. Compatibility numeric zeroes on missing sessions never mean zero tokens or zero dollars.
- Parent groups originate only from explicit direct `session_meta.source.subagent.thread_spawn.parent_thread_id`; groups overlap top-level sessions/model rows, therefore must remain visually separate and never be summed.
- A range is observed only with time-verifiable token evidence. `unknownRangeAgents` is a subset of missing usage, not an extra total; model `agentCount` may cover observed sessions only, so display observed plus missing counts.
- A parent sync row can overlap child consumption. When `parentUsageStatus=unknown_may_overlap`, render the explicit non-additive warning instead of treating zero or a parent value as trustworthy.
- History parsing may read rollout files. The status page must not auto-poll that query until a backend cached lightweight revision API exists; manual session sync invalidates usage queries.

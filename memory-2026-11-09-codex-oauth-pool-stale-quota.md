# 2026-11-09 Codex OAuth pool stale quota snapshot root fix

## Relationship to the ImageGen failure

- `Codex MultiRouter v2 did not resolve model gpt-image-2` and the later `POOL-001` 503 are separate failures in the same request chain.
- The model-resolution fix is recorded in `memory-2026-11-09-gpt-image-2-router-fallback.md`.
- After that fix, ImageGen reached OpenAI Official and exposed the account-pool bug documented by thread `01a1bf08-baf7-7d40-baa6-e1c07af39000`.

## Root cause

- `CodexPoolRuntimeState` used `quota_checked_at_ms` for both the latest refresh attempt and the latest successful quota observation.
- A failed `/wham/usage` request called `mark_quota_checked`, which renewed the stale observation time without replacing `remaining_percent`.
- If the stale remaining value was at or below `reservePercent`, the account remained excluded from candidates even after the observation should have expired.
- The existing soft-avoid fallback also refused the account because it treated the stale reserve value as current, producing an empty pool and the immediate `cc_switch_no_available_provider` 503 without any upstream attempt.

## Fix

- Split runtime state into:
  - `quota_attempted_at_ms`: controls refresh throttling.
  - `quota_observed_at_ms`: controls freshness of the quota snapshot.
- Successful quota probes update both timestamps.
- Failed probes update only the attempt timestamp.
- Candidate reserve checks use `fresh_remaining_percent`; an observation older than `QUOTA_REFRESH_TTL_MS` no longer enforces its stale reserve value.
- Credential invalidation clears both quota timestamps and the cached remaining value.
- Renamed manager/runtime APIs to make the distinction explicit:
  - `mark_pool_quota_attempted`
  - `mark_quota_attempted`

## Regression coverage

- `stale_quota_observation_expires_even_after_failed_refresh_attempt`
- `successful_quota_refresh_renews_observation_and_retry_time`
- Full pool state suite: 10/10 passed.
- OAuth manager suite: 48/48 passed.
- Official pool forwarder regression filter: 4/4 passed.

## Remaining runtime boundary

- The installed 3.20.4-11 process does not include this source fix yet.
- Direct access to `chatgpt.com` currently resolves to `198.18.0.15` and fails TLS. The configured upstream proxy `10.106.130.5:3128` reaches the endpoint and returns HTTP 401 without credentials.
- The live pool also inherited stale runtime soft-avoid/reserve state from before this fix. A new build or runtime proxy/client reset is still required for live ImageGen acceptance.

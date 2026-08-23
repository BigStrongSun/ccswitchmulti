# 2026-08-23 Route label write-back boundary

- MultiRouter has four distinct identities: route `id` for stable identity,
  `targetProviderId` for target resolution, visible catalog `model` for request
  matching, and `upstreamModel` for the outbound request. A human-readable
  route label is not a routing key.
- The UUID-label presentation fix had allowed `createRoutePolicyDraft()` to
  replace an opaque legacy label with the target Provider display name. Saving
  the untouched draft could therefore persist a display fallback as route data.
  Although the current V2 runtime resolves by compiled visible model and
  `targetProviderId`, presentation code must not mutate configuration because
  legacy semantic fallback paths still inspect labels.
- `createRoutePolicyDraft()` now preserves the persisted label verbatim; UI
  rendering continues to use `routeSummaryDisplayName()` for UUID fallback.
  Regression coverage locks this behavior, alongside V2 exact/prefix/default
  resolution tests.
- A related legacy-recovery hazard existed in `findSemanticRouteProvider()`:
  missing `targetProviderId` could be inferred from the first Provider sharing
  a model name. Recovery now accepts a name or model match only when it is
  unique; ambiguous legacy routes remain unresolved and cannot silently bind
  to a different Provider during a later save.
- Read-only local evidence on 2026-08-23: the installed configuration remains
  a legacy plan and its recent logs route `gpt-5.6-terra` to
  `router-codex-official` and `deepseek-v4-flash` to the DeepSeek route. This
  does not prove the reported user's V2 configuration, so the report must not
  be closed without a trace from that configuration.

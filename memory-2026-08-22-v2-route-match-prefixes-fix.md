# 2026-08-22 V2 route `matchPrefixes` runtime compatibility

- Issue #29 exposed that raw-settings routing helpers still recognized only legacy
  `match.prefixes` / `modelPrefixes`, while persisted MultiRouter V2 routes use
  top-level `matchPrefixes` and `modelSelection`.
- The compiled V2 route resolver already consumed `match_prefixes`; the remaining
  defect was in raw fallbacks used by request routing, Sub-Agent/catalog
  classification, and external model listing.
- Preserve old fields and add V2 fields everywhere: `matchPrefixes` for prefix
  matching and `modelSelection.models` for V2 `include` exact matching. This keeps
  a DeepSeek/GLM route from silently falling back to the first official route.
- Regression: a V2 DeepSeek prefix and V2 include entry both resolve before the
  official fallback route.

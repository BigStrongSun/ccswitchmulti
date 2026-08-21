# 2026-08-21 MultiRouter upstreamModel alias regression

- `3.19.2-10` feedback reproduced on current `main` (`fc0cc7c6`, version `3.19.2-12`): a Provider catalog row with `model=deepseek-v4-flash` and `upstreamModel=deepseek-v4-flash-0731` plus a route alias targeting `deepseek-v4-flash-0731` failed with `alias_target_missing`.
- Root cause was a split canonical identity: the frontend/wizard treated `upstreamModel` as canonical, while the Rust MultiRouter compiler and schema validator only indexed `model`. This was a real current-main regression, not only an old `3.19.2-10` artifact.
- Fixed in the local commit after this note:
  - compiler indexes both visible and upstream identities, uses upstream as the canonical model, preserves the visible catalog name for automatic projection, and accepts aliases targeting either identity;
  - schema validation treats visible and upstream names from the same catalog row as equivalent for include selection;
  - compile diagnostics show route label and Provider name while retaining stable IDs in parentheses;
  - wizard alias diagnostics now show route label and Provider name.
- Regression coverage: Rust compiler 13/13, schema 6/6, `tests/lib/codexMultiRouterWizard.test.ts` 34/34, and `tests/components/CodexMultiRouterWizard.test.tsx` 31/31.
- GitHub API verification on 2026-08-21: `BigStrongSun/ccswitchmulti` PR `#11` is a closed release-notes PR (`docs(release): finalize v3.19.1-31 notes`), not this alias bug. Therefore “bug #11” in the local audit is historical shorthand; the visible alias/upstream behavior was claimed as published before, but this concrete `alias_target_missing` path remained untested and unfixed until this commit.
- Independent search status: Codex web search returned no useful result for the private/fork issue; Matrix WebSearch also returned no reliable repository result. The conclusion is based on official GitHub API plus local reproduction and tests.

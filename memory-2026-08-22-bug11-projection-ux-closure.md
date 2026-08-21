# Bug11 projection UX closure

## Root cause

The backend already maintained a Codex MultiRouter projection as the source of
truth and exposed read-only inspection plus retry APIs. The workspace status
tab did not call those APIs, so users could see a running proxy while the
Provider/model catalog and Codex live projection were stale. Historical
projection diagnostics also carried `router-<UUID>` labels that were not
appropriate for user-facing text.

## Change

- Added a status panel to the Codex MultiRouter workspace.
- The panel reports pending/ready projection state, error code/reason, route
  mappings, and a direct resync action.
- Route labels now use the shared display fallback: opaque route IDs fall back
  to the readable target Provider name; raw IDs remain in diagnostic metadata.
- Added an async regression test covering stale state, alias
  `visibleModel -> upstreamModel`, readable Provider display, and successful
  resync.

## Verification

- `pnpm exec vitest run src/components/codex/CodexRouterWorkspacePage.test.ts`
  passed: 61/61.
- `cargo test --manifest-path src-tauri/Cargo.toml codex_multirouter --lib`
  passed: 56/56.
- `pnpm run typecheck` passed.
- Prettier check passed for both changed files.
- UTF-8 strict decode passed; no BOM was introduced.

## Runtime/release boundary

The release and installation boundary was closed on 2026-08-21:

- Release metadata points to commit `687f503b` and version `3.19.2-13`.
- The transactional installer completed with `Status=Success`; rollback was
  not needed.
- Installed
  `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` reports
  `3.19.2-13`, runs from the product directory, and owns listener
  `127.0.0.1:15721`.
- Installed SHA-256
  `27B36758C27F5079F4D90F156E402719A9E6FA3A2CEAA976CB6C70D13FD28`
  matches the exported NSIS installed-exe manifest.
- `/health` returned HTTP 200. A direct unauthenticated `/v1/models` request
  returned HTTP 403 because the external OpenAI API profile is disabled; this
  is expected and is not the Codex router canary.
- The live Codex router log recorded recent `/responses` requests through
  `router-codex-official` with upstream HTTP 200.
- The live Codex catalog contains 9 models including `qwen3.8` and no
  `qwen3.6`; the current DB route labels are readable Provider names rather
  than opaque UUIDs.

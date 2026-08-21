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

The installed app remains `3.19.2-12` at
`C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`. The latest local
release raw executable has a different SHA-256, so this source fix is not yet
installed or runtime-verified. Do not claim the installed app contains this
panel until a new release is built and the transactional installer completes.

# 2026-08-21 MultiRouter route label residual

- The installed `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe` reports
  version `3.19.2-12`, but the local release metadata still points to commit
  `7fa34836`; it is not evidence of the current alias fix being installed.
- The compiler fix for `alias_target_missing` is present on `main` and has
  regression coverage for both `All` and `Include` selection when a catalog row
  has `model=deepseek-v4-flash` and
  `upstreamModel=deepseek-v4-flash-0731`.
- A separate presentation defect remained: the MultiRouter settings panel built
  its route-name lookup from the selected plan entries rather than the global
  target Provider map, and the wizard still rendered legacy UUID labels directly.
- Fixed by resolving route labels against the target Provider map in settings,
  using one `wizardRouteDisplayLabel` fallback in previews and alias errors, and
  adding a regression assertion for a UUID label falling back to `Relay`.
- Focused verification after the fix: 103 Vitest tests passed, TypeScript
  typecheck passed, Prettier check passed, and `git diff --check` passed.

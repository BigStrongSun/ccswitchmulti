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

## Follow-up audit

- The suspected remaining settings-panel defect was rechecked against the
  actual JSX. `routeOptions` already stores the resolved
  `routeDisplayName(route, providersById)` value, and the `<option>` renders
  that mapped `route.label`; it does not render the persisted UUID label.
- Added an end-to-end workspace regression test proving a legacy
  `router-<UUID>` label appears as the target Provider name in the “默认路由”
  select.
- The installed executable is still not the local e6 release: its timestamp
  is 2026-08-21 20:06, while the e6 local release was generated at 22:48, and
  their SHA-256 hashes differ. A user running that installed binary can still
  reproduce the v3.19.2-10 behavior even though current source and the staged
  e6 artifact contain the fix.

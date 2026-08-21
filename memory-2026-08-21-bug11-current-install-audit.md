# 2026-08-21 Bug11 and current-install audit

- The attached feedback screenshot is the old `alias_target_missing` shape:
  route `router-<UUID>` and Provider `<UUID>` are shown directly, while the
  alias target is `deepseek-v4-flash-0731`.
- The actual alias identity fix is already on `main`:
  `bd217d9a` accepts both catalog `model` and `upstreamModel` identities for
  selection and alias validation. The route/provider display fixes are in
  `1b33242f`, `f433fb2d`, `255a6beb`, and `e6f52948`; `93da81dd` adds the
  legacy-label workspace regression.
- The current local database (`~/.cc-switch/cc-switch.db`) does not contain
  `deepseek-v4-flash-0731`, the screenshot UUID, or an `alias_target_missing`
  record. Its MultiRouter routes use readable labels such as `OpenAI Official`,
  `DeepSeek-responses`, and `Qwen`; recent router logs contain successful
  routed requests and no matching alias error.
- The installed executable is version `3.19.2-12`, installed at 23:18:40,
  SHA-256 `C14927EB6C800A7C3F50B6D1CC35D0D773399D20756D539308181FBCC08BFF22`.
  The latest local release metadata is also `3.19.2-12`, but is bound to
  `e678fa37` and its newly exported raw executable has a different hash
  `335F6A5CB5C69FCA1EA07F10C41347832B5CAFCC462105DA21B829ADEF3007D6`;
  `e678fa37` is documentation-only after the functional fix, so behavior is
  expected to match the installed `93da81dd` artifact.
- All relevant fix commits are contained only by local `main`; no other local
  branch contains an unmerged alias/UUID fix. The public searches did not
  produce a reliable repository result for “bug11”; local GitHub/API audit
  identified PR #11 as an unrelated closed release-notes PR, not this alias
  defect.
- If a user still sees the exact screenshot on a claimed `3.19.2-12` build,
  first verify the executable hash and inspect the active `~/.cc-switch` data.
  If the error still names only `router-<UUID>`, the process is using an older
  binary or stale route/catalog data; if the current compiler names the
  Provider and still rejects the alias, the Provider catalog no longer
  contains either the visible model or its `upstreamModel`, which is a real
  configuration mismatch rather than the fixed identity bug.

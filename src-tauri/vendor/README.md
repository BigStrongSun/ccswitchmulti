# Vendored Rust dependencies

## tao 0.34.6

- Source: crates.io `tao` 0.34.6 (`97737b1aa2f36b260a425a0cd4dd5843abbf1e2b`).
- Reason: tao 0.34.3 introduced a Windows `WM_ENDSESSION` handler that emits
  `LoopDestroyed` from inside `DispatchMessageW`. The Win32 message loop is
  still alive at that point, so a queued paint or user event can attempt to
  leave the terminal `Destroyed` state and panic.
- Local change: a confirmed end-session posts `WM_QUIT`; `run_return` then
  exits `GetMessageW` and emits `LoopDestroyed` once from the normal terminal
  path. A cancelled end-session leaves the message loop running.
- Removal condition: delete the `[patch.crates-io]` entry and this vendored
  copy only after upstream tao ships equivalent ordering and its regression
  test is retained.

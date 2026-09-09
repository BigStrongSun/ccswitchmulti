# 2026-09-09 History Integrity Source Fix

## Scope

- Isolated branch `bigstrongsun/history-integrity-fix`, based on `e69830b7`.
- Do not install this branch or treat tests as proof of repaired user history.
  The active checkout has unrelated work and was not merged or overwritten.
- Earlier diagnosis is in the LLMservice root:
  `memory-2026-09-09-codex-history-root-causes.md`.

## Root Causes And Contracts

1. Provider migration rewrites canonical rollout bytes and restores mtime without
   updating projection cursors or cross-rollout `history_base` byte offsets.
   Offline execution and backups alone do not make this safe.
2. Active-lineage validation incorrectly required ancestor session IDs to equal
   the child thread ID. Valid forks have different ancestor IDs.
3. Relaxing identity validation alone would admit the observed migrated ancestor
   whose cutoff points inside a record. A cutoff must end after a newline.

## Implementation

- `codex_history_migration_guard.rs` preflights sessions, archived sessions, and
  resolved state databases before a batch can mutate history. Paginated/unknown
  modes, ordinal/history-base markers, unreadable or unknown headers, and
  uninspected compressed histories fail closed.
- Database-only paginated rows are protected even when the rollout is absent.
  SQLite inspection is read-only. State DB discovery uses the passed Codex home,
  config sqlite_home, and the existing environment override precedence.
- Migration, official restore, visibility repair, and direct writers use the
  same guard. Direct JSONL writers inspect the exact content they would rewrite.
  Legacy histories remain supported. A protected batch is not marked migrated.
- Stable refusal marker: `codex_paginated_history_immutable`. Exiting the App
  does not remove this format-level protection.
- Only the active segment must match the requested thread ID. Cycle, missing
  parent, paginated-mode, and bounds checks remain; a zero or mid-record cutoff
  is rejected with `history_base_offset_not_record_boundary`.
- UI wording no longer attributes all history failures to Codex alone.

## Verification And Delivery Boundary

- RED: six migration-integrity cases failed; legacy control passed. Two lineage
  cases failed; foreign-active-thread and existing lineage controls passed.
- First GREEN migration run: 61 passed. Expanded `cargo test --offline --lib
  codex_ -- --test-threads=1`: 1510 passed, 1 existing ignored test.
- Final source-bound Cargo rerun of `codex_history_migration` plus
  `active_history_base_lineage`: 68 passed, including all 9 integrity cases and
  all 5 lineage cases. Shared target binaries can be overwritten by concurrent
  worktrees; do not reuse an arbitrary test EXE as build-provenance evidence.
- Scoped rustfmt, locale JSON Prettier, git diff checks, and strict UTF-8 without
  BOM or replacement characters passed. No frontend layout or interaction was
  changed; no Desktop UI validation or full application release build is claimed.
- No real rollout, state database, projection cursor, config, installed binary,
  or running Codex/CCSM process was modified.
- Separate Codex-source work corrects resume ordinal allocation. This CCSM fix
  prevents unsafe provider rewrites; it does not recover old damaged history.
- Built-in Web and Matrix were independently attempted. No sufficient public
  evidence resolved the exact local failure; local source and RED/GREEN tests
  establish these changes, not a claimed upstream release fix.

# Post-PR36 Main-Aligned Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve the valid defects identified in PRs #57-#64 and #66 without reintroducing Router-owned Provider snapshots or unsafe ownership guesses.

**Architecture:** Provider records remain the model/capability SSOT. Every MultiRouter live projection derives a fresh artifact and a fresh Provider classification context from the same database boundary. Codex live-config and MCP changes share one optimistic writer and explicit ownership receipts; startup recovery trusts a listener only when process identity and ownership evidence agree.

**Tech Stack:** Rust, SQLite/rusqlite, TOML, Tauri, React/Vitest, Windows process APIs.

**Spec:** `memory.md` section `2026-08-25 协议探测合入后 PR #37-#66 重新评估`.

## Global Constraints

- Do not persist Provider-derived `modelCatalog` into schema-v2 Router records.
- Do not infer MCP ownership from a matching identifier alone.
- Do not replace or kill a listener using PID existence or self-reported HTTP identity alone.
- Add a failing regression before each production change.
- Preserve unrelated untracked build and preview artifacts.

---

### Task 1: Provider-Aware Projection Publishing

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/codex_multirouter/mutation.rs`
- Modify: `src-tauri/src/codex_multirouter/projection.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`

- [ ] Add a regression proving a no-prefix `mode=all` route publishes its managed Agent file from the target Provider catalog.
- [ ] Run the regression and verify that the current no-context publisher fails.
- [ ] Introduce a database-bound projection publisher that derives `ProviderClassificationContext` immediately before projection.
- [ ] Route Provider update, deletion, activation, retry, and recovery publication through that boundary.
- [ ] Run projection, mutation, Provider, and Codex catalog tests.
- [ ] Commit the independently passing change.

### Task 2: Safe Prefix-Only Legacy Migration

**Files:**
- Modify: `src-tauri/src/codex_multirouter/migration.rs`

- [ ] Add regressions for prefix-only migration, alias resolution, frozen include semantics, and `enabled=false` exclusion.
- [ ] Run them and verify prefix-only routes currently produce an empty include or incorrect selection.
- [ ] Expand legacy prefixes only against enabled current Provider catalog entries and retain visible/upstream identity mapping.
- [ ] Reject empty or unmatched prefix-only migrations with stable diagnostic codes.
- [ ] Run migration and compiler tests.
- [ ] Commit the independently passing change.

### Task 3: Unified Codex Live-Config Writer and MCP Ownership

**Files:**
- Modify or create focused modules under `src-tauri/src/codex_config/` as the existing layout permits.
- Modify: `src-tauri/src/codex_config.rs`
- Modify MCP service/database schema files identified during root-cause tracing.

- [ ] Add concurrency regressions for an external edit between snapshot and commit.
- [ ] Add ownership regressions proving a same-ID user MCP is preserved without a CCSM receipt.
- [ ] Implement one bounded optimistic writer used by Provider projection and MCP reconciliation.
- [ ] Persist explicit Codex MCP ownership receipts only after a verified successful managed write.
- [ ] Make deletion reconcile receipts, not all CCSM database MCP identifiers.
- [ ] Run database, MCP, Provider, and live-config suites.
- [ ] Commit the independently passing change.

### Task 4: Startup Listener Ownership and Recovery Outcomes

**Files:**
- Modify startup/proxy recovery modules identified during root-cause tracing.
- Modify database schema and frontend recovery presentation only where required by the final model.

- [ ] Add regressions for PID reuse, executable mismatch, listener mismatch, and stale markers.
- [ ] Add regressions proving one app's success cannot overwrite another app's failure.
- [ ] Bind owned-process evidence to PID, executable identity, process creation time, listener, and compatible runtime/config version.
- [ ] Refuse takeover or termination when any ownership proof is absent or contradictory.
- [ ] Store bounded per-app/per-operation outcomes with severity ordering and acknowledgment/generation lifecycle.
- [ ] Run startup, proxy, recovery, database, and frontend presentation tests.
- [ ] Commit the independently passing change.

### Task 5: Final Verification and Repository Memory

**Files:**
- Modify: `memory.md`

- [ ] Run focused tests for every repaired boundary.
- [ ] Run Rust library tests, typecheck, renderer build, formatting, diff, and strict UTF-8/no-BOM checks.
- [ ] Record exact behavior, evidence, remaining uncertainty, and install/release boundary in `memory.md`.
- [ ] Commit the verification record with the required attribution footer.

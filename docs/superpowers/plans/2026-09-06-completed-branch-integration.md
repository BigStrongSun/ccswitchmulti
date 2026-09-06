# Completed Branch Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Audit every local branch that diverges from `main`, merge only completed product work that is still missing, and preserve active, historical, superseded, documentation-only, and no-go branches.

**Architecture:** Treat Git ancestry as the first filter and repository memory plus current source behavior as the semantic filter. Perform integration in an ignored linked worktree based on `main`, verify the exact merged tree, then fast-forward the checked-out local `main` without deleting branches or worktrees.

**Tech Stack:** Git worktrees, Rust/Cargo, pnpm/Vitest/TypeScript, PowerShell/Pester.

**Spec:** User request in the 2026-09-06 task: inspect current branch divergence, identify completed versus ongoing branches, and merge all completed product work into `main`.

## Global Constraints

- Preserve the main checkout's untracked `.tmp/` and `docs/provider-settings-layout-preview.html`.
- Do not merge branches classified as no-go, historical/backup, documentation-only, superseded by current `main`, or incomplete product prototypes.
- Do not delete any branch or worktree during this integration.
- Repository text remains UTF-8 without BOM or U+FFFD.
- Every integration-related commit message ends with `本次提交由BigStrongsSun完成`.

---

### Task 1: Build the branch disposition matrix

**Files:**
- Modify: `memory.md`
- Create: `docs/superpowers/plans/2026-09-06-completed-branch-integration.md`

**Interfaces:**
- Consumes: local refs, `git worktree list`, branch tips, existing branch-audit memory.
- Produces: an auditable classification of each divergent branch.

- [ ] Record branches already contained in `main` as no-op.
- [ ] Record superseded and patch-equivalent branches without merging them.
- [ ] Record explicit no-go, historical, backup, and documentation-only branches without merging them.
- [ ] Record incomplete product prototypes as active work, not completed work.
- [ ] Identify completed product branches whose behavior is absent from `main`.

### Task 2: Integrate completed product work

**Files:**
- Merge: `bigstrongsun/fix-gpt6-astra-metadata`
- Modify on conflict only: files touched by that branch

**Interfaces:**
- Consumes: clean integration branch based on `main@770968b5` and Astra branch `1e138230`.
- Produces: a merge commit preserving both branch commits and current main history.

- [ ] Merge with `--no-ff` and a traceable message.
- [ ] Resolve conflicts by retaining current main architecture plus the branch's official reasoning metadata propagation and legacy effort migration.
- [ ] Inspect the resulting diff and ensure no unrelated branch content entered.

### Task 3: Verify and advance local main

**Files:**
- Verify: all merged source, tests, release scripts, and documentation
- Modify: `memory.md`

**Interfaces:**
- Consumes: the exact merged integration tree.
- Produces: a tested local `main` and a durable audit record.

- [ ] Run TypeScript typecheck and the full frontend unit suite.
- [ ] Run Rust full tests, rustfmt check, and `git diff --check`.
- [ ] Run installation transaction Pester tests.
- [ ] Strictly decode changed text as UTF-8 and reject BOM/U+FFFD.
- [ ] Commit the final branch audit to the integration branch.
- [ ] Fast-forward local `main` to the verified integration tip.
- [ ] Recheck main ancestry, main checkout preservation, and repository status.

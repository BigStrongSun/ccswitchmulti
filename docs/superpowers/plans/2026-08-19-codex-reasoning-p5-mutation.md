# Codex Reasoning P5 Mutation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 P4 的 reasoning 只读 inspect/list/validate/export 扩展为安全的 detect/plan/apply/reset 写入闭环，同时保持 GUI、CLI 与未来 MCP 共用同一个领域服务。

**Architecture:** 在现有 P2/P3 resolver 外增加 reasoning mutation domain service。`detect` 只生成并缓存候选；`plan` 与 `apply` 调用同一个规范化/校验函数；`apply/reset` 通过全局 mutation 锁、SQLite 事务、资源 revision、一次性 plan token 和派生产物回读证明完成原子变更。Tauri 与 CLI 只负责参数和 JSON transport，不自带推断或第二套校验。

**Tech Stack:** Rust 2021、Tauri 2 commands、rusqlite、serde/serde_json、SQLite schema migrations、现有 Codex catalog/sub-agent projection、Vitest。

**Spec:** `docs/superpowers/specs/2026-08-17-ccsm-ai-configuration-plane-design.md` 第 8、9、11 节；`docs/superpowers/plans/2026-08-17-codex-reasoning-capability-correction.md` 的 P5/P6。

## Global Constraints

- P0–P5 不改版本号、不发布 release、不替换正在运行的 CCSM。
- 用户模型声明优先；Provider 检测结果只能作为 candidate，采用后才写入 `source=user` 或 `user_confirmed_detection`。
- 不允许通过命令行参数传递密钥；输入使用 JSON 文件、stdin 或安全 secret reference；stdout、stderr、审计、read-back 均不得出现密钥明文。
- `plan` 与 `apply` 必须使用同一语义校验和规范化函数；transport 层不得复制推断规则。
- `apply` 必须携带有效 `planToken`、`expectedRevision` 和显式确认；revision 冲突、token 过期、重复使用均失败并保持原状态。
- 成功返回前必须验证数据库、resolver、Codex 派生产物和受影响 role TOML；任一失败都自动回滚。

---

### Task 1: 建立 mutation 核心状态与 revision/token 持久化

**Files:**
- Create: `src-tauri/src/config_plane/mod.rs`
- Create: `src-tauri/src/config_plane/revision.rs`
- Create: `src-tauri/src/config_plane/plan_token.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/database/mod.rs`
- Modify: `src-tauri/src/database/schema.rs`
- Test: `src-tauri/src/config_plane/mod.rs` unit tests

**Interfaces:**
- Produces `ConfigResourceKey { domain, resource }`, `ResourceRevision { value, updated_at }`, `PlanTokenClaims`, `MutationErrorCode` and `MutationEnvelope<T>`.
- Produces `ConfigPlane::read_revision`, `ConfigPlane::begin_plan`, `ConfigPlane::consume_plan_token` and a process-wide mutation mutex.

- [ ] **Step 1: Write failing tests** for missing revision initialization, monotonic increment, token binding to normalized spec hash/resource/revision, TTL expiry, tamper rejection, and one-time consumption.
- [ ] **Step 2: Run the focused Rust tests** and verify they fail because the config-plane types and tables do not exist.
- [ ] **Step 3: Add schema migration** for `config_revisions` and `config_plan_tokens`; seed a first revision from the current provider content hash without changing existing provider rows.
- [ ] **Step 4: Implement token generation/verification** with at least 128 bits of randomness, a 900-second expiry, normalized-spec hash, resource key, expected revision, and consumed marker.
- [ ] **Step 5: Run the focused tests** and verify all state/token invariants pass.
- [ ] **Step 6: Commit** with the root-cause and migration compatibility in the message, ending with `本次提交由BigStrongsSun完成`.

### Task 2: Implement reasoning detect as a non-persistent candidate operation

**Files:**
- Modify: `src-tauri/src/reasoning_capabilities/provider_metadata.rs`
- Modify: `src-tauri/src/reasoning_capabilities/mod.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Test: `src-tauri/src/reasoning_capabilities/*` tests and `src-tauri/src/codex_config.rs` tests

**Interfaces:**
- Produces `detect_codex_model_reasoning_capability(provider_id, model) -> DiscoveryOutcome` with candidate, evidence, diff against persisted declaration, and no database mutation.
- Reuses the existing TTL detection cache; candidate source remains `provider`/`detection`, never authoritative user configuration.

- [ ] **Step 1: Add failing tests** for vLLM/OpenAI-compatible metadata, unavailable endpoint, malformed metadata, and secret-free evidence allowlisting.
- [ ] **Step 2: Run the focused tests** and capture the missing candidate/diff behavior.
- [ ] **Step 3: Implement endpoint and field allowlists** using the existing provider metadata adapter; classify missing metadata as `unknown`, never `confirmed_unsupported`.
- [ ] **Step 4: Expose the detect result through the shared domain service** without opening a write transaction or changing provider rows.
- [ ] **Step 5: Run tests and verify a real Qwen detect dry-run** leaves the persisted declaration and revision unchanged.
- [ ] **Step 6: Commit** with the detection evidence boundary and no-persistence guarantee, ending with `本次提交由BigStrongsSun完成`.

### Task 3: Implement plan/validate/diff for user-confirmed declarations

**Files:**
- Create: `src-tauri/src/config_plane/reasoning.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/bin/ccsm.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/codexSubagentV2.ts`
- Modify: `src/types/codexSubagentV2.ts`
- Test: `tests/lib/codexReasoningMutationApi.test.ts`
- Test: Rust tests in `src-tauri/src/config_plane/reasoning.rs`

**Interfaces:**
- Consumes `ReasoningDeclarationSpec` containing `schemaVersion`, `kind`, `resource`, and `spec`.
- Produces `ReasoningPlanResponse { planToken, expectedRevision, expiresAt, diff, derivedImpact, warnings }`.
- `ccsm reasoning plan --file <path>` and `--stdin` parse the same JSON model; no YAML or ad-hoc CLI fields are accepted in P5.

- [ ] **Step 1: Write failing tests** for valid boolean, graded, budget, unsupported, and unknown declarations; invalid effort maps; resource mismatch; and secret-free diff output.
- [ ] **Step 2: Run tests** to confirm the current P4 CLI rejects mutation verbs and no plan token is issued.
- [ ] **Step 3: Implement one normalization/validation function** that calls the existing capability schema validation and resolver, canonicalizes aliases, computes before/after field diffs, and records derived catalog/request/sub-agent impact.
- [ ] **Step 4: Store only the opaque plan token claims**; never store plaintext secrets or reasoning content in the token or plan cache.
- [ ] **Step 5: Add CLI/Tauri plan transport** with structured error codes and stable exit codes (`validation_failed=3`, `revision_conflict=4`, `approval_required=5`).
- [ ] **Step 6: Run Rust, Vitest, TypeScript, format and diff checks**, then commit with `本次提交由BigStrongsSun完成`.

### Task 4: Implement atomic apply/reset with derived-artifact read-back

**Files:**
- Modify: `src-tauri/src/config_plane/reasoning.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/database/dao/providers.rs`
- Test: Rust mutation integration tests in `src-tauri/src/config_plane/reasoning.rs` and `src-tauri/src/services/provider/mod.rs`

**Interfaces:**
- `apply_reasoning_plan(spec, plan_token, expected_revision, confirmed) -> ReasoningMutationResponse`.
- `reset_reasoning_capability(provider_id, model, expected_revision, confirmed) -> ReasoningMutationResponse`.

- [ ] **Step 1: Write failing tests** for revision conflict, invalid/expired/replayed token, no-op idempotence, current versus non-current provider, projection failure, and automatic rollback.
- [ ] **Step 2: Run tests** to establish that no provider row or generated file changes on every failure path.
- [ ] **Step 3: Acquire the process mutex and cross-process lock**, re-read the current revision, verify token claims and explicit confirmation, then start the SQLite transaction.
- [ ] **Step 4: Update only the targeted model reasoning declaration**, preserving unrelated provider fields and never promoting detection metadata directly to authoritative source.
- [ ] **Step 5: Rebuild Codex catalog/inline models and managed role files through existing service functions**, then read back database, resolver fingerprint, catalog and role TOML.
- [ ] **Step 6: Commit the transaction only after all read-back checks pass; otherwise restore provider/file snapshots and return a redacted rollback result.**
- [ ] **Step 7: Run the complete Rust suite and targeted front-end tests**, then commit with `本次提交由BigStrongsSun完成`.

### Task 5: Expose the mutation surface and guard the running installation

**Files:**
- Modify: `src-tauri/src/bin/ccsm.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/codexSubagentV2.ts`
- Modify: `src/types/codexSubagentV2.ts`
- Create: `tests/lib/codexReasoningMutationContract.test.ts`
- Modify: `MEMORY.md` in the project root after implementation

**Interfaces:**
- CLI: `reasoning detect`, `reasoning plan`, `reasoning apply`, `reasoning reset`.
- Tauri: `detect_codex_reasoning_capability`, `plan_codex_reasoning_capability`, `apply_codex_reasoning_capability`, `reset_codex_reasoning_capability`.

- [ ] **Step 1: Add transport contract tests** proving CLI, Tauri and shared domain responses use the same envelope and never send secrets in argv.
- [ ] **Step 2: Add a feature flag defaulting mutation off** for the installed/running application; test disabled mutation returns `mutation_disabled` before opening a write transaction.
- [ ] **Step 3: Verify the currently running `cc-switch.exe` process, installation path and PID before any build/install action.**
- [ ] **Step 4: Build only the development binary and run mutation tests against an isolated temporary HOME/database; do not stop or overwrite the live installation.**
- [ ] **Step 5: Update the project-root memory with exact commits, schema version, test results, and remaining P6 canary scope; validate UTF-8/no BOM/no U+FFFD.**
- [ ] **Step 6: Commit the transport, feature flag, and memory update separately, each ending with `本次提交由BigStrongsSun完成`.**

## Verification gates before P6

- `cargo test --lib` has zero failures, including all existing provider/sub-agent tests.
- Mutation integration tests prove DB/file rollback and no-op idempotence.
- `pnpm exec vitest run` targeted contract tests, `npx tsc --noEmit`, `cargo check --lib --bin ccsm`, `cargo fmt --check`, and `git diff --check` pass.
- A real Qwen `detect` can collect evidence without persistence; a test-home `plan → apply → inspect` proves the exact declaration, resolver fingerprint and generated Codex artifacts.
- The installed/running CCSM remains untouched; P6 alone may request a separate canary and release decision.

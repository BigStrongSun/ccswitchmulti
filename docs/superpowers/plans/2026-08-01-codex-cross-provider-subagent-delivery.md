# Codex Cross-Provider Subagent Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Codex subagent task bodies readable when an official GPT parent dispatches work to a third-party model through MultiRouter.

**Architecture:** Codex Multi-Agent V2 encrypts `spawn_agent.message` for the official Responses backend, so a third-party child cannot decrypt it. Catalog projection will select plaintext Multi-Agent V1 for a router containing any enabled non-official route, while all-official managed OAuth routers retain V2.

**Tech Stack:** Rust, serde_json, TOML projection, Codex model catalog, Cargo tests.

## Global Constraints

- Detect compatibility from enabled route authentication semantics, not model names.
- Treat legacy or ambiguous routes conservatively as third-party.
- Do not modify disabled routes' effective policy.
- Existing V2 task history remains immutable; the fix applies when Codex creates a new task after catalog refresh.

---

### Task 1: Reproduce the mixed-router policy failure

**Files:**
- Modify: `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consumes: `prepare_codex_config_text_with_model_catalog(settings, config_text, profile)`.
- Produces: regression assertions over generated `cc-switch-model-catalog.json` and inline provider models.

- [ ] Add a failing test with one managed OAuth route and one `provider_config` route; assert every projected model uses `multi_agent_version = "v1"`.
- [ ] Add controls proving an all-managed-OAuth router preserves official `v2`, and a disabled third-party route does not force V1.
- [ ] Run the focused `codex_config` tests and verify the mixed-router assertion fails because the official GPT entry is still V2.
- [ ] Commit the RED tests independently for source tracing.

### Task 2: Apply the protocol compatibility policy

**Files:**
- Modify: `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consumes: `settings.codexRouting.routes[].enabled` and `upstream.auth.source`.
- Produces: catalog entries whose `multi_agent_version` matches the router's decryptability boundary.

- [ ] Add a pure predicate that returns true when any enabled route is not explicitly `managed_codex_oauth`.
- [ ] Add a catalog projection helper that writes `multi_agent_version = "v1"` to all routed entries only when the predicate is true.
- [ ] Apply the helper after official metadata enrichment so same-slug official metadata cannot overwrite the compatibility decision.
- [ ] Run focused tests and verify all policy cases pass.
- [ ] Commit the production fix independently.

### Task 3: Document and validate the release lines

**Files:**
- Modify: `memory.md`
- Port: `src-tauri/src/codex_config.rs` and `memory.md` on the main development line.

**Interfaces:**
- Consumes: the verified 3.16.5 implementation and its regression tests.
- Produces: identical protocol behavior on both active release lines.

- [ ] Record the upstream Codex encryption contract, introducing CCSM behavior, fix boundary, and existing-session limitation in `memory.md`.
- [ ] Commit the memory update.
- [ ] Port the test and implementation commits to the main development line without overwriting unrelated work.
- [ ] Run focused tests, Rust formatting, `cargo check --lib`, and the broad relevant Rust suite on both lines.
- [ ] Compare single dispatch, concurrent dispatch, follow-up delivery, official-to-official, third-party-to-third-party, and official-to-third-party behavior against the protocol policy.

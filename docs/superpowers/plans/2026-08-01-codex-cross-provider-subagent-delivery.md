# Codex Cross-Provider Subagent Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Codex subagent task bodies readable when an official GPT parent dispatches work to a third-party model through MultiRouter.

**Architecture:** Keep the task and every catalog entry on Multi-Agent V2. For a mixed router, rewrite only official-parent Responses requests so the `message` property of `spawn_agent`, `send_message`, and `followup_task` no longer carries the Responses-only `encrypted: true` marker. The official backend then returns ordinary V2 function arguments, and Codex renders a plaintext V2 `agent_message` that both official and third-party children can consume.

**Tech Stack:** Rust, serde_json, TOML projection, Codex model catalog, Cargo tests.

## Global Constraints

- Detect compatibility from enabled route authentication semantics, not model names.
- Treat legacy or ambiguous enabled routes conservatively as requiring cross-provider plaintext delivery.
- Do not modify disabled routes' effective policy.
- Preserve encrypted V2 delivery for all-official routers and for non-collaboration tools.
- Apply the request rewrite only when the resolved upstream is the official ChatGPT Codex backend.

---

### Task 1: Correct the protocol acceptance tests

**Files:**
- Modify: `src-tauri/src/codex_config.rs`

**Interfaces:**
- Consumes: `prepare_codex_config_text_with_model_catalog(settings, config_text, profile)`.
- Produces: regression assertions over generated `cc-switch-model-catalog.json` and official upstream request tools.

- [ ] Change the mixed-router catalog regression to require `multi_agent_version = "v2"` for every model.
- [ ] Add a failing request-shape regression requiring the three collaboration `message` schemas to lose only `encrypted`, while unrelated tools and fields remain unchanged.
- [ ] Add controls proving all-official routers and disabled third-party routes keep encrypted V2 delivery.
- [ ] Run the focused tests and verify failures come from the old V1 downgrade and missing request rewrite.
- [ ] Commit the RED tests independently for source tracing.

### Task 2: Keep V2 and select plaintext collaboration arguments

**Files:**
- Modify: `src-tauri/src/codex_config.rs`
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/proxy/providers/openai_compat.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`

**Interfaces:**
- Consumes: enabled route authentication semantics and the official upstream Responses request body.
- Produces: V2 function calls whose cross-provider `message` argument remains plaintext.

- [ ] Remove the task-wide catalog V1 override and retain official V2 metadata.
- [ ] Add a pure mixed-route predicate shared by forwarder request preparation.
- [ ] Remove only `parameters.properties.message.encrypted` from the three collaboration tools after official OAuth normalization.
- [ ] Keep all-official, disabled-third-party, third-party-upstream, unknown-tool, and non-message schema behavior unchanged.
- [ ] Run focused tests and verify all policy cases pass.
- [ ] Commit the production fix independently.

### Task 3: Document and validate the release lines

**Files:**
- Modify: `memory.md`
- Port: `src-tauri/src/codex_config.rs` and `memory.md` on the main development line.

**Interfaces:**
- Consumes: the verified 3.16.5 implementation and its regression tests.
- Produces: identical protocol behavior on both active release lines.

- [ ] Record the upstream `JsonSchema::with_encrypted()` and `encrypted_function_args` contract, live HTTP A/B proof, and V2 plaintext fix boundary in `memory.md`.
- [ ] Commit the memory update.
- [ ] Port the test and implementation commits to the main development line without overwriting unrelated work.
- [ ] Run focused tests, Rust formatting, `cargo check --lib`, and the broad relevant Rust suite on both lines.
- [ ] Compare single dispatch, concurrent dispatch, send/follow-up delivery, official-to-official, third-party-to-third-party, and official-to-third-party behavior while confirming the task remains V2.

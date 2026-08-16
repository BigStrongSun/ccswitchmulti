# Open Issue Maintenance Batch Design

## Scope

This batch handles four independently testable open issues without combining their behavior changes:

- `#35`: preserve user-owned Claude Desktop profile fields while CCSwitchMulti refreshes the fields it owns.
- `#34`: make Codex Responses requests accepted by LM Studio when the client omits `text.format`.
- `#6`: report transport failures to an already resolved Codex upstream as upstream connectivity failures, not generic local-proxy failures.
- `#3`: document the safe, app-scoped procedure for opening the current unsigned and unnotarized macOS build.

The batch does not include `#32` because its local lifecycle fix has not crossed the release/runtime acceptance boundary, and it does not include `#28`, `#16`, `#2`, `#7`, or `#37` because those require separate implementation, reproduction, feature design, or external coordination.

## Design Principles

Each issue receives its own regression boundary and local Git commit. Production behavior is changed only after a focused test reproduces the failure. Existing explicit user configuration always wins over a compatibility default, and compatibility behavior is scoped to the provider or failure stage that needs it.

No issue is closed merely because source code exists locally. Closure requires that the repository-visible deliverable is complete and that the issue's acceptance boundary has been verified. Publishing, pushing, merging, or releasing remains separate from local implementation unless explicitly authorized.

## `#35`: Claude Desktop Profile Ownership

`apply_provider_to_paths_inner` currently generates a complete profile and overwrites the existing profile file. The root cause is that the write path has no ownership-aware merge step, so fields outside CCSwitchMulti's generated profile disappear.

Before writing, the implementation will read the existing profile object and merge only keys absent from the newly generated object. The generated object wins for all CCSwitchMulti-managed keys; existing values survive only for keys CCSwitchMulti did not generate. This avoids maintaining a fragile allowlist of user extras such as `autoModeEnabled`, `toolSearchEnabled`, `prefer1m`, and `inferenceCredentialKind`.

The regression test will create an existing profile containing both a user-owned extra and a stale managed value, apply a provider, and prove that the extra survives while the managed value is replaced.

## `#34`: LM Studio Responses Compatibility

The captured request reaches LM Studio with `text.verbosity` but without the `text.format` object required by LM Studio's Responses implementation. The compatibility layer will add `text.format = {"type":"text"}` only when all of the following are true:

- the effective route targets LM Studio;
- the endpoint is Responses;
- the request has no explicit `text.format`.

An explicit client format is never overwritten. Other providers retain their current payload. Focused tests will cover the missing-format insertion, explicit-format preservation, and non-LM-Studio no-op cases.

## `#6`: Upstream Send Failure Classification

The current response wrapper labels failures as `CC Switch local proxy failed` even after route resolution and authentication succeeded and the outbound request failed before any upstream HTTP status was received. That wording assigns the failure to the wrong component.

The implementation will classify this stage as an upstream connection/send failure. For the official Codex target, the user-facing message will identify the OpenAI Codex upstream connection and retain the provider, model, target, proxy-use, and underlying cause needed for diagnosis. HTTP behavior and retry policy will not change in this batch; only classification and diagnostics change.

Tests will prove that a pre-status official upstream send failure does not contain the generic local-proxy wording, while genuine local proxy handling failures keep their existing classification.

## `#3`: Unsigned macOS Documentation

The current release workflow explicitly produces unsigned and unnotarized macOS artifacts, while the existing documentation or stale PR text may imply otherwise. The current-branch README will describe both Apple’s graphical `Open Anyway` flow and the targeted command-line removal of `com.apple.quarantine` from `/Applications/CCSwitchMulti.app`.

The documentation must state that the command affects only CCSwitchMulti and does not disable Gatekeeper globally. Artifact names must match current release assets. The stale draft PR will not be merged blindly; its useful text will be reconciled against current README content.

## Verification and Closure

For each code issue, verification includes a RED run proving the regression test fails before production changes, a GREEN focused run, relevant neighboring tests, Rust formatting/checks, and diff validation. The documentation issue receives link/name/content checks and diff validation.

The final batch verification will run the relevant Rust test groups and repository formatting checks. Project memory will record root causes, exact commits, test evidence, and the remaining release or remote-work boundary. Only issues whose completed changes are visible on GitHub and whose acceptance criteria are met will be closed.

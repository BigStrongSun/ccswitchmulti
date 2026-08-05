# Cross-Provider Reasoning Replay Design

## Problem

CCSwitchMulti converts a Chat Completions response from Qwen into Responses
items. Qwen reasoning becomes a synthetic item such as
`rs_resp_chatcmpl-*`. Codex persists that item and replays it when the user
switches to an official OpenAI model.

The ChatGPT Codex Responses backend treats a plain `reasoning` item carrying an
`rs_*` ID as a reference to server state. CCSwitchMulti uses `store: false`, so
the synthetic ID does not exist in the official backend and the request fails
with HTTP 404. A live A/B probe confirmed that the same plain reasoning item is
accepted when its `id` is omitted.

## Ownership Boundary

Codex owns conversation history. CCSwitchMulti owns protocol projection at the
provider boundary. The fix belongs in the existing official Responses request
normalizer, immediately before forwarding to the official provider.

CCSwitchMulti must not enable `store: true`, persist response items, delete the
reasoning summary, or rewrite official encrypted reasoning. Those approaches
would respectively change privacy/state semantics, add proxy-owned history,
lose useful context, or corrupt provider-bound ciphertext.

## Design

For every top-level Responses `input` item sent to an official OpenAI provider:

- If `type` is `reasoning`, the item has no non-empty `encrypted_content`, and
  it has an `id`, remove only the `id`.
- Preserve its readable `summary` and all other accepted fields.
- Preserve official reasoning items with non-empty `encrypted_content`
  byte-for-byte, including their IDs.
- Keep the existing deterministic normalization for noncanonical message,
  function-call, custom-tool-call, and web-search IDs.

This makes third-party reasoning an inline value instead of a nonexistent
official server reference while retaining the model-visible summary.

## Verification

Automated coverage must reproduce `rs_resp_chatcmpl-*`, assert that its ID is
removed, assert that the summary survives, and assert that encrypted official
reasoning is untouched. Existing replay-ID tests and the complete
`transform_codex_chat` suite must remain green.

Runtime acceptance requires three real sequences through port 15721:

1. Qwen turn, switch to `gpt-5.6-sol`, then complete an official turn without
   the prior 404.
2. Official turn, switch to Qwen, then complete a Qwen turn with a terminal
   `response.completed` event.
3. Two consecutive Qwen turns to prove the Chat bridge still replays its own
   history.

Router logs must prove the expected effective providers, endpoints, upstream
statuses, and terminal behavior without recording prompt or reasoning text.

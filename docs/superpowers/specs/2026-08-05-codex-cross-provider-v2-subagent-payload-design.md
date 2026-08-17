# Codex Cross-Provider V2 Subagent Payload Design

## Problem

Codex Multi-Agent V2 marks `spawn_agent.message`, `send_message.message`, and
`followup_task.message` as encrypted. The OpenAI backend therefore returns an
opaque function argument and Codex stores the actual task as
`agent_message.content[].encrypted_content`. A third-party child provider cannot
decrypt that value, so it sees the `NEW_TASK` envelope with an empty `Payload:`.

Changing the namespace from reserved `collaboration` to non-reserved `agents`
prevents official schema validation failures, but does not remove the encrypted
parameter marker. Rewriting reserved `collaboration.*` schemas is forbidden
because the official backend rejects any deviation with HTTP 400.

## Approved Architecture

The compatibility layer has two independent stages.

Stage A runs for any Codex parent request that belongs to a MultiRouter with at
least one enabled third-party or ownership-ambiguous route. It removes only
`parameters.properties.message.encrypted` from the non-reserved
`agents.spawn_agent`, `agents.send_message`, and `agents.followup_task` schemas.
It handles both top-level `tools` and Responses Lite `additional_tools`, including
nested namespace containers. It never changes `collaboration.*`, unrelated
functions, unrelated `encrypted` fields, or pure official routers.

Stage B runs only after the effective child route is known to be third-party. It
projects every plaintext `agent_message` into a standard Responses
`type=message, role=user` input before any native Responses, Chat, or Anthropic
conversion. If an agent message still contains opaque encrypted content, the
request fails explicitly instead of sending an empty task or ciphertext to the
third-party model. Clearly plaintext content stored under `encrypted_content` by
older third-party bridges is recovered as text for history compatibility.

## State Propagation

Retry routing may materialize an effective provider before `forward()` runs.
Therefore the mixed-router Stage A decision is copied into the resolved route as
the request-local boolean `codexRouterPlaintextV2Collaboration`. Only that
boolean is propagated; the full router configuration is not copied and routing
is not repeated.

## Security and Privacy

The proxy never decrypts OpenAI ciphertext. Plaintext collaboration tasks remain
in memory and in Codex's own rollout semantics, but CCSwitchMulti must not add
task bodies to router logs, request logs, the database, or sidecar files. Logs may
record only counts and provider/route identity. Pure official parent-child flows
retain OpenAI encryption.

## Acceptance

- Reserved `collaboration.*` schemas are byte/schema preserving.
- Mixed-router parents produce plaintext arguments only for `agents.*`.
- Top-level tools and Lite `additional_tools` behave identically.
- Third-party child inputs contain ordinary user messages with readable payloads.
- Opaque child ciphertext returns an explicit compatibility error.
- Pure OpenAI-to-OpenAI routers remain encrypted.
- OpenAI-to-Qwen and OpenAI-to-DeepSeek children return a unique nonce plus real
  tool output in live tests after a full CCSwitchMulti and Codex restart.

# 2026-09-23 GPT-6 prompt caching compatibility

## Scope

This change covers the CCSwitchMulti native Responses request normalizer. It
does not add a cache policy, synthesize cache breakpoints, alter cache keys, or
claim that an arbitrary third-party Chat Completions upstream supports the
GPT-6 Responses cache wire protocol.

## Root cause

The native Responses normalizer intentionally lifts ordinary `system` and
`developer` message items into top-level `instructions`. That is normally a
transport compatibility transformation, but it silently loses a GPT-5.6+/GPT-6
explicit cache boundary: OpenAI accepts `prompt_cache_breakpoint: {"mode":
"explicit"}` only on a supported input content block, while top-level
`instructions` cannot contain it. A developer message carrying such a boundary
was therefore rewritten into instructions and could no longer produce or look
up the chosen cache prefix.

## Fix and boundary

`openai_compat::lift_codex_responses_control_messages` now keeps a control
message in `input` only when one of its content blocks has the current official
explicit marker object. It recognizes exactly `{"mode":"explicit"}`; legacy
boolean markers and unknown extensions retain the old lift behavior. The proxy
does not add breakpoints or change text/order, so it does not invent cache-write
charges or modify otherwise stable prefixes.

Existing native Responses behavior already preserves top-level
`prompt_cache_options` (including `mode`, `ttl`, and `prewarm`) and unknown
input items, which includes `configuration_update`. Existing response and
conversion usage paths already map `input_tokens_details.cached_tokens` and
`input_tokens_details.cache_write_tokens` (plus the standard prompt-token
details fallbacks); no accounting change was needed.

Responses-to-Chat conversion intentionally remains capability-gated for the
older `prompt_cache_key` and `prompt_cache_retention` fields. OpenAI's current
GPT-6 caching controls are Responses-specific in the cited guide, so forwarding
them to arbitrary Chat-compatible providers would be an unverified protocol
injection rather than compatibility support.

## Evidence and verification

Two independent searches were used. Codex built-in Web opened OpenAI's current
Prompt Caching guide; Matrix WebSearch's index query had no result, but Matrix
then directly fetched the same official developer page successfully. Both
sources establish that explicit mode requires content-block markers, top-level
instructions cannot hold one, GPT-6 supports append-only
`configuration_update` for reasoning effort, and usage should track cached and
cache-write tokens. The requested OpenAI blog URL was not necessary to define
the wire contract; the developer guide is the authoritative schema source.

Focused regressions cover preservation of an explicit developer cache boundary
alongside `prompt_cache_options` and `configuration_update`, and prove that a
noncanonical boolean marker does not broaden the exception.

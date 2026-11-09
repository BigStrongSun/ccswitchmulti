# 2026-11-09 Codex ImageGen `gpt-image-2` MultiRouter route fallback

## Evidence

- Installed Codex `26.1103.4239.0` standalone ImageGen sends `model: "gpt-image-2"` to `images/generations`.
- Official OpenAI documentation lists `gpt-image-2` as a valid image-generation model for both Images and Responses image-generation tool surfaces.
- Local schema-v2 MultiRouter had an enabled `OpenAI Official` route with `modelSelection: { "mode": "all" }`, but the official Provider catalog contained text/mainline models only.
- `resolve_codex_v2_routed_provider` previously accepted only an exact model already present in the compiled catalog. That made Codex ImageGen fail before provider/auth materialization with `Codex MultiRouter v2 did not resolve model gpt-image-2`.

## Root fix

- Added a narrow schema-v2 fallback for `gpt-image-*` and `chatgpt-image-*`.
- The fallback selects an enabled route whose target Provider is the OpenAI Official Provider by identity.
- An explicit catalog entry or explicit third-party image route still wins first.
- Unknown text models remain closed; the image fallback does not make arbitrary models routable.
- `matched_by` records `official_image_fallback` for traceability.

## Runtime recovery

- Added `gpt-image-2` to the live `codex-official` Provider catalog as an enabled, hidden image model so the installed 3.20.4-11 proxy can resolve it without restarting CC Switch or Codex.
- The current `New Codex MultiRouter` route still uses `mode: all`, so the new official catalog entry is immediately routable.
- Pre-change consistent SQLite backup:
  `C:\Users\sunda\Documents\LLMservice\cc-switch\.local-archives\cc-switch-before-gpt-image-2-20261109-144832.db`
- Backup SHA-256:
  `5805d3dda0d6cb351134423f76ad8118f5d6de08f82bac3c7653b54289042b95`

## Verification

- `cargo test ... v2_image_generation_routes_to_official_identity_without_catalog_entry --lib`: 1 passed.
- `cargo test ... image_generation --lib`: 15 passed.
- `cargo fmt --check` and `git diff --check`: passed.
- No paid image request was sent. Live end-to-end image generation remains the user's next acceptance step.

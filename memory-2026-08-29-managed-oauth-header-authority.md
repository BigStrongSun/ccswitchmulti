# Managed OAuth header authority (2026-08-29)

- A Codex Desktop request can carry its own `Authorization` bearer and `chatgpt-account-id`, while a CCSM `managed_codex_oauth` route intentionally substitutes a different managed bearer and account id.
- Root cause: the normal Responses forwarding header loop replaced `Authorization` but preserved the inbound Desktop `chatgpt-account-id`; it then appended the managed account id. This could send duplicate, conflicting account-id headers to the official upstream. Raw and realtime passthrough already rebuilt headers and dropped the inbound account id, so they did not share the gap.
- In `src-tauri/src/proxy/forwarder.rs`, managed OAuth now strips the inbound `chatgpt-account-id` before the managed identity headers are inserted. Native routes remain unchanged and preserve their original Desktop bearer/account-id pair.
- Regression: `managed_codex_oauth_does_not_forward_desktop_account_header`; targeted `proxy::forwarder::tests` passed (163 tests).

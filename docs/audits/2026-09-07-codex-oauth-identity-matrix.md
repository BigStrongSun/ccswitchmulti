# Codex OAuth identity and credential ownership matrix

This audit records the identity boundary used by CCSwitchMulti after the
v3.20.1 Task 6 migration. It deliberately separates a stable local binding
from the ChatGPT workspace header and the JWT subject. Treating any two of
those values as interchangeable can merge unrelated users, misattribute
usage, or send a CCSwitchMulti UUID to the ChatGPT backend.

## Identity matrix

| Route/auth shape              | Stable provider or pool binding                        | Credential owner                                        | Upstream `chatgpt-account-id`                                                        | Live `auth.json` owner                                            | Fallback policy                                                                                           | Usage attribution                                                                     |
| ----------------------------- | ------------------------------------------------------ | ------------------------------------------------------- | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Native official login         | Sentinel `native_codex_auth`                           | Codex Desktop                                           | The inbound Desktop account header may pass through only on the trusted native route | Codex Desktop                                                     | No substitution with a managed account; if native credentials are unavailable, fail that candidate        | Native sentinel plus the Desktop account identity observed by the request path        |
| CCSM managed Codex account    | CCSM-local UUID; a targeted reauth keeps the same UUID | `codex_oauth_auth.json`, managed by `CodexOAuthManager` | Persisted `chatgpt_account_id` decoded from the authenticated token package          | Not written by ordinary managed proxy forwarding or quota queries | No silent fallback to Desktop or another managed account for an account-bound request                     | CCSM-local UUID and credential generation; workspace is not the local attribution key |
| Account-pool candidate        | Selected local account ID plus credential generation   | Resolved from the selected manager record               | Resolved record's `chatgpt_account_id`                                               | Unchanged                                                         | A failed bound candidate follows the explicit pool policy; it is never replaced under the same account ID | Selected local account ID/generation, including retry and quota state                 |
| Third-party `provider_config` | Provider/route ID                                      | Provider settings or external secret source             | Never injected by CCSM                                                               | Unchanged                                                         | Must use its declared provider credential; it cannot borrow Desktop or managed OAuth                      | Provider/route identity, not a ChatGPT workspace                                      |

## Stored managed-account identity

The managed store now has three distinct identity layers:

- `account_id`: a stable CCSwitchMulti-local UUID (or a preserved legacy
  binding ID during migration). Provider bindings, pool entries, quota cache
  keys, targeted reauth, and request attribution use this value.
- `chatgpt_account_id`: the ChatGPT workspace/account value used only where
  the upstream Codex API requires `chatgpt-account-id`.
- `id_token` subject (`sub`): the authenticated user identity used to reject a
  duplicate login in the same workspace. A shared workspace does not imply a
  shared user.

Store-v1 records that lack enough identity evidence are retained but
quarantined with `requires_reauth=true`. They cannot become the default,
provide a token/workspace pair, or enter the account pool until a targeted
reauth repairs the record in place.

## Transition invariants

1. Normal login rejects the same workspace plus the same JWT subject as
   `DuplicateAccount`, while the same workspace with a different subject gets
   a different local UUID.
2. Targeted reauth serializes with refresh and deletion. It obtains the
   per-account refresh lock before its final pending-flow/generation check, and
   the final check, pending removal, atomic persistence, and memory publication
   run under the commit lock.
3. Removing an account clears its pending flow and generation under the same
   commit boundary, so an in-flight reauth cannot resurrect it.
4. Forwarding, model discovery, quota queries, hosted-tool continuations, and
   pool quota refresh resolve one `(access token, chatgpt_account_id)` pair.
   The local UUID must never be emitted as `chatgpt-account-id`.
5. Organization claims are not workspace fallbacks. Missing workspace
   evidence makes the record unusable until reauthentication.

## Desktop deduplication and fail-open policy

The native Desktop entry and managed entries are different credential
domains. A new managed local UUID cannot be compared directly with a Desktop
workspace ID. Deduplication is therefore evidence-bound: only an exact,
verified identity match may suppress a duplicate candidate. Ambiguous or
legacy records remain visible and fail open as separate candidates; a
quarantined record remains outside the pool.

This can temporarily show two candidates for one human user, but it avoids the
more dangerous outcome of deleting or disabling another user who happens to
share the same workspace.

## `auth.json` and MultiRouter boundary

Official CC Switch v3.20.1 evolved around a single active Provider and writes
selected managed OAuth material into live `auth.json`. CCSwitchMulti instead
uses managed OAuth primarily for proxy forwarding, quota, and account-pool
selection. It must not overwrite the Codex Desktop login slot merely to select
a managed proxy account.

The MultiRouter live facade intentionally keeps
`requires_openai_auth = true`. In fully managed mode it also carries the
non-secret `PROXY_MANAGED` bearer placeholder in `config.toml`; native or mixed
mode omits that placeholder. This combination preserves Codex Desktop's login,
account, quota, and logout surfaces while the local proxy resolves the real
route credential. Upstream's config-only rule that forces
`requires_openai_auth = false` for a proxy-injected OAuth card is therefore not
applicable to this facade.

Ordinary third-party providers remain config-owned: their bearer belongs in
the active provider table in `config.toml`, while the user's Desktop
`auth.json` remains untouched. The remaining upstream UI reconciliation and
stale reserved-table migrations are tracked explicitly in the upstream commit
matrix for the later provider-form/preset batch; they are not claimed as part
of this identity implementation.

## Evidence

- CCSwitchMulti implementation commit: `f17b2f95`.
- Managed device cancellation commit: `837c3afb`.
- Focused Task 6 verification: OAuth manager 48/48, forwarder 163/163, Codex
  OAuth model service 9/9, frontend managed-auth/UI/locale 13/13.
- Integration checkpoint B: `pnpm typecheck`,
  `cargo check --all-targets --no-default-features`, rustfmt, diff, and strict
  UTF-8/no-BOM/no-U+FFFD checks passed.
- This evidence is source/test evidence only. No Tauri/NSIS package was built,
  installed, or used to replace the running CCSwitchMulti/Codex instance in
  Task 6.

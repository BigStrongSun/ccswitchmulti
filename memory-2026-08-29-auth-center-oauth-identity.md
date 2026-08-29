# Auth Center OAuth identity boundaries (2026-08-29)

- `CodexOAuthSection` previously showed the Desktop identity only inside the account-pool editor. The ordinary Auth Center account cards render only CCSM-managed OAuth accounts, so a CCSM “default” badge could be mistaken for the account used by Codex Desktop.
- Credential ownership is route-level. `native_codex_auth` uses the Desktop bearer, `managed_codex_oauth` / `managed_account` uses the bound account or CCSM managed default, and `account_pool` selects from the pool. A MultiRouter can expose more than one of these at once, so do not collapse them to one “current account”.
- `src/lib/codexOAuthIdentity.ts` derives one display record per enabled route from the active provider. Auth Center displays the Desktop identity, CCSM managed default, and active route sources separately; `CodexOAuthSection.test.ts` and `codexOAuthIdentity.test.ts` cover this boundary.

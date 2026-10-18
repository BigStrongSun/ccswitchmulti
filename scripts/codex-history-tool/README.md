# Codex History Tool

Standalone Python tool for listing and repairing Codex Desktop history visibility.

It uses only the Python standard library. No helper executable is required.

This tool complements the official **Unified Codex Session History** setting:

- Official unified history merges the `openai` and `custom` resume buckets and uses a backup ledger for exact restore.
- This tool repairs Desktop visibility when the data still exists but local `state_*.sqlite` files, `session_index.jsonl`, workspace hints, or rollout metadata keep sessions out of the sidebar.

For the official bucket-unification behavior and safety model, see:

- `docs/guides/codex-unified-session-history-guide-zh.md`
- `docs/guides/codex-unified-session-history-guide-en.md`
- `docs/guides/codex-unified-session-history-guide-ja.md`

## List Sessions

```powershell
python .\codex_history_tool.py list --limit 20 --json
```

Optional filters:

```powershell
python .\codex_history_tool.py list --codex-home "$env:USERPROFILE\.codex" --project-path "C:\Users\sunda\Documents\LLMservice" --query "MultiRouter"
```

## Preview Repair

The default repair mode matches the currently successful Desktop-sidebar fix:
`source=vscode`, project top 10, global recent window 300, and rollout mtime sync.

```powershell
python .\codex_history_tool.py repair --project-path "C:\Users\sunda\Documents\LLMservice" --json
```

## Reconcile Stores

Use `reconcile` before repairing when you need to see every discovered state DB and JSONL provider bucket side by side.

```powershell
python .\codex_history_tool.py reconcile --codex-home "$env:USERPROFILE\.codex"
```

## Apply Repair

Close Codex Desktop first unless you intentionally pass `--force`.

```powershell
python .\codex_history_tool.py repair --project-path "C:\Users\sunda\Documents\LLMservice" --apply
```

## Repair Selected Sessions

Use `list` to find IDs, then pass one or more `--session-id` values.

```powershell
python .\codex_history_tool.py repair --session-id "<session-id>" --apply
```

Useful overrides:

- `--codex-home <path>`: choose another Codex directory.
- `--state-db <path>`: force a specific state SQLite file for `list`/`repair`. Without an explicit path, the tool follows `sqlite_home` from `config.toml` or `CODEX_SQLITE_HOME`, then prefers the current root `~/.codex/state_5.sqlite`, and only falls back to the legacy `~/.codex/sqlite/state_5.sqlite`. `reconcile` scans all discovered `state_*.sqlite` files under every known location.
- `--target-provider <id>`: default is the live `config.toml` provider, falling back to `codex_model_router_v2`.
- `--max-per-project 10 --max-total 300`: balanced recent-window caps.

## Recover Legacy Agent Plaintext

`repair_agent_plaintext.py` is an explicit offline recovery utility for old
CCSM router histories that stored readable agent replies inside
`agent_message.content[].encrypted_content`. Native official Codex rejects
those replies as invalid ciphertext after proxy normalization is removed.
The recovery converts only clearly readable parts to `input_text`, retaining
the complete text, item IDs, ordering, other fields, and real opaque payloads.
Native startup and automatic synchronization do not invoke this utility.

Prepare a complete local snapshot first: retain the original `sessions/` and
`archived_sessions/` files under `<backup>/codex/`, snapshot the state SQLite
databases and session index, and include a `manifest.json` with `codex_home`
and a `files` array containing each file's `relative_path`, `bytes`, and
`sha256`. Keep this backup private because it contains conversation history.

```powershell
python .\repair_agent_plaintext.py --codex-home <codex-home> --backup <backup>
python .\repair_agent_plaintext.py --codex-home <codex-home> --backup <backup> --apply --expected-plan-sha256 <preview-hash>
```

The first command writes a preview plan into the backup. Applying requires its
exact hash and unchanged originals. Every affected file must be exclusively
available before any writes begin; close Codex if the tool reports busy files.
The full original backup is verified again before writing. Write failures
trigger an attempt to restore every affected original, with any restoration
failures reported together.

Each decompressed JSONL line retains its exact byte length using insignificant
JSON whitespace, preserving paginated offsets. Compressed `.jsonl.zst` files
require Python 3.14's `compression.zstd`; the decoded stream keeps its byte
boundaries while the compressed file is regenerated and verified. File paths
and SQLite indexes remain unchanged. Reopen Codex after recovery to refresh
cached conversation state, then test a real continuation separately.

Run the isolated regression tests with:

```powershell
python .\test_repair_agent_plaintext.py
```

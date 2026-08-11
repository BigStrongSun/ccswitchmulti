# Updater check unification design

## Problem

CCSwitchMulti has two updater clients. The About page checks for releases through the JavaScript updater plugin, while download and installation use a Rust updater builder that applies the saved runtime proxy and a bounded timeout. Commit `0d693c8a` added proxy support only to the Rust path, so routine checks can fail on networks where GitHub requires the configured proxy even though the same release can be downloaded by the backend.

## Design

Add a backend command that returns the current release metadata needed by the UI: current version, available version, notes, and publication date. It must build the updater through `updater_builder_with_runtime_proxy`, which remains the single source of updater endpoint, timeout, proxy, target, signature, and version-comparison behavior.

The frontend `checkForUpdate` helper will invoke this backend command and map `null` to `up-to-date`. It will no longer import or call the JavaScript updater plugin. Existing UpdateContext and AboutSection behavior remains unchanged.

## Installation strategy

Normal Windows updates continue through Tauri updater 2.10 and its NSIS `/UPDATE` mode. Before starting the installer, CCSwitchMulti saves window state, restores managed live configuration, stops the proxy, closes application-owned resources, removes the tray icon, and releases the single-instance lock. The external NSIS installer then replaces the installed bundle and relaunches the application.

The transaction script that performs stop, database snapshot, uninstall, install, health verification, and rollback is reserved for damaged installations or installer-family migration. It must not become the default in-app update path because uninstall hooks can remove user-selected integration state such as autostart, and a normal update does not need to move the SQLite database.

The database remains under the user configuration directory, outside the application install directory. Normal shutdown is the ownership boundary for SQLite, WAL, and SHM handles; the installer must not copy or restore a live database. The transaction script remains the recovery path that snapshots and integrity-checks the full database state when a full reinstall is necessary.

## Error handling

Backend check failures cross the invoke boundary with their specific phase and underlying updater error. Existing callers continue to show the detailed error for installation and the localized check failure message for routine checks. Logs remain available for network-level diagnosis without exposing proxy credentials.

## Verification

- A frontend unit test proves routine update checks invoke the backend command and never require the JavaScript updater plugin.
- Rust unit coverage proves release metadata mapping preserves all UI fields.
- Type checking, focused unit tests, Rust tests, and a production frontend build must pass.
- Installed-version acceptance checks that `3.19.1-19` detects `3.19.1-20`; actual installation is performed only through the controlled update path with pre/post process, version, listener, and SQLite integrity evidence.

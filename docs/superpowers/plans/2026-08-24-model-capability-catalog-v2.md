# Model Capability Catalog v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a traceable, layered model capability bundle and safely consume its factual capabilities during model discovery.

**Architecture:** The root `preset-table` compiler emits schema v2 with source locks, shared policies and deployments. CCSwitch reads v1/v2, resolves policy/deployment overlays deterministically, and uses only safe factual fields to enrich `/models` discovery.

**Tech Stack:** Python 3.10 plus `toml`; Rust/Serde; TypeScript/Vitest.

**Spec:** `docs/superpowers/specs/2026-08-24-model-capability-catalog-v2-design.md`

## Global Constraints

- Keep `preset-table.json` as the existing WebDAV/S3 artifact.
- Accept schema v1 bundles during migration.
- Do not store credentials, endpoint URLs, runtime probes, latency, or quota observations in a shared bundle.
- Never infer request dialect or reasoning parameter mapping from models.dev.

---

### Task 1: Compile v2 Bundle Metadata

**Files:**
- Modify: `../preset-table/tools/build_bundle.py`
- Create: `../preset-table/sources/models-dev.toml`
- Test: `../preset-table/tools/test_build_bundle.py`

**Interfaces:**
- Produces `manifest.source_locks`, `manifest.layer_hashes`, and `deployments` in `preset-table.json`.

- [x] Write compiler tests for source lock loading, deterministic hashes, and unsafe deployment rejection.
- [x] Implement v2 source lock collection and validated shared deployment collection.
- [x] Run `python -X utf8 -m unittest preset-table/tools/test_build_bundle.py`.

### Task 2: Resolve v1/v2 Catalog Entries

**Files:**
- Modify: `src-tauri/src/services/preset_catalog.rs`
- Test: `src-tauri/src/services/preset_catalog.rs`

**Interfaces:**
- Produces `resolve_with_deployment(bundle, provider, model, plan, deployment)`.

- [x] Add failing tests for v1 acceptance and deployment precedence.
- [x] Add optional manifest/deployment fields and resolve helpers while preserving `resolve`.
- [x] Run `cargo test preset_catalog`.

### Task 3: Consume Safe Capabilities During Discovery

**Files:**
- Modify: `src-tauri/src/services/model_fetch.rs`
- Modify: `src/lib/api/model-fetch.ts`
- Test: `src-tauri/src/services/model_fetch.rs`

**Interfaces:**
- `FetchedModel` retains context, input modalities and image support from the source response or catalog fallback.

- [x] Add tests proving missing capabilities are filled and explicit upstream values remain unchanged.
- [x] Apply resolved baseline facts only when `/models` omitted them.
- [x] Run focused Rust and TypeScript tests.

### Task 4: Build, Verify, Document, Commit

**Files:**
- Modify: `README`/catalog documentation only if build interface requires it.
- Modify: project `memory.md`.

- [x] Generate the bundle and assert schema v2/source locks.
- [x] Run Rust unit tests, TypeScript typecheck and focused Vitest suite.
- [x] Record operational boundaries and commit only owned files.

# Model Capability Catalog v2 Design

## Goal

Turn the WebDAV-synchronized model preset table from a context-window fallback
into a traceable capability catalog, without allowing community metadata to
silently alter a provider's request protocol.

## Layers

1. **Baseline** is a frozen import from an external catalog. It supplies
   provider/model facts such as limits, modalities, tool calling, prices and
   provider-side reasoning options.
2. **Policy** is reviewed project data. It may correct a baseline value and is
   the only layer allowed to declare request transport, reasoning parameter
   mappings, first-party subscription behaviour, and tool semantics.
3. **Deployment** is an overlay for a named Provider deployment. Shared
   deployment overlays contain aliases and approved behaviour; device-local
   endpoint, probe, latency and quota observations stay out of the bundle.
4. **Resolved entry** is the deterministic deep merge of baseline, optional
   policy and optional deployment. It carries the provenance for every source
   layer and is what a caller reads.

## Bundle Contract

Schema v2 keeps `baseline` and `plans` for v1 consumers. It adds `manifest`
and `deployments`. `manifest` contains source locks and deterministic hashes
of the baseline, policy and deployment inputs. A source lock has an id, URL,
revision, fetched time, checksum and authority. The compiler rejects an
unknown deployment base model or a deployment that contains a URL, credential,
runtime observation or quota field.

The Rust reader accepts schema v1 and v2. A v1 bundle has an empty manifest
and no deployments. The sync artifact name remains `preset-table.json`; its
existing sync manifest hash continues to protect transport integrity.

## Safe Runtime Projection

The `/models` fetcher may use resolved catalog data only to fill missing
context windows, input modalities and image support. Explicit upstream
metadata always wins. The saved Codex model catalog receives those facts.

The fetcher must not infer `apiFormat`, reasoning request parameters, web
search or computer-use support from models.dev. Those fields require an
explicit policy/deployment entry and remain untouched when absent.

## Sync And Trust

The existing preset registry remains the only remote-update trust gate:
signed manifests verify a registry release before it is accepted; WebDAV is
transport, not an authority. The normal WebDAV/S3 profile snapshot synchronizes
the compiled shared bundle. Device-local deployment observations are not added
to that bundle and are therefore not replicated.

## Verification

- Compiler tests verify deterministic layer hashes, source locks and rejection
  of unsafe shared deployment data.
- Rust tests verify v1 compatibility and policy/deployment merge precedence.
- Model fetch tests verify catalog capability enrichment only fills missing
  fields and never overwrites an explicit `/models` response.

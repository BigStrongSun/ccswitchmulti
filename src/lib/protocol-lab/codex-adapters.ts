import {
  commitCodexProviderSet,
  getCodexProviderEditorSnapshot,
  prepareCodexProviderSet,
  preflightCodexProviderProtocolCompatibility,
  type CodexProviderAdaptationView,
  type CodexProviderEditorSnapshot,
  type CodexProviderProtocolPreflightOutcome,
  type CodexProviderSetCommitIntent,
  type CodexProviderSetCommitOutcome,
  type CodexProviderSetPreview,
  type CodexProviderSetBatchCommitOutcome,
  type CodexProviderSetBatchPreview,
  type CodexProviderSetBatchSource,
  type CodexProtocolProbeProgressEvent,
  type UniversalProviderSetCommitOutcome,
  type UniversalProviderSetPreview,
  commitUniversalProviderSet,
  preflightUniversalCodexProtocolCompatibility,
  prepareUniversalProviderSet,
  commitCodexProviderSetBatch,
  prepareCodexProviderSetBatch,
} from "@/lib/api/protocol-compatibility";
import type { Provider, UniversalProvider } from "@/types";

import { normalizeCodexPublicModelKey } from "./codex-overrides";
import type { ProtocolLabAdapter } from "./useProtocolLabWorkflow";

interface SingleCodexProtocolLabApi {
  preflightCodexProviderProtocolCompatibility: typeof preflightCodexProviderProtocolCompatibility;
  prepareCodexProviderSet: typeof prepareCodexProviderSet;
  commitCodexProviderSet: typeof commitCodexProviderSet;
  getCodexProviderEditorSnapshot: typeof getCodexProviderEditorSnapshot;
}

const singleCodexProtocolLabApi: SingleCodexProtocolLabApi = {
  preflightCodexProviderProtocolCompatibility,
  prepareCodexProviderSet,
  commitCodexProviderSet,
  getCodexProviderEditorSnapshot,
};

export type SingleCodexProtocolLabAdapter = ProtocolLabAdapter<
  Provider,
  CodexProviderAdaptationView,
  CodexProviderSetPreview,
  CodexProviderEditorSnapshot,
  CodexProtocolProbeProgressEvent,
  CodexProviderProtocolPreflightOutcome,
  CodexProviderSetCommitOutcome
>;

export function createSingleCodexProtocolLabAdapter(
  api: SingleCodexProtocolLabApi = singleCodexProtocolLabApi,
): SingleCodexProtocolLabAdapter {
  return {
    requiresProbe(provider, receiptIds) {
      if (receiptIds.length > 0) return false;
      return providerHasAutomaticCodexModels(provider);
    },
    isManual(provider) {
      return provider.meta?.codexProtocolMode === "manual";
    },
    async preflight(provider, onProgress) {
      const outcome = await api.preflightCodexProviderProtocolCompatibility(
        provider,
        onProgress,
      );
      return {
        outcome,
        receiptIds: outcome.receiptIds,
        adaptationPreview: outcome.adaptationPreview,
      };
    },
    prepare: api.prepareCodexProviderSet,
    plan(preview) {
      return preview.plan.kind;
    },
    async commit(provider, receiptIds, preview, intent) {
      const outcome = await api.commitCodexProviderSet(
        provider,
        receiptIds,
        preview.digest,
        intent as CodexProviderSetCommitIntent,
      );
      return {
        outcome,
        snapshot: outcome.snapshot,
        projectionWarning: outcome.status === "committed_with_projection_error",
        projectionErrorCode: outcome.projectionErrorCode,
      };
    },
    isDependencyChanged(error) {
      return errorText(error).includes("codex_provider_set_dependency_changed");
    },
    async loadSnapshot(providerId) {
      const snapshot = await api.getCodexProviderEditorSnapshot(providerId);
      return { snapshot, draft: snapshot.logicalProvider };
    },
    errorCode: codexProtocolLabErrorCode,
  };
}

interface UniversalCodexProtocolLabApi {
  preflightUniversalCodexProtocolCompatibility: typeof preflightUniversalCodexProtocolCompatibility;
  prepareUniversalProviderSet: typeof prepareUniversalProviderSet;
  commitUniversalProviderSet: typeof commitUniversalProviderSet;
}

const universalCodexProtocolLabApi: UniversalCodexProtocolLabApi = {
  preflightUniversalCodexProtocolCompatibility,
  prepareUniversalProviderSet,
  commitUniversalProviderSet,
};

export type UniversalCodexProtocolLabAdapter = ProtocolLabAdapter<
  UniversalProvider,
  CodexProviderAdaptationView | null,
  UniversalProviderSetPreview,
  CodexProviderEditorSnapshot | null,
  CodexProtocolProbeProgressEvent,
  CodexProviderProtocolPreflightOutcome | null,
  UniversalProviderSetCommitOutcome
>;

export function createUniversalCodexProtocolLabAdapter(
  api: UniversalCodexProtocolLabApi = universalCodexProtocolLabApi,
): UniversalCodexProtocolLabAdapter {
  return {
    requiresProbe(provider, receiptIds) {
      if (!provider.apps.codex || receiptIds.length > 0) return false;
      const model = provider.models.codex?.model?.trim() ?? "";
      const overrides = provider.meta?.codexProtocolOverrides ?? {};
      const hasOverride = Object.keys(overrides).some(
        (key) =>
          normalizeCodexPublicModelKey(key) ===
          normalizeCodexPublicModelKey(model),
      );
      if (hasOverride) return false;
      return provider.meta?.codexProtocolMode !== "manual";
    },
    isManual(provider) {
      return provider.meta?.codexProtocolMode === "manual";
    },
    async preflight(provider, onProgress) {
      const outcome = await api.preflightUniversalCodexProtocolCompatibility(
        provider,
        onProgress,
      );
      return {
        outcome,
        receiptIds: outcome?.receiptIds ?? [],
        adaptationPreview: outcome?.adaptationPreview ?? null,
      };
    },
    prepare: api.prepareUniversalProviderSet,
    plan(preview) {
      return preview.codex?.plan.kind ?? "single";
    },
    async commit(provider, receiptIds, preview, intent) {
      const outcome = await api.commitUniversalProviderSet(
        provider,
        receiptIds,
        preview.digest,
        intent as CodexProviderSetCommitIntent,
      );
      return {
        outcome,
        snapshot: outcome.codexSnapshot ?? null,
        projectionWarning: outcome.status === "committed_with_projection_error",
        projectionErrorCode: outcome.projectionErrorCode,
      };
    },
    isDependencyChanged(error) {
      return errorText(error).includes("codex_provider_set_dependency_changed");
    },
    errorCode: codexProtocolLabErrorCode,
  };
}

export interface CodexProviderSetBatchDraft {
  sources: CodexProviderSetBatchSource[];
  router: Provider;
}

export interface CodexProviderSetBatchProbeOutcome {
  outcomes: Array<{
    providerId: string;
    inputProvider: Provider;
    outcome: CodexProviderProtocolPreflightOutcome;
  }>;
  sources: CodexProviderSetBatchSource[];
}

export interface CodexProviderProtocolProbeTarget {
  providerId: string;
  providerName: string;
  model: string;
}

export type CodexProviderScopedProtocolProbeProgressEvent =
  CodexProtocolProbeProgressEvent & {
    providerId?: string;
    providerName?: string;
  };

interface BatchCodexProtocolLabApi {
  preflightCodexProviderProtocolCompatibility: typeof preflightCodexProviderProtocolCompatibility;
  prepareCodexProviderSetBatch: typeof prepareCodexProviderSetBatch;
  commitCodexProviderSetBatch: typeof commitCodexProviderSetBatch;
}

const batchCodexProtocolLabApi: BatchCodexProtocolLabApi = {
  preflightCodexProviderProtocolCompatibility,
  prepareCodexProviderSetBatch,
  commitCodexProviderSetBatch,
};

export type BatchCodexProtocolLabAdapter = ProtocolLabAdapter<
  CodexProviderSetBatchDraft,
  CodexProviderAdaptationView[],
  CodexProviderSetBatchPreview,
  CodexProviderSetBatchCommitOutcome,
  CodexProviderScopedProtocolProbeProgressEvent,
  CodexProviderSetBatchProbeOutcome,
  CodexProviderSetBatchCommitOutcome
>;

export function createBatchCodexProtocolLabAdapter(
  api: BatchCodexProtocolLabApi = batchCodexProtocolLabApi,
): BatchCodexProtocolLabAdapter {
  return {
    requiresProbe(draft) {
      return draft.sources.some(
        (source) =>
          sourceNeedsProtocolProbe(source.provider) &&
          source.receiptIds.length === 0,
      );
    },
    isManual() {
      // Batch commit is a router-level automatic transaction. Individual manual
      // source choices are already compiled into each source plan.
      return false;
    },
    async preflight(draft, onProgress) {
      const outcomes: CodexProviderSetBatchProbeOutcome["outcomes"] = [];
      const sources: CodexProviderSetBatchSource[] = [];
      for (const source of draft.sources) {
        if (
          !sourceNeedsProtocolProbe(source.provider) ||
          source.receiptIds.length > 0
        ) {
          sources.push(source);
          continue;
        }
        const outcome = await api.preflightCodexProviderProtocolCompatibility(
          source.provider,
          (event) =>
            onProgress({
              ...event,
              providerId: source.provider.id,
              providerName: source.provider.name,
            }),
        );
        outcomes.push({
          providerId: source.provider.id,
          inputProvider: source.provider,
          outcome,
        });
        sources.push({
          provider: outcome.provider,
          receiptIds: outcome.receiptIds,
        });
      }
      return {
        outcome: { outcomes, sources },
        receiptIds: sources.flatMap((source) => source.receiptIds),
        adaptationPreview: outcomes.map(
          ({ outcome }) => outcome.adaptationPreview,
        ),
        draft: { ...draft, sources },
      };
    },
    async prepare(draft) {
      return api.prepareCodexProviderSetBatch(draft.sources, draft.router);
    },
    plan(preview) {
      if (preview.blocked) return "blocked";
      return preview.sourcePreviews.some(
        (source) => source.plan.kind === "split",
      )
        ? "split"
        : "single";
    },
    async commit(draft, _receiptIds, preview) {
      const outcome = await api.commitCodexProviderSetBatch(
        draft.sources,
        draft.router,
        preview.digest,
        "accept_auto",
      );
      return {
        outcome,
        snapshot: outcome,
        projectionWarning: outcome.status === "committed_with_projection_error",
        projectionErrorCode: outcome.projectionErrorCode,
      };
    },
    isDependencyChanged(error) {
      return errorText(error).includes("codex_provider_set_dependency_changed");
    },
    errorCode: codexProtocolLabErrorCode,
  };
}

export function providerHasAutomaticCodexModels(provider: Provider): boolean {
  if (skipsCodexProtocolProbe(provider)) return false;
  const models = enabledPublicModels(provider);
  if (provider.meta?.codexProtocolMode === "manual" && models.length === 0) {
    return false;
  }
  if (models.length === 0) return true;
  return codexProviderModelsRequiringProtocolProbe(provider).length > 0;
}

export function codexProviderModelsRequiringProtocolProbe(
  provider: Provider,
): string[] {
  if (skipsCodexProtocolProbe(provider)) return [];
  const overrides = provider.meta?.codexProtocolOverrides ?? {};
  const normalizedOverrides = new Set(
    Object.keys(overrides).map(normalizeCodexPublicModelKey),
  );
  if (
    provider.meta?.codexProtocolMode === "manual" &&
    normalizedOverrides.size === 0
  ) {
    return [];
  }
  return enabledPublicModels(provider).filter(
    (model) => !normalizedOverrides.has(normalizeCodexPublicModelKey(model)),
  );
}

export function codexProtocolLabErrorCode(error: unknown): string | undefined {
  const detail = errorText(error).toLowerCase();
  if (/\b(?:401|403)\b/.test(detail)) return "authentication_unavailable";
  if (/\b429\b/.test(detail)) return "rate_limited";
  if (/\b521\b/.test(detail) || /\b5\d\d\b/.test(detail)) {
    return "upstream_unavailable";
  }
  if (detail.includes("timeout") || detail.includes("超时")) return "timeout";
  if (
    detail.includes("network") ||
    detail.includes("connection") ||
    detail.includes("网络")
  ) {
    return "network_unavailable";
  }
  if (detail.includes("dependency_changed")) return "dependency_changed";
  return undefined;
}

function enabledPublicModels(provider: Provider): string[] {
  const catalog = provider.settingsConfig.modelCatalog as
    | { models?: Array<{ model?: unknown; enabled?: unknown }> }
    | undefined;
  return (catalog?.models ?? [])
    .filter((model) => model.enabled !== false)
    .map((model) => (typeof model.model === "string" ? model.model.trim() : ""))
    .filter(Boolean);
}

function sourceNeedsProtocolProbe(provider: Provider): boolean {
  return providerHasAutomaticCodexModels(provider);
}

function skipsCodexProtocolProbe(provider: Provider): boolean {
  const providerType = provider.meta?.providerType?.trim().toLowerCase();
  return (
    provider.category === "official" ||
    providerType === "codex_oauth" ||
    providerType === "xai_oauth" ||
    providerType === "github_copilot"
  );
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

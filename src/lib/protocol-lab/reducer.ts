import type { ProtocolLabEvent, ProtocolLabState } from "./types";

export function createProtocolLabState<
  TDraft,
  TAdaptation = unknown,
  TPreparePreview = unknown,
  TSnapshot = TDraft,
  TProgress = unknown,
>(
  initial: Partial<
    ProtocolLabState<TDraft, TAdaptation, TPreparePreview, TSnapshot, TProgress>
  > = {},
): ProtocolLabState<
  TDraft,
  TAdaptation,
  TPreparePreview,
  TSnapshot,
  TProgress
> {
  return {
    phase: "idle",
    draft: null,
    snapshot: null,
    adaptationPreview: null,
    preparePreview: null,
    progress: [],
    receiptIds: [],
    pendingIntent: null,
    evidenceStale: false,
    staleRetryCount: 0,
    errorCode: null,
    errorDetail: null,
    ...initial,
  };
}

export function protocolLabReducer<
  TDraft,
  TAdaptation = unknown,
  TPreparePreview = unknown,
  TSnapshot = TDraft,
  TProgress = unknown,
>(
  state: ProtocolLabState<
    TDraft,
    TAdaptation,
    TPreparePreview,
    TSnapshot,
    TProgress
  >,
  event: ProtocolLabEvent<
    TDraft,
    TAdaptation,
    TPreparePreview,
    TSnapshot,
    TProgress
  >,
): ProtocolLabState<
  TDraft,
  TAdaptation,
  TPreparePreview,
  TSnapshot,
  TProgress
> {
  switch (event.type) {
    case "snapshot_loading":
      return {
        ...state,
        phase: "loading_snapshot",
        errorCode: null,
        errorDetail: null,
      };
    case "snapshot_loaded":
      return {
        ...state,
        phase: "ready",
        snapshot: event.snapshot,
        draft: event.draft ?? state.draft,
        evidenceStale: false,
        staleRetryCount: 0,
        errorCode: null,
        errorDetail: null,
      };
    case "draft_changed":
      return {
        ...state,
        phase: "draft_dirty",
        draft: event.draft,
        receiptIds: [],
        preparePreview: null,
        evidenceStale:
          state.adaptationPreview !== null || state.snapshot !== null,
        errorCode: null,
        errorDetail: null,
      };
    case "validation_requested":
      return {
        ...state,
        phase: "awaiting_probe_consent",
        pendingIntent: "validate",
        progress: [],
        errorCode: null,
        errorDetail: null,
      };
    case "save_requested":
      return {
        ...state,
        phase: event.requiresProbe ? "awaiting_probe_consent" : "preparing",
        pendingIntent: "save",
        progress: event.requiresProbe ? [] : state.progress,
        errorCode: null,
        errorDetail: null,
      };
    case "probe_consented":
    case "stale_retry_started":
      return {
        ...state,
        phase: "probing",
        receiptIds: [],
        progress: [],
        errorCode: null,
        errorDetail: null,
      };
    case "probe_progress":
      return { ...state, progress: [...state.progress, event.progress] };
    case "probe_succeeded":
      return {
        ...state,
        phase: state.pendingIntent === "save" ? "preparing" : "draft_dirty",
        draft: event.draft ?? state.draft,
        receiptIds: [...event.receiptIds],
        adaptationPreview: event.adaptationPreview,
        evidenceStale: false,
        errorCode: null,
        errorDetail: null,
      };
    case "prepare_started":
      return {
        ...state,
        phase: "preparing",
        errorCode: null,
        errorDetail: null,
      };
    case "prepare_succeeded":
      return {
        ...state,
        phase: event.plan === "blocked" ? "blocked" : "committing",
        preparePreview: event.preview,
        errorCode: null,
        errorDetail: null,
      };
    case "commit_succeeded":
      return {
        ...state,
        phase: event.projectionWarning
          ? "committed_projection_warning"
          : "committed",
        snapshot: event.snapshot,
        pendingIntent: null,
        evidenceStale: false,
        errorCode: event.projectionErrorCode ?? null,
        errorDetail: null,
      };
    case "dependency_changed":
      if (state.staleRetryCount === 0) {
        return {
          ...state,
          phase: "stale_retry",
          staleRetryCount: 1,
          receiptIds: [],
          preparePreview: null,
          evidenceStale: true,
          errorCode: "dependency_changed",
          errorDetail: null,
        };
      }
      return {
        ...state,
        phase: "failed",
        errorCode: "dependency_changed_twice",
        errorDetail: null,
      };
    case "failed":
      return {
        ...state,
        phase: "failed",
        errorCode: event.errorCode ?? "request_failed",
        errorDetail: event.detail,
      };
    case "retry_requested":
      return {
        ...state,
        phase: "awaiting_probe_consent",
        pendingIntent: state.pendingIntent ?? "save",
        errorCode: null,
        errorDetail: null,
      };
    case "reset":
      return createProtocolLabState({ draft: event.draft ?? null });
  }
}

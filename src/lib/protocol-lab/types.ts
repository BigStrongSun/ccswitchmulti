export type ProtocolLabPhase =
  | "idle"
  | "loading_snapshot"
  | "ready"
  | "draft_dirty"
  | "awaiting_probe_consent"
  | "probing"
  | "preparing"
  | "committing"
  | "blocked"
  | "stale_retry"
  | "committed"
  | "committed_projection_warning"
  | "failed";

export type ProtocolLabPendingIntent = "validate" | "save" | null;
export type ProtocolLabPreparedPlan = "single" | "split" | "blocked";

export interface ProtocolLabState<
  TDraft,
  TAdaptation = unknown,
  TPreparePreview = unknown,
  TSnapshot = TDraft,
  TProgress = unknown,
> {
  phase: ProtocolLabPhase;
  draft: TDraft | null;
  snapshot: TSnapshot | null;
  adaptationPreview: TAdaptation | null;
  preparePreview: TPreparePreview | null;
  progress: TProgress[];
  receiptIds: string[];
  pendingIntent: ProtocolLabPendingIntent;
  evidenceStale: boolean;
  staleRetryCount: number;
  errorCode: string | null;
  errorDetail: string | null;
}

export type ProtocolLabEvent<
  TDraft,
  TAdaptation = unknown,
  TPreparePreview = unknown,
  TSnapshot = TDraft,
  TProgress = unknown,
> =
  | { type: "snapshot_loading" }
  | { type: "snapshot_loaded"; snapshot: TSnapshot; draft?: TDraft }
  | { type: "draft_changed"; draft: TDraft }
  | { type: "validation_requested" }
  | { type: "save_requested"; requiresProbe: boolean }
  | { type: "probe_consented" }
  | { type: "probe_progress"; progress: TProgress }
  | {
      type: "probe_succeeded";
      receiptIds: string[];
      adaptationPreview: TAdaptation;
      draft?: TDraft;
    }
  | { type: "prepare_started" }
  | {
      type: "prepare_succeeded";
      preview: TPreparePreview;
      plan: ProtocolLabPreparedPlan;
    }
  | {
      type: "commit_succeeded";
      snapshot: TSnapshot;
      projectionWarning: boolean;
      projectionErrorCode?: string | null;
    }
  | { type: "dependency_changed" }
  | { type: "stale_retry_started" }
  | { type: "failed"; errorCode?: string; detail: string }
  | { type: "retry_requested" }
  | { type: "reset"; draft?: TDraft };

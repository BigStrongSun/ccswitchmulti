import { useCallback, useRef, useState } from "react";

import { createProtocolLabState, protocolLabReducer } from "./reducer";
import type {
  ProtocolLabEvent,
  ProtocolLabPreparedPlan,
  ProtocolLabState,
} from "./types";

export type ProtocolLabCommitIntent = "accept_auto" | "confirm_manual";

export interface ProtocolLabPreflightResult<
  TDraft,
  TAdaptation,
  TProbeOutcome,
> {
  outcome: TProbeOutcome;
  receiptIds: string[];
  adaptationPreview: TAdaptation;
  draft?: TDraft;
}

export interface ProtocolLabCommitResult<TSnapshot, TCommitOutcome> {
  outcome: TCommitOutcome;
  snapshot: TSnapshot;
  projectionWarning: boolean;
  projectionErrorCode?: string | null;
}

export interface ProtocolLabAdapter<
  TDraft,
  TAdaptation,
  TPreparePreview,
  TSnapshot,
  TProgress,
  TProbeOutcome = unknown,
  TCommitOutcome = unknown,
> {
  requiresProbe: (draft: TDraft, receiptIds: string[]) => boolean;
  isManual: (draft: TDraft) => boolean;
  preflight: (
    draft: TDraft,
    onProgress: (progress: TProgress) => void,
  ) => Promise<ProtocolLabPreflightResult<TDraft, TAdaptation, TProbeOutcome>>;
  prepare: (draft: TDraft, receiptIds: string[]) => Promise<TPreparePreview>;
  plan: (preview: TPreparePreview) => ProtocolLabPreparedPlan;
  commit: (
    draft: TDraft,
    receiptIds: string[],
    preview: TPreparePreview,
    intent: ProtocolLabCommitIntent,
  ) => Promise<ProtocolLabCommitResult<TSnapshot, TCommitOutcome>>;
  isDependencyChanged: (error: unknown) => boolean;
  loadSnapshot?: (
    providerId: string,
  ) => Promise<{ snapshot: TSnapshot; draft: TDraft }>;
  errorCode?: (error: unknown) => string | undefined;
}

interface ActiveOperation<TDraft> {
  kind: "validate" | "save";
  draft: TDraft;
  receiptIds: string[];
  resolve: (outcome: unknown) => void;
  reject: (error: unknown) => void;
}

export class ProtocolLabCancelled extends Error {}
export class ProtocolLabBlocked extends Error {
  constructor() {
    super("protocol_lab_blocked");
  }
}

export function useProtocolLabWorkflow<
  TDraft,
  TAdaptation,
  TPreparePreview,
  TSnapshot,
  TProgress,
  TProbeOutcome = unknown,
  TCommitOutcome = unknown,
>(
  adapter: ProtocolLabAdapter<
    TDraft,
    TAdaptation,
    TPreparePreview,
    TSnapshot,
    TProgress,
    TProbeOutcome,
    TCommitOutcome
  >,
) {
  type State = ProtocolLabState<
    TDraft,
    TAdaptation,
    TPreparePreview,
    TSnapshot,
    TProgress
  >;
  type Event = ProtocolLabEvent<
    TDraft,
    TAdaptation,
    TPreparePreview,
    TSnapshot,
    TProgress
  >;

  const [state, setState] = useState<State>(() => createProtocolLabState());
  const stateRef = useRef(state);
  const activeOperation = useRef<ActiveOperation<TDraft> | null>(null);
  const runProbeRef = useRef<
    (operation: ActiveOperation<TDraft>, staleRetry: boolean) => Promise<void>
  >(async () => undefined);
  const runPrepareRef = useRef<
    (
      operation: ActiveOperation<TDraft>,
      staleRetryCount: number,
    ) => Promise<void>
  >(async () => undefined);
  const [probeOutcome, setProbeOutcome] = useState<TProbeOutcome | null>(null);

  const transition = useCallback((event: Event) => {
    setState((current) => {
      const next = protocolLabReducer(current, event);
      stateRef.current = next;
      return next;
    });
  }, []);

  const errorDetail = useCallback(
    (error: unknown) =>
      error instanceof Error ? error.message : String(error),
    [],
  );

  const finish = useCallback(
    (operation: ActiveOperation<TDraft>, outcome: unknown) => {
      if (activeOperation.current !== operation) return;
      activeOperation.current = null;
      operation.resolve(outcome);
    },
    [],
  );

  runPrepareRef.current = async (operation, staleRetryCount) => {
    if (activeOperation.current !== operation) return;
    transition({ type: "prepare_started" });
    try {
      const preview = await adapter.prepare(
        operation.draft,
        operation.receiptIds,
      );
      if (activeOperation.current !== operation) return;
      const plan = adapter.plan(preview);
      transition({ type: "prepare_succeeded", preview, plan });
      if (plan === "blocked") return;

      const result = await adapter.commit(
        operation.draft,
        operation.receiptIds,
        preview,
        adapter.isManual(operation.draft) ? "confirm_manual" : "accept_auto",
      );
      if (activeOperation.current !== operation) return;
      transition({
        type: "commit_succeeded",
        snapshot: result.snapshot,
        projectionWarning: result.projectionWarning,
        projectionErrorCode: result.projectionErrorCode,
      });
      finish(operation, result.outcome);
    } catch (error) {
      if (activeOperation.current !== operation) return;
      if (adapter.isDependencyChanged(error)) {
        transition({ type: "dependency_changed" });
        if (staleRetryCount === 0) {
          operation.receiptIds = [];
          await runProbeRef.current(operation, true);
          return;
        }
      }
      transition({
        type: "failed",
        errorCode: adapter.errorCode?.(error),
        detail: errorDetail(error),
      });
    }
  };

  runProbeRef.current = async (operation, staleRetry) => {
    if (activeOperation.current !== operation) return;
    transition({
      type: staleRetry ? "stale_retry_started" : "probe_consented",
    });
    try {
      const result = await adapter.preflight(operation.draft, (progress) => {
        if (activeOperation.current === operation) {
          transition({ type: "probe_progress", progress });
        }
      });
      if (activeOperation.current !== operation) return;
      operation.receiptIds = [...result.receiptIds];
      if (result.draft !== undefined) {
        operation.draft = result.draft;
      }
      setProbeOutcome(result.outcome);
      transition({
        type: "probe_succeeded",
        receiptIds: result.receiptIds,
        adaptationPreview: result.adaptationPreview,
        draft: operation.draft,
      });
      if (operation.kind === "validate") {
        finish(operation, result.outcome);
      } else {
        await runPrepareRef.current(operation, staleRetry ? 1 : 0);
      }
    } catch (error) {
      if (activeOperation.current !== operation) return;
      transition({
        type: "failed",
        errorCode: adapter.errorCode?.(error),
        detail: errorDetail(error),
      });
    }
  };

  const createOperation = useCallback(
    <TOutcome>(
      kind: ActiveOperation<TDraft>["kind"],
      draft: TDraft,
      receiptIds: string[],
    ): { operation: ActiveOperation<TDraft>; promise: Promise<TOutcome> } => {
      if (activeOperation.current) {
        throw new Error("protocol_lab_operation_in_progress");
      }
      let resolve!: (outcome: unknown) => void;
      let reject!: (error: unknown) => void;
      const promise = new Promise<TOutcome>((promiseResolve, promiseReject) => {
        resolve = (outcome) => promiseResolve(outcome as TOutcome);
        reject = promiseReject;
      });
      const operation = {
        kind,
        draft,
        receiptIds: [...receiptIds],
        resolve,
        reject,
      };
      activeOperation.current = operation;
      return { operation, promise };
    },
    [],
  );

  const save = useCallback(
    (draft: TDraft, receiptIds: string[] = []): Promise<TCommitOutcome> => {
      const { operation, promise } = createOperation<TCommitOutcome>(
        "save",
        draft,
        receiptIds,
      );
      transition({ type: "draft_changed", draft });
      const requiresProbe = adapter.requiresProbe(draft, operation.receiptIds);
      transition({ type: "save_requested", requiresProbe });
      if (!requiresProbe) {
        void Promise.resolve().then(() => runPrepareRef.current(operation, 0));
      }
      return promise;
    },
    [adapter, createOperation, transition],
  );

  const validate = useCallback(
    (draft: TDraft): Promise<TProbeOutcome> => {
      const { promise } = createOperation<TProbeOutcome>("validate", draft, []);
      transition({ type: "draft_changed", draft });
      transition({ type: "validation_requested" });
      return promise;
    },
    [createOperation, transition],
  );

  const confirmProbe = useCallback(async () => {
    const operation = activeOperation.current;
    if (!operation) throw new Error("protocol_lab_operation_missing");
    await runProbeRef.current(operation, false);
  }, []);

  const retry = useCallback(() => {
    const operation = activeOperation.current;
    if (!operation) return;
    operation.receiptIds = [];
    transition({ type: "retry_requested" });
    void runProbeRef.current(operation, false);
  }, [transition]);

  const cancel = useCallback(
    (error: unknown = new ProtocolLabCancelled()) => {
      const operation = activeOperation.current;
      if (!operation) return;
      activeOperation.current = null;
      operation.reject(error);
      transition({ type: "reset", draft: operation.draft });
    },
    [transition],
  );

  const reset = useCallback(
    (draft?: TDraft) => {
      const operation = activeOperation.current;
      if (operation) {
        activeOperation.current = null;
        operation.reject(new ProtocolLabCancelled());
      }
      setProbeOutcome(null);
      transition({ type: "reset", draft });
    },
    [transition],
  );

  const loadSnapshot = useCallback(
    async (providerId: string) => {
      if (!adapter.loadSnapshot) {
        throw new Error("protocol_lab_snapshot_loader_missing");
      }
      transition({ type: "snapshot_loading" });
      try {
        const loaded = await adapter.loadSnapshot(providerId);
        transition({
          type: "snapshot_loaded",
          snapshot: loaded.snapshot,
          draft: loaded.draft,
        });
        return loaded;
      } catch (error) {
        transition({
          type: "failed",
          errorCode: adapter.errorCode?.(error),
          detail: errorDetail(error),
        });
        throw error;
      }
    },
    [adapter, errorDetail, transition],
  );

  return {
    state,
    probeOutcome,
    save,
    validate,
    confirmProbe,
    retry,
    cancel,
    reset,
    loadSnapshot,
  };
}

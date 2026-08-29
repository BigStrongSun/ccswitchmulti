import { useCallback, useRef, useState } from "react";
import { CodexProtocolProbeProgressDialog } from "@/components/providers/forms/CodexProtocolProbeProgressDialog";
import { CodexProviderSetPreviewDialog } from "@/components/providers/forms/CodexProviderSetPreviewDialog";
import {
  commitUniversalProviderSet,
  preflightUniversalCodexProtocolCompatibility,
  prepareUniversalProviderSet,
  type CodexProtocolProbeProgressEvent,
  type CodexProviderProtocolPreflightOutcome,
  type UniversalProviderSetPreview,
} from "@/lib/api/protocol-compatibility";
import type { UniversalProvider } from "@/types";

export class UniversalProviderSetCancelled extends Error {}

interface PendingUniversalProviderSetOperation {
  provider: UniversalProvider;
  resolve: () => void;
  reject: (error: unknown) => void;
  receiptIds: string[];
  preview: UniversalProviderSetPreview | null;
}

export function isUniversalProviderSetCancelled(error: unknown): boolean {
  return error instanceof UniversalProviderSetCancelled;
}

export function useUniversalProviderSetSave() {
  const activeOperation = useRef<PendingUniversalProviderSetOperation | null>(
    null,
  );
  const [probeOpen, setProbeOpen] = useState(false);
  const [probeRunning, setProbeRunning] = useState(false);
  const [probeEvents, setProbeEvents] = useState<
    CodexProtocolProbeProgressEvent[]
  >([]);
  const [probeOutcome, setProbeOutcome] =
    useState<CodexProviderProtocolPreflightOutcome | null>(null);
  const [probeError, setProbeError] = useState("");
  const [probeModels, setProbeModels] = useState<string[]>([]);
  const [preview, setPreview] = useState<UniversalProviderSetPreview | null>(
    null,
  );
  const [commitPending, setCommitPending] = useState(false);

  const finishOperation = useCallback((error?: unknown) => {
    const operation = activeOperation.current;
    if (!operation) return;
    activeOperation.current = null;
    setProbeOpen(false);
    setProbeRunning(false);
    setPreview(null);
    setCommitPending(false);
    if (error === undefined) operation.resolve();
    else operation.reject(error);
  }, []);

  const commitPrepared = useCallback(
    async (
      operation: PendingUniversalProviderSetOperation,
      intent: "accept_single" | "confirm_split" | "confirm_manual",
    ) => {
      if (!operation.preview) {
        throw new Error("Universal Provider Set preview is missing");
      }
      await commitUniversalProviderSet(
        operation.provider,
        operation.receiptIds,
        operation.preview.digest,
        intent,
      );
    },
    [],
  );

  const runOperation = useCallback(
    async (operation: PendingUniversalProviderSetOperation) => {
      const codexModel = operation.provider.models.codex?.model?.trim();
      const automaticCodexProbe =
        operation.provider.apps.codex &&
        operation.provider.meta?.codexProtocolMode !== "manual";
      setProbeModels(codexModel ? [codexModel] : []);
      setProbeEvents([]);
      setProbeOutcome(null);
      setProbeError("");
      setPreview(null);
      setProbeOpen(automaticCodexProbe);
      setProbeRunning(automaticCodexProbe);

      try {
        const outcome = automaticCodexProbe
          ? await preflightUniversalCodexProtocolCompatibility(
              operation.provider,
              (event) => setProbeEvents((current) => [...current, event]),
            )
          : null;
        if (activeOperation.current !== operation) return;
        setProbeOutcome(outcome);
        setProbeRunning(false);
        operation.receiptIds = outcome?.receiptIds ?? [];
        operation.preview = await prepareUniversalProviderSet(
          operation.provider,
          operation.receiptIds,
        );
        if (activeOperation.current !== operation) return;

        const codexPlan = operation.preview.codex?.plan;
        if (codexPlan?.kind === "split" || codexPlan?.kind === "blocked") {
          setProbeOpen(false);
          setPreview(operation.preview);
          return;
        }

        await commitPrepared(
          operation,
          operation.provider.meta?.codexProtocolMode === "manual"
            ? "confirm_manual"
            : "accept_single",
        );
        finishOperation();
      } catch (error) {
        if (activeOperation.current !== operation) return;
        setProbeRunning(false);
        setProbeOpen(true);
        setProbeError(error instanceof Error ? error.message : String(error));
      }
    },
    [commitPrepared, finishOperation],
  );

  const persistUniversalProviderSet = useCallback(
    (provider: UniversalProvider) => {
      if (activeOperation.current) {
        return Promise.reject(
          new Error(
            "Another Universal Provider Set operation is already running",
          ),
        );
      }
      return new Promise<void>((resolve, reject) => {
        const operation: PendingUniversalProviderSetOperation = {
          provider,
          resolve,
          reject,
          receiptIds: [],
          preview: null,
        };
        activeOperation.current = operation;
        void runOperation(operation);
      });
    },
    [runOperation],
  );

  const handleBack = useCallback(() => {
    finishOperation(new UniversalProviderSetCancelled());
  }, [finishOperation]);

  const handleRetry = useCallback(() => {
    const operation = activeOperation.current;
    if (operation && !probeRunning && !commitPending) {
      void runOperation(operation);
    }
  }, [commitPending, probeRunning, runOperation]);

  const handleConfirmSplit = useCallback(async () => {
    const operation = activeOperation.current;
    if (!operation || commitPending) return;
    setCommitPending(true);
    try {
      await commitPrepared(operation, "confirm_split");
      finishOperation();
    } catch (error) {
      finishOperation(error);
    }
  }, [commitPending, commitPrepared, finishOperation]);

  const dialogs = (
    <>
      <CodexProtocolProbeProgressDialog
        open={probeOpen}
        running={probeRunning}
        expectedModels={probeModels}
        events={probeEvents}
        outcome={probeOutcome}
        error={probeError}
        onOpenChange={(open) => {
          if (!open) handleBack();
        }}
        onRetry={probeError ? handleRetry : undefined}
      />
      <CodexProviderSetPreviewDialog
        open={preview !== null}
        preview={preview?.codex ?? null}
        pending={commitPending}
        onBack={handleBack}
        onConfirmSplit={handleConfirmSplit}
        onRetry={handleRetry}
      />
    </>
  );

  return { persistUniversalProviderSet, dialogs };
}

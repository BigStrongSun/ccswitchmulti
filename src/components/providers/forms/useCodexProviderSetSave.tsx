import { useCallback, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { providersApi } from "@/lib/api";
import {
  commitCodexProviderSet,
  prepareCodexProviderSet,
  preflightCodexProviderProtocolCompatibility,
  type CodexProviderProtocolPreflightOutcome,
  type CodexProviderSetPreview,
  type CodexProtocolProbeProgressEvent,
} from "@/lib/api/protocol-compatibility";
import type { Provider } from "@/types";
import { CodexProtocolProbeProgressDialog } from "./CodexProtocolProbeProgressDialog";
import { CodexProviderSetPreviewDialog } from "./CodexProviderSetPreviewDialog";

interface PendingCodexProviderSetOperation {
  provider: Provider;
  receiptIds: string[];
  preview: CodexProviderSetPreview | null;
  resolve: () => void;
  reject: (error: unknown) => void;
}

export class CodexProviderSetCancelled extends Error {}

export function isCodexProviderSetCancelled(error: unknown): boolean {
  return error instanceof CodexProviderSetCancelled;
}

export function useCodexProviderSetSave() {
  const queryClient = useQueryClient();
  const activeOperation = useRef<PendingCodexProviderSetOperation | null>(null);
  const runOperationRef = useRef<
    (
      operation: PendingCodexProviderSetOperation,
      forceProbe?: boolean,
    ) => Promise<void>
  >(() => Promise.resolve());
  const [probeOpen, setProbeOpen] = useState(false);
  const [probeRunning, setProbeRunning] = useState(false);
  const [probeEvents, setProbeEvents] = useState<
    CodexProtocolProbeProgressEvent[]
  >([]);
  const [probeOutcome, setProbeOutcome] =
    useState<CodexProviderProtocolPreflightOutcome | null>(null);
  const [probeError, setProbeError] = useState("");
  const [preview, setPreview] = useState<CodexProviderSetPreview | null>(null);
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

  const refreshProviderViews = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.warn(
        "Failed to refresh tray menu after Provider Set commit",
        error,
      );
      toast.warning(
        "模型源已保存，但托盘菜单刷新失败；重新打开应用后会自动恢复。",
      );
    }
  }, [queryClient]);

  const commitPrepared = useCallback(
    async (
      operation: PendingCodexProviderSetOperation,
      intent: "accept_single" | "confirm_split" | "confirm_manual",
    ) => {
      if (!operation.preview) {
        throw new Error("Codex Provider Set preview is missing");
      }
      const outcome = await commitCodexProviderSet(
        operation.provider,
        operation.receiptIds,
        operation.preview.digest,
        intent,
      );
      await refreshProviderViews();
      if (outcome.status === "committed_with_projection_error") {
        toast.warning(
          "模型源已保存，但 Codex 当前配置尚未完成刷新；可稍后重新激活该模型源。",
        );
      }
    },
    [refreshProviderViews],
  );

  const runOperation = useCallback(
    async (operation: PendingCodexProviderSetOperation, forceProbe = false) => {
      const automatic = operation.provider.meta?.codexProtocolMode !== "manual";
      const shouldProbe =
        automatic && (forceProbe || operation.receiptIds.length === 0);
      setProbeEvents([]);
      setProbeOutcome(null);
      setProbeError("");
      setPreview(null);
      setProbeOpen(shouldProbe);
      setProbeRunning(shouldProbe);

      try {
        if (shouldProbe) {
          const outcome = await preflightCodexProviderProtocolCompatibility(
            operation.provider,
            (event) => setProbeEvents((current) => [...current, event]),
          );
          if (activeOperation.current !== operation) return;
          operation.receiptIds = outcome.receiptIds;
          setProbeOutcome(outcome);
          setProbeRunning(false);
        }

        operation.preview = await prepareCodexProviderSet(
          operation.provider,
          operation.receiptIds,
        );
        if (activeOperation.current !== operation) return;

        if (
          operation.preview.plan.kind === "split" ||
          operation.preview.plan.kind === "blocked"
        ) {
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
        const detail = error instanceof Error ? error.message : String(error);
        if (
          automatic &&
          !shouldProbe &&
          (detail.includes("codex_provider_set_probe_required") ||
            detail.includes("codex_provider_set_probe_target_mismatch"))
        ) {
          operation.receiptIds = [];
          await runOperationRef.current?.(operation, true);
          return;
        }
        setProbeRunning(false);
        setProbeOpen(true);
        setProbeError(detail);
      }
    },
    [commitPrepared, finishOperation],
  );
  runOperationRef.current = runOperation;

  const persistCodexProviderSet = useCallback(
    (provider: Provider, receiptIds: string[] = []) => {
      if (activeOperation.current) {
        return Promise.reject(
          new Error("Another Codex Provider Set operation is already running"),
        );
      }
      return new Promise<void>((resolve, reject) => {
        const operation: PendingCodexProviderSetOperation = {
          provider,
          receiptIds: [...receiptIds],
          preview: null,
          resolve,
          reject,
        };
        activeOperation.current = operation;
        void runOperation(operation);
      });
    },
    [runOperation],
  );

  const handleBack = useCallback(() => {
    finishOperation(new CodexProviderSetCancelled());
  }, [finishOperation]);

  const handleRetry = useCallback(() => {
    const operation = activeOperation.current;
    if (operation && !probeRunning && !commitPending) {
      operation.receiptIds = [];
      void runOperation(operation, true);
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
      const detail = error instanceof Error ? error.message : String(error);
      if (detail.includes("codex_provider_set_dependency_changed")) {
        setCommitPending(false);
        operation.receiptIds = [];
        void runOperationRef.current?.(operation, true);
        return;
      }
      finishOperation(error);
    }
  }, [commitPending, commitPrepared, finishOperation]);

  const expectedModels = providerModels(activeOperation.current?.provider);
  const dialogs = (
    <>
      <CodexProtocolProbeProgressDialog
        open={probeOpen}
        running={probeRunning}
        expectedModels={expectedModels}
        events={probeEvents}
        outcome={probeOutcome}
        error={probeError}
        onOpenChange={(open) => {
          if (!open) handleBack();
        }}
        onRetry={handleRetry}
      />
      <CodexProviderSetPreviewDialog
        open={preview !== null}
        preview={preview}
        pending={commitPending}
        onBack={handleBack}
        onConfirmSplit={handleConfirmSplit}
        onRetry={handleRetry}
      />
    </>
  );

  return { persistCodexProviderSet, dialogs };
}

function providerModels(provider: Provider | undefined): string[] {
  const catalog = provider?.settingsConfig.modelCatalog as
    | { models?: Array<{ model?: unknown; enabled?: unknown }> }
    | undefined;
  return (catalog?.models ?? [])
    .filter((model) => model.enabled !== false)
    .map((model) => (typeof model.model === "string" ? model.model.trim() : ""))
    .filter(Boolean);
}

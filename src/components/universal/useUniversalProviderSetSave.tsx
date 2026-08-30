import { useCallback, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { CodexProtocolProbeProgressDialog } from "@/components/providers/forms/CodexProtocolProbeProgressDialog";
import { CodexProviderSetPreviewDialog } from "@/components/providers/forms/CodexProviderSetPreviewDialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { providersApi } from "@/lib/api";
import type {
  UniversalProviderSetCommitOutcome,
  UniversalProviderSetPreview,
} from "@/lib/api/protocol-compatibility";
import { createUniversalCodexProtocolLabAdapter } from "@/lib/protocol-lab/codex-adapters";
import {
  ProtocolLabCancelled,
  useProtocolLabWorkflow,
} from "@/lib/protocol-lab/useProtocolLabWorkflow";
import type { UniversalProvider } from "@/types";

export class UniversalProviderSetCancelled extends ProtocolLabCancelled {}

export function isUniversalProviderSetCancelled(error: unknown): boolean {
  return error instanceof UniversalProviderSetCancelled;
}

interface UseUniversalProviderSetSaveOptions {
  onCommitted?: () => void | Promise<void>;
}

export function useUniversalProviderSetSave({
  onCommitted,
}: UseUniversalProviderSetSaveOptions = {}) {
  const queryClient = useQueryClient();
  const adapter = useMemo(() => createUniversalCodexProtocolLabAdapter(), []);
  const workflow = useProtocolLabWorkflow(adapter);

  const refreshProviderViews = useCallback(async () => {
    const refreshes = await Promise.allSettled([
      queryClient.invalidateQueries({ queryKey: ["providers"] }),
      queryClient.invalidateQueries({
        queryKey: ["codex-provider-adaptation-summaries"],
      }),
      Promise.resolve(onCommitted?.()),
    ]);
    if (refreshes.some((refresh) => refresh.status === "rejected")) {
      console.warn(
        "Universal Provider Set committed, but one or more views failed to refresh",
        refreshes,
      );
      toast.warning(
        "模型源已保存，但部分页面未能立即刷新；重新打开对应页面即可恢复。",
      );
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.warn(
        "Failed to refresh tray menu after Universal Provider Set commit",
        error,
      );
      toast.warning(
        "模型源已保存，但托盘菜单刷新失败；重新打开应用后会自动恢复。",
      );
    }
  }, [onCommitted, queryClient]);

  const persistUniversalProviderSet = useCallback(
    async (provider: UniversalProvider) => {
      const outcome = await workflow.save(provider);
      await refreshProviderViews();
      return outcome;
    },
    [refreshProviderViews, workflow],
  );

  const retryProjection = useCallback(
    async (outcome: UniversalProviderSetCommitOutcome) => {
      const routerIds = Array.from(
        new Set(
          outcome.projections
            .filter((projection) => projection.state === "pending")
            .map((projection) => projection.routerProviderId),
        ),
      );
      if (routerIds.length === 0) {
        throw new Error("codex_provider_set_projection_retry_target_missing");
      }
      const projections = await Promise.all(
        routerIds.map((providerId) =>
          providersApi.retryCodexMultiRouterProjection(providerId),
        ),
      );
      await refreshProviderViews();
      if (projections.some((projection) => projection.state !== "ready")) {
        throw new Error("codex_provider_set_live_projection_failed");
      }
      return projections;
    },
    [refreshProviderViews],
  );

  const handleBack = useCallback(() => {
    workflow.cancel(new UniversalProviderSetCancelled());
  }, [workflow]);

  const handleRetry = useCallback(() => {
    workflow.retry();
  }, [workflow]);

  const probeModel = workflow.state.draft?.models.codex?.model?.trim();
  const awaitingConsent =
    workflow.state.phase === "awaiting_probe_consent" ||
    workflow.state.phase === "stale_retry";
  const probeOpen =
    workflow.state.phase === "probing" || workflow.state.phase === "failed";
  const preview =
    workflow.state.phase === "blocked"
      ? (workflow.state.preparePreview as UniversalProviderSetPreview | null)
      : null;
  const dialogs = (
    <>
      <Dialog
        open={awaitingConsent}
        onOpenChange={(open) => {
          if (!open) handleBack();
        }}
      >
        <DialogContent className="max-w-lg" zIndex="top">
          <DialogHeader>
            <DialogTitle>确认测试 Chat / Responses</DialogTitle>
            <DialogDescription className="space-y-2 text-left">
              <span className="block">
                “保存并同步”会先对 Codex 模型发送少量真实请求，分别验证
                Responses 与 Chat Completions，然后以同一原子事务保存最终配置。
              </span>
              <span className="block">
                测试可能产生少量额度或流量消耗。认证、限流、网络、HTTP 521
                和其他 5xx 会单独显示为可用性问题，不会据此推荐错误协议。
              </span>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={handleBack}>
              取消
            </Button>
            <Button type="button" onClick={() => void workflow.confirmProbe()}>
              确认测试
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <CodexProtocolProbeProgressDialog
        open={probeOpen}
        running={workflow.state.phase === "probing"}
        expectedModels={probeModel ? [probeModel] : []}
        events={workflow.state.progress}
        outcome={workflow.probeOutcome}
        error={workflow.state.errorDetail ?? ""}
        onOpenChange={(open) => {
          if (!open) handleBack();
        }}
        onRetry={handleRetry}
      />
      <CodexProviderSetPreviewDialog
        open={preview !== null}
        preview={preview?.codex ?? null}
        pending={workflow.state.phase === "committing"}
        onBack={handleBack}
        onConfirmSplit={() => undefined}
        onRetry={handleRetry}
      />
    </>
  );

  return {
    persistUniversalProviderSet,
    retryProjection,
    dialogs,
    workflow,
  };
}

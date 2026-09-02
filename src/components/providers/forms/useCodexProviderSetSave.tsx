import { useCallback, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { providersApi } from "@/lib/api";
import type {
  CodexProviderSetCommitOutcome,
  CodexProviderSetPreview,
} from "@/lib/api/protocol-compatibility";
import { createSingleCodexProtocolLabAdapter } from "@/lib/protocol-lab/codex-adapters";
import {
  ProtocolLabCancelled,
  useProtocolLabWorkflow,
} from "@/lib/protocol-lab/useProtocolLabWorkflow";
import type { Provider } from "@/types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { CodexProtocolProbeProgressDialog } from "./CodexProtocolProbeProgressDialog";
import { CodexProviderSetPreviewDialog } from "./CodexProviderSetPreviewDialog";

export class CodexProviderSetCancelled extends ProtocolLabCancelled {}

export function isCodexProviderSetCancelled(error: unknown): boolean {
  return error instanceof CodexProviderSetCancelled;
}

export function useCodexProviderSetSave() {
  const queryClient = useQueryClient();
  const adapter = useMemo(() => createSingleCodexProtocolLabAdapter(), []);
  const workflow = useProtocolLabWorkflow(adapter);

  const refreshProviderViews = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    await queryClient.invalidateQueries({
      queryKey: ["codex-provider-adaptation-summaries"],
    });
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

  const persistCodexProviderSet = useCallback(
    async (provider: Provider, receiptIds: string[] = []) => {
      const outcome = await workflow.save(provider, receiptIds);
      await refreshProviderViews();
      if (
        (outcome as CodexProviderSetCommitOutcome).status ===
        "committed_with_projection_error"
      ) {
        toast.warning(
          "模型源已保存，但 Codex 当前配置尚未完成刷新；可稍后重新投影或重新激活该模型源。",
        );
      }
    },
    [refreshProviderViews, workflow],
  );

  const handleBack = useCallback(() => {
    workflow.cancel(new CodexProviderSetCancelled());
  }, [workflow]);

  const handleRetry = useCallback(() => {
    workflow.retry();
  }, [workflow]);

  const expectedModels = providerModels(workflow.state.draft ?? undefined);
  const awaitingConsent =
    workflow.state.phase === "awaiting_probe_consent" ||
    workflow.state.phase === "stale_retry";
  const probeOpen =
    workflow.state.phase === "probing" || workflow.state.phase === "failed";
  const preview =
    workflow.state.phase === "blocked"
      ? (workflow.state.preparePreview as CodexProviderSetPreview | null)
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
                保存前需要向上游发送少量真实请求，分别验证 Responses 和 Chat
                Completions；会消耗模型 Token
                并可能产生费用，也可能触发限流。完成后会显示上游已报告的 Token
                和按当前模型定价估算的费用。
              </span>
              <span className="block">
                每个启用模型会依次测试基础响应、SSE
                流式、思考内容、强制工具调用和工具结果续轮。认证失败、限流、网络错误、HTTP
                521 或其他 5xx 会显示为上游不可用，不会被误判为协议不支持。
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
        expectedModels={expectedModels}
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
        preview={preview}
        pending={workflow.state.phase === "committing"}
        onBack={handleBack}
        onConfirmSplit={() => undefined}
        onRetry={handleRetry}
      />
    </>
  );

  return {
    persistCodexProviderSet,
    dialogs,
    workflow,
  };
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

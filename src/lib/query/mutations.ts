import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, sessionsApi, settingsApi, type AppId } from "@/lib/api";
import type { DeleteSessionOptions } from "@/lib/api/sessions";
import type { SwitchResult } from "@/lib/api/providers";
import type { Provider, SessionMeta, Settings } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { generateUUID } from "@/utils/uuid";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { invalidateHermesProviderCaches } from "@/hooks/useHermes";
import { proxyKeys } from "@/lib/query/proxy";
import { usageKeys } from "@/lib/query/usage";
import {
  syncCodexMultiRouterPlansAfterProviderChange,
  type CodexMultiRouterPlanSyncResult,
} from "@/lib/codexMultiRouterSync";

// Provider 保存结果需要携带 Codex MultiRouter 同步报告，供上层决定是否提示用户补选子 Agent 模型。
export interface UpdateProviderMutationResult {
  provider: Provider;
  codexMultiRouterSyncResults?: CodexMultiRouterPlanSyncResult[];
}
import {
  CODEX_OFFICIAL_PROVIDER_ID,
  GROKBUILD_OFFICIAL_PROVIDER_ID,
} from "@/utils/providerCapabilities";

interface ProviderSwitchFailureToastAction {
  label: string;
  onClick: () => void | Promise<void>;
}

interface ProviderSwitchFailureToastOptions {
  action: ProviderSwitchFailureToastAction;
  cancel?: ProviderSwitchFailureToastAction;
}

interface CreateProviderSwitchFailureToastOptionsInput {
  appId: AppId;
  providerId: string;
  detail: string;
  copy: (detail: string) => void;
  forceRepair: (providerId: string) => void | Promise<void>;
  t: (key: string, fallback: string) => string;
}

/** Build recovery actions without losing the failed provider identity. */
export function createProviderSwitchFailureToastOptions({
  appId,
  providerId,
  detail,
  copy,
  forceRepair,
  t,
}: CreateProviderSwitchFailureToastOptionsInput): ProviderSwitchFailureToastOptions {
  return {
    action: {
      label: t("common.copy", "复制"),
      onClick: () => copy(detail),
    },
    ...(appId === "codex"
      ? {
          cancel: {
            label: t("notifications.forceRepair", "强制覆盖"),
            onClick: () => forceRepair(providerId),
          },
        }
      : {}),
  };
}

const codexForceRepairInFlight = new Set<string>();

export const useAddProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (
      providerInput: Omit<Provider, "id"> & {
        providerKey?: string;
        addToLive?: boolean;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureCodexOfficialSeed?: boolean;
        ensureGrokBuildOfficialSeed?: boolean;
      },
    ) => {
      const {
        providerKey: _providerKey,
        addToLive,
        ensureClaudeDesktopOfficialSeed,
        ensureCodexOfficialSeed,
        ensureGrokBuildOfficialSeed,
        ...rest
      } = providerInput;

      if (appId === "claude-desktop" && ensureClaudeDesktopOfficialSeed) {
        await providersApi.ensureClaudeDesktopOfficialProvider();
        const providers = await providersApi.getAll(appId);
        const officialProvider = providers["claude-desktop-official"];
        if (!officialProvider) {
          throw new Error("Claude Desktop official provider was not created");
        }
        return officialProvider;
      }

      if (appId === "codex" && ensureCodexOfficialSeed) {
        await providersApi.ensureCodexOfficialProvider();
        const providers = await providersApi.getAll(appId);
        const officialProvider = providers[CODEX_OFFICIAL_PROVIDER_ID];
        if (!officialProvider) {
          throw new Error("Codex official provider was not created");
        }
        return officialProvider;
      }

      if (appId === "grokbuild" && ensureGrokBuildOfficialSeed) {
        await providersApi.ensureGrokBuildOfficialProvider();
        const providers = await providersApi.getAll(appId);
        const officialProvider = providers[GROKBUILD_OFFICIAL_PROVIDER_ID];
        if (!officialProvider) {
          throw new Error("Grok Build official provider was not created");
        }
        return officialProvider;
      }

      let id: string;

      if (appId === "opencode" || appId === "openclaw" || appId === "hermes") {
        if (
          providerInput.category === "omo" ||
          providerInput.category === "omo-slim"
        ) {
          const prefix = providerInput.category === "omo" ? "omo" : "omo-slim";
          id = `${prefix}-${generateUUID()}`;
        } else {
          if (!providerInput.providerKey) {
            throw new Error(`Provider key is required for ${appId}`);
          }
          id = providerInput.providerKey;
        }
      } else {
        id = generateUUID();
      }

      const newProvider: Provider = {
        ...rest,
        id,
        createdAt: Date.now(),
      };
      delete (newProvider as any).providerKey;

      await providersApi.add(newProvider, appId, addToLive);
      return newProvider;
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });

      if (appId === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["omo", "current-provider-id"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo", "provider-count"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo-slim", "current-provider-id"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo-slim", "provider-count"],
        });
      }

      if (appId === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      }

      if (appId === "hermes") {
        await invalidateHermesProviderCaches(queryClient);
      }

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after adding provider",
          trayError,
        );
      }

      toast.success(
        t("notifications.providerAdded", {
          defaultValue: "供应商已添加",
        }),
        {
          closeButton: true,
        },
      );
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.addFailed", {
          defaultValue: "添加供应商失败: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useUpdateProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({
      provider,
      originalId,
    }: {
      provider: Provider;
      originalId?: string;
    }) => {
      await providersApi.update(provider, appId, originalId);
      let codexMultiRouterSyncResults:
        | CodexMultiRouterPlanSyncResult[]
        | undefined;
      if (appId === "codex") {
        const providerMap = await providersApi.getAll(appId);
        codexMultiRouterSyncResults =
          syncCodexMultiRouterPlansAfterProviderChange(
            Object.values(providerMap),
            provider,
            originalId,
          );
        for (const result of codexMultiRouterSyncResults) {
          await providersApi.update(result.plan, appId);
        }
      }
      return { provider, codexMultiRouterSyncResults };
    },
    onSuccess: async (result, variables) => {
      const provider = result.provider;
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
      await queryClient.invalidateQueries({
        queryKey: usageKeys.script(provider.id, appId),
      });
      if (variables.originalId && variables.originalId !== provider.id) {
        await queryClient.invalidateQueries({
          queryKey: usageKeys.script(variables.originalId, appId),
        });
      }
      if (appId === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      }
      if (appId === "hermes") {
        await invalidateHermesProviderCaches(queryClient);
      }
      toast.success(
        t("notifications.updateSuccess", {
          defaultValue: "供应商更新成功",
        }),
        {
          closeButton: true,
        },
      );
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.updateFailed", {
          defaultValue: "更新供应商失败: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useDeleteProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (providerId: string) => {
      await providersApi.delete(providerId, appId);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });

      if (appId === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["omo", "current-provider-id"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo", "provider-count"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo-slim", "current-provider-id"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo-slim", "provider-count"],
        });
      }

      if (appId === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      }

      if (appId === "hermes") {
        await invalidateHermesProviderCaches(queryClient);
      }

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after deleting provider",
          trayError,
        );
      }

      toast.success(
        t("notifications.deleteSuccess", {
          defaultValue: "供应商已删除",
        }),
        {
          closeButton: true,
        },
      );
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.deleteFailed", {
          defaultValue: "删除供应商失败: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useSwitchProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (providerId: string): Promise<SwitchResult> => {
      return await providersApi.switch(providerId, appId);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
      if (appId === "codex") {
        await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
        await queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
        await queryClient.invalidateQueries({
          queryKey: ["proxyTakeoverStatus"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["liveTakeoverActive"],
        });
      }
      if (appId === "claude-desktop") {
        await queryClient.invalidateQueries({ queryKey: proxyKeys.status });
        await queryClient.invalidateQueries({
          queryKey: ["claudeDesktopStatus"],
        });
      }

      // OpenCode/OpenClaw: also invalidate live provider IDs cache to update button state
      if (appId === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["opencodeLiveProviderIds"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["opencode", "runtime-models"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo", "current-provider-id"],
        });
        await queryClient.invalidateQueries({
          queryKey: ["omo-slim", "current-provider-id"],
        });
      }
      if (appId === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.liveProviderIds,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.defaultModel,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      }
      if (appId === "hermes") {
        await invalidateHermesProviderCaches(queryClient);
      }

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after switching provider",
          trayError,
        );
      }
    },
    onError: (error: Error, providerId: string) => {
      const detail = extractErrorMessage(error) || t("common.unknown");

      const recoveryActions = createProviderSwitchFailureToastOptions({
        appId,
        providerId,
        detail,
        copy: (text) => {
          navigator.clipboard?.writeText(text).catch(() => undefined);
        },
        forceRepair: async (failedProviderId) => {
          if (codexForceRepairInFlight.has(failedProviderId)) return;
          const confirmed = window.confirm(
            t("notifications.forceRepairConfirm", {
              defaultValue:
                "将先备份当前 Codex 配置，再修复旧字段并用该供应商重建 CCSM 托管配置。MCP、项目和用户自定义配置会保留。是否继续？",
            }),
          );
          if (!confirmed) return;

          codexForceRepairInFlight.add(failedProviderId);
          try {
            const outcome =
              await providersApi.forceRepairAndSwitchCodexProvider(
                failedProviderId,
              );
            await queryClient.invalidateQueries({
              queryKey: ["providers", appId],
            });
            await queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
            await queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
            await queryClient.invalidateQueries({
              queryKey: ["proxyTakeoverStatus"],
            });
            await queryClient.invalidateQueries({
              queryKey: ["liveTakeoverActive"],
            });
            toast.success(
              t("notifications.forceRepairSuccess", {
                defaultValue: "Codex 配置已修复并切换成功",
              }),
              {
                description: t("notifications.forceRepairBackup", {
                  defaultValue: "原配置已备份到：{{path}}",
                  path: outcome.backupDirectory,
                }),
                duration: 10000,
              },
            );
          } catch (repairError) {
            toast.error(
              t("notifications.forceRepairFailed", {
                defaultValue: "强制覆盖失败",
              }),
              {
                description:
                  extractErrorMessage(repairError) || t("common.unknown"),
                duration: 12000,
              },
            );
          } finally {
            codexForceRepairInFlight.delete(failedProviderId);
          }
        },
        t: (key, fallback) => t(key, { defaultValue: fallback }),
      });

      toast.error(
        t("notifications.switchFailedTitle", { defaultValue: "切换失败" }),
        {
          description: t("notifications.switchFailed", {
            defaultValue: "切换失败：{{error}}",
            error: detail,
          }),
          duration: 12000,
          ...recoveryActions,
        },
      );
    },
  });
};

export const useDeleteSessionMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (input: DeleteSessionOptions) => {
      await sessionsApi.delete(input);
      return input;
    },
    onSuccess: async (input) => {
      queryClient.setQueryData<SessionMeta[]>(["sessions"], (current) =>
        (current ?? []).filter(
          (session) =>
            !(
              session.providerId === input.providerId &&
              session.sessionId === input.sessionId &&
              session.sourcePath === input.sourcePath
            ),
        ),
      );
      queryClient.removeQueries({
        queryKey: ["sessionMessages", input.providerId, input.sourcePath],
      });

      await queryClient.invalidateQueries({ queryKey: ["sessions"] });

      toast.success(
        t("sessionManager.sessionDeleted", {
          defaultValue: "会话已删除",
        }),
      );
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("sessionManager.deleteFailed", {
          defaultValue: "删除会话失败: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useSaveSettingsMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (settings: Settings) => {
      await settingsApi.save(settings);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      await queryClient.invalidateQueries({
        queryKey: ["opencode", "runtime-models"],
      });
    },
  });
};

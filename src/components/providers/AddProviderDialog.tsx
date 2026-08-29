import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider, CustomEndpoint, UniversalProvider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import { UniversalProviderPanel } from "@/components/universal";
import { useUniversalProviderSetSave } from "@/components/universal/useUniversalProviderSetSave";
import {
  isCodexProviderSetCancelled,
  useCodexProviderSetSave,
} from "@/components/providers/forms/useCodexProviderSetSave";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import { extractGrokBuildBaseUrl } from "@/utils/grokBuildConfig";
import { GROKBUILD_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";
import { generateUUID } from "@/utils/uuid";

interface AddProviderDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appId: AppId;
  panelZIndexClassName?: string;
  onSubmit: (
    provider: Omit<Provider, "id"> & {
      providerKey?: string;
      suggestedDefaults?: OpenClawSuggestedDefaults;
      ensureClaudeDesktopOfficialSeed?: boolean;
      ensureCodexOfficialSeed?: boolean;
      ensureGrokBuildOfficialSeed?: boolean;
    },
  ) => Promise<void> | void;
}

export function AddProviderDialog({
  open,
  onOpenChange,
  appId,
  panelZIndexClassName,
  onSubmit,
}: AddProviderDialogProps) {
  const { t } = useTranslation();
  // OpenCode and OpenClaw don't support universal providers
  const showUniversalTab =
    appId !== "opencode" &&
    appId !== "openclaw" &&
    appId !== "hermes" &&
    appId !== "grokbuild" &&
    appId !== "claude-desktop";
  const isCodexRouterEntry = appId === "codex";
  const [activeTab, setActiveTab] = useState<"app-specific" | "universal">(
    isCodexRouterEntry ? "universal" : "app-specific",
  );
  const [universalFormOpen, setUniversalFormOpen] = useState(false);
  const [selectedUniversalPreset, setSelectedUniversalPreset] =
    useState<UniversalProviderPreset | null>(null);
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  const { persistUniversalProviderSet, dialogs: universalProviderSetDialogs } =
    useUniversalProviderSetSave();
  const { persistCodexProviderSet, dialogs: codexProviderSetDialogs } =
    useCodexProviderSetSave();
  const codexDraftProviderIdRef = useRef<string | null>(null);

  useEffect(() => {
    // Codex 的添加入口实际是在创建多路路由，默认引导到模型源选择页。
    setActiveTab(isCodexRouterEntry ? "universal" : "app-specific");
    if (
      open &&
      isCodexRouterEntry &&
      codexDraftProviderIdRef.current === null
    ) {
      codexDraftProviderIdRef.current = generateUUID();
    } else if (!open) {
      codexDraftProviderIdRef.current = null;
    }
  }, [isCodexRouterEntry, open]);

  const handleUniversalProviderSave = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await persistUniversalProviderSet(provider);
        toast.success(
          t("universalProvider.addedAndSynced", {
            defaultValue: "统一供应商已添加并同步",
          }),
        );
      } catch (error) {
        console.error(
          "[AddProviderDialog] Failed to atomically save and sync universal provider",
          error,
        );
        toast.error(
          t("universalProvider.saveAndSyncError", {
            defaultValue: "保存并同步失败，原配置未被修改",
          }),
        );
        return;
      }

      setUniversalFormOpen(false);
      setSelectedUniversalPreset(null);
      onOpenChange(false);
    },
    [persistUniversalProviderSet, t, onOpenChange],
  );

  const handleUniversalFormClose = useCallback(() => {
    setUniversalFormOpen(false);
    setSelectedUniversalPreset(null);
  }, []);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;

      // 构造基础提交数据
      const providerData: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureCodexOfficialSeed?: boolean;
        ensureGrokBuildOfficialSeed?: boolean;
      } = {
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        ...(values.meta ? { meta: values.meta } : {}),
      };

      if (appId === "claude-desktop" && values.presetId) {
        const presetIndex = parseInt(
          values.presetId.replace("claude-desktop-", ""),
        );
        const preset = claudeDesktopProviderPresets[presetIndex];
        providerData.ensureClaudeDesktopOfficialSeed =
          values.presetCategory === "official" &&
          preset?.category === "official";
      }

      if (appId === "codex" && values.presetId) {
        const presetIndex = parseInt(values.presetId.replace("codex-", ""));
        const preset = codexProviderPresets[presetIndex];
        providerData.ensureCodexOfficialSeed =
          values.presetCategory === "official" &&
          preset?.category === "official";
      }

      if (appId === "grokbuild" && values.presetId) {
        providerData.ensureGrokBuildOfficialSeed =
          values.presetCategory === "official" &&
          values.presetId === GROKBUILD_OFFICIAL_PROVIDER_ID;
      }

      // OpenCode/OpenClaw: pass providerKey for ID generation
      if (
        (appId === "opencode" || appId === "openclaw" || appId === "hermes") &&
        values.providerKey
      ) {
        providerData.providerKey = values.providerKey;
      }

      const hasCustomEndpoints =
        providerData.meta?.custom_endpoints &&
        Object.keys(providerData.meta.custom_endpoints).length > 0;

      if (!hasCustomEndpoints && values.presetCategory !== "omo") {
        const urlSet = new Set<string>();

        const addUrl = (rawUrl?: string) => {
          const url = (rawUrl || "").trim().replace(/\/+$/, "");
          if (url && url.startsWith("http")) {
            urlSet.add(url);
          }
        };

        if (values.presetId) {
          if (appId === "claude") {
            const presets = providerPresets;
            const presetIndex = parseInt(
              values.presetId.replace("claude-", ""),
            );
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (preset?.endpointCandidates) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          } else if (appId === "codex") {
            const presets = codexProviderPresets;
            const presetIndex = parseInt(values.presetId.replace("codex-", ""));
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (Array.isArray(preset.endpointCandidates)) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          } else if (appId === "gemini") {
            const presets = geminiProviderPresets;
            const presetIndex = parseInt(
              values.presetId.replace("gemini-", ""),
            );
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (Array.isArray(preset.endpointCandidates)) {
                preset.endpointCandidates.forEach(addUrl);
              }
            }
          } else if (appId === "claude-desktop") {
            const presets = claudeDesktopProviderPresets;
            const presetIndex = parseInt(
              values.presetId.replace("claude-desktop-", ""),
            );
            if (
              !isNaN(presetIndex) &&
              presetIndex >= 0 &&
              presetIndex < presets.length
            ) {
              const preset = presets[presetIndex];
              if (Array.isArray(preset.endpointCandidates)) {
                preset.endpointCandidates.forEach(addUrl);
              }
              addUrl(preset.baseUrl);
            }
          }
        }

        if (appId === "claude") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.ANTHROPIC_BASE_URL) {
            addUrl(env.ANTHROPIC_BASE_URL);
          }
        } else if (appId === "claude-desktop") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.ANTHROPIC_BASE_URL) {
            addUrl(env.ANTHROPIC_BASE_URL);
          }
        } else if (appId === "codex") {
          const config = parsedConfig.config as string | undefined;
          if (config) {
            const extractedBaseUrl = extractCodexBaseUrl(config);
            if (extractedBaseUrl) {
              addUrl(extractedBaseUrl);
            }
          }
        } else if (appId === "gemini") {
          const env = parsedConfig.env as Record<string, any> | undefined;
          if (env?.GOOGLE_GEMINI_BASE_URL) {
            addUrl(env.GOOGLE_GEMINI_BASE_URL);
          }
        } else if (appId === "grokbuild") {
          const config = parsedConfig.config as string | undefined;
          if (config) {
            addUrl(extractGrokBuildBaseUrl(config));
          }
        } else if (appId === "opencode") {
          const options = parsedConfig.options as
            | Record<string, any>
            | undefined;
          if (options?.baseURL) {
            addUrl(options.baseURL);
          }
        } else if (appId === "openclaw") {
          // OpenClaw uses baseUrl directly
          if (parsedConfig.baseUrl) {
            addUrl(parsedConfig.baseUrl as string);
          }
        } else if (appId === "hermes") {
          if (parsedConfig.base_url) {
            addUrl(parsedConfig.base_url as string);
          }
        }

        const urls = Array.from(urlSet);
        if (urls.length > 0) {
          const now = Date.now();
          const customEndpoints: Record<string, CustomEndpoint> = {};
          urls.forEach((url) => {
            customEndpoints[url] = {
              url,
              addedAt: now,
              lastUsed: undefined,
            };
          });

          providerData.meta = {
            ...(providerData.meta ?? {}),
            custom_endpoints: customEndpoints,
          };
        }
      }

      // OpenClaw: pass suggestedDefaults for model registration
      if (appId === "openclaw" && values.suggestedDefaults) {
        providerData.suggestedDefaults = values.suggestedDefaults;
      }

      const codexRouting = providerData.settingsConfig.codexRouting;
      const authSource = providerData.meta?.authBinding?.source;
      const isEligibleCodexLogicalSource =
        appId === "codex" &&
        providerData.category !== "official" &&
        providerData.meta?.apiFormat !== "anthropic" &&
        authSource !== "managed_account" &&
        authSource !== "managed_codex_oauth" &&
        !(codexRouting && typeof codexRouting === "object");

      if (isEligibleCodexLogicalSource) {
        const provider: Provider = {
          id: codexDraftProviderIdRef.current ?? generateUUID(),
          name: providerData.name,
          notes: providerData.notes,
          websiteUrl: providerData.websiteUrl,
          settingsConfig: providerData.settingsConfig,
          icon: providerData.icon,
          iconColor: providerData.iconColor,
          category: providerData.category,
          meta: providerData.meta,
          createdAt: Date.now(),
        };
        codexDraftProviderIdRef.current = provider.id;
        try {
          await persistCodexProviderSet(
            provider,
            values.protocolProbeReceiptIds ?? [],
          );
        } catch (error) {
          if (isCodexProviderSetCancelled(error)) return;
          throw error;
        }
        onOpenChange(false);
        return;
      }

      await onSubmit(providerData);
      onOpenChange(false);
    },
    [appId, onSubmit, onOpenChange, persistCodexProviderSet, t],
  );

  const footer =
    !showUniversalTab || activeTab === "app-specific" ? (
      <>
        <span className="mr-auto min-w-0 text-xs text-muted-foreground truncate">
          {t("provider.addFooterHint")}
        </span>
        <Button
          variant="outline"
          onClick={() => onOpenChange(false)}
          className="border-border/20 hover:bg-accent hover:text-accent-foreground"
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4 mr-2" />
          {t("common.add")}
        </Button>
      </>
    ) : (
      <>
        <Button
          variant="outline"
          onClick={() => onOpenChange(false)}
          className="border-border/20 hover:bg-accent hover:text-accent-foreground"
        >
          {t("common.cancel")}
        </Button>
        <Button
          onClick={() => setUniversalFormOpen(true)}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Plus className="h-4 w-4 mr-2" />
          {isCodexRouterEntry
            ? t("codexMultiRouter.addSource", {
                defaultValue: "添加模型源",
              })
            : t("universalProvider.add")}
        </Button>
      </>
    );

  return (
    <FullScreenPanel
      isOpen={open}
      title={
        isCodexRouterEntry
          ? t("codexMultiRouter.createTitle", {
              defaultValue: "创建多路路由",
            })
          : t("provider.addNewProvider")
      }
      onClose={() => onOpenChange(false)}
      footer={footer}
      zIndexClassName={panelZIndexClassName}
      contentClassName="pt-3"
    >
      {isCodexRouterEntry && (
        <div className="rounded-lg border border-primary/20 bg-primary/5 p-4 text-sm text-muted-foreground">
          <div className="font-medium text-foreground">
            {t("codexMultiRouter.guideTitle", {
              defaultValue: "多路路由创建方式",
            })}
          </div>
          <div className="mt-2 grid gap-2 md:grid-cols-3">
            <div>
              {t("codexMultiRouter.guideStepSelect", {
                defaultValue: "1. 先选择或接入模型源。",
              })}
            </div>
            <div>
              {t("codexMultiRouter.guideStepSync", {
                defaultValue: "2. 同步后自动生成 Codex 可用配置。",
              })}
            </div>
            <div>
              {t("codexMultiRouter.guideStepRules", {
                defaultValue: "3. 再到路由规则里调整模型分流。",
              })}
            </div>
          </div>
        </div>
      )}

      {showUniversalTab ? (
        <Tabs
          value={activeTab}
          onValueChange={(v) => setActiveTab(v as "app-specific" | "universal")}
        >
          <TabsList className="grid w-full grid-cols-2 mb-6">
            <TabsTrigger value="app-specific">
              {isCodexRouterEntry
                ? t("codexMultiRouter.singleSourceTab", {
                    defaultValue: "单独接入模型源",
                  })
                : `${t(`apps.${appId}`)} ${t("provider.tabProvider")}`}
            </TabsTrigger>
            <TabsTrigger value="universal">
              {isCodexRouterEntry
                ? t("codexMultiRouter.sourceTab", {
                    defaultValue: "选择模型源",
                  })
                : t("provider.tabUniversal")}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="app-specific" className="mt-0">
            <ProviderForm
              appId={appId}
              submitLabel={t("common.add")}
              onSubmit={handleSubmit}
              onCancel={() => onOpenChange(false)}
              onSubmittingChange={setIsFormSubmitting}
              showButtons={false}
            />
          </TabsContent>

          <TabsContent value="universal" className="mt-0">
            <UniversalProviderPanel
              context={isCodexRouterEntry ? "codex-router-source" : "default"}
            />
          </TabsContent>
        </Tabs>
      ) : (
        // OpenCode/OpenClaw: directly show form without tabs
        <ProviderForm
          appId={appId}
          submitLabel={t("common.add")}
          onSubmit={handleSubmit}
          onCancel={() => onOpenChange(false)}
          onSubmittingChange={setIsFormSubmitting}
          showButtons={false}
        />
      )}

      {showUniversalTab && (
        <UniversalProviderFormModal
          isOpen={universalFormOpen}
          onClose={handleUniversalFormClose}
          onSave={handleUniversalProviderSave}
          initialPreset={selectedUniversalPreset}
        />
      )}
      {universalProviderSetDialogs}
      {codexProviderSetDialogs}
    </FullScreenPanel>
  );
}

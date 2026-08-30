import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import {
  isCodexProviderSetCancelled,
  useCodexProviderSetSave,
} from "@/components/providers/forms/useCodexProviderSetSave";
import { openclawApi, providersApi, vscodeApi, type AppId } from "@/lib/api";
import {
  getCodexProviderEditorSnapshot,
  type CodexProviderEditorSnapshot,
} from "@/lib/api/protocol-compatibility";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
  }) => Promise<void> | void;
  appId: AppId;
  isProxyTakeover?: boolean; // 代理接管模式下不读取 live（避免显示被接管后的代理配置）
}

export function EditProviderDialog({
  open,
  provider,
  onOpenChange,
  onSubmit,
  appId,
  isProxyTakeover = false,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  const { persistCodexProviderSet, dialogs: codexProviderSetDialogs } =
    useCodexProviderSetSave();
  const [codexProviderEditorSnapshot, setCodexProviderEditorSnapshot] =
    useState<CodexProviderEditorSnapshot | null>(null);
  const [logicalProviderError, setLogicalProviderError] = useState("");

  const isGeneratedSplitFacade =
    appId === "codex" &&
    provider?.settingsConfig.codexProtocolSet != null &&
    (provider.settingsConfig.codexProtocolSet as { role?: unknown }).role ===
      "facade";
  const editingProvider =
    codexProviderEditorSnapshot?.logicalProvider ?? provider;

  useEffect(() => {
    let cancelled = false;
    setCodexProviderEditorSnapshot(null);
    setLogicalProviderError("");
    if (!open || !provider || appId !== "codex") {
      return;
    }
    void (async () => {
      try {
        const snapshot = await getCodexProviderEditorSnapshot(provider.id);
        if (!cancelled && snapshot?.logicalProvider) {
          setCodexProviderEditorSnapshot(snapshot);
        }
      } catch (error) {
        if (!cancelled) {
          setLogicalProviderError(
            error instanceof Error ? error.message : String(error),
          );
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [appId, open, provider?.id]);

  // 默认使用传入的 provider.settingsConfig，若当前编辑对象是"当前生效供应商"，则尝试读取实时配置替换初始值
  const [liveSettings, setLiveSettings] = useState<Record<
    string,
    unknown
  > | null>(null);

  // 使用 ref 标记是否已经加载过，防止重复读取覆盖用户编辑
  const [hasLoadedLive, setHasLoadedLive] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!open || !editingProvider) {
        setLiveSettings(null);
        setHasLoadedLive(false);
        return;
      }

      if (isGeneratedSplitFacade && !codexProviderEditorSnapshot) return;

      // 关键修复：只在首次打开时加载一次
      if (hasLoadedLive) {
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      if (isProxyTakeover) {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      // OpenCode uses additive mode - each provider's config is stored independently in DB
      // Reading live config would return the full opencode.json (with $schema, provider, mcp etc.)
      // instead of just the provider fragment, causing incorrect nested structure on save
      if (appId === "opencode") {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      if (appId === "openclaw") {
        try {
          const live = await openclawApi.getLiveProvider(editingProvider.id);
          if (!cancelled && live && typeof live === "object") {
            setLiveSettings(live);
          } else if (!cancelled) {
            setLiveSettings(null);
          }
        } catch {
          if (!cancelled) {
            setLiveSettings(null);
          }
        } finally {
          if (!cancelled) {
            setHasLoadedLive(true);
          }
        }
        return;
      }

      try {
        const currentId = await providersApi.getCurrent(appId);
        if (currentId && editingProvider.id === currentId) {
          try {
            const live = (await vscodeApi.getLiveProviderSettings(
              appId,
            )) as Record<string, unknown>;
            if (!cancelled && live && typeof live === "object") {
              setLiveSettings(live);
              setHasLoadedLive(true);
            }
          } catch {
            // 读取实时配置失败则回退到 SSOT（不打断编辑流程）
            if (!cancelled) {
              setLiveSettings(null);
              setHasLoadedLive(true);
            }
          }
        } else {
          if (!cancelled) {
            setLiveSettings(null);
            setHasLoadedLive(true);
          }
        }
      } finally {
        // no-op
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [
    open,
    editingProvider?.id,
    appId,
    hasLoadedLive,
    isProxyTakeover,
    isGeneratedSplitFacade,
    codexProviderEditorSnapshot,
  ]); // 只依赖 provider.id，不依赖整个 provider 对象

  const initialSettingsConfig = useMemo(() => {
    const base = (liveSettings ??
      editingProvider?.settingsConfig ??
      {}) as Record<string, unknown>;

    // Codex 的 modelCatalog 是 cc-switch 私有字段，SSOT 在数据库。Live 的 config.toml
    // 仅在写入时投影出 model_catalog_json 指针；Codex.app 改写配置、代理接管/恢复周期、
    // 来回切换供应商都可能让 Live 丢失该投影，从而 read_live_settings 反解为空。
    // 若放任 Live 覆盖，编辑界面会显示空映射表，保存后连同数据库里的映射一起清空（数据丢失）。
    // 因此始终以数据库 SSOT 的 modelCatalog 为准，仅在数据库确实没有时才回退到 Live 反解结果。
    if (
      appId === "codex" &&
      liveSettings &&
      editingProvider?.settingsConfig &&
      typeof editingProvider.settingsConfig === "object"
    ) {
      const dbConfig = editingProvider.settingsConfig as Record<
        string,
        unknown
      >;
      let merged = base;
      for (const privateField of ["modelCatalog", "codexRouting"]) {
        const dbValue = dbConfig[privateField];
        if (dbValue !== undefined) {
          merged = { ...merged, [privateField]: dbValue };
        }
      }
      if (merged !== base) {
        return merged;
      }
    }

    return base;
  }, [liveSettings, editingProvider?.settingsConfig, appId]); // 只依赖 settingsConfig，不依赖整个 provider

  // 固定 initialData，防止 provider 对象更新时重置表单
  const initialData = useMemo(() => {
    if (!editingProvider) return null;
    return {
      name: editingProvider.name,
      notes: editingProvider.notes,
      websiteUrl: editingProvider.websiteUrl,
      settingsConfig: initialSettingsConfig,
      category: editingProvider.category,
      meta: editingProvider.meta,
      icon: editingProvider.icon,
      iconColor: editingProvider.iconColor,
    };
  }, [
    open, // 修复：编辑保存后再次打开显示旧数据，依赖 open 确保每次打开时重新读取最新 provider 数据
    editingProvider?.id, // 只依赖 ID，provider 对象更新不会触发重新计算
    editingProvider?.meta, // 供应商元数据变化时重新初始化表单
    initialSettingsConfig,
  ]);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      if (!provider || !editingProvider) return;

      // 注意：values.settingsConfig 已经是最终的配置字符串
      // ProviderForm 已经为不同的 app 类型（Claude/Codex/Gemini）正确组装了配置
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;
      const nextProviderId =
        (appId === "opencode" || appId === "openclaw") &&
        values.providerKey?.trim()
          ? values.providerKey.trim()
          : provider.id;

      const updatedProvider: Provider = {
        ...editingProvider,
        id: nextProviderId,
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        // 保留或更新 meta 字段
        ...(values.meta ? { meta: values.meta } : {}),
      };

      const codexRouting = updatedProvider.settingsConfig.codexRouting;
      const protocolSet = updatedProvider.settingsConfig.codexProtocolSet as
        | { role?: unknown }
        | undefined;
      const isGeneratedFacade = protocolSet?.role === "facade";
      const authSource = updatedProvider.meta?.authBinding?.source;
      const isEligibleCodexLogicalSource =
        appId === "codex" &&
        updatedProvider.category !== "official" &&
        updatedProvider.meta?.apiFormat !== "anthropic" &&
        authSource !== "managed_account" &&
        authSource !== "managed_codex_oauth" &&
        (!(codexRouting && typeof codexRouting === "object") ||
          isGeneratedFacade);

      if (isEligibleCodexLogicalSource) {
        try {
          await persistCodexProviderSet(
            updatedProvider,
            values.protocolProbeReceiptIds ?? [],
          );
        } catch (error) {
          if (isCodexProviderSetCancelled(error)) return;
          throw error;
        }
        onOpenChange(false);
        return;
      }

      await onSubmit({
        provider: updatedProvider,
        originalId: provider.id,
      });
      onOpenChange(false);
    },
    [
      appId,
      editingProvider,
      onSubmit,
      onOpenChange,
      persistCodexProviderSet,
      provider,
    ],
  );

  if (!provider || (isGeneratedSplitFacade && !codexProviderEditorSnapshot)) {
    if (!logicalProviderError) return null;
    return (
      <FullScreenPanel
        isOpen={open}
        title={t("provider.editProvider")}
        onClose={() => onOpenChange(false)}
      >
        <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-4 text-sm text-destructive">
          无法恢复自动拆分模型源的逻辑配置：{logicalProviderError}
        </div>
      </FullScreenPanel>
    );
  }
  if (!initialData || !editingProvider) {
    return null;
  }

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.editProvider")}
      onClose={() => onOpenChange(false)}
      footer={
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save className="h-4 w-4 mr-2" />
          {t("common.save")}
        </Button>
      }
    >
      <ProviderForm
        appId={appId}
        providerId={editingProvider.id}
        submitLabel={t("common.save")}
        onSubmit={handleSubmit}
        onCancel={() => onOpenChange(false)}
        onSubmittingChange={setIsFormSubmitting}
        initialData={initialData}
        codexProviderEditorSnapshot={codexProviderEditorSnapshot}
        showButtons={false}
        isProxyTakeover={isProxyTakeover}
      />
      {codexProviderSetDialogs}
    </FullScreenPanel>
  );
}

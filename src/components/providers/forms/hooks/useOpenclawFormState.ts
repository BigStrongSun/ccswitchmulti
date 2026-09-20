import { useState, useCallback, useMemo } from "react";
import type {
  OpenClawModel,
  OpenClawProviderConfig,
  OpenClawTeProviderSettings,
} from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import { OPENCLAW_DEFAULT_CONFIG } from "../helpers/opencodeFormUtils";
import {
  buildOpenClawTeProviderConfig,
  canonicalizeTeProviderSettings,
  sanitizeTeProviderDraft,
  validateTeProviderSettings,
} from "@/utils/teProvider";

interface UseOpenclawFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export const OPENCLAW_DEFAULT_USER_AGENT =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

export interface OpenclawFormState {
  openclawProviderKey: string;
  setOpenclawProviderKey: (key: string) => void;
  openclawBaseUrl: string;
  openclawApiKey: string;
  openclawApi: string;
  openclawModels: OpenClawModel[];
  openclawUserAgent: boolean;
  openclawTeProvider: OpenClawTeProviderSettings | null;
  existingOpenclawKeys: string[];
  handleOpenclawBaseUrlChange: (baseUrl: string) => void;
  handleOpenclawApiKeyChange: (apiKey: string) => void;
  handleOpenclawApiChange: (api: string) => void;
  handleOpenclawModelsChange: (models: OpenClawModel[]) => void;
  handleOpenclawUserAgentChange: (enabled: boolean) => void;
  handleOpenclawTeProviderChange: (
    settings: OpenClawTeProviderSettings | null,
  ) => void;
  resetOpenclawState: (config?: OpenClawProviderConfig) => void;
}

function parseOpenclawField<T>(
  initialData: UseOpenclawFormStateParams["initialData"],
  field: string,
  fallback: T,
): T {
  try {
    const config = JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : OPENCLAW_DEFAULT_CONFIG,
    );
    return (config[field] as T) || fallback;
  } catch {
    return fallback;
  }
}

export function useOpenclawFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseOpenclawFormStateParams): OpenclawFormState {
  // Query existing providers for duplicate key checking
  const { data: openclawProvidersData } = useProvidersQuery("openclaw");
  const existingOpenclawKeys = useMemo(() => {
    if (!openclawProvidersData?.providers) return [];
    return Object.keys(openclawProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [openclawProvidersData?.providers, providerId]);

  const [openclawProviderKey, setOpenclawProviderKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return providerId || "";
  });

  const [openclawBaseUrl, setOpenclawBaseUrl] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return parseOpenclawField(initialData, "baseUrl", "");
  });

  const [openclawApiKey, setOpenclawApiKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    return parseOpenclawField(initialData, "apiKey", "");
  });

  const [openclawApi, setOpenclawApi] = useState<string>(() => {
    if (appId !== "openclaw") return "openai-completions";
    return parseOpenclawField(initialData, "api", "openai-completions");
  });

  const [openclawModels, setOpenclawModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "openclaw") return [];
    return parseOpenclawField<OpenClawModel[]>(initialData, "models", []);
  });

  const [openclawUserAgent, setOpenclawUserAgent] = useState<boolean>(() => {
    if (appId !== "openclaw") return true;
    const headers = parseOpenclawField<Record<string, string>>(
      initialData,
      "headers",
      {},
    );
    return "User-Agent" in headers;
  });

  const [openclawTeProvider, setOpenclawTeProvider] =
    useState<OpenClawTeProviderSettings | null>(() => {
      if (appId !== "openclaw") return null;
      const stored = parseOpenclawField<unknown>(
        initialData,
        "teProvider",
        null,
      );
      // 只保留静态字段；历史/手写配置里的 runtime/secret/unknown 字段不进入表单。
      return sanitizeTeProviderDraft(stored);
    });

  const updateOpenclawConfig = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() || OPENCLAW_DEFAULT_CONFIG,
        );
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleOpenclawBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpenclawBaseUrl(baseUrl);
      updateOpenclawConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpenclawApiKey(apiKey);
      updateOpenclawConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiChange = useCallback(
    (api: string) => {
      setOpenclawApi(api);
      updateOpenclawConfig((config) => {
        config.api = api;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawModelsChange = useCallback(
    (models: OpenClawModel[]) => {
      setOpenclawModels(models);
      updateOpenclawConfig((config) => {
        config.models = models;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawUserAgentChange = useCallback(
    (enabled: boolean) => {
      setOpenclawUserAgent(enabled);
      updateOpenclawConfig((config) => {
        if (enabled) {
          config.headers = { "User-Agent": OPENCLAW_DEFAULT_USER_AGENT };
        } else {
          delete config.headers;
        }
      });
    },
    [updateOpenclawConfig],
  );

  /**
   * 更新 TE Provider 静态设置。
   *
   * 草稿始终可编辑，但只有 canonical descriptor 才能写入 OpenClaw 配置；
   * 这样保存中间态不会把 runtime/secret/unknown 字段或非法端点带入持久化层。
   */
  const handleOpenclawTeProviderChange = useCallback(
    (settings: OpenClawTeProviderSettings | null) => {
      const draft = sanitizeTeProviderDraft(settings);
      setOpenclawTeProvider(draft);
      updateOpenclawConfig((config) => {
        if (!draft) {
          delete config.teProvider;
          return;
        }
        const canonical = canonicalizeTeProviderSettings(draft);
        if (!canonical || validateTeProviderSettings(canonical).length > 0) {
          delete config.teProvider;
          return;
        }
        const projected = buildOpenClawTeProviderConfig(canonical);
        config.teProvider = canonical;
        // 投影结果里 baseUrl/apiKey/models 由 builder 固定生成；这里显式判空，避免把 undefined
        // 写回配置覆盖掉用户既有值。
        if (projected.baseUrl) {
          config.baseUrl = projected.baseUrl;
          setOpenclawBaseUrl(projected.baseUrl);
        }
        if (projected.apiKey) {
          config.apiKey = projected.apiKey;
          setOpenclawApiKey(projected.apiKey);
        }
        config.models = projected.models ?? [];
        setOpenclawModels(projected.models ?? []);
      });
    },
    [updateOpenclawConfig],
  );

  const resetOpenclawState = useCallback((config?: OpenClawProviderConfig) => {
    setOpenclawProviderKey("");
    setOpenclawBaseUrl(config?.baseUrl || "");
    setOpenclawApiKey(config?.apiKey || "");
    setOpenclawApi(config?.api || "openai-completions");
    setOpenclawModels(config?.models || []);
    const ua = config?.headers ? "User-Agent" in config.headers : false;
    setOpenclawUserAgent(ua);
    const draft = sanitizeTeProviderDraft(config?.teProvider);
    setOpenclawTeProvider(draft);
    if (draft && canonicalizeTeProviderSettings(draft)) {
      const projected = buildOpenClawTeProviderConfig(draft);
      setOpenclawBaseUrl(projected.baseUrl ?? "");
      setOpenclawApiKey(projected.apiKey ?? "");
      setOpenclawModels(projected.models ?? []);
    }
  }, []);

  return {
    openclawProviderKey,
    setOpenclawProviderKey,
    openclawBaseUrl,
    openclawApiKey,
    openclawApi,
    openclawModels,
    openclawUserAgent,
    openclawTeProvider,
    existingOpenclawKeys,
    handleOpenclawBaseUrlChange,
    handleOpenclawApiKeyChange,
    handleOpenclawApiChange,
    handleOpenclawModelsChange,
    handleOpenclawUserAgentChange,
    handleOpenclawTeProviderChange,
    resetOpenclawState,
  };
}

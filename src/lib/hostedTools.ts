import type { Provider } from "@/types";

export interface HostedToolsConfig {
  webSearch: {
    enabled: boolean;
  };
  imageGeneration: {
    enabled: boolean;
  };
}

export const DEFAULT_HOSTED_TOOLS_CONFIG: HostedToolsConfig = {
  webSearch: { enabled: true },
  imageGeneration: { enabled: true },
};

function readBool(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function normalizeHostedToolsConfig(value: unknown): HostedToolsConfig {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return DEFAULT_HOSTED_TOOLS_CONFIG;
  }
  const raw = value as Record<string, unknown>;
  const webSearch =
    raw.webSearch && typeof raw.webSearch === "object"
      ? (raw.webSearch as Record<string, unknown>)
      : {};
  const imageGeneration =
    raw.imageGeneration && typeof raw.imageGeneration === "object"
      ? (raw.imageGeneration as Record<string, unknown>)
      : {};
  return {
    webSearch: {
      enabled: readBool(webSearch.enabled, true),
    },
    imageGeneration: {
      enabled: readBool(imageGeneration.enabled, true),
    },
  };
}

export function readHostedToolsConfig(
  provider: Pick<Provider, "settingsConfig">,
): HostedToolsConfig {
  return normalizeHostedToolsConfig(provider.settingsConfig?.hostedTools);
}

export function writeHostedToolsConfig(
  settings: Record<string, unknown>,
  config: HostedToolsConfig,
): Record<string, unknown> {
  return {
    ...settings,
    hostedTools: {
      webSearch: { enabled: config.webSearch.enabled },
      imageGeneration: { enabled: config.imageGeneration.enabled },
    },
  };
}

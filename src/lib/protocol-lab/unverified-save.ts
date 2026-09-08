import type { Provider, UniversalProvider } from "@/types";

type ManualCodexApiFormat = "openai_chat" | "openai_responses";

function declaredCodexApiFormat(provider: Provider): ManualCodexApiFormat {
  const value = provider.meta?.apiFormat ?? provider.settingsConfig.apiFormat;
  if (value === "openai_chat" || value === "openai_responses") return value;
  throw new Error("codex_provider_set_unverified_protocol_required");
}

export function prepareUnverifiedCodexProvider(provider: Provider): Provider {
  const apiFormat = declaredCodexApiFormat(provider);
  return {
    ...provider,
    meta: {
      ...provider.meta,
      apiFormat,
      codexProtocolMode: "manual",
    },
  };
}

export function prepareUnverifiedUniversalProvider(
  provider: UniversalProvider,
): UniversalProvider {
  if (!provider.apps.codex) return provider;
  const configuredFormat = provider.meta?.apiFormat;
  const apiFormat: ManualCodexApiFormat =
    configuredFormat === "openai_chat" ||
    configuredFormat === "openai_responses"
      ? configuredFormat
      : "openai_responses";
  return {
    ...provider,
    meta: {
      ...provider.meta,
      apiFormat,
      codexProtocolMode: "manual",
    },
  };
}

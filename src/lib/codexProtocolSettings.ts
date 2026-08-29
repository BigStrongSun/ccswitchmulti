import { normalizeCodexChatReasoningForSave } from "@/lib/codexChatReasoning";
import { buildLocalProxyRequestOverrides } from "@/lib/requestOverrides";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  CodexHistoryReplay,
  CodexProtocolMode,
  CodexReasoningProjection,
  CodexToolSchemaDialect,
  PromptCacheRoutingMode,
  Provider,
  ProviderCategory,
  ProviderMeta,
} from "@/types";

export interface CodexManualProtocolSettings {
  protocolMode: CodexProtocolMode;
  apiFormat: CodexApiFormat;
  reasoningProjection: CodexReasoningProjection;
  toolSchemaDialect: CodexToolSchemaDialect;
  historyReplay: CodexHistoryReplay;
}

export type CodexProtocolSettingState = Omit<
  CodexManualProtocolSettings,
  "apiFormat"
>;

export function readCodexProtocolSettings(
  meta: ProviderMeta | undefined,
  apiFormat: CodexApiFormat,
): CodexProtocolSettingState {
  const reasoningProjection = ["raw_reasoning_text", "none"].includes(
    meta?.codexReasoningProjection ?? "",
  )
    ? (meta?.codexReasoningProjection as CodexReasoningProjection)
    : "none";
  const toolSchemaDialect = ["openai", "moonshot_mfjs"].includes(
    meta?.codexToolSchemaDialect ?? "",
  )
    ? (meta?.codexToolSchemaDialect as CodexToolSchemaDialect)
    : "openai";
  const responsesHistory = [
    "native_only",
    "responses_reasoning_text_content",
    "omit",
  ].includes(meta?.codexHistoryReplay ?? "")
    ? (meta?.codexHistoryReplay as CodexHistoryReplay)
    : "native_only";

  return {
    protocolMode: meta?.codexProtocolMode === "manual" ? "manual" : "auto",
    reasoningProjection,
    toolSchemaDialect,
    historyReplay:
      apiFormat === "openai_chat" ? "chat_reasoning_content" : responsesHistory,
  };
}

export function buildCodexProtocolMeta(
  current: ProviderMeta | undefined,
  settings: CodexManualProtocolSettings,
): ProviderMeta {
  const {
    codexProtocolMode: _mode,
    codexReasoningProjection: _projection,
    codexToolSchemaDialect: _schema,
    codexHistoryReplay: _history,
    ...base
  } = current ?? {};

  if (
    settings.protocolMode === "auto" ||
    !matchesProbeTransport(settings.apiFormat)
  ) {
    return base;
  }

  return {
    ...base,
    codexProtocolMode: "manual",
    codexToolSchemaDialect: settings.toolSchemaDialect,
    codexHistoryReplay:
      settings.apiFormat === "openai_chat"
        ? "chat_reasoning_content"
        : settings.historyReplay,
    ...(settings.apiFormat === "openai_chat"
      ? {
          codexReasoningProjection:
            settings.reasoningProjection === "raw_reasoning_text"
              ? "raw_reasoning_text"
              : "none",
        }
      : {}),
  };
}

export interface CodexProtocolProbeProviderDraftInput
  extends CodexManualProtocolSettings {
  providerId?: string;
  providerName: string;
  baseUrl: string;
  apiKey: string;
  isFullUrl: boolean;
  defaultModel: string;
  models: CodexCatalogModel[];
  websiteUrl?: string;
  category?: ProviderCategory;
  customUserAgent: string;
  localProxyHeadersOverride: string;
  localProxyBodyOverride: string;
  takeoverEnabled: boolean;
  codexChatReasoning: CodexChatReasoning;
  promptCacheRouting: PromptCacheRoutingMode;
}

export function buildCodexProtocolProbeProviderDraft(
  input: CodexProtocolProbeProviderDraftInput,
): Provider {
  if (!matchesProbeTransport(input.apiFormat)) {
    throw new Error("深度探测仅支持 Responses 与 Chat Completions 上游");
  }
  const overrides = buildLocalProxyRequestOverrides(
    input.localProxyHeadersOverride,
    input.localProxyBodyOverride,
  );
  if (overrides.error) {
    throw new Error(`本地代理请求覆盖格式错误：${overrides.error}`);
  }

  const apiFormat = input.apiFormat;
  const meta = buildCodexProtocolMeta(
    {
      apiFormat,
      ...(input.isFullUrl ? { isFullUrl: true } : {}),
      ...(input.customUserAgent.trim()
        ? { customUserAgent: input.customUserAgent.trim() }
        : {}),
      ...(overrides.overrides
        ? { localProxyRequestOverrides: overrides.overrides }
        : {}),
      ...(apiFormat === "openai_chat" && input.takeoverEnabled
        ? {
            codexChatReasoning: normalizeCodexChatReasoningForSave(
              input.codexChatReasoning,
              {
                providerName: input.providerName,
                baseUrl: input.baseUrl,
                models: input.models,
              },
            ),
          }
        : {}),
      ...(apiFormat === "openai_chat" && input.promptCacheRouting !== "auto"
        ? { promptCacheRouting: input.promptCacheRouting }
        : {}),
    },
    input,
  );

  return {
    id: input.providerId ?? "codex-draft",
    name: input.providerName.trim() || "Codex provider",
    settingsConfig: {
      auth: { OPENAI_API_KEY: input.apiKey },
      config: [
        `model = ${JSON.stringify(input.defaultModel)}`,
        'model_provider = "ccswitch_probe"',
        "[model_providers.ccswitch_probe]",
        `base_url = ${JSON.stringify(input.baseUrl.trim())}`,
        `wire_api = ${JSON.stringify(apiFormat === "openai_chat" ? "chat" : "responses")}`,
      ].join("\n"),
      apiFormat,
      modelCatalog: { models: input.models },
    },
    ...(input.websiteUrl ? { websiteUrl: input.websiteUrl } : {}),
    ...(input.category ? { category: input.category } : {}),
    meta,
    inFailoverQueue: false,
  };
}

function matchesProbeTransport(
  apiFormat: CodexApiFormat,
): apiFormat is "openai_chat" | "openai_responses" {
  return apiFormat === "openai_chat" || apiFormat === "openai_responses";
}

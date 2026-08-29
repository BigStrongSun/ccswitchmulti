import type { CodexCatalogModel, CodexChatReasoning } from "@/types";

export interface CodexChatReasoningSaveContext {
  providerName?: string;
  baseUrl?: string;
  models?: CodexCatalogModel[];
}

const QWEN_VLLM_MIN_OUTPUT_TOKENS = 2048;

function normalizeOutputTokens(value: number | undefined): number | undefined {
  if (value === undefined) return undefined;
  const normalized = Math.floor(Number(value));
  return Number.isFinite(normalized) && normalized > 0 ? normalized : undefined;
}

function shouldApplyQwenVllmDefaults(
  context?: CodexChatReasoningSaveContext,
): boolean {
  const haystack = [
    context?.providerName,
    context?.baseUrl,
    ...(context?.models ?? []).flatMap((model) => [
      model.model,
      model.upstreamModel,
      model.upstream_model,
      model.displayName,
    ]),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return (
    haystack.includes("qwen") &&
    (haystack.includes("vllm") || haystack.includes("matrixminecraft"))
  );
}

export function normalizeCodexChatReasoningForSave(
  value?: CodexChatReasoning,
  context?: CodexChatReasoningSaveContext,
): CodexChatReasoning | undefined {
  const supportsEffort = value?.supportsEffort === true;
  const supportsThinking = value?.supportsThinking === true || supportsEffort;
  const hasExplicitConfig = value && Object.keys(value).length > 0;
  const minOutputTokens = normalizeOutputTokens(value?.minOutputTokens);
  const defaultOutputTokens = normalizeOutputTokens(value?.defaultOutputTokens);

  if (!supportsThinking && !supportsEffort) {
    return hasExplicitConfig
      ? {
          supportsThinking: false,
          supportsEffort: false,
          thinkingParam: "none",
          effortParam: "none",
          ...(minOutputTokens ? { minOutputTokens } : {}),
          ...(defaultOutputTokens ? { defaultOutputTokens } : {}),
          outputFormat: value?.outputFormat ?? "auto",
        }
      : undefined;
  }

  const useQwenVllmDefaults = shouldApplyQwenVllmDefaults(context);
  const thinkingParam =
    supportsThinking &&
    useQwenVllmDefaults &&
    (!value?.thinkingParam || value.thinkingParam === "thinking")
      ? "enable_thinking"
      : supportsThinking
        ? (value?.thinkingParam ?? "thinking")
        : "none";
  const safeMinOutputTokens = useQwenVllmDefaults
    ? Math.max(minOutputTokens ?? 0, QWEN_VLLM_MIN_OUTPUT_TOKENS)
    : minOutputTokens;

  return {
    supportsThinking,
    supportsEffort,
    thinkingParam,
    effortParam: supportsEffort
      ? (value?.effortParam ?? "reasoning_effort")
      : "none",
    effortValueMode: supportsEffort
      ? (value?.effortValueMode ?? "passthrough")
      : undefined,
    ...(safeMinOutputTokens ? { minOutputTokens: safeMinOutputTokens } : {}),
    ...(defaultOutputTokens ? { defaultOutputTokens } : {}),
    outputFormat: value?.outputFormat ?? "auto",
  };
}

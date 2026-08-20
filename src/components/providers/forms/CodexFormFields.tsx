import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  ChevronDown,
  ChevronRight,
  ArrowDown,
  ArrowUp,
  Plus,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  fetchModelsForConfig,
  probeCodexChatForConfig,
  probeCodexResponsesForConfig,
  fetchXaiOauthModels,
  showFetchModelsError,
  type CodexResponsesProbeResult,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import { CodexProviderReadinessSection } from "./CodexProviderReadinessSection";
import { CodexModelReasoningCard } from "./CodexModelReasoningCard";
import { cn } from "@/lib/utils";
import { resolveFetchedCodexModelContextWindow } from "@/utils/codexModelContext";
import {
  codexPlanModelListAction,
  codexCatalogOnlyPlanModelFetchMessage,
  isCodexCatalogOnlyPlanModelFetch,
} from "@/utils/codexPlanModelFetch";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  CodexModelReasoningCapability,
  CodexReasoningEffort,
  CodexRoutingConfig,
  PromptCacheRoutingMode,
  Provider,
  ProviderCategory,
} from "@/types";
import type { AppId } from "@/lib/api";
import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";
import type {
  CodexModelReasoningResolution,
  CodexReasoningDiscoveryOutcome,
} from "@/types/codexSubagentV2";

interface EndpointCandidate {
  url: string;
}

interface CodexProtocolProbeOutcome {
  model: string;
  responses: CodexResponsesProbeResult;
  chat: CodexResponsesProbeResult;
}

const CODEX_PROTOCOL_PROBE_MODEL_CONCURRENCY = 3;
const CODEX_REASONING_EFFORT_CHOICES: CodexReasoningEffort[] = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
];

export type CodexReasoningCapabilitySourceMode =
  | "automatic"
  | "builtin"
  | "manual";

export function applyCodexReasoningCapabilitySource(
  mode: CodexReasoningCapabilitySourceMode,
  current?: CodexModelReasoningCapability,
  maintained?: CodexModelReasoningCapability,
): CodexModelReasoningCapability | undefined {
  if (mode === "automatic") return undefined;
  if (mode === "builtin") {
    return maintained ? structuredClone(maintained) : undefined;
  }
  const seed = current ?? maintained;
  if (seed) return { ...structuredClone(seed), source: "user" };
  return {
    schemaVersion: 2,
    supportStatus: "confirmed_supported",
    controlKind: "none",
    supportedEfforts: [],
    disableAllowed: false,
    upstream: { format: "none", parameter: "none" },
    source: "user",
  };
}

export function validateCodexReasoningCapabilityDraft(
  capability: CodexModelReasoningCapability,
): void {
  const allowed = new Set<CodexReasoningEffort>([
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "ultra",
  ]);
  if (typeof capability?.supported !== "boolean") {
    throw new Error("supported must be boolean");
  }
  if (
    !Array.isArray(capability.supportedEfforts) ||
    capability.supportedEfforts.some((effort) => !allowed.has(effort))
  ) {
    throw new Error("supportedEfforts contains an unknown Provider effort");
  }
  // schema v2 用 supportStatus；legacy 数据用 supported。至少声明其一，
  // 同时存在时不得矛盾。
  if (
    capability.supportStatus === undefined &&
    typeof capability.supported !== "boolean"
  ) {
    throw new Error("supportStatus or supported must be declared");
  }
  if (
    capability.supportStatus !== undefined &&
    typeof capability.supported === "boolean" &&
    (capability.supportStatus === "confirmed_supported") !==
      capability.supported
  ) {
    throw new Error("supportStatus contradicts legacy supported field");
  }
  if (
    capability.defaultEffort !== undefined &&
    !capability.supportedEfforts.includes(capability.defaultEffort)
  ) {
    throw new Error("defaultEffort must be Provider-supported");
  }
  if (typeof capability.disableAllowed !== "boolean") {
    throw new Error("disableAllowed must be boolean");
  }
  if (
    !capability.upstream ||
    typeof capability.upstream.parameter !== "string" ||
    !capability.upstream.parameter.trim()
  ) {
    throw new Error("upstream.parameter must be nonempty");
  }
  for (const target of Object.values(capability.upstream.effortMap ?? {})) {
    if (target && !capability.supportedEfforts.includes(target)) {
      throw new Error(`mapping target ${target} is not Provider-supported`);
    }
  }
}

// 用小并发池执行真实上游探测，避免串行太慢，也避免一次性打爆供应商限流。
async function runCodexProtocolProbePool(
  models: string[],
  concurrency: number,
  probeModel: (
    model: string,
    index: number,
  ) => Promise<CodexProtocolProbeOutcome>,
): Promise<CodexProtocolProbeOutcome[]> {
  const outcomes = new Array<CodexProtocolProbeOutcome>(models.length);
  let nextIndex = 0;

  // 每个 worker 领取下一个模型；Promise.all 保证全部 worker 结束后再汇总。
  async function worker() {
    while (nextIndex < models.length) {
      const index = nextIndex;
      nextIndex += 1;
      outcomes[index] = await probeModel(models[index], index);
    }
  }

  const workerCount = Math.min(Math.max(1, concurrency), models.length);
  await Promise.all(Array.from({ length: workerCount }, () => worker()));
  return outcomes;
}

// 把单模型双协议探测结果归类，供汇总文案和协议自动选择复用。
function classifyProtocolProbeOutcome(outcome: CodexProtocolProbeOutcome) {
  if (outcome.responses.ok && outcome.chat.ok) return "both";
  if (outcome.responses.ok) return "responses";
  if (outcome.chat.ok) return "chat";
  return "failed";
}

// 将模型名压缩成适合内联展示的列表，避免大量模型时把表单撑爆。
function summarizeProbeModels(
  outcomes: CodexProtocolProbeOutcome[],
  limit = 4,
) {
  if (outcomes.length === 0) return "";
  const names = outcomes.slice(0, limit).map((outcome) => outcome.model);
  return `${names.join("、")}${outcomes.length > limit ? ` 等 ${outcomes.length} 个` : ""}`;
}

// 生成每个协议探测分类的摘要，确保用户能同时看到其它模型是成功、部分成功还是失败。
export function summarizeCodexProtocolProbeOutcomes(
  outcomes: CodexProtocolProbeOutcome[],
) {
  const groups = {
    both: outcomes.filter(
      (outcome) => classifyProtocolProbeOutcome(outcome) === "both",
    ),
    responses: outcomes.filter(
      (outcome) => classifyProtocolProbeOutcome(outcome) === "responses",
    ),
    chat: outcomes.filter(
      (outcome) => classifyProtocolProbeOutcome(outcome) === "chat",
    ),
    failed: outcomes.filter(
      (outcome) => classifyProtocolProbeOutcome(outcome) === "failed",
    ),
  };

  const details = [
    groups.both.length > 0
      ? `双协议通过：${summarizeProbeModels(groups.both)}`
      : "",
    groups.responses.length > 0
      ? `仅 Responses 通过：${summarizeProbeModels(groups.responses)}`
      : "",
    groups.chat.length > 0
      ? `仅 Chat 通过：${summarizeProbeModels(groups.chat)}`
      : "",
    groups.failed.length > 0
      ? `双协议失败：${groups.failed
          .slice(0, 3)
          .map(
            (outcome) =>
              `${outcome.model}（Responses=${outcome.responses.detail}; Chat=${outcome.chat.detail}）`,
          )
          .join("；")}${groups.failed.length > 3 ? "；..." : ""}`
      : "",
  ].filter(Boolean);

  return {
    responsesPass: groups.both.length + groups.responses.length,
    chatPass: groups.both.length + groups.chat.length,
    failedCount: groups.failed.length,
    detail: details.length > 0 ? ` 结果明细：${details.join("；")}。` : "",
  };
}

// 根据真实探测结果生成拆分建议：双协议通过默认归入 Responses，只 Chat 通过归入 Chat，双失败不参与建议。
function buildSplitCodexProviderSuggestionForProbeOutcomes({
  providerName,
  outcomes,
}: {
  providerName?: string;
  outcomes: CodexProtocolProbeOutcome[];
}): CodexProviderSplitSuggestion | null {
  const responsesModels = outcomes
    .filter((outcome) => {
      const kind = classifyProtocolProbeOutcome(outcome);
      return kind === "both" || kind === "responses";
    })
    .map((outcome) => outcome.model);
  const chatModels = outcomes
    .filter((outcome) => classifyProtocolProbeOutcome(outcome) === "chat")
    .map((outcome) => outcome.model);

  if (responsesModels.length === 0 || chatModels.length === 0) return null;
  return {
    providerName: providerName?.trim() || "provider",
    responsesModels,
    chatModels,
  };
}

// 为模型行生成紧凑的协议状态 tag，用户不需要回读长摘要也能知道每个模型该走哪种协议。
function getProtocolProbeBadge(outcome?: CodexProtocolProbeOutcome) {
  if (!outcome) return null;
  const kind = classifyProtocolProbeOutcome(outcome);
  if (kind === "both") {
    return {
      label: "双协议",
      title: `Responses=${outcome.responses.detail}; Chat=${outcome.chat.detail}`,
      className:
        "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    };
  }
  if (kind === "responses") {
    return {
      label: "Responses",
      title: `Responses=${outcome.responses.detail}; Chat=${outcome.chat.detail}`,
      className:
        "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    };
  }
  if (kind === "chat") {
    return {
      label: "Chat",
      title: `Responses=${outcome.responses.detail}; Chat=${outcome.chat.detail}`,
      className:
        "border-sky-500/40 bg-sky-500/10 text-sky-700 dark:text-sky-300",
    };
  }
  return {
    label: "不可用",
    title: `Responses=${outcome.responses.detail}; Chat=${outcome.chat.detail}`,
    className: "border-destructive/40 bg-destructive/10 text-destructive",
  };
}

interface CodexFormFieldsProps {
  appId?: AppId;
  providerId?: string;
  // 当前表单里的 provider 名称；自动生成混合协议 route 标签时使用。
  providerName?: string;
  // xAI OAuth 托管预设（Grok 订阅）：隐藏 API Key / 端点输入，挂账号选择区块
  isXaiOauthPreset?: boolean;
  isMaintainedPreset?: boolean;
  isXaiOauthAuthenticated?: boolean;
  selectedXaiAccountId?: string | null;
  onXaiAccountSelect?: (accountId: string | null) => void;
  // API Key
  codexApiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  planAccessKeyId?: string;
  planSecretAccessKey?: string;

  // Base URL
  shouldShowSpeedTest: boolean;
  codexBaseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Codex 菜单映射开关；仅控制是否把目录投射到 /model 菜单，不再控制目录/上下文的编辑和保存。
  takeoverEnabled?: boolean;
  onTakeoverEnabledChange?: (enabled: boolean) => void;
  allowModelMenuProjectionToggle?: boolean;

  codexModel?: string;
  onModelChange?: (model: string) => void;

  // API Format
  // Note: wire_api is always "responses" for Codex; apiFormat controls proxy-layer conversion
  apiFormat: CodexApiFormat;
  onApiFormatChange: (format: CodexApiFormat) => void;
  anthropicAuthField?: ClaudeApiKeyField;
  onAnthropicAuthFieldChange?: (value: ClaudeApiKeyField) => void;
  impersonateClaudeCode?: boolean;
  onImpersonateClaudeCodeChange?: (value: boolean) => void;
  maxOutputTokens?: string;
  onMaxOutputTokensChange?: (value: string) => void;
  codexChatReasoning?: CodexChatReasoning;
  onCodexChatReasoningChange?: (value: CodexChatReasoning) => void;
  promptCacheRouting?: PromptCacheRoutingMode;
  onPromptCacheRoutingChange?: (value: PromptCacheRoutingMode) => void;

  // Model Catalog
  catalogModels?: CodexCatalogModel[];
  // Current maintained preset baseline, used only for explicit override/restore.
  presetCatalogModels?: CodexCatalogModel[];
  onCatalogModelsChange?: (models: CodexCatalogModel[]) => void;
  spawnAgentModels?: string[];
  onSpawnAgentModelsChange?: (models: string[]) => void;
  codexRouting?: CodexRoutingConfig;
  onCodexRoutingChange?: (routing: CodexRoutingConfig) => void;
  onProviderSplitSuggestionChange?: (
    suggestion: CodexProviderSplitSuggestion | null,
  ) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;
}

function capabilityFromReasoningDetection(
  outcome: CodexReasoningDiscoveryOutcome,
): CodexModelReasoningCapability | undefined {
  if (typeof outcome !== "object" || !("found" in outcome)) return undefined;
  const reasoning = outcome.found.reasoning;
  if (!reasoning) return undefined;
  const supportedEfforts = reasoning.supportedEfforts.filter(
    (effort): effort is CodexReasoningEffort =>
      ["none", ...CODEX_REASONING_EFFORT_CHOICES].includes(effort),
  );
  return {
    schemaVersion: 2,
    supportStatus: "confirmed_supported",
    controlKind: supportedEfforts.length > 0 ? "graded" : "boolean",
    supportedEfforts,
    defaultEffort: supportedEfforts.includes(
      reasoning.defaultEffort as CodexReasoningEffort,
    )
      ? (reasoning.defaultEffort as CodexReasoningEffort)
      : supportedEfforts[0],
    disableAllowed: !reasoning.mandatory,
    upstream: { format: "reasoning_object", parameter: "reasoning.effort" },
    outputFormat: "auto",
    source: "user",
    confidence: "authoritative",
  };
}

function unknownReasoningResolution(
  model: string,
): CodexModelReasoningResolution {
  return {
    model,
    capability: null,
    source: "unknown",
    fingerprint: "",
    resolved: {
      supportKind: "unknown",
      confidence: "unverified",
      codexSelectableEfforts: [],
      providerAcceptedEfforts: [],
      providerDefaultEffort: null,
      disableAllowed: false,
      effortMap: {},
    },
    hasDetectionCandidate: false,
    detection: null,
  };
}

type CodexCatalogRow = CodexCatalogModel & { rowId: string };

export interface CodexProviderSplitSuggestion {
  providerName: string;
  responsesModels: string[];
  chatModels: string[];
}

interface PendingCodexProviderSplitRouting {
  identity: string;
  suggestion: CodexProviderSplitSuggestion;
}

function createCatalogRow(seed?: Partial<CodexCatalogModel>): CodexCatalogRow {
  const inputModalities = seed?.inputModalities ?? seed?.input_modalities;
  const supportsImage =
    seed?.supportsImage ?? seed?.supports_image ?? seed?.vision;
  return {
    rowId: crypto.randomUUID(),
    model: seed?.model ?? "",
    upstreamModel: seed?.upstreamModel ?? seed?.upstream_model ?? "",
    displayName: seed?.displayName ?? "",
    contextWindow: seed?.contextWindow ?? "",
    // Carry native-profile overrides verbatim (not user-editable in the row UI,
    // but must survive load->save so the official catalog fidelity is kept).
    ...(seed?.supportsParallelToolCalls !== undefined
      ? { supportsParallelToolCalls: seed.supportsParallelToolCalls }
      : {}),
    ...(inputModalities !== undefined
      ? { inputModalities: [...inputModalities] }
      : {}),
    ...(supportsImage !== undefined ? { supportsImage } : {}),
    ...(seed?.baseInstructions
      ? { baseInstructions: seed.baseInstructions }
      : {}),
    ...(seed?.reasoning ? { reasoning: seed.reasoning } : {}),
  };
}

// 读取 catalog 行的真实上游模型名；为空时回退到可见模型名，兼容旧配置。
function catalogRowUpstreamModel(
  row: Pick<CodexCatalogModel, "model" | "upstreamModel" | "upstream_model">,
): string {
  return (row.upstreamModel ?? row.upstream_model ?? row.model ?? "").trim();
}

// Compares rows (with rowId) to incoming models (without) by data fields only,
// so both sync effects can use the same equality definition. Hidden native-profile
// fields are included so switching between providers with identical visible fields
// but different base_instructions / tools / modalities still rebuilds the rows.
function catalogRowsMatchModels(
  rows: Array<
    Pick<
      CodexCatalogRow,
      | "model"
      | "upstreamModel"
      | "upstream_model"
      | "displayName"
      | "contextWindow"
      | "supportsParallelToolCalls"
      | "baseInstructions"
      | "inputModalities"
      | "supportsImage"
      | "reasoning"
    >
  >,
  models: CodexCatalogModel[],
): boolean {
  if (rows.length !== models.length) return false;
  return rows.every((row, i) => {
    const incoming = models[i];
    return (
      row.model === (incoming.model ?? "") &&
      catalogRowUpstreamModel(row) === catalogRowUpstreamModel(incoming) &&
      (row.displayName ?? "") === (incoming.displayName ?? "") &&
      String(row.contextWindow ?? "") ===
        String(incoming.contextWindow ?? "") &&
      (row.supportsParallelToolCalls ?? null) ===
        (incoming.supportsParallelToolCalls ?? null) &&
      (row.baseInstructions ?? "") === (incoming.baseInstructions ?? "") &&
      JSON.stringify(row.inputModalities ?? []) ===
        JSON.stringify(
          incoming.inputModalities ?? incoming.input_modalities ?? [],
        ) &&
      (row.supportsImage ?? null) ===
        (incoming.supportsImage ??
          incoming.supports_image ??
          incoming.vision ??
          null) &&
      JSON.stringify(row.reasoning ?? null) ===
        JSON.stringify(incoming.reasoning ?? null)
    );
  });
}

interface CodexProviderReadinessIdentityInput {
  providerId?: string;
  providerName?: string;
  baseUrl: string;
  isFullUrl: boolean;
  apiKey: string;
  isXaiOauthPreset?: boolean;
  isXaiOauthAuthenticated?: boolean;
  selectedXaiAccountId?: string | null;
  partnerPromotionKey?: string;
  planAccessKeyId?: string;
  planSecretAccessKey?: string;
  customUserAgent: string;
  localProxyHeadersOverride: string;
  localProxyBodyOverride: string;
  apiFormat: CodexApiFormat;
  anthropicAuthField: ClaudeApiKeyField;
  impersonateClaudeCode: boolean;
  maxOutputTokens: string;
  codexChatReasoning: CodexChatReasoning;
  promptCacheRouting: PromptCacheRoutingMode;
  defaultModel: string;
  catalogModels: CodexCatalogModel[];
}

// 连接验证结果只属于发起请求时的完整 Provider 身份。这里保留精确凭据值用于
// 内存内比较，但不会写入日志、DOM 或持久化；catalog 顺序也属于当前配置身份。
function buildCodexProviderReadinessIdentity({
  providerId,
  providerName,
  baseUrl,
  isFullUrl,
  apiKey,
  isXaiOauthPreset,
  isXaiOauthAuthenticated,
  selectedXaiAccountId,
  partnerPromotionKey,
  planAccessKeyId,
  planSecretAccessKey,
  customUserAgent,
  localProxyHeadersOverride,
  localProxyBodyOverride,
  apiFormat,
  anthropicAuthField,
  impersonateClaudeCode,
  maxOutputTokens,
  codexChatReasoning,
  promptCacheRouting,
  defaultModel,
  catalogModels,
}: CodexProviderReadinessIdentityInput): string {
  return JSON.stringify({
    provider: {
      id: providerId ?? null,
      name: providerName ?? null,
    },
    endpoint: {
      baseUrl: baseUrl.trim(),
      isFullUrl,
    },
    auth: {
      apiKey,
      isXaiOauthPreset: isXaiOauthPreset === true,
      isXaiOauthAuthenticated: isXaiOauthAuthenticated === true,
      selectedXaiAccountId: selectedXaiAccountId ?? null,
      partnerPromotionKey: partnerPromotionKey ?? null,
      planAccessKeyId: planAccessKeyId ?? null,
      planSecretAccessKey: planSecretAccessKey ?? null,
      anthropicAuthField,
    },
    requestOverrides: {
      customUserAgent,
      localProxyHeadersOverride,
      localProxyBodyOverride,
    },
    protocol: {
      apiFormat,
      impersonateClaudeCode,
      maxOutputTokens,
      codexChatReasoning,
      promptCacheRouting,
    },
    defaultModel: defaultModel.trim(),
    catalog: catalogModels.map((model) => ({
      model: model.model.trim(),
      upstreamModel: catalogRowUpstreamModel(model),
      displayName: (model.displayName ?? model.display_name ?? "").trim(),
      contextWindow: String(model.contextWindow ?? model.context_window ?? ""),
      inputModalities: model.inputModalities ?? model.input_modalities ?? null,
      supportsImage:
        model.supportsImage ?? model.supports_image ?? model.vision ?? null,
      textOnly: model.textOnly ?? model.text_only ?? null,
      supportsParallelToolCalls:
        model.supportsParallelToolCalls ??
        model.supports_parallel_tool_calls ??
        null,
      baseInstructions:
        model.baseInstructions ?? model.base_instructions ?? null,
      reasoning: model.reasoning ?? null,
    })),
  });
}

// 将远端 /models 返回合并进 Codex 模型映射；已有行保留用户显示名和已填上下文，
// 同步服务端明确返回的能力字段，并追加新模型。
function mergeFetchedModelsIntoCatalogRows(
  rows: CodexCatalogRow[],
  fetchedModels: FetchedModel[],
  source: {
    providerId?: string;
    providerName?: string;
    baseUrl?: string;
    websiteUrl?: string;
  } = {},
): CodexCatalogRow[] {
  const next = [...rows];
  const rowByFetchedModel = new Map<
    string,
    { row: CodexCatalogRow; index: number }
  >();
  next.forEach((row, index) => {
    const upstreamModel = catalogRowUpstreamModel(row);
    if (upstreamModel) {
      rowByFetchedModel.set(upstreamModel, { row, index });
    }
    const visibleModel = row.model.trim();
    if (visibleModel && !rowByFetchedModel.has(visibleModel)) {
      rowByFetchedModel.set(visibleModel, { row, index });
    }
  });

  for (const fetched of fetchedModels) {
    const model = fetched.id.trim();
    if (!model) continue;
    const contextWindow = resolveFetchedCodexModelContextWindow(fetched, {
      ...source,
      existingModels: rows,
    });
    const contextWindowText = contextWindow ? String(contextWindow) : undefined;
    const capabilityPatch: Partial<CodexCatalogModel> = {
      ...(Array.isArray(fetched.inputModalities)
        ? { inputModalities: [...fetched.inputModalities] }
        : {}),
      ...(typeof fetched.supportsImage === "boolean"
        ? { supportsImage: fetched.supportsImage }
        : {}),
    };
    const existing = rowByFetchedModel.get(model);
    if (existing) {
      const updatedRow = {
        ...existing.row,
        ...(!existing.row.contextWindow && contextWindowText
          ? { contextWindow: contextWindowText }
          : {}),
        ...capabilityPatch,
      };
      next[existing.index] = updatedRow;
      rowByFetchedModel.set(model, { row: updatedRow, index: existing.index });
      continue;
    }
    const row = createCatalogRow({
      model,
      upstreamModel: model,
      displayName: model,
      ...(contextWindowText ? { contextWindow: contextWindowText } : {}),
      ...capabilityPatch,
    });
    rowByFetchedModel.set(model, { row, index: next.length });
    next.push(row);
  }

  return next;
}

// 判断模型名是否大概率属于支持 Responses 的 OpenAI/GPT 系列。
// 这里故意只做保守启发式，避免把 qwen/deepseek 等中转模型误归到 Responses route。
export function isLikelyCodexResponsesModel(model: string): boolean {
  const normalized = model.trim().toLowerCase();
  if (!normalized) return false;
  const lastSegment =
    normalized.split(/[/:]/).filter(Boolean).pop() ?? normalized;
  return /^(gpt-|gpt\d|o[1345](?:-|$)|chatgpt-|codex-)/.test(lastSegment);
}

// 将 /models 结果按“原生 Responses 候选”和“需要 Chat 转换候选”分组。
export function splitFetchedModelsByLikelyCodexProtocol(
  models: FetchedModel[],
): { responses: string[]; chat: string[] } {
  const responses: string[] = [];
  const chat: string[] = [];
  const seen = new Set<string>();

  for (const fetched of models) {
    const id = fetched.id.trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    if (isLikelyCodexResponsesModel(id)) {
      responses.push(id);
    } else {
      chat.push(id);
    }
  }

  return { responses, chat };
}

// 为同一个中转 provider 生成“拆成两个 provider”的建议；GPT-like 走 Responses，非 GPT-like 走 Chat 转换。
export function buildSplitCodexProviderSuggestionForFetchedModels({
  providerName,
  models,
}: {
  providerName?: string;
  models: FetchedModel[];
}): CodexProviderSplitSuggestion | null {
  const split = splitFetchedModelsByLikelyCodexProtocol(models);
  if (split.responses.length === 0 || split.chat.length === 0) return null;

  const labelBase = providerName?.trim() || "provider";
  return {
    providerName: labelBase,
    responsesModels: split.responses,
    chatModels: split.chat,
  };
}

export function CodexFormFields({
  appId = "codex",
  providerId,
  providerName,
  isXaiOauthPreset,
  isMaintainedPreset = false,
  isXaiOauthAuthenticated,
  selectedXaiAccountId,
  onXaiAccountSelect,
  codexApiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  planAccessKeyId,
  planSecretAccessKey,
  shouldShowSpeedTest,
  codexBaseUrl,
  onBaseUrlChange,
  isFullUrl,
  onFullUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  takeoverEnabled = false,
  onTakeoverEnabledChange = () => undefined,
  allowModelMenuProjectionToggle = true,
  codexModel = "",
  onModelChange,
  apiFormat,
  onApiFormatChange,
  anthropicAuthField = "ANTHROPIC_AUTH_TOKEN",
  onAnthropicAuthFieldChange = () => undefined,
  impersonateClaudeCode = false,
  onImpersonateClaudeCodeChange = () => undefined,
  maxOutputTokens = "",
  onMaxOutputTokensChange = () => undefined,
  codexChatReasoning = {},
  onCodexChatReasoningChange,
  promptCacheRouting = "auto",
  onPromptCacheRoutingChange = () => undefined,
  catalogModels = [],
  presetCatalogModels = [],
  onCatalogModelsChange,
  onProviderSplitSuggestionChange,
  speedTestEndpoints,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [reasoningResolutions, setReasoningResolutions] = useState<
    Record<string, CodexModelReasoningResolution>
  >({});
  const [redetectingReasoningModel, setRedetectingReasoningModel] = useState<
    string | null
  >(null);
  const reasoningResolutionRequestRef = useRef(0);
  const [isProtocolProbeConfirmOpen, setIsProtocolProbeConfirmOpen] =
    useState(false);
  const [isProbingProtocol, setIsProbingProtocol] = useState(false);
  const [protocolProbeSummary, setProtocolProbeSummary] = useState("");
  const [protocolProbeTone, setProtocolProbeTone] = useState<
    "muted" | "success" | "warning" | "error"
  >("muted");
  const [protocolProbeOutcomesByModel, setProtocolProbeOutcomesByModel] =
    useState<Record<string, CodexProtocolProbeOutcome>>({});
  const [protocolProbeIdentity, setProtocolProbeIdentity] = useState<
    string | null
  >(null);
  const protocolProbeIdentityRef = useRef<string | null>(null);
  const protocolProbeSeqRef = useRef(0);
  const [shouldHighlightFetchModels, setShouldHighlightFetchModels] =
    useState(false);
  const [pendingSplitRoutingState, setPendingSplitRoutingState] =
    useState<PendingCodexProviderSplitRouting | null>(null);
  // takeoverEnabled 现在只表示“Codex 菜单映射”开关；模型目录和上下文元数据可独立编辑。
  // isChatFormat 仅在选了 Chat Completions 上游格式时为真（思考能力是 Chat 专属）。
  // 拉取请求序号：请求身份（Base URL / 完整地址开关 / API Key / 自定义 UA）
  // 一变即自增，清空旧列表并作废在途响应——/models 结果可能按 Key 的模型
  // 授权返回，换号后残留旧列表会误导选择
  const fetchModelsSeqRef = useRef(0);

  useEffect(() => {
    fetchModelsSeqRef.current += 1;
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
  }, [
    codexBaseUrl,
    isFullUrl,
    codexApiKey,
    customUserAgent,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
  ]);
  // 思考能力随 Chat 格式显示（仅 Chat Completions 转换路径用得上）；模型映射常驻
  //（填了才生成 catalog）。两者都已与「路由接管」概念解耦。
  const isChatFormat = apiFormat === "openai_chat";
  const isAnthropicFormat = apiFormat === "anthropic";
  const canEditCatalog = Boolean(onCatalogModelsChange);

  // 普通 Provider 表单只消费并原样回传历史 codexRouting；可见编辑入口统一收口到
  // CodexRouterWorkspacePage，避免与完整 MultiRouter 工作台形成两套配置界面。
  const canEditReasoning = Boolean(onCodexChatReasoningChange);
  const supportsThinking =
    codexChatReasoning.supportsThinking === true ||
    codexChatReasoning.supportsEffort === true;
  const supportsEffort = codexChatReasoning.supportsEffort === true;
  // 高级区在有任何可见配置时自动展开；只做折叠到展开，避免编辑旧 provider 时藏起关键状态。
  const hasRequestOverrides = Boolean(
    localProxyHeadersOverride.trim() || localProxyBodyOverride.trim(),
  );
  const hasAnyAdvancedValue =
    !!customUserAgent ||
    hasRequestOverrides ||
    (!isMaintainedPreset &&
      (isAnthropicFormat || supportsThinking || supportsEffort)) ||
    promptCacheRouting !== "auto" ||
    !!maxOutputTokens;
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );

  // 预设/编辑加载填充高级值后自动展开（仅从折叠→展开，不会自动折叠）；
  // xAI OAuth 托管预设的高级值都是预设自带的，无需展示，保持折叠
  useEffect(() => {
    if (isXaiOauthPreset) {
      return;
    }
    if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue, isXaiOauthPreset]);

  const [catalogRows, setCatalogRows] = useState<CodexCatalogRow[]>(() =>
    catalogModels.map((m) => createCatalogRow(m)),
  );

  const reasoningSettingsConfig = useMemo(
    () => ({ modelCatalog: { models: catalogRows } }),
    [catalogRows],
  );
  const reasoningDetectionProvider = useMemo<Provider>(
    () => ({
      id: providerId ?? "codex-draft",
      name: providerName?.trim() || "Codex provider",
      settingsConfig: {
        base_url: codexBaseUrl.trim(),
      },
      category,
    }),
    [providerId, providerName, codexBaseUrl, category],
  );

  useEffect(() => {
    const models = catalogRows
      .map((row) => catalogRowUpstreamModel(row) || row.model.trim())
      .filter(Boolean);
    if (models.length === 0) {
      setReasoningResolutions({});
      return;
    }
    const requestId = ++reasoningResolutionRequestRef.current;
    let cancelled = false;
    void Promise.all(
      models.map(async (model) => {
        try {
          return [
            model,
            await codexSubagentV2Api.resolveModelReasoningCapability(
              reasoningSettingsConfig,
              providerId ?? "codex-draft",
              model,
            ),
          ] as const;
        } catch (error) {
          console.warn("[CodexFormFields] reasoning resolution failed", {
            model,
            error,
          });
          return [model, unknownReasoningResolution(model)] as const;
        }
      }),
    ).then((results) => {
      if (cancelled || requestId !== reasoningResolutionRequestRef.current) {
        return;
      }
      const next: Record<string, CodexModelReasoningResolution> = {};
      for (const result of results) {
        if (result) next[result[0]] = result[1];
      }
      setReasoningResolutions(next);
    });
    return () => {
      cancelled = true;
    };
  }, [catalogRows, providerId, reasoningSettingsConfig]);
  const catalogRowsRef = useRef<CodexCatalogRow[]>(catalogRows);
  const modelMappingSectionRef = useRef<HTMLDivElement | null>(null);
  const fetchModelsButtonRef = useRef<HTMLButtonElement | null>(null);
  // 记录上次发送给父组件的数据，避免重复触发
  const lastSentModelsRef = useRef<CodexCatalogModel[]>(catalogModels);
  const catalogPropKeyRef = useRef(JSON.stringify(catalogModels));
  const skipCatalogEchoRef = useRef(false);

  // 保留最新的模型映射行给异步刷新回调用，避免点击“获取模型列表”时合并到旧闭包里的 catalogRows。
  useEffect(() => {
    catalogRowsRef.current = catalogRows;
  }, [catalogRows]);

  const buildReadinessIdentityFor = useCallback(
    (nextApiFormat: CodexApiFormat, nextCatalogModels: CodexCatalogModel[]) =>
      buildCodexProviderReadinessIdentity({
        providerId,
        providerName,
        baseUrl: codexBaseUrl,
        isFullUrl,
        apiKey: codexApiKey,
        isXaiOauthPreset,
        isXaiOauthAuthenticated,
        selectedXaiAccountId,
        partnerPromotionKey,
        planAccessKeyId,
        planSecretAccessKey,
        customUserAgent,
        localProxyHeadersOverride,
        localProxyBodyOverride,
        apiFormat: nextApiFormat,
        anthropicAuthField,
        impersonateClaudeCode,
        maxOutputTokens,
        codexChatReasoning,
        promptCacheRouting,
        defaultModel: codexModel,
        catalogModels: nextCatalogModels,
      }),
    [
      anthropicAuthField,
      codexApiKey,
      codexBaseUrl,
      codexChatReasoning,
      codexModel,
      customUserAgent,
      impersonateClaudeCode,
      isFullUrl,
      isXaiOauthAuthenticated,
      isXaiOauthPreset,
      localProxyBodyOverride,
      localProxyHeadersOverride,
      maxOutputTokens,
      partnerPromotionKey,
      planAccessKeyId,
      planSecretAccessKey,
      promptCacheRouting,
      providerId,
      providerName,
      selectedXaiAccountId,
    ],
  );
  const readinessIdentity = useMemo(
    () => buildReadinessIdentityFor(apiFormat, catalogRows),
    [apiFormat, buildReadinessIdentityFor, catalogRows],
  );
  const readinessIdentityRef = useRef(readinessIdentity);
  readinessIdentityRef.current = readinessIdentity;
  const bindProtocolProbeIdentity = useCallback((identity: string) => {
    protocolProbeIdentityRef.current = identity;
    setProtocolProbeIdentity(identity);
  }, []);
  const bindPendingSplitRouting = useCallback(
    (suggestion: CodexProviderSplitSuggestion, identity: string) => {
      setPendingSplitRoutingState({ suggestion, identity });
    },
    [],
  );

  // 任一身份输入变化都立即使旧结果失效并取消其 UI ownership。异步请求本身可以
  // 自然结束，但 sequence/identity guard 会阻止旧进度与最终结果回写到新配置。
  useEffect(() => {
    if (protocolProbeIdentityRef.current !== readinessIdentity) {
      protocolProbeSeqRef.current += 1;
      protocolProbeIdentityRef.current = null;
      setProtocolProbeIdentity(null);
      setIsProbingProtocol(false);
      setIsProtocolProbeConfirmOpen(false);
      setProtocolProbeTone("muted");
      setProtocolProbeSummary("");
      setProtocolProbeOutcomesByModel({});
    }
    setPendingSplitRoutingState((current) =>
      current === null || current.identity === readinessIdentity
        ? current
        : null,
    );
  }, [readinessIdentity]);

  const isProtocolProbeStateCurrent =
    protocolProbeIdentity === readinessIdentity;
  const pendingSplitRouting =
    pendingSplitRoutingState?.identity === readinessIdentity
      ? pendingSplitRoutingState.suggestion
      : null;

  const revealModelCatalogFetchAction = useCallback(() => {
    bindProtocolProbeIdentity(readinessIdentity);
    setProtocolProbeTone("warning");
    setProtocolProbeSummary(
      "请先在“模型与兼容性”同步模型，或在高级设置中手动添加至少一个模型后再验证。",
    );
    setShouldHighlightFetchModels(true);
    window.setTimeout(() => {
      modelMappingSectionRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
      fetchModelsButtonRef.current?.focus({ preventScroll: true });
    }, 0);
    window.setTimeout(() => setShouldHighlightFetchModels(false), 3000);
  }, [bindProtocolProbeIdentity, readinessIdentity]);

  // 父 → 子：仅当 prop 数据真的变化（预设切换 / 编辑加载）时才重建 rowId；
  // 同 shape 时保留现有 rowId，避免编辑过程中焦点丢失。
  useEffect(() => {
    const incomingCatalogKey = JSON.stringify(catalogModels);
    const isExternalCatalogChange =
      incomingCatalogKey !== catalogPropKeyRef.current;
    catalogPropKeyRef.current = incomingCatalogKey;

    setCatalogRows((current) => {
      if (catalogRowsMatchModels(current, catalogModels)) return current;
      if (isExternalCatalogChange) {
        skipCatalogEchoRef.current = true;
      }
      return catalogModels.map((m) => createCatalogRow(m));
    });
    // 同步更新 ref，避免父组件传入新数据时子→父 effect 误判为本地修改
    lastSentModelsRef.current = catalogModels;
  }, [catalogModels]);

  // 子 → 父：rowId 是视图层概念，不应进入持久化数据；剥离后再回传。
  // 注意：依赖数组不包含 catalogModels，避免父→子更新触发子→父回调形成循环。
  useEffect(() => {
    if (!onCatalogModelsChange) return;
    // 外部 catalog 同步进来时先等本地 rowId 重建完成，再允许子组件回写。
    if (skipCatalogEchoRef.current) {
      if (!catalogRowsMatchModels(catalogRows, catalogModels)) return;
      skipCatalogEchoRef.current = false;
    }
    const next: CodexCatalogModel[] = catalogRows.map(
      ({ rowId: _rowId, ...rest }) => rest,
    );
    // 只有当数据真的变化时才通知父组件
    if (catalogRowsMatchModels(catalogRows, lastSentModelsRef.current)) return;
    lastSentModelsRef.current = next;
    onCatalogModelsChange(next);
  }, [catalogRows, catalogModels, onCatalogModelsChange]);

  const handleReasoningThinkingChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked,
        supportsEffort: checked ? codexChatReasoning.supportsEffort : false,
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleReasoningEffortChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked ? true : codexChatReasoning.supportsThinking,
        supportsEffort: checked,
        effortParam: checked
          ? (codexChatReasoning.effortParam ?? "reasoning_effort")
          : "none",
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleFetchModels = useCallback(() => {
    // xAI OAuth 托管预设不使用表单里的 Base URL 与 API Key。
    if (isXaiOauthPreset) {
      if (!isXaiOauthAuthenticated) {
        toast.error(
          t("xaiOauth.loginRequired", {
            defaultValue: "请先登录 xAI 账号",
          }),
        );
        return;
      }
      const seq = ++fetchModelsSeqRef.current;
      setIsFetchingModels(true);
      fetchXaiOauthModels(selectedXaiAccountId ?? null)
        .then((models) => {
          if (seq !== fetchModelsSeqRef.current) return;
          setFetchedModels(models);
          if (models.length === 0) {
            toast.info(t("providerForm.fetchModelsEmpty"));
          } else {
            toast.success(
              t("providerForm.fetchModelsSuccess", { count: models.length }),
            );
          }
        })
        .catch((err) => {
          if (seq !== fetchModelsSeqRef.current) return;
          console.warn("[XaiOAuth] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (seq === fetchModelsSeqRef.current) setIsFetchingModels(false);
        });
      return;
    }

    const planFetchSource = {
      baseUrl: codexBaseUrl,
      partnerPromotionKey,
      providerName,
      apiKey: codexApiKey,
      accessKeyId: planAccessKeyId,
      secretAccessKey: planSecretAccessKey,
    };
    const planModelListAction = codexPlanModelListAction(planFetchSource);
    const isCatalogOnlyPlan = isCodexCatalogOnlyPlanModelFetch(planFetchSource);
    if (isCatalogOnlyPlan) {
      const hasModelCatalog = catalogRowsRef.current.some((row) =>
        row.model.trim(),
      );
      const message = codexCatalogOnlyPlanModelFetchMessage(
        hasModelCatalog,
        planFetchSource,
      );
      if (hasModelCatalog) {
        toast.info(message);
      } else {
        toast.warning(message);
      }
      return;
    }

    if (!codexBaseUrl || (!codexApiKey && !planModelListAction)) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    const seq = ++fetchModelsSeqRef.current;
    setIsFetchingModels(true);
    fetchModelsForConfig(
      codexBaseUrl,
      codexApiKey,
      isFullUrl,
      undefined,
      customUserAgent,
      planModelListAction
        ? {
            action: planModelListAction,
            accessKeyId: planAccessKeyId ?? "",
            secretAccessKey: planSecretAccessKey ?? "",
          }
        : undefined,
    )
      .then((models) => {
        if (seq !== fetchModelsSeqRef.current) return;
        setFetchedModels(models);
        let splitCatalogRows = catalogRowsRef.current;
        if (onCatalogModelsChange && models.length > 0) {
          const mergedRows = mergeFetchedModelsIntoCatalogRows(
            catalogRowsRef.current,
            models,
            {
              providerId,
              providerName,
              baseUrl: codexBaseUrl,
              websiteUrl,
            },
          );
          catalogRowsRef.current = mergedRows;
          splitCatalogRows = mergedRows;
          setCatalogRows(mergedRows);
        }
        const shouldAutoSplitRouting =
          models.length > 0 && Boolean(onProviderSplitSuggestionChange);
        if (shouldAutoSplitRouting) {
          const splitRouting =
            buildSplitCodexProviderSuggestionForFetchedModels({
              providerName,
              models,
            });
          if (splitRouting) {
            bindPendingSplitRouting(
              splitRouting,
              buildReadinessIdentityFor(apiFormat, splitCatalogRows),
            );
          }
        }
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        if (seq !== fetchModelsSeqRef.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (seq === fetchModelsSeqRef.current) setIsFetchingModels(false);
      });
  }, [
    apiFormat,
    bindPendingSplitRouting,
    buildReadinessIdentityFor,
    codexBaseUrl,
    codexApiKey,
    isFullUrl,
    customUserAgent,
    providerId,
    providerName,
    partnerPromotionKey,
    planAccessKeyId,
    planSecretAccessKey,
    websiteUrl,
    onCatalogModelsChange,
    onProviderSplitSuggestionChange,
    isXaiOauthPreset,
    isXaiOauthAuthenticated,
    selectedXaiAccountId,
    t,
  ]);

  const handleProtocolProbe = useCallback(async () => {
    if (!codexBaseUrl || !codexApiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    const models = Array.from(
      new Set(
        [
          ...catalogRowsRef.current.map((row) => catalogRowUpstreamModel(row)),
          ...fetchedModels.map((model) => model.id.trim()),
        ].filter(Boolean),
      ),
    );
    if (models.length === 0) {
      setIsProtocolProbeConfirmOpen(false);
      toast.warning("请先点击“获取模型列表”，或手动添加至少一个模型。");
      revealModelCatalogFetchAction();
      return;
    }

    const probeIdentity = readinessIdentity;
    const probeSeq = ++protocolProbeSeqRef.current;
    const ownsCurrentIdentity = () =>
      probeSeq === protocolProbeSeqRef.current &&
      readinessIdentityRef.current === probeIdentity;
    bindProtocolProbeIdentity(probeIdentity);
    setIsProtocolProbeConfirmOpen(false);
    setIsProbingProtocol(true);
    setProtocolProbeTone("muted");
    setProtocolProbeOutcomesByModel({});
    setProtocolProbeSummary(
      `正在并发测试 ${models.length} 个模型的 Chat / Responses 基础连通性，最多同时测试 ${CODEX_PROTOCOL_PROBE_MODEL_CONCURRENCY} 个模型...`,
    );
    try {
      let completedCount = 0;
      const outcomes = await runCodexProtocolProbePool(
        models,
        CODEX_PROTOCOL_PROBE_MODEL_CONCURRENCY,
        async (model) => {
          const [responses, chat] = await Promise.all([
            probeCodexResponsesForConfig(
              codexBaseUrl,
              codexApiKey,
              model,
              isFullUrl,
              customUserAgent,
            ),
            probeCodexChatForConfig(
              codexBaseUrl,
              codexApiKey,
              model,
              isFullUrl,
              customUserAgent,
            ),
          ]);
          completedCount += 1;
          const outcome = { model, responses, chat };
          if (!ownsCurrentIdentity()) return outcome;
          setProtocolProbeSummary(
            `正在并发测试 ${completedCount}/${models.length}：刚完成 ${model}。失败会在这里显示。`,
          );
          setProtocolProbeOutcomesByModel((current) => ({
            ...current,
            [model]: outcome,
          }));
          return outcome;
        },
      );
      if (!ownsCurrentIdentity()) return;

      const { responsesPass, chatPass, failedCount, detail } =
        summarizeCodexProtocolProbeOutcomes(outcomes);
      setProtocolProbeOutcomesByModel(
        Object.fromEntries(outcomes.map((outcome) => [outcome.model, outcome])),
      );
      const splitSuggestion = buildSplitCodexProviderSuggestionForProbeOutcomes(
        {
          providerName,
          outcomes,
        },
      );
      const canApplySplitSuggestion = Boolean(
        splitSuggestion && onProviderSplitSuggestionChange,
      );
      if (splitSuggestion && onProviderSplitSuggestionChange) {
        bindPendingSplitRouting(splitSuggestion, probeIdentity);
        onProviderSplitSuggestionChange(null);
      }

      if (responsesPass > 0 && chatPass > 0) {
        const summary = `Responses 和 Chat 的基础请求都有模型可用，保留当前上游格式；Responses 通常是 Codex 原生优先选择，但你可以继续使用 Chat Completions。Responses 通过 ${responsesPass}/${models.length}，Chat 通过 ${chatPass}/${models.length}。${
          canApplySplitSuggestion
            ? "检测到真实协议结果混合，建议下一步拆成 Responses / Chat 两个 provider。"
            : ""
        }通过不等于完整 Codex 功能验证。${detail}`;
        const tone = failedCount > 0 ? "warning" : "success";
        bindProtocolProbeIdentity(probeIdentity);
        setProtocolProbeTone(tone);
        setProtocolProbeSummary(summary);
        if (tone === "warning") {
          toast.warning(summary, { closeButton: true });
        } else {
          toast.success(summary, { closeButton: true });
        }
        return;
      }

      if (responsesPass > 0) {
        const resultIdentity = buildReadinessIdentityFor(
          "openai_responses",
          catalogRowsRef.current,
        );
        bindProtocolProbeIdentity(resultIdentity);
        onApiFormatChange("openai_responses");
        const summary = `只有 Responses 基础请求可用，已切换为 Responses。Responses 通过 ${responsesPass}/${models.length}。通过不等于完整 Codex 功能验证。${detail}`;
        const tone = failedCount > 0 ? "warning" : "success";
        setProtocolProbeTone(tone);
        setProtocolProbeSummary(summary);
        if (tone === "warning") {
          toast.warning(summary, { closeButton: true });
        } else {
          toast.success(summary, { closeButton: true });
        }
        return;
      }
      if (chatPass > 0) {
        const resultIdentity = buildReadinessIdentityFor(
          "openai_chat",
          catalogRowsRef.current,
        );
        bindProtocolProbeIdentity(resultIdentity);
        onApiFormatChange("openai_chat");
        const summary = `Responses 不通但 Chat 可用，已切换为 Chat Completions。Chat 通过 ${chatPass}/${models.length}。${
          canApplySplitSuggestion
            ? "检测到真实协议结果混合，建议下一步拆成 Responses / Chat 两个 provider。"
            : ""
        }${detail}`;
        setProtocolProbeTone("warning");
        setProtocolProbeSummary(summary);
        toast.warning(summary, { closeButton: true });
        return;
      }

      const summary = `Responses 和 Chat Completions 都不通，请检查 API Key、Base URL、模型权限、额度、网络或上游状态。${detail}`;
      bindProtocolProbeIdentity(probeIdentity);
      setProtocolProbeTone("error");
      setProtocolProbeSummary(summary);
      toast.error(summary, { closeButton: true });
    } catch (error) {
      if (!ownsCurrentIdentity()) return;
      const summary = `协议测试中断：${error instanceof Error ? error.message : String(error)}`;
      bindProtocolProbeIdentity(probeIdentity);
      setProtocolProbeTone("error");
      setProtocolProbeSummary(summary);
      toast.error(summary, { closeButton: true });
    } finally {
      if (probeSeq === protocolProbeSeqRef.current) {
        setIsProbingProtocol(false);
      }
    }
  }, [
    bindPendingSplitRouting,
    bindProtocolProbeIdentity,
    buildReadinessIdentityFor,
    codexBaseUrl,
    codexApiKey,
    customUserAgent,
    fetchedModels,
    isFullUrl,
    onApiFormatChange,
    onProviderSplitSuggestionChange,
    providerName,
    readinessIdentity,
    revealModelCatalogFetchAction,
    t,
  ]);

  const handleAddCatalogRow = useCallback(() => {
    if (!onCatalogModelsChange) return;
    setCatalogRows((current) => [...current, createCatalogRow()]);
  }, [onCatalogModelsChange]);

  const handleUpdateCatalogRow = useCallback(
    (index: number, patch: Partial<CodexCatalogModel>) => {
      setCatalogRows((current) =>
        current.map((row, i) => {
          if (i !== index) return row;
          const next = { ...row, ...patch };
          if (
            patch.model !== undefined &&
            patch.upstreamModel === undefined &&
            patch.upstream_model === undefined
          ) {
            const previousVisibleModel = row.model.trim();
            const previousUpstreamModel = catalogRowUpstreamModel(row);
            if (
              previousVisibleModel &&
              (!previousUpstreamModel ||
                previousUpstreamModel === previousVisibleModel)
            ) {
              next.upstreamModel = previousVisibleModel;
            }
          }
          return next;
        }),
      );
    },
    [],
  );

  const handleUpdateCatalogReasoningJson = useCallback(
    (index: number, value: string) => {
      const trimmed = value.trim();
      if (!trimmed) {
        handleUpdateCatalogRow(index, { reasoning: undefined });
        return;
      }
      try {
        const reasoning = JSON.parse(trimmed) as CodexCatalogModel["reasoning"];
        if (!reasoning) {
          throw new Error("reasoning capability must be an object");
        }
        validateCodexReasoningCapabilityDraft(reasoning);
        handleUpdateCatalogRow(index, {
          reasoning: { ...reasoning, source: "user" },
        });
      } catch (error) {
        toast.error(
          `推理能力 JSON 无效，未修改草稿：${error instanceof Error ? error.message : String(error)}`,
        );
      }
    },
    [handleUpdateCatalogRow],
  );

  const presetReasoningByModel = useMemo(() => {
    const entries: Array<
      readonly [string, NonNullable<CodexCatalogModel["reasoning"]>]
    > = [];
    for (const model of presetCatalogModels) {
      if (!model.reasoning) continue;
      const visible = model.model.trim();
      const upstream = catalogRowUpstreamModel(model);
      if (visible) entries.push([visible, model.reasoning]);
      if (upstream && upstream !== visible) {
        entries.push([upstream, model.reasoning]);
      }
    }
    return new Map(entries);
  }, [presetCatalogModels]);

  const handleSelectFetchedCatalogModel = useCallback(
    (
      index: number,
      modelId: string,
      currentVisibleModel?: string,
      currentDisplayName?: string,
    ) => {
      const fetched = fetchedModels.find((model) => model.id === modelId);
      const contextWindow = fetched
        ? resolveFetchedCodexModelContextWindow(fetched, {
            providerId,
            baseUrl: codexBaseUrl,
            websiteUrl,
            existingModels: catalogRows,
          })
        : undefined;

      handleUpdateCatalogRow(index, {
        model: currentVisibleModel?.trim() ? currentVisibleModel : modelId,
        upstreamModel: modelId,
        displayName: currentDisplayName?.trim() ? currentDisplayName : modelId,
        ...(contextWindow ? { contextWindow: String(contextWindow) } : {}),
        ...(Array.isArray(fetched?.inputModalities)
          ? { inputModalities: [...fetched.inputModalities] }
          : {}),
        ...(typeof fetched?.supportsImage === "boolean"
          ? { supportsImage: fetched.supportsImage }
          : {}),
      });
    },
    [
      catalogRows,
      codexBaseUrl,
      fetchedModels,
      handleUpdateCatalogRow,
      providerId,
      websiteUrl,
    ],
  );

  const handleRemoveCatalogRow = useCallback((index: number) => {
    setCatalogRows((current) => current.filter((_, i) => i !== index));
  }, []);

  // 移动模型目录行本身；单 provider 表格里的顺序代表保留下来的模型展示/路由顺序，不再混用子 Agent 候选顺序。
  const handleMoveCatalogRow = useCallback(
    (index: number, direction: -1 | 1) => {
      setCatalogRows((current) => {
        const targetIndex = index + direction;
        if (index < 0 || targetIndex < 0 || targetIndex >= current.length) {
          return current;
        }
        const next = [...current];
        [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
        return next;
      });
    },
    [],
  );

  const handleConfirmSplitRouting = useCallback(() => {
    if (!pendingSplitRouting || !onProviderSplitSuggestionChange) return;
    onTakeoverEnabledChange(true);
    onProviderSplitSuggestionChange(pendingSplitRouting);
    setPendingSplitRoutingState(null);
    toast.info(
      `保存时将生成 ${pendingSplitRouting.providerName}-responses / ${pendingSplitRouting.providerName}-chat 两个 provider。`,
    );
  }, [
    onTakeoverEnabledChange,
    onProviderSplitSuggestionChange,
    pendingSplitRouting,
  ]);

  const handleCancelSplitRouting = useCallback(() => {
    setPendingSplitRoutingState(null);
    onProviderSplitSuggestionChange?.(null);
  }, [onProviderSplitSuggestionChange]);

  const splitRoutingProviderName = providerName?.trim() || "provider";
  const pendingResponsesModels = pendingSplitRouting?.responsesModels ?? [];
  const pendingChatModels = pendingSplitRouting?.chatModels ?? [];

  const renderCatalogActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <>
      <Dialog
        open={isProtocolProbeConfirmOpen}
        onOpenChange={setIsProtocolProbeConfirmOpen}
      >
        <DialogContent className="max-w-lg" zIndex="top">
          <DialogHeader>
            <DialogTitle>确认测试 Chat / Responses</DialogTitle>
            <DialogDescription className="space-y-2 text-left">
              <span className="block">
                这个测试会帮助判断当前 provider 应该选择 Responses 还是 Chat
                Completions。它会对当前模型目录里的模型发送真实请求，可能产生少量额度或流量消耗，也可能触发限流。
              </span>
              <span className="block">
                如果还没有模型目录，请先到上方“模型目录与上下文”点击“获取模型列表”，或手动添加至少一个模型。
              </span>
              <span className="block">
                每个模型会分别测试对应的 Responses 和 Chat Completions
                endpoint，输出上限为 1024。都不通时通常不是协议问题，而是 API
                Key、Base URL、模型权限、额度、网络或上游故障。
              </span>
              <span className="block">
                注意：Responses 通过只证明最小非流式请求能返回成功，不等于完整
                Codex
                功能验证；真实会话里的流式输出、工具调用、长上下文和限流稳定性仍要继续观察。
              </span>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setIsProtocolProbeConfirmOpen(false)}
            >
              取消
            </Button>
            <Button type="button" onClick={handleProtocolProbe}>
              确认测试
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* xAI OAuth 认证（Grok 订阅托管账号） */}
      {isXaiOauthPreset && (
        <XaiOAuthSection
          selectedAccountId={selectedXaiAccountId}
          onAccountSelect={onXaiAccountSelect}
        />
      )}

      {/* Codex API Key 输入框（托管 OAuth 预设无需 Key） */}
      {!isXaiOauthPreset && (
        <ApiKeySection
          id="codexApiKey"
          label="API Key"
          value={codexApiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
          placeholder={{
            official: t("providerForm.codexOfficialNoApiKey", {
              defaultValue: "官方供应商无需 API Key",
            }),
            thirdParty: t("providerForm.codexApiKeyAutoFill", {
              defaultValue: "输入 API Key，将自动填充到配置",
            }),
          }}
        />
      )}

      {/* Codex Base URL 输入框（托管 OAuth 端点由 adapter 硬定向，不展示） */}
      {shouldShowSpeedTest && !isXaiOauthPreset && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          hint={t("providerForm.codexApiHint")}
          showFullUrlToggle
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {category !== "official" && onModelChange && (
        <div className="space-y-1.5">
          <FormLabel htmlFor="codexDefaultModel">
            {t("codexConfig.defaultModelLabel", { defaultValue: "默认模型" })}
          </FormLabel>
          <div className="flex gap-1">
            <Input
              id="codexDefaultModel"
              value={codexModel}
              onChange={(event) => onModelChange(event.target.value)}
              placeholder={t("codexConfig.defaultModelPlaceholder", {
                defaultValue: "例如: gpt-5.6",
              })}
            />
            {fetchedModels.length > 0 && (
              <ModelDropdown
                models={fetchedModels}
                onSelect={(id) => onModelChange(id)}
              />
            )}
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t("codexConfig.defaultModelHint", {
              defaultValue:
                "Codex 默认请求的模型，随时可改，无需等待预设更新。",
            })}
          </p>
        </div>
      )}

      <Dialog
        open={Boolean(pendingSplitRouting)}
        onOpenChange={(open) => {
          if (!open) handleCancelSplitRouting();
        }}
      >
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>检测到混合协议模型</DialogTitle>
            <DialogDescription>
              当前中转同时返回了 GPT-like 模型和非 GPT-like 模型。建议保存时拆成
              Responses 与 Chat 两个
              provider，避免把两种协议混在同一个配置里导致后续分不清。
              确认后不会立即保存；点击新增时才会创建两个 provider。
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3 px-6 pb-2">
            <div className="rounded-md border border-emerald-500/40 bg-emerald-500/10 p-3">
              <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                <span>{`${splitRoutingProviderName}-responses`}</span>
                <span className="rounded bg-background/70 px-1.5 py-0.5 text-[11px] text-muted-foreground">
                  OpenAI Responses
                </span>
                <span className="rounded bg-background/70 px-1.5 py-0.5 text-[11px] text-muted-foreground">
                  单独 provider
                </span>
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                匹配模型：
                {pendingResponsesModels.join(", ") || "-"}
              </p>
            </div>
            <div className="rounded-md border border-sky-500/40 bg-sky-500/10 p-3">
              <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                <span>{`${splitRoutingProviderName}-chat`}</span>
                <span className="rounded bg-background/70 px-1.5 py-0.5 text-[11px] text-muted-foreground">
                  OpenAI Chat Completions
                </span>
                <span className="rounded bg-background/70 px-1.5 py-0.5 text-[11px] text-muted-foreground">
                  单独 provider
                </span>
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                匹配模型：{pendingChatModels.join(", ") || "-"}
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={handleCancelSplitRouting}
            >
              暂不拆分
            </Button>
            <Button type="button" onClick={handleConfirmSplitRouting}>
              确认生成两个 provider
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {category !== "official" && canEditCatalog && (
        <CodexProviderReadinessSection
          models={catalogRows}
          defaultModel={codexModel}
          apiFormat={apiFormat}
          isMaintainedPreset={isMaintainedPreset}
          isSyncingModels={isFetchingModels}
          isValidatingConnection={
            isProbingProtocol && isProtocolProbeStateCurrent
          }
          validationSummary={
            isProtocolProbeStateCurrent ? protocolProbeSummary : ""
          }
          validationTone={
            isProtocolProbeStateCurrent ? protocolProbeTone : "muted"
          }
          highlightSync={shouldHighlightFetchModels}
          syncButtonRef={fetchModelsButtonRef}
          sectionRef={modelMappingSectionRef}
          onSyncModels={handleFetchModels}
          onValidateConnection={() => {
            bindProtocolProbeIdentity(readinessIdentity);
            setProtocolProbeTone("muted");
            setProtocolProbeSummary(
              "已打开验证确认框；如果没有看到弹窗，请按 Esc 后重试。",
            );
            setIsProtocolProbeConfirmOpen(true);
          }}
        />
      )}

      {/* 高级选项只保留手动协议与模型能力覆盖、请求覆盖和目录明细。 */}
      {category !== "official" && (
        <Collapsible
          open={advancedExpanded}
          onOpenChange={setAdvancedExpanded}
          className="rounded-lg border border-border-default p-4"
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {advancedExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("providerForm.advancedOptionsToggle", {
                defaultValue: "高级选项",
              })}
            </Button>
          </CollapsibleTrigger>
          {!advancedExpanded && (
            <p className="mt-1 ml-1 text-xs text-muted-foreground">
              {t("codexConfig.advancedSectionHint", {
                defaultValue:
                  "包含模型目录、协议检测、上游格式、Codex 菜单映射、思考能力与自定义 User-Agent；Chat Completions 供应商需走本地代理转换。",
              })}
            </p>
          )}
          <CollapsibleContent className="space-y-3 pt-3">
            {/* 上游格式与协议探测沿用 shouldShowSpeedTest 门控，
                cloud_provider 保持不可切换；xAI OAuth 托管预设格式固定为 Responses。 */}
            {shouldShowSpeedTest && !isXaiOauthPreset && (
              <div className="space-y-3">
                {/* 上游格式 —— 顶层独立选择，与路由开关解耦 */}
                <div className="space-y-1.5">
                  <FormLabel htmlFor="codex-upstream-format">
                    {t("codexConfig.upstreamFormatLabel", {
                      defaultValue: "上游格式",
                    })}
                  </FormLabel>
                  <Select
                    value={apiFormat}
                    onValueChange={(value) =>
                      onApiFormatChange(value as CodexApiFormat)
                    }
                  >
                    <SelectTrigger
                      id="codex-upstream-format"
                      className="w-full"
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="openai_chat">
                        {t("codexConfig.upstreamFormatChat", {
                          defaultValue: "Chat Completions（需本地代理转换）",
                        })}
                      </SelectItem>
                      <SelectItem value="openai_responses">
                        {t("codexConfig.upstreamFormatResponses", {
                          defaultValue: "Responses（原生）",
                        })}
                      </SelectItem>
                      <SelectItem value="anthropic">
                        {t("codexConfig.upstreamFormatAnthropic", {
                          defaultValue: "Anthropic Messages（需开启路由）",
                        })}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.upstreamFormatHint", {
                      defaultValue:
                        "供应商原生是 Responses API 就选 Responses；使用 Chat Completions 协议就选 Chat；只提供 Anthropic Messages 时选择 Anthropic。后两者均需本地代理转换。",
                    })}
                  </p>
                  {isAnthropicFormat && (
                    <div className="space-y-3 rounded-md border border-border-default p-3">
                      <div className="space-y-1.5">
                        <FormLabel>
                          {t("codexConfig.anthropicAuthFieldLabel")}
                        </FormLabel>
                        <Select
                          value={anthropicAuthField}
                          onValueChange={(value) =>
                            onAnthropicAuthFieldChange(
                              value as ClaudeApiKeyField,
                            )
                          }
                        >
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                              {t("codexConfig.anthropicAuthFieldAuthToken")}
                            </SelectItem>
                            <SelectItem value="ANTHROPIC_API_KEY">
                              {t("codexConfig.anthropicAuthFieldApiKey")}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      <label className="flex items-center justify-between gap-3 text-sm">
                        {t("codexConfig.impersonateClaudeCodeLabel")}
                        <Switch
                          checked={impersonateClaudeCode}
                          onCheckedChange={onImpersonateClaudeCodeChange}
                        />
                      </label>
                      <div className="space-y-1.5">
                        <FormLabel htmlFor="codexMaxOutputTokens">
                          {t("codexConfig.maxOutputTokensLabel")}
                        </FormLabel>
                        <Input
                          id="codexMaxOutputTokens"
                          inputMode="numeric"
                          value={maxOutputTokens}
                          onChange={(event) =>
                            onMaxOutputTokensChange(
                              event.target.value.replace(/\D/g, ""),
                            )
                          }
                          placeholder="8192"
                        />
                      </div>
                    </div>
                  )}
                  {(isChatFormat || isAnthropicFormat) && (
                    <div className="space-y-1.5">
                      <FormLabel>
                        {t("codexConfig.promptCacheRoutingLabel")}
                      </FormLabel>
                      <Select
                        value={promptCacheRouting}
                        onValueChange={(value) =>
                          onPromptCacheRoutingChange(
                            value as PromptCacheRoutingMode,
                          )
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="auto">
                            {t("codexConfig.promptCacheRoutingAuto")}
                          </SelectItem>
                          <SelectItem value="enabled">
                            {t("codexConfig.promptCacheRoutingEnabled")}
                          </SelectItem>
                          <SelectItem value="disabled">
                            {t("codexConfig.promptCacheRoutingDisabled")}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                  )}
                  <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs leading-relaxed text-amber-900 dark:text-amber-200">
                    上游格式通常由维护预设或主流程的连接验证确定。只有自动识别不正确时才在这里手动覆盖；验证会发送真实模型请求，可能产生少量额度或流量消耗。
                  </div>
                </div>
              </div>
            )}

            {takeoverEnabled && isChatFormat && canEditReasoning && (
              <div
                className={cn(
                  "space-y-3",
                  shouldShowSpeedTest && "border-t border-border-default pt-3",
                )}
              >
                <div className="space-y-1">
                  <FormLabel>
                    {t("codexConfig.reasoningGroupTitle", {
                      defaultValue: "思考能力",
                    })}
                  </FormLabel>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.reasoningSectionHint", {
                      defaultValue:
                        "预设供应商已自动配置；自定义供应商会按名称/地址自动推断。仅当自动识别不准时才需手动覆盖。",
                    })}
                  </p>
                </div>

                <div className="flex items-center justify-between gap-4">
                  <div className="space-y-1">
                    <FormLabel>
                      {t("codexConfig.reasoningModeToggle", {
                        defaultValue: "支持思考模式",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.reasoningModeHint", {
                        defaultValue:
                          "上游 Chat Completions 接口支持开启或关闭 thinking 时启用。Kimi、GLM、Qwen 等通常属于这一类。",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={supportsThinking}
                    onCheckedChange={handleReasoningThinkingChange}
                    aria-label={t("codexConfig.reasoningModeToggle", {
                      defaultValue: "支持思考模式",
                    })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 border-t border-border-default pt-3">
                  <div className="space-y-1">
                    <FormLabel>
                      {t("codexConfig.reasoningEffortToggle", {
                        defaultValue: "支持思考等级",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.reasoningEffortHint", {
                        defaultValue:
                          "上游支持 low/high/max 等思考深度控制时启用。启用后会自动启用思考模式，并把 Codex 的 reasoning.effort 转成上游 Chat 参数。",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={supportsEffort}
                    onCheckedChange={handleReasoningEffortChange}
                    aria-label={t("codexConfig.reasoningEffortToggle", {
                      defaultValue: "支持思考等级",
                    })}
                  />
                </div>
              </div>
            )}

            {/* 模型映射 / 模型目录 —— 与「路由接管」解耦，常驻显示（可编辑即渲染）。
                填了才生成 catalog：Chat 模式生成兼容路由、原生 Responses 生成
                model-catalogs.json；留空则不生成。排在自定义 UA 之前。 */}
            {canEditCatalog && (
              <div
                className={cn(
                  "space-y-4",
                  (shouldShowSpeedTest ||
                    (takeoverEnabled && isChatFormat && canEditReasoning)) &&
                    "border-t border-border-default pt-3",
                )}
              >
                <div className="space-y-1">
                  <div className="flex items-center justify-between gap-3">
                    <FormLabel>
                      {t("codexConfig.modelMappingTitle", {
                        defaultValue: "模型目录明细",
                      })}
                    </FormLabel>
                    {renderCatalogActionButtons(
                      handleAddCatalogRow,
                      t("codexConfig.addCatalogModel", {
                        defaultValue: "添加模型",
                      }),
                    )}
                  </div>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("codexConfig.modelMappingHint", {
                      defaultValue:
                        "这里保存候选模型、真实上游模型和上下文窗口。开启“在 Codex /model 菜单中显示”后，菜单显示名和上游模型名才会参与 Codex 菜单映射；关闭时仍会作为目录元数据保存。",
                    })}
                  </p>
                </div>

                {catalogRows.length > 0 && (
                  <div className="space-y-2">
                    {/* 列头：md+ 显示 */}
                    <div className="hidden grid-cols-[88px_1fr_1fr_1fr_132px_76px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                      <span>
                        {t("codexConfig.keepCatalogModelColumn", {
                          defaultValue: "保留",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnDisplay", {
                          defaultValue: "菜单显示名",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnModel", {
                          defaultValue: "候选模型名",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnUpstreamModel", {
                          defaultValue: "上游模型名",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogColumnContext", {
                          defaultValue: "上下文窗口",
                        })}
                      </span>
                      <span>
                        {t("codexConfig.catalogOrderColumn", {
                          defaultValue: "顺序",
                        })}
                      </span>
                      <span />
                    </div>

                    {catalogRows.map((row, index) => {
                      const probeModel =
                        catalogRowUpstreamModel(row) || row.model.trim();
                      const reasoningResolution =
                        reasoningResolutions[probeModel];
                      const probeBadge = getProtocolProbeBadge(
                        protocolProbeOutcomesByModel[probeModel],
                      );
                      const presetReasoning =
                        presetReasoningByModel.get(row.model.trim()) ??
                        presetReasoningByModel.get(
                          catalogRowUpstreamModel(row),
                        );
                      const isBuiltinReasoning =
                        row.reasoning?.source === "builtin";
                      const isUserPresetOverride =
                        Boolean(presetReasoning) &&
                        row.reasoning?.source === "user";
                      const reasoningSourceMode: CodexReasoningCapabilitySourceMode =
                        isBuiltinReasoning
                          ? "builtin"
                          : row.reasoning
                            ? "manual"
                            : "automatic";

                      return (
                        <div
                          key={row.rowId}
                          className="grid grid-cols-1 gap-2 rounded-md border border-transparent p-1 md:grid-cols-[88px_1fr_1fr_1fr_132px_76px_36px]"
                        >
                          <label className="flex h-9 items-center gap-2 text-xs text-muted-foreground">
                            <input
                              type="checkbox"
                              className="h-4 w-4 rounded border-border-default"
                              checked
                              onChange={(event) => {
                                if (!event.target.checked) {
                                  handleRemoveCatalogRow(index);
                                }
                              }}
                              aria-label={t("codexConfig.keepCatalogModel", {
                                model: row.model || row.displayName || "",
                                defaultValue: `保留 ${row.model || row.displayName || "这个模型"}`,
                              })}
                            />
                            <span className="md:hidden">
                              {t("codexConfig.keepCatalogModelColumn", {
                                defaultValue: "保留",
                              })}
                            </span>
                          </label>
                          <Input
                            value={row.displayName ?? ""}
                            onChange={(event) =>
                              handleUpdateCatalogRow(index, {
                                displayName: event.target.value,
                              })
                            }
                            placeholder={t(
                              "codexConfig.catalogDisplayNamePlaceholder",
                              {
                                defaultValue: "例如: DeepSeek V4 Flash",
                              },
                            )}
                            aria-label={t("codexConfig.catalogColumnDisplay", {
                              defaultValue: "菜单显示名",
                            })}
                          />
                          <Input
                            value={row.model}
                            onChange={(event) =>
                              handleUpdateCatalogRow(index, {
                                model: event.target.value,
                              })
                            }
                            placeholder={t(
                              "codexConfig.catalogModelPlaceholder",
                              {
                                defaultValue: "例如: gpt-5.5-thirdparty",
                              },
                            )}
                            aria-label={t("codexConfig.catalogColumnModel", {
                              defaultValue: "候选模型名",
                            })}
                          />
                          <div className="space-y-1">
                            <div className="flex gap-1">
                              <Input
                                value={
                                  row.upstreamModel ?? row.upstream_model ?? ""
                                }
                                onChange={(event) =>
                                  handleUpdateCatalogRow(index, {
                                    upstreamModel: event.target.value,
                                  })
                                }
                                placeholder={t(
                                  "codexConfig.catalogUpstreamModelPlaceholder",
                                  {
                                    defaultValue: "留空则使用候选模型名",
                                  },
                                )}
                                aria-label={t(
                                  "codexConfig.catalogColumnUpstreamModel",
                                  {
                                    defaultValue: "上游模型名",
                                  },
                                )}
                                className="flex-1"
                              />
                              {fetchedModels.length > 0 && (
                                <ModelDropdown
                                  models={fetchedModels}
                                  onSelect={(id) =>
                                    handleSelectFetchedCatalogModel(
                                      index,
                                      id,
                                      row.model,
                                      row.displayName,
                                    )
                                  }
                                />
                              )}
                            </div>
                            {probeBadge && (
                              <span
                                className={cn(
                                  "inline-flex w-fit items-center rounded border px-1.5 py-0.5 text-[11px] font-medium",
                                  probeBadge.className,
                                )}
                                title={probeBadge.title}
                              >
                                {probeBadge.label}
                              </span>
                            )}
                          </div>
                          <Input
                            type="number"
                            min={1}
                            inputMode="numeric"
                            value={row.contextWindow ?? ""}
                            onChange={(event) =>
                              handleUpdateCatalogRow(index, {
                                contextWindow: event.target.value.replace(
                                  /[^\d]/g,
                                  "",
                                ),
                              })
                            }
                            placeholder={t(
                              "codexConfig.contextWindowPlaceholder",
                              {
                                defaultValue: "例如: 128000",
                              },
                            )}
                            aria-label={t("codexConfig.catalogColumnContext", {
                              defaultValue: "上下文窗口",
                            })}
                          />
                          <div className="flex h-9 items-center gap-1">
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 text-muted-foreground"
                              disabled={index <= 0}
                              onClick={() => handleMoveCatalogRow(index, -1)}
                              title={t("common.moveUp", {
                                defaultValue: "上移",
                              })}
                            >
                              <ArrowUp className="h-4 w-4" />
                            </Button>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 text-muted-foreground"
                              disabled={index >= catalogRows.length - 1}
                              onClick={() => handleMoveCatalogRow(index, 1)}
                              title={t("common.moveDown", {
                                defaultValue: "下移",
                              })}
                            >
                              <ArrowDown className="h-4 w-4" />
                            </Button>
                          </div>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-9 w-9 text-muted-foreground hover:text-destructive"
                            onClick={() => handleRemoveCatalogRow(index)}
                            title={t("common.delete", { defaultValue: "删除" })}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                          <details className="md:col-span-7">
                            <summary className="cursor-pointer text-xs text-muted-foreground">
                              Codex 推理能力
                              {isBuiltinReasoning
                                ? "（内置预设，只读）"
                                : isUserPresetOverride
                                  ? "（已偏离内置预设）"
                                  : row.reasoning
                                    ? "（用户覆盖）"
                                    : "（未声明，使用保守模式）"}
                            </summary>
                            <div className="mt-2 space-y-3 rounded-md border p-3 text-xs">
                              {reasoningResolution ? (
                                <CodexModelReasoningCard
                                  resolution={reasoningResolution}
                                  hasBuiltinPreset={Boolean(presetReasoning)}
                                  redetecting={
                                    redetectingReasoningModel === probeModel
                                  }
                                  onRedetect={async () => {
                                    setRedetectingReasoningModel(probeModel);
                                    try {
                                      const outcome =
                                        await codexSubagentV2Api.triggerModelReasoningDetection(
                                          reasoningDetectionProvider,
                                          probeModel,
                                        );
                                      if (
                                        typeof outcome === "object" &&
                                        "found" in outcome
                                      ) {
                                        const next =
                                          await codexSubagentV2Api.resolveModelReasoningCapability(
                                            reasoningSettingsConfig,
                                            providerId ?? "codex-draft",
                                            probeModel,
                                          );
                                        setReasoningResolutions((current) => ({
                                          ...current,
                                          [probeModel]: next,
                                        }));
                                        toast.success(
                                          "已更新模型推理能力检测结果",
                                        );
                                      } else {
                                        toast.info(
                                          "未获得可采纳的模型推理能力声明，继续使用服务端默认。",
                                        );
                                      }
                                    } catch (error) {
                                      console.error(
                                        "[CodexFormFields] reasoning detection failed",
                                        error,
                                      );
                                      toast.error("模型推理能力检测失败");
                                    } finally {
                                      setRedetectingReasoningModel(null);
                                    }
                                  }}
                                  onAdoptDetection={() => {
                                    const detected =
                                      reasoningResolution.detection
                                        ? capabilityFromReasoningDetection({
                                            found:
                                              reasoningResolution.detection,
                                          })
                                        : undefined;
                                    if (!detected) {
                                      toast.info(
                                        "当前检测结果没有可采纳的推理档位声明。",
                                      );
                                      return;
                                    }
                                    handleUpdateCatalogRow(index, {
                                      reasoning: detected,
                                    });
                                    toast.success("已采用检测到的推理能力");
                                  }}
                                  onManualDeclare={() =>
                                    handleUpdateCatalogRow(index, {
                                      reasoning:
                                        applyCodexReasoningCapabilitySource(
                                          "manual",
                                          row.reasoning,
                                          presetReasoning,
                                        ),
                                    })
                                  }
                                  onRestoreBuiltin={() =>
                                    handleUpdateCatalogRow(index, {
                                      reasoning:
                                        applyCodexReasoningCapabilitySource(
                                          "builtin",
                                          row.reasoning,
                                          presetReasoning,
                                        ),
                                    })
                                  }
                                />
                              ) : (
                                <p className="text-muted-foreground">
                                  正在读取该模型的统一推理能力解析结果…
                                </p>
                              )}
                              <label className="grid gap-1">
                                <span>能力来源</span>
                                <select
                                  className="rounded-md border bg-background px-3 py-2"
                                  value={reasoningSourceMode}
                                  aria-label={`${row.model || "模型"}推理能力来源`}
                                  onChange={(event) =>
                                    handleUpdateCatalogRow(index, {
                                      reasoning:
                                        applyCodexReasoningCapabilitySource(
                                          event.target
                                            .value as CodexReasoningCapabilitySourceMode,
                                          row.reasoning,
                                          presetReasoning,
                                        ),
                                    })
                                  }
                                >
                                  <option value="automatic">自动发现</option>
                                  <option
                                    value="builtin"
                                    disabled={!presetReasoning}
                                  >
                                    使用 CCSM 受维护声明
                                  </option>
                                  <option value="manual">手动声明</option>
                                </select>
                              </label>
                              <div className="flex flex-wrap gap-2">
                                {isBuiltinReasoning && row.reasoning ? (
                                  <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    onClick={() =>
                                      handleUpdateCatalogRow(index, {
                                        reasoning:
                                          applyCodexReasoningCapabilitySource(
                                            "manual",
                                            row.reasoning,
                                            presetReasoning,
                                          ),
                                      })
                                    }
                                  >
                                    创建高级覆盖
                                  </Button>
                                ) : null}
                                {isUserPresetOverride && presetReasoning ? (
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() =>
                                      handleUpdateCatalogRow(index, {
                                        reasoning:
                                          applyCodexReasoningCapabilitySource(
                                            "builtin",
                                            row.reasoning,
                                            presetReasoning,
                                          ),
                                      })
                                    }
                                  >
                                    恢复内置默认
                                  </Button>
                                ) : null}
                              </div>
                              <p className="text-muted-foreground">
                                自动发现只读取可验证的模型能力；证据不足时保持未声明，不会套用
                                GPT 通用档位。
                              </p>

                              {row.reasoning ? (
                                <div className="grid gap-3 md:grid-cols-2">
                                  <fieldset className="space-y-2 rounded-md border p-2">
                                    <legend className="px-1">
                                      Provider 原生档位
                                    </legend>
                                    <div className="flex flex-wrap gap-2">
                                      {CODEX_REASONING_EFFORT_CHOICES.map(
                                        (effort) => (
                                          <label
                                            key={effort}
                                            className="flex items-center gap-1"
                                          >
                                            <input
                                              type="checkbox"
                                              checked={row.reasoning!.supportedEfforts.includes(
                                                effort,
                                              )}
                                              disabled={isBuiltinReasoning}
                                              onChange={(event) => {
                                                const checked =
                                                  event.target.checked;
                                                const supportedEfforts = checked
                                                  ? [
                                                      ...row.reasoning!
                                                        .supportedEfforts,
                                                      effort,
                                                    ]
                                                  : row.reasoning!.supportedEfforts.filter(
                                                      (item) => item !== effort,
                                                    );
                                                const defaultEffort =
                                                  supportedEfforts.includes(
                                                    row.reasoning!
                                                      .defaultEffort as CodexReasoningEffort,
                                                  )
                                                    ? row.reasoning!
                                                        .defaultEffort
                                                    : supportedEfforts[0];
                                                // 取消勾选时同步清理 effortMap 中指向被移除档位的孤儿映射，
                                                // 否则保存后后端 validate（target 必须在 supportedEfforts）
                                                // 会拒绝整份声明并被静默清空（v27/v28 回归）。
                                                const nextEffortMap: Record<
                                                  string,
                                                  CodexReasoningEffort
                                                > = {
                                                  ...(row.reasoning!.upstream
                                                    .effortMap ?? {}),
                                                };
                                                if (!checked) {
                                                  for (const [
                                                    source,
                                                    target,
                                                  ] of Object.entries(
                                                    nextEffortMap,
                                                  )) {
                                                    if (target === effort) {
                                                      delete nextEffortMap[
                                                        source
                                                      ];
                                                    }
                                                  }
                                                }
                                                handleUpdateCatalogRow(index, {
                                                  reasoning: {
                                                    ...row.reasoning!,
                                                    supportedEfforts,
                                                    defaultEffort,
                                                    upstream: {
                                                      ...row.reasoning!
                                                        .upstream,
                                                      effortMap: nextEffortMap,
                                                    },
                                                    source: "user",
                                                  },
                                                });
                                              }}
                                            />
                                            {effort}
                                          </label>
                                        ),
                                      )}
                                    </div>
                                  </fieldset>
                                  <label className="grid gap-1">
                                    <span>Provider 默认档位</span>
                                    <select
                                      className="rounded-md border bg-background px-3 py-2"
                                      value={row.reasoning.defaultEffort ?? ""}
                                      disabled={isBuiltinReasoning}
                                      onChange={(event) =>
                                        handleUpdateCatalogRow(index, {
                                          reasoning: {
                                            ...row.reasoning!,
                                            defaultEffort: event.target
                                              .value as CodexReasoningEffort,
                                            source: "user",
                                          },
                                        })
                                      }
                                    >
                                      <option value="">未声明</option>
                                      {row.reasoning.supportedEfforts.map(
                                        (effort) => (
                                          <option key={effort} value={effort}>
                                            {effort}
                                          </option>
                                        ),
                                      )}
                                    </select>
                                  </label>
                                  <label className="flex items-center gap-2">
                                    <input
                                      type="checkbox"
                                      checked={row.reasoning.disableAllowed}
                                      disabled={isBuiltinReasoning}
                                      onChange={(event) =>
                                        handleUpdateCatalogRow(index, {
                                          reasoning: {
                                            ...row.reasoning!,
                                            disableAllowed:
                                              event.target.checked,
                                            source: "user",
                                          },
                                        })
                                      }
                                    />
                                    Provider 支持关闭推理
                                  </label>
                                  <label className="grid gap-1">
                                    <span>上游参数</span>
                                    <Input
                                      value={row.reasoning.upstream.parameter}
                                      readOnly={isBuiltinReasoning}
                                      onChange={(event) =>
                                        handleUpdateCatalogRow(index, {
                                          reasoning: {
                                            ...row.reasoning!,
                                            upstream: {
                                              ...row.reasoning!.upstream,
                                              parameter: event.target
                                                .value as CodexModelReasoningCapability["upstream"]["parameter"],
                                            },
                                            source: "user",
                                          },
                                        })
                                      }
                                    />
                                  </label>
                                  <div className="md:col-span-2 space-y-2">
                                    <span>Codex → Provider 映射</span>
                                    <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                                      {CODEX_REASONING_EFFORT_CHOICES.map(
                                        (effort) => (
                                          <label
                                            key={effort}
                                            className="grid grid-cols-[1fr_auto_1fr] items-center gap-1"
                                          >
                                            <span>{effort}</span>
                                            <span>→</span>
                                            <select
                                              className="rounded border bg-background px-2 py-1"
                                              value={
                                                row.reasoning!.upstream
                                                  .effortMap?.[effort] ?? ""
                                              }
                                              disabled={isBuiltinReasoning}
                                              onChange={(event) =>
                                                handleUpdateCatalogRow(index, {
                                                  reasoning: {
                                                    ...row.reasoning!,
                                                    upstream: {
                                                      ...row.reasoning!
                                                        .upstream,
                                                      effortMap: {
                                                        ...row.reasoning!
                                                          .upstream.effortMap,
                                                        [effort]: event.target
                                                          .value as CodexReasoningEffort,
                                                      },
                                                    },
                                                    source: "user",
                                                  },
                                                })
                                              }
                                            >
                                              <option value="">未映射</option>
                                              {row.reasoning!.supportedEfforts.map(
                                                (target) => (
                                                  <option
                                                    key={target}
                                                    value={target}
                                                  >
                                                    {target}
                                                  </option>
                                                ),
                                              )}
                                            </select>
                                          </label>
                                        ),
                                      )}
                                    </div>
                                  </div>
                                </div>
                              ) : null}

                              <details>
                                <summary className="cursor-pointer text-muted-foreground">
                                  专家 JSON
                                </summary>
                                <Textarea
                                  key={`${row.rowId}:${JSON.stringify(row.reasoning)}`}
                                  className="mt-2 min-h-28 font-mono text-xs"
                                  defaultValue={
                                    row.reasoning
                                      ? JSON.stringify(row.reasoning, null, 2)
                                      : ""
                                  }
                                  onBlur={(event) => {
                                    if (!isBuiltinReasoning) {
                                      handleUpdateCatalogReasoningJson(
                                        index,
                                        event.target.value,
                                      );
                                    }
                                  }}
                                  readOnly={isBuiltinReasoning}
                                  aria-label={`${row.model || "模型"}推理能力 JSON`}
                                />
                              </details>
                            </div>
                          </details>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            <div
              className={cn(
                "space-y-3",
                (shouldShowSpeedTest ||
                  (isChatFormat && canEditReasoning) ||
                  canEditCatalog) &&
                  "border-t border-border-default pt-3",
              )}
            >
              <CustomUserAgentField
                id="codex-custom-user-agent"
                value={customUserAgent}
                onChange={onCustomUserAgentChange}
              />
              <div className="border-t border-border-default pt-3">
                <LocalProxyRequestOverridesField
                  headersJson={localProxyHeadersOverride}
                  bodyJson={localProxyBodyOverride}
                  onHeadersJsonChange={onLocalProxyHeadersOverrideChange}
                  onBodyJsonChange={onLocalProxyBodyOverrideChange}
                />
              </div>
            </div>

            {/* 仅自定义 Provider 可以退出 CCSwitchMulti 的目录管理；维护预设始终投影正确目录。 */}
            {appId === "codex" &&
              !isXaiOauthPreset &&
              allowModelMenuProjectionToggle && (
                <div className="flex items-center justify-between gap-4 rounded-md border border-blue-200 bg-blue-50/60 p-3 dark:border-blue-900/60 dark:bg-blue-950/20">
                  <div className="space-y-1.5">
                    <FormLabel>
                      {t("codexConfig.localRoutingToggle", {
                        defaultValue: "在 Codex /model 菜单中显示",
                      })}
                    </FormLabel>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("codexConfig.localRoutingDescription", {
                        defaultValue:
                          "开启后，CCSwitchMulti 会生成 Codex 启动时加载的模型目录，让这里配置的模型、显示名、上下文窗口和推理档位出现在 /model 中，并把显示名映射到真实上游模型。它不控制 Provider、代理或 MultiRouter 是否可用；仅当你要使用自己维护的 model_catalog_json 时关闭。",
                      })}
                    </p>
                    <p
                      className={cn(
                        "text-xs leading-relaxed",
                        takeoverEnabled
                          ? "text-muted-foreground"
                          : "text-amber-700 dark:text-amber-300",
                      )}
                    >
                      {takeoverEnabled
                        ? t("codexConfig.localRoutingOnHint", {
                            defaultValue:
                              "推荐保持开启。模型目录会在下次 Codex 启动时加载。",
                          })
                        : t("codexConfig.localRoutingOffHint", {
                            defaultValue:
                              "当前已关闭：Provider 和直接指定的真实模型仍可使用，目录数据也会继续保存，但 Codex /model 不再获得这些模型、别名、上下文窗口和推理档位。仅当你要使用自己维护的 model_catalog_json 时关闭。",
                          })}
                    </p>
                  </div>
                  <Switch
                    checked={takeoverEnabled}
                    onCheckedChange={onTakeoverEnabledChange}
                    aria-label={t("codexConfig.localRoutingToggle", {
                      defaultValue: "在 Codex /model 菜单中显示",
                    })}
                  />
                </div>
              )}
          </CollapsibleContent>
        </Collapsible>
      )}

      {/* 端点测速弹窗 - Codex */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId={appId}
          providerId={providerId}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}
    </>
  );
}

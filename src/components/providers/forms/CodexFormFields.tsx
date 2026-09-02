import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
  AlertTriangle,
  Plus,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  fetchModelsForConfig,
  fetchXaiOauthModels,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  preflightCodexProviderProtocolCompatibility,
  type CodexProtocolProbeProgressEvent,
  type CodexProviderProtocolPreflightOutcome,
} from "@/lib/api/protocol-compatibility";
import { CodexProtocolProbeProgressDialog } from "./CodexProtocolProbeProgressDialog";
import { CodexProtocolAdvancedSettings } from "./CodexProtocolAdvancedSettings";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import { CodexProviderReadinessSection } from "./CodexProviderReadinessSection";
import { CodexModelReasoningCard } from "./CodexModelReasoningCard";
import { CodexModelReasoningEditor } from "./CodexModelReasoningEditor";
import { CodexModelReasoningSummary } from "./CodexModelReasoningSummary";
import { cn } from "@/lib/utils";
import { buildCodexProtocolProbeProviderDraft } from "@/lib/codexProtocolSettings";
import {
  migrateCodexProtocolOverrideKey,
  normalizeCodexPublicModelKey,
  setCodexProtocolOverride,
  type CodexProtocolChoiceInput,
} from "@/lib/protocol-lab/codex-overrides";
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
  CodexProtocolMode,
  CodexProtocolOverride,
  CodexReasoningProjection,
  CodexToolSchemaDialect,
  CodexHistoryReplay,
  Provider,
  ProviderCategory,
} from "@/types";
import type { CodexProviderEditorSnapshot } from "@/lib/api/protocol-compatibility";
import type { AppId } from "@/lib/api";
import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";
import type {
  CodexModelReasoningResolution,
  CodexReasoningDiscoveryOutcome,
} from "@/types/codexSubagentV2";

interface EndpointCandidate {
  url: string;
}

const PROVIDER_REASONING_EFFORT_CHOICES: CodexReasoningEffort[] = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];

const ignoreProtocolProbeReceiptIds = (_receiptIds: string[]) => undefined;

export type CodexReasoningCapabilitySourceMode =
  | "automatic"
  | "builtin"
  | "manual";

export function applyCodexReasoningCapabilitySource(
  mode: CodexReasoningCapabilitySourceMode,
  current?: CodexModelReasoningCapability,
  maintained?: CodexModelReasoningCapability,
  discovered?: CodexModelReasoningCapability,
): CodexModelReasoningCapability | undefined {
  if (mode === "automatic") return undefined;
  if (mode === "builtin") {
    return maintained ? structuredClone(maintained) : undefined;
  }
  const seed = current ?? maintained ?? discovered;
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
  const allowed = new Set<CodexReasoningEffort>(
    PROVIDER_REASONING_EFFORT_CHOICES,
  );
  if (
    !Array.isArray(capability.supportedEfforts) ||
    capability.supportedEfforts.some((effort) => !allowed.has(effort))
  ) {
    throw new Error("支持的推理强度包含未知档位，或包含仅供 Codex 使用的档位");
  }
  // schema v2 用 supportStatus；legacy 数据用 supported。至少声明其一，
  // 同时存在时不得矛盾。
  if (
    capability.supportStatus === undefined &&
    typeof capability.supported !== "boolean"
  ) {
    throw new Error("必须声明该模型是否支持推理");
  }
  if (
    capability.supportStatus !== undefined &&
    typeof capability.supported === "boolean" &&
    (capability.supportStatus === "confirmed_supported") !==
      capability.supported
  ) {
    throw new Error("新旧推理支持状态相互冲突");
  }
  if (
    capability.defaultEffort !== undefined &&
    !capability.supportedEfforts.includes(capability.defaultEffort)
  ) {
    throw new Error("默认推理强度不是供应商支持的档位");
  }
  if (typeof capability.disableAllowed !== "boolean") {
    throw new Error("是否允许关闭推理必须是布尔值");
  }
  if (
    !capability.upstream ||
    typeof capability.upstream.parameter !== "string" ||
    !capability.upstream.parameter.trim()
  ) {
    throw new Error("上游推理参数名不能为空");
  }
  for (const target of Object.values(capability.upstream.effortMap ?? {})) {
    if (target && !capability.supportedEfforts.includes(target)) {
      throw new Error(`映射目标 ${target} 不是供应商支持的推理强度`);
    }
  }
  const requiresEffortMap =
    (capability.supportStatus !== undefined
      ? capability.supportStatus === "confirmed_supported"
      : capability.supported === true) &&
    (capability.controlKind ??
      (capability.supportedEfforts.length > 0 ? "graded" : "unknown")) ===
      "graded" &&
    capability.upstream.format !== "none" &&
    capability.upstream.format !== "boolean";
  if (requiresEffortMap) {
    const missing = capability.supportedEfforts.filter(
      (effort) => !capability.upstream.effortMap?.[effort],
    );
    if (missing.length > 0) {
      throw new Error(`推理强度映射缺少 ${missing.join(", ")} 档`);
    }
  }
  if (capability.codexUltraOrchestration?.enabled) {
    const ultraTarget = capability.upstream.effortMap?.max;
    if (!ultraTarget || !capability.supportedEfforts.includes(ultraTarget)) {
      throw new Error("解锁 Ultra 档需要有效的 max 到供应商推理强度映射");
    }
  }
}

interface CodexFormFieldsProps {
  codexProviderEditorSnapshot?: CodexProviderEditorSnapshot | null;
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
  protocolMode?: CodexProtocolMode;
  onProtocolModeChange?: (value: CodexProtocolMode) => void;
  protocolOverrides?: Record<string, CodexProtocolOverride>;
  onProtocolOverridesChange?: (
    value: Record<string, CodexProtocolOverride>,
  ) => void;
  reasoningProjection?: CodexReasoningProjection;
  onReasoningProjectionChange?: (value: CodexReasoningProjection) => void;
  toolSchemaDialect?: CodexToolSchemaDialect;
  onToolSchemaDialectChange?: (value: CodexToolSchemaDialect) => void;
  historyReplay?: CodexHistoryReplay;
  onHistoryReplayChange?: (value: CodexHistoryReplay) => void;
  /** Current deep-probe leases for the outer Provider save coordinator. */
  onProtocolProbeReceiptIdsChange?: (receiptIds: string[]) => void;

  // Model Catalog
  catalogModels?: CodexCatalogModel[];
  // Current maintained preset baseline, used only for explicit override/restore.
  presetCatalogModels?: CodexCatalogModel[];
  onCatalogModelsChange?: (models: CodexCatalogModel[]) => void;
  spawnAgentModels?: string[];
  onSpawnAgentModelsChange?: (models: string[]) => void;
  codexRouting?: CodexRoutingConfig;
  onCodexRoutingChange?: (routing: CodexRoutingConfig) => void;

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
      ["none", ...PROVIDER_REASONING_EFFORT_CHOICES].includes(effort),
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
    ...(seed?.textOnly !== undefined ? { textOnly: seed.textOnly } : {}),
    ...(seed?.baseInstructions
      ? { baseInstructions: seed.baseInstructions }
      : {}),
    ...(seed?.reasoning ? { reasoning: seed.reasoning } : {}),
    ...(seed?.codexUltra ? { codexUltra: seed.codexUltra } : {}),
    ...(seed?.apiFormat ? { apiFormat: seed.apiFormat } : {}),
    ...(seed?.codexCache ? { codexCache: seed.codexCache } : {}),
    ...(seed?.enabled !== undefined ? { enabled: seed.enabled } : {}),
    ...(seed?.sortIndex !== undefined ? { sortIndex: seed.sortIndex } : {}),
  };
}

function catalogInputCapabilityPatch(
  supportsImage: boolean,
): Pick<CodexCatalogModel, "inputModalities" | "supportsImage" | "textOnly"> {
  return {
    inputModalities: supportsImage ? ["text", "image"] : ["text"],
    supportsImage,
    textOnly: !supportsImage,
  };
}

function catalogSupportsImage(model: CodexCatalogModel): boolean {
  const modalities = model.inputModalities ?? model.input_modalities ?? [];
  return (
    model.supportsImage === true ||
    model.supports_image === true ||
    model.vision === true ||
    modalities.some((modality) => modality.toLowerCase() === "image")
  );
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
      | "textOnly"
      | "reasoning"
      | "codexUltra"
      | "apiFormat"
      | "codexCache"
      | "enabled"
      | "sortIndex"
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
      (row.textOnly ?? null) ===
        (incoming.textOnly ?? incoming.text_only ?? null) &&
      JSON.stringify(row.reasoning ?? null) ===
        JSON.stringify(incoming.reasoning ?? null) &&
      JSON.stringify(row.codexUltra ?? null) ===
        JSON.stringify(incoming.codexUltra ?? null) &&
      (row.apiFormat ?? null) ===
        (incoming.apiFormat ?? incoming.api_format ?? null) &&
      JSON.stringify(row.codexCache ?? null) ===
        JSON.stringify(incoming.codexCache ?? incoming.codex_cache ?? null) &&
      (row.enabled ?? true) === (incoming.enabled ?? true) &&
      (row.sortIndex ?? null) === (incoming.sortIndex ?? null)
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
      codexUltra: model.codexUltra ?? null,
      codexCache: model.codexCache ?? model.codex_cache ?? null,
      sortIndex: model.sortIndex ?? null,
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

export function CodexFormFields({
  codexProviderEditorSnapshot = null,
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
  protocolMode = "auto",
  onProtocolModeChange = () => undefined,
  protocolOverrides = {},
  onProtocolOverridesChange = () => undefined,
  reasoningProjection = "none",
  onReasoningProjectionChange = () => undefined,
  toolSchemaDialect = "openai",
  onToolSchemaDialectChange = () => undefined,
  historyReplay = "native_only",
  onHistoryReplayChange = () => undefined,
  onProtocolProbeReceiptIdsChange = ignoreProtocolProbeReceiptIds,
  catalogModels = [],
  presetCatalogModels = [],
  onCatalogModelsChange,
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
  const [isProtocolProbeProgressOpen, setIsProtocolProbeProgressOpen] =
    useState(false);
  const [protocolProbeEvents, setProtocolProbeEvents] = useState<
    CodexProtocolProbeProgressEvent[]
  >([]);
  const [protocolProbeExpectedModels, setProtocolProbeExpectedModels] =
    useState<string[]>([]);
  const [protocolProbeOutcome, setProtocolProbeOutcome] =
    useState<CodexProviderProtocolPreflightOutcome | null>(null);
  const [protocolProbeError, setProtocolProbeError] = useState("");
  const [protocolProbeIdentity, setProtocolProbeIdentity] = useState<
    string | null
  >(null);
  const protocolProbeIdentityRef = useRef<string | null>(null);
  const protocolProbeSeqRef = useRef(0);
  const [shouldHighlightFetchModels, setShouldHighlightFetchModels] =
    useState(false);
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
  const hasLegacyProviderReasoningConfig =
    Object.keys(codexChatReasoning).length > 0;
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
    protocolMode === "manual" ||
    !!maxOutputTokens;
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );
  const [catalogMountElement, setCatalogMountElement] =
    useState<HTMLDivElement | null>(null);

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
  const hasLegacyMixedProtocolCatalog = useMemo(() => {
    let hasResponses = false;
    let hasChat = false;
    for (const row of catalogRows) {
      if (row.enabled === false) continue;
      if (row.apiFormat === "openai_responses") hasResponses = true;
      if (row.apiFormat === "openai_chat") hasChat = true;
      if (hasResponses && hasChat) return true;
    }
    return false;
  }, [catalogRows]);
  const [expandedReasoningRowId, setExpandedReasoningRowId] = useState<
    string | null
  >(null);

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
    (nextCatalogModels: CodexCatalogModel[]) =>
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
    () => buildReadinessIdentityFor(catalogRows),
    [buildReadinessIdentityFor, catalogRows],
  );
  const snapshotReadinessIdentityRef = useRef<{
    snapshot: CodexProviderEditorSnapshot;
    identity: string;
  } | null>(null);
  if (
    codexProviderEditorSnapshot &&
    snapshotReadinessIdentityRef.current?.snapshot !==
      codexProviderEditorSnapshot
  ) {
    snapshotReadinessIdentityRef.current = {
      snapshot: codexProviderEditorSnapshot,
      identity: readinessIdentity,
    };
  } else if (!codexProviderEditorSnapshot) {
    snapshotReadinessIdentityRef.current = null;
  }
  const readinessIdentityRef = useRef(readinessIdentity);
  readinessIdentityRef.current = readinessIdentity;
  const bindProtocolProbeIdentity = useCallback((identity: string) => {
    protocolProbeIdentityRef.current = identity;
    setProtocolProbeIdentity(identity);
  }, []);

  // 任一身份输入变化都立即使旧结果失效并取消其 UI ownership。异步请求本身可以
  // 自然结束，但 sequence/identity guard 会阻止旧进度与最终结果回写到新配置。
  useEffect(() => {
    if (protocolProbeIdentityRef.current !== readinessIdentity) {
      protocolProbeSeqRef.current += 1;
      protocolProbeIdentityRef.current = null;
      setProtocolProbeIdentity(null);
      setIsProbingProtocol(false);
      setIsProtocolProbeConfirmOpen(false);
      setIsProtocolProbeProgressOpen(false);
      setProtocolProbeTone("muted");
      setProtocolProbeSummary("");
      setProtocolProbeEvents([]);
      setProtocolProbeExpectedModels([]);
      setProtocolProbeOutcome(null);
      setProtocolProbeError("");
      onProtocolProbeReceiptIdsChange([]);
    }
  }, [onProtocolProbeReceiptIdsChange, readinessIdentity]);

  const isProtocolProbeStateCurrent =
    protocolProbeIdentity === readinessIdentity;
  const snapshotAdaptation = codexProviderEditorSnapshot?.adaptation ?? null;
  const displayedAdaptation =
    isProtocolProbeStateCurrent && protocolProbeOutcome?.adaptationPreview
      ? protocolProbeOutcome.adaptationPreview
      : snapshotAdaptation
        ? snapshotReadinessIdentityRef.current?.identity === readinessIdentity
          ? snapshotAdaptation
          : { ...snapshotAdaptation, status: "stale" as const }
        : null;
  const revealModelCatalogFetchAction = useCallback(() => {
    bindProtocolProbeIdentity(readinessIdentity);
    setProtocolProbeTone("warning");
    setProtocolProbeSummary(
      "请先在“模型与兼容性”同步模型，或在高级设置中启用/添加至少一个模型后再验证。",
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
          setCatalogRows(mergedRows);
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
    const probeModels = catalogRowsRef.current
      .filter(
        (row) =>
          row.enabled !== false &&
          (row.model.trim() || catalogRowUpstreamModel(row)),
      )
      .map((row) => {
        const { rowId: _rowId, ...model } = row;
        const publicModel =
          model.model.trim() || catalogRowUpstreamModel(model);
        return {
          ...model,
          model: publicModel,
          upstreamModel: catalogRowUpstreamModel(model) || publicModel,
        };
      });
    if (probeModels.length === 0) {
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
    setIsProtocolProbeProgressOpen(true);
    setIsProbingProtocol(true);
    setProtocolProbeTone("muted");
    setProtocolProbeEvents([]);
    setProtocolProbeExpectedModels(probeModels.map((model) => model.model));
    setProtocolProbeOutcome(null);
    setProtocolProbeError("");
    setProtocolProbeSummary(
      `正在深度测试 ${probeModels.length} 个模型的 Responses / Chat、SSE、思考内容、工具调用和工具续轮。`,
    );
    try {
      const defaultModel =
        codexModel?.trim() ||
        probeModels[0].model ||
        probeModels[0].upstreamModel;
      const providerDraft: Provider = buildCodexProtocolProbeProviderDraft({
        providerId,
        providerName: providerName?.trim() || "Codex provider",
        baseUrl: codexBaseUrl,
        apiKey: codexApiKey,
        apiFormat,
        isFullUrl,
        defaultModel,
        models: probeModels,
        websiteUrl: websiteUrl || undefined,
        category,
        customUserAgent,
        localProxyHeadersOverride,
        localProxyBodyOverride,
        takeoverEnabled,
        codexChatReasoning,
        promptCacheRouting,
        protocolMode,
        reasoningProjection,
        toolSchemaDialect,
        historyReplay,
      });
      const outcome = await preflightCodexProviderProtocolCompatibility(
        providerDraft,
        (event) => {
          if (!ownsCurrentIdentity()) return;
          setProtocolProbeEvents((current) => [...current, event]);
        },
        "all_enabled_models",
      );
      if (!ownsCurrentIdentity()) return;
      const returnedModels = new Set(
        outcome.records.map((record) => record.target.public_model.trim()),
      );
      const missingModels = probeModels
        .map((model) => model.model.trim())
        .filter((model) => !returnedModels.has(model));
      if (missingModels.length > 0) {
        throw new Error(
          `后端没有返回已启用模型 ${missingModels.join("、")} 的探测结果`,
        );
      }
      setProtocolProbeOutcome(outcome);
      onProtocolProbeReceiptIdsChange(outcome.receiptIds);
      const verifiedRecords = outcome.records.filter(
        (record) =>
          record.result.readiness === "verified" &&
          record.result.selected_transport !== null,
      );
      const allVerified =
        outcome.records.length > 0 &&
        verifiedRecords.length === outcome.records.length;
      const allResponses =
        allVerified &&
        verifiedRecords.every(
          (record) => record.result.selected_transport === "open_ai_responses",
        );
      const allChat =
        allVerified &&
        verifiedRecords.every(
          (record) => record.result.selected_transport === "open_ai_chat",
        );
      const verified = outcome.records.filter(
        (record) => record.result.readiness === "verified",
      ).length;
      const partial = outcome.records.filter(
        (record) => record.result.readiness === "partial",
      ).length;
      const failed = outcome.records.length - verified - partial;
      const resultCounts = `Verified ${verified}，Partial ${partial}，Failed ${failed}`;

      if (allResponses) {
        const resultIdentity = buildReadinessIdentityFor(
          catalogRowsRef.current,
        );
        bindProtocolProbeIdentity(resultIdentity);
        const summary = `深度探测完成：全部模型选择 Responses；${resultCounts}。`;
        const tone = failed > 0 || partial > 0 ? "warning" : "success";
        setProtocolProbeTone(tone);
        setProtocolProbeSummary(summary);
        if (tone === "warning") {
          toast.warning(summary, { closeButton: true });
        } else {
          toast.success(summary, { closeButton: true });
        }
        return;
      }
      if (allChat) {
        const resultIdentity = buildReadinessIdentityFor(
          catalogRowsRef.current,
        );
        bindProtocolProbeIdentity(resultIdentity);
        const summary = `深度探测完成：全部模型选择 Chat Completions；${resultCounts}。`;
        const tone = failed > 0 || partial > 0 ? "warning" : "success";
        setProtocolProbeTone(tone);
        setProtocolProbeSummary(summary);
        if (tone === "warning") {
          toast.warning(summary, { closeButton: true });
        } else {
          toast.success(summary, { closeButton: true });
        }
        return;
      }

      const hasMixedVerifiedTransports =
        verifiedRecords.some(
          (record) => record.result.selected_transport === "open_ai_responses",
        ) &&
        verifiedRecords.some(
          (record) => record.result.selected_transport === "open_ai_chat",
        );
      const summary =
        allVerified && hasMixedVerifiedTransports
          ? `深度探测完成：不同模型选择了不同协议；${resultCounts}。保存时将自动拆分为两个同协议 Provider，并仍作为一个模型源使用。`
          : `深度探测完成：存在 Partial/Failed 模型，当前不能自动保存；${resultCounts}。请查看失败阶段并重试。`;
      const resultIdentity = buildReadinessIdentityFor(catalogRowsRef.current);
      bindProtocolProbeIdentity(resultIdentity);
      const tone = allVerified
        ? "success"
        : verifiedRecords.length > 0
          ? "warning"
          : "error";
      setProtocolProbeTone(tone);
      setProtocolProbeSummary(summary);
      if (tone === "success") {
        toast.success(summary, { closeButton: true });
      } else if (tone === "warning") {
        toast.warning(summary, { closeButton: true });
      } else {
        toast.error(summary, { closeButton: true });
      }
    } catch (error) {
      if (!ownsCurrentIdentity()) return;
      onProtocolProbeReceiptIdsChange([]);
      const detail = error instanceof Error ? error.message : String(error);
      const summary = `协议测试中断：${detail}`;
      bindProtocolProbeIdentity(probeIdentity);
      setProtocolProbeTone("error");
      setProtocolProbeSummary(summary);
      setProtocolProbeError(detail);
      toast.error(summary, { closeButton: true });
    } finally {
      if (probeSeq === protocolProbeSeqRef.current) {
        setIsProbingProtocol(false);
      }
    }
  }, [
    bindProtocolProbeIdentity,
    buildReadinessIdentityFor,
    codexBaseUrl,
    codexApiKey,
    codexModel,
    apiFormat,
    category,
    codexChatReasoning,
    isFullUrl,
    localProxyBodyOverride,
    localProxyHeadersOverride,
    onApiFormatChange,
    onProtocolProbeReceiptIdsChange,
    providerName,
    providerId,
    readinessIdentity,
    reasoningProjection,
    revealModelCatalogFetchAction,
    t,
    takeoverEnabled,
    toolSchemaDialect,
    historyReplay,
    protocolMode,
    promptCacheRouting,
    customUserAgent,
    websiteUrl,
  ]);

  const handleAddCatalogRow = useCallback(() => {
    if (!onCatalogModelsChange) return;
    setCatalogRows((current) => [...current, createCatalogRow()]);
  }, [onCatalogModelsChange]);

  const handleUpdateCatalogRow = useCallback(
    (index: number, patch: Partial<CodexCatalogModel>) => {
      const previousModel = catalogRowsRef.current[index]?.model ?? "";
      if (patch.model !== undefined && patch.model !== previousModel) {
        onProtocolOverridesChange(
          migrateCodexProtocolOverrideKey(
            protocolOverrides,
            previousModel,
            patch.model,
          ),
        );
      }
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
    [onProtocolOverridesChange, protocolOverrides],
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
          throw new Error("推理能力配置必须是一个对象");
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

  const presetCatalogByModel = useMemo(() => {
    const entries = new Map<string, CodexCatalogModel>();
    for (const model of presetCatalogModels) {
      const visible = model.model.trim();
      const upstream = catalogRowUpstreamModel(model);
      if (visible) entries.set(visible, model);
      if (upstream && upstream !== visible) entries.set(upstream, model);
    }
    return entries;
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
                Completions。它会对当前模型目录里的模型发送真实请求，会消耗模型
                Token 并可能产生费用，也可能触发限流；完成后会显示上游已报告的
                Token 和费用估算。
              </span>
              <span className="block">
                如果还没有模型目录，请先到上方“模型目录与上下文”点击“获取模型列表”，或手动添加至少一个模型。
              </span>
              <span className="block">
                每个模型会分别测试对应的 Responses 和 Chat Completions
                endpoint，并依次测试基础响应、SSE
                流式、思考内容、强制工具调用和工具结果续轮；每个模型最多发送 8
                次小请求，单次输出上限为 128。
              </span>
              <span className="block">
                页面会实时显示当前模型、协议和失败阶段。都不通时通常不是协议本身的问题，也可能是
                API Key、Base URL、模型权限、额度、网络、限流或上游故障。
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

      <CodexProtocolProbeProgressDialog
        open={isProtocolProbeProgressOpen}
        running={isProbingProtocol}
        expectedModels={protocolProbeExpectedModels}
        events={protocolProbeEvents}
        outcome={protocolProbeOutcome}
        error={protocolProbeError}
        onOpenChange={setIsProtocolProbeProgressOpen}
        onRetry={() => {
          setIsProtocolProbeProgressOpen(false);
          setIsProtocolProbeConfirmOpen(true);
        }}
      />

      {hasLegacyMixedProtocolCatalog && (
        <div
          role="status"
          className="flex gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-amber-900 dark:text-amber-100"
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="space-y-1">
            <div className="text-sm font-medium">
              旧版混合协议配置，需重新验证并迁移
            </div>
            <p className="text-xs leading-relaxed opacity-90">
              当前模型目录仍保存逐模型协议。普通模式保存时会重新执行双协议深度探测，并归一为一个单协议
              Provider
              或一个门面加两个内部同协议叶子；完成迁移前仍以顶层协议为准。
            </p>
          </div>
        </div>
      )}

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

      {category !== "official" && canEditCatalog && (
        <CodexProviderReadinessSection
          models={catalogRows}
          defaultModel={codexModel}
          apiFormat={apiFormat}
          adaptation={displayedAdaptation}
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
            const hasProbeModel = catalogRowsRef.current.some(
              (row) =>
                row.enabled !== false &&
                Boolean(row.model.trim() || catalogRowUpstreamModel(row)),
            );
            if (!hasProbeModel) {
              revealModelCatalogFetchAction();
              return;
            }
            bindProtocolProbeIdentity(readinessIdentity);
            setProtocolProbeTone("muted");
            setProtocolProbeSummary(
              "已打开验证确认框；如果没有看到弹窗，请按 Esc 后重试。",
            );
            setIsProtocolProbeConfirmOpen(true);
          }}
        />
      )}

      {category !== "official" && canEditCatalog && (
        <div ref={setCatalogMountElement} />
      )}

      {category !== "official" && canEditCatalog && (
        <section
          aria-labelledby="codex-model-reasoning-title"
          className="space-y-4 rounded-lg border border-border-default bg-muted/10 p-4"
        >
          <div className="space-y-1">
            <h3
              id="codex-model-reasoning-title"
              className="text-sm font-semibold text-foreground"
            >
              模型推理能力
            </h3>
            <p className="text-xs leading-relaxed text-muted-foreground">
              每个模型独立配置。这里决定 Codex
              可以选择哪些推理档位，以及请求最终如何发送给
              Provider；能力不完整时会在保存 Provider 前阻止并指出缺失项。
            </p>
          </div>

          {catalogRows.length === 0 ? (
            <p className="rounded-md border border-dashed p-3 text-xs text-muted-foreground">
              暂无模型。请先在上方同步模型，或在高级选项的模型目录明细中添加模型。
            </p>
          ) : (
            <div className="space-y-3">
              {catalogRows.map((row, index) => {
                const model = row.model.trim();
                const probeModel = catalogRowUpstreamModel(row) || model;
                const reasoningResolution = reasoningResolutions[probeModel];
                const presetReasoning =
                  presetReasoningByModel.get(model) ??
                  presetReasoningByModel.get(catalogRowUpstreamModel(row));
                const isBuiltinReasoning = row.reasoning?.source === "builtin";
                const isUserPresetOverride =
                  Boolean(presetReasoning) && row.reasoning?.source === "user";
                const reasoningSourceMode: CodexReasoningCapabilitySourceMode =
                  isBuiltinReasoning
                    ? "builtin"
                    : row.reasoning
                      ? "manual"
                      : "automatic";
                const isReasoningEditorExpanded =
                  expandedReasoningRowId === row.rowId;
                const reasoningSourceLabel = isBuiltinReasoning
                  ? "CCSM 受维护声明"
                  : isUserPresetOverride
                    ? "用户声明（已覆盖维护值）"
                    : row.reasoning
                      ? "用户声明"
                      : reasoningResolution?.source === "detection"
                        ? "自动检测"
                        : reasoningResolution?.source === "library"
                          ? "维护能力库"
                          : "自动发现或服务端默认";
                const selectableEfforts =
                  reasoningResolution?.resolved.codexSelectableEfforts ??
                  row.reasoning?.supportedEfforts ??
                  [];
                const defaultEffort =
                  reasoningResolution?.resolved.providerDefaultEffort ??
                  row.reasoning?.defaultEffort;
                const discoveredReasoning = reasoningResolution?.capability
                  ? (reasoningResolution.capability as unknown as CodexModelReasoningCapability)
                  : undefined;

                return (
                  <article
                    key={`reasoning:${row.rowId}`}
                    className="space-y-3 rounded-md border bg-background p-3 text-xs"
                  >
                    <CodexModelReasoningSummary
                      model={row.displayName?.trim() || model || "未命名模型"}
                      source={reasoningSourceLabel}
                      selectableEfforts={selectableEfforts}
                      defaultEffort={defaultEffort}
                      ultraEnabled={row.codexUltra?.enabled === true}
                      ultraEffort={row.codexUltra?.providerEffort}
                      ultraEfforts={
                        reasoningResolution?.resolved.providerAcceptedEfforts ??
                        []
                      }
                      onUltraChange={(codexUltra) =>
                        handleUpdateCatalogRow(index, { codexUltra })
                      }
                      expanded={isReasoningEditorExpanded}
                      onToggle={() =>
                        setExpandedReasoningRowId((current) =>
                          current === row.rowId ? null : row.rowId,
                        )
                      }
                    />

                    {isReasoningEditorExpanded && (
                      <>
                        <label className="grid min-w-52 gap-1">
                          <span className="font-medium">能力来源</span>
                          <select
                            className="rounded-md border bg-background px-3 py-2"
                            value={reasoningSourceMode}
                            aria-label={`${model || "模型"}推理能力来源`}
                            onChange={(event) =>
                              handleUpdateCatalogRow(index, {
                                reasoning: applyCodexReasoningCapabilitySource(
                                  event.target
                                    .value as CodexReasoningCapabilitySourceMode,
                                  row.reasoning,
                                  presetReasoning,
                                  discoveredReasoning,
                                ),
                              })
                            }
                          >
                            <option value="automatic">自动发现</option>
                            <option value="builtin" disabled={!presetReasoning}>
                              使用 CCSM 受维护声明
                            </option>
                            <option value="manual">手动声明</option>
                          </select>
                          {reasoningSourceMode === "automatic" ? (
                            <span className="text-muted-foreground">
                              自动发现会按当前
                              Provider、模型和已验证声明解析能力；它不会写入本模型配置。需要调整档位、映射或开启
                              Ultra 时，请按当前结果创建用户覆盖。
                            </span>
                          ) : null}
                        </label>

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
                                  toast.success("已更新模型推理能力检测结果");
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
                              const detected = reasoningResolution.detection
                                ? capabilityFromReasoningDetection({
                                    found: reasoningResolution.detection,
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
                                reasoning: applyCodexReasoningCapabilitySource(
                                  "manual",
                                  row.reasoning,
                                  presetReasoning,
                                  discoveredReasoning,
                                ),
                              })
                            }
                            onCustomizeEffective={
                              !row.reasoning && discoveredReasoning
                                ? () =>
                                    handleUpdateCatalogRow(index, {
                                      reasoning:
                                        applyCodexReasoningCapabilitySource(
                                          "manual",
                                          row.reasoning,
                                          presetReasoning,
                                          discoveredReasoning,
                                        ),
                                    })
                                : undefined
                            }
                            onRestoreBuiltin={() =>
                              handleUpdateCatalogRow(index, {
                                reasoning: applyCodexReasoningCapabilitySource(
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

                        {isUserPresetOverride ? (
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() =>
                              handleUpdateCatalogRow(index, {
                                reasoning: applyCodexReasoningCapabilitySource(
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
                        {isBuiltinReasoning && row.reasoning ? (
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() =>
                              handleUpdateCatalogRow(index, {
                                reasoning: applyCodexReasoningCapabilitySource(
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

                        {row.reasoning ? (
                          <CodexModelReasoningEditor
                            model={model || "模型"}
                            capability={row.reasoning}
                            readOnly={isBuiltinReasoning}
                            onChange={(reasoning) =>
                              handleUpdateCatalogRow(index, { reasoning })
                            }
                          />
                        ) : null}

                        <details>
                          <summary className="cursor-pointer text-muted-foreground">
                            专家 JSON
                          </summary>
                          <Textarea
                            key={`reasoning-json:${row.rowId}:${JSON.stringify(row.reasoning)}`}
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
                            aria-label={`${model || "模型"}推理能力 JSON`}
                          />
                        </details>
                      </>
                    )}
                  </article>
                );
              })}
            </div>
          )}
        </section>
      )}

      {/* 高级选项只保留手动协议、请求覆盖和模型目录明细。 */}
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
                  {(apiFormat === "openai_chat" ||
                    apiFormat === "openai_responses") && (
                    <CodexProtocolAdvancedSettings
                      mode={protocolMode}
                      apiFormat={apiFormat}
                      reasoningProjection={reasoningProjection}
                      toolSchemaDialect={toolSchemaDialect}
                      historyReplay={historyReplay}
                      onModeChange={onProtocolModeChange}
                      onApiFormatChange={onApiFormatChange}
                      onReasoningProjectionChange={onReasoningProjectionChange}
                      onToolSchemaDialectChange={onToolSchemaDialectChange}
                      onHistoryReplayChange={onHistoryReplayChange}
                    />
                  )}
                </div>
              </div>
            )}

            {takeoverEnabled &&
              isChatFormat &&
              canEditReasoning &&
              hasLegacyProviderReasoningConfig && (
                <details
                  className={cn(
                    "space-y-3 rounded-md border border-amber-500/30 bg-amber-500/10 p-3",
                    shouldShowSpeedTest &&
                      "border-t border-border-default pt-3",
                  )}
                >
                  <summary className="cursor-pointer text-sm font-medium text-foreground">
                    旧版兼容兜底
                  </summary>
                  <div className="space-y-3 pt-2">
                    <div className="space-y-1">
                      <p className="text-xs leading-relaxed text-muted-foreground">
                        这是一份旧 Provider
                        级推理配置：它会影响所有未单独配置模型推理能力的模型。请优先使用上方“模型推理能力”为每个模型声明能力；这里只保留对既有配置的兼容编辑。
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
                          旧配置：为没有模型级声明的 Chat 模型启用 thinking
                          开关。
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
                          旧配置：为没有模型级声明的 Chat 模型启用 effort
                          参数转换。
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
                </details>
              )}
          </CollapsibleContent>

          {/* 模型映射 / 模型目录 —— 与「路由接管」解耦，常驻显示（可编辑即渲染）。
                填了才生成 catalog：Chat 模式生成兼容路由、原生 Responses 生成
                model-catalogs.json；留空则不生成。排在自定义 UA 之前。 */}
          {catalogMountElement &&
            canEditCatalog &&
            createPortal(
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
                    <div className="hidden grid-cols-[88px_1fr_1fr_1fr_132px_150px_76px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
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
                      <span>协议覆盖</span>
                      <span>
                        {t("codexConfig.catalogOrderColumn", {
                          defaultValue: "顺序",
                        })}
                      </span>
                      <span />
                    </div>

                    {catalogRows.map((row, index) => {
                      const model = row.model.trim();
                      const presetCatalogModel =
                        presetCatalogByModel.get(model) ??
                        presetCatalogByModel.get(catalogRowUpstreamModel(row));
                      const supportsImage = catalogSupportsImage(row);
                      const presetDeclaresInputCapability = Boolean(
                        presetCatalogModel &&
                          (presetCatalogModel.inputModalities !== undefined ||
                            presetCatalogModel.input_modalities !== undefined ||
                            presetCatalogModel.supportsImage !== undefined ||
                            presetCatalogModel.supports_image !== undefined ||
                            presetCatalogModel.vision !== undefined ||
                            presetCatalogModel.textOnly !== undefined ||
                            presetCatalogModel.text_only !== undefined),
                      );

                      return (
                        <div
                          key={row.rowId}
                          className="grid grid-cols-1 gap-2 rounded-md border border-transparent p-1 md:grid-cols-[88px_1fr_1fr_1fr_132px_150px_76px_36px]"
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
                          <select
                            className="h-9 rounded-md border border-border-default bg-background px-2 text-xs text-foreground"
                            aria-label={`${model || "模型"} 协议选择`}
                            value={
                              protocolOverrides[
                                normalizeCodexPublicModelKey(model)
                              ] ?? "follow_auto"
                            }
                            onChange={(event) =>
                              onProtocolOverridesChange(
                                setCodexProtocolOverride(
                                  protocolOverrides,
                                  model,
                                  event.target
                                    .value as CodexProtocolChoiceInput,
                                ),
                              )
                            }
                          >
                            <option value="follow_auto">跟随自动</option>
                            <option value="openai_responses">Responses</option>
                            <option value="openai_chat">Chat</option>
                          </select>
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
                          <fieldset
                            className="col-span-full flex flex-wrap items-center gap-2 border-t border-border-default pt-2 text-xs"
                            aria-label={`${model || "模型"} 输入能力`}
                          >
                            <legend className="mr-1 font-medium">
                              输入能力
                            </legend>
                            <div
                              className="inline-flex overflow-hidden rounded-md border"
                              role="radiogroup"
                              aria-label={`${model || "模型"} 输入能力选择`}
                            >
                              <Button
                                type="button"
                                size="sm"
                                variant={supportsImage ? "default" : "ghost"}
                                className="rounded-none"
                                aria-pressed={supportsImage}
                                aria-label={`${model || "模型"} 文本与图像`}
                                onClick={() =>
                                  handleUpdateCatalogRow(
                                    index,
                                    catalogInputCapabilityPatch(true),
                                  )
                                }
                              >
                                文本与图像
                              </Button>
                              <Button
                                type="button"
                                size="sm"
                                variant={!supportsImage ? "default" : "ghost"}
                                className="rounded-none border-l"
                                aria-pressed={!supportsImage}
                                aria-label={`${model || "模型"} 仅文本`}
                                onClick={() =>
                                  handleUpdateCatalogRow(
                                    index,
                                    catalogInputCapabilityPatch(false),
                                  )
                                }
                              >
                                仅文本
                              </Button>
                            </div>
                            <span className="text-muted-foreground">
                              保存后覆盖当前 Provider 的预设，不需要等待发布。
                            </span>
                            {presetDeclaresInputCapability &&
                            presetCatalogModel ? (
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                aria-label={`${model || "模型"} 恢复 CCSM 输入能力预设`}
                                onClick={() =>
                                  handleUpdateCatalogRow(
                                    index,
                                    catalogInputCapabilityPatch(
                                      catalogSupportsImage(presetCatalogModel),
                                    ),
                                  )
                                }
                              >
                                恢复 CCSM 预设
                              </Button>
                            ) : null}
                          </fieldset>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>,
              catalogMountElement,
            )}

          <CollapsibleContent className="space-y-3 pt-3">
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

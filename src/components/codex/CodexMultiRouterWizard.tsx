import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CheckCircle2,
  Database,
  GitBranch,
  RefreshCw,
  Route,
  Server,
  ShieldAlert,
  Wand2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type {
  CodexCacheConfig,
  CodexCatalogModel,
  CodexOfficialAuthConfig,
  CodexOfficialAuthMode,
  CodexRoutingRoute,
} from "@/types";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { providersApi } from "@/lib/api/providers";
import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";
import {
  fetchCodexOauthCachedModels,
  fetchCodexOauthModels,
  fetchModelsForConfig,
  probeCodexChatForConfig,
  probeCodexResponsesForConfig,
} from "@/lib/api/model-fetch";
import {
  CODEX_MULTI_ROUTER_DEFAULT_NAME,
  CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY,
  DEFAULT_CODEX_OFFICIAL_AUTH,
  applyWizardConnectivityApiFormatOverrides,
  buildCodexMultiRouterWizardPlan,
  buildWizardModelCatalog,
  canContinueAfterConnectivity,
  classifyWizardDualProtocolConnectivityResult,
  classifyWizardConnectivityResult,
  collectWizardModelNameCollisions,
  defaultWizardModelSources,
  getWizardConnectivityProbeModels,
  getWizardConfigIssues,
  getWizardModelFetchConfig,
  isWizardCatalogOnlyModelSource,
  isWizardCodexOAuthSource,
  inferCodexOfficialAuth,
  isCodexMultiRouterPlan,
  mergeFetchedModelsIntoWizardProvider,
  readWizardCodexOAuthAccountId,
  readWizardModelCatalog,
  readWizardProviderBaseUrl,
  resolveWizardModelNameCollisions,
  skippedWizardConnectivityResult,
  type WizardConnectivityResult,
  type WizardModelFetchConfig,
} from "@/lib/codexMultiRouterWizard";
import {
  DEFAULT_HOSTED_TOOLS_CONFIG,
  readHostedToolsConfig,
} from "@/lib/hostedTools";
import type { WorkspaceTab } from "@/components/codex/CodexRouterWorkspacePage";
import { codexCatalogOnlyPlanModelFetchMessage } from "@/utils/codexPlanModelFetch";
import { useCodexOauth } from "@/components/providers/forms/hooks/useCodexOauth";

interface CodexMultiRouterWizardProps {
  open: boolean;
  providers: Provider[];
  mode?: "create" | "edit";
  planId?: string;
  onOpenChange: (open: boolean) => void;
  onCreateProvider: () => void;
  onOpenProviderConfig?: (provider: Provider) => void;
  onOpenWorkspace: (provider: Provider, tab: WorkspaceTab) => void;
  onEnablePlan: (provider: Provider) => void | Promise<void>;
}

type WizardStepKey = "sources" | "prepare" | "review" | "activate";

interface WizardStep {
  key: WizardStepKey;
  title: string;
  description: string;
  icon: typeof Wand2;
}

interface WizardIssue {
  id: string;
  stage: WizardStepKey;
  severity: "error" | "warning";
  title: string;
  detail: string;
  canContinue: boolean;
  providerName?: string;
}

type ModelFetchCardStatus =
  | "idle"
  | "loading"
  | "updated"
  | "unchanged"
  | "skipped"
  | "error";

interface ModelFetchDiff {
  added: string[];
  removed: string[];
  changed: string[];
}

interface ModelFetchCardState {
  status: ModelFetchCardStatus;
  message: string;
  modelCount: number;
  diff?: ModelFetchDiff;
}

const STEPS: WizardStep[] = [
  {
    key: "sources",
    title: "选择模型源",
    description: "选择已经接入的 Provider，或先添加一个新的 Codex 模型源。",
    icon: Server,
  },
  {
    key: "prepare",
    title: "自动准备与验证",
    description: "同步模型目录、验证协议，并自动处理不同模型源之间的重名。",
    icon: RefreshCw,
  },
  {
    key: "review",
    title: "选择模型并预览路由",
    description:
      "命名方案，选择要展示给 Codex 的模型，并确认生成的路由与认证策略。",
    icon: GitBranch,
  },
  {
    key: "activate",
    title: "启用并验证",
    description: "保存并启用 MultiRouter，然后在状态页完成一次真实请求验证。",
    icon: CheckCircle2,
  },
];

type WizardFlowStatus =
  | "opened"
  | "needSources"
  | "reviewProviderConfig"
  | "configIncomplete"
  | "readyToFetchModels"
  | "fetchingModels"
  | "modelFetchPartial"
  | "modelsFetched"
  | "probingConnectivity"
  | "connectivityPassed"
  | "connectivityPartial"
  | "connectivityFailed"
  | "collisionReviewRequired"
  | "routePreview"
  | "savingPlan"
  | "saveFailed"
  | "published"
  | "enablePrompt"
  | "enabling"
  | "enableFailed"
  | "enabled"
  | "completed"
  | "dismissed";

interface WizardFlowState {
  status: WizardFlowStatus;
  stepKey: WizardStepKey;
  lastError?: string;
  fetchSummary?: {
    successCount: number;
    skippedCount: number;
    failedCount: number;
  };
  connectivitySummary?: {
    passCount: number;
    warnCount: number;
    skippedCount: number;
    failCount: number;
  };
}

type WizardFlowEvent =
  | { type: "INIT"; hasSources: boolean }
  | { type: "GOTO_STEP"; stepKey: WizardStepKey }
  | { type: "NEXT"; nextStatus: WizardFlowStatus; nextStepKey: WizardStepKey }
  | { type: "FETCH_START" }
  | {
      type: "FETCH_DONE";
      partial: boolean;
      summary: WizardFlowState["fetchSummary"];
    }
  | { type: "PROBE_START" }
  | {
      type: "PROBE_DONE";
      canContinue: boolean;
      hasWarnings: boolean;
      summary: WizardFlowState["connectivitySummary"];
    }
  | { type: "SAVE_START" }
  | { type: "SAVE_SUCCESS" }
  | { type: "SAVE_ERROR"; error: string }
  | { type: "ENABLE_START" }
  | { type: "ENABLE_SUCCESS" }
  | { type: "ENABLE_ERROR"; error: string }
  | { type: "DISMISS" }
  | { type: "COMPLETE" };

const INITIAL_FLOW_STATE: WizardFlowState = {
  status: "opened",
  stepKey: "sources",
};

// 将左侧教程步骤映射到业务状态；手动跳步也会进入对应的状态分支，避免 UI 步骤和流程状态脱节。
function statusForStep(stepKey: WizardStepKey): WizardFlowStatus {
  switch (stepKey) {
    case "sources":
      return "reviewProviderConfig";
    case "prepare":
      return "readyToFetchModels";
    case "review":
      return "routePreview";
    case "activate":
      return "enablePrompt";
    default:
      return "opened";
  }
}

// reducer 是向导的状态机核心；所有异步动作只发事件，不直接改流程状态。
function wizardFlowReducer(
  state: WizardFlowState,
  event: WizardFlowEvent,
): WizardFlowState {
  switch (event.type) {
    case "INIT":
      return {
        status: event.hasSources ? "opened" : "needSources",
        stepKey: "sources",
      };
    case "GOTO_STEP":
      return {
        ...state,
        status: statusForStep(event.stepKey),
        stepKey: event.stepKey,
        lastError: undefined,
      };
    case "NEXT":
      return {
        ...state,
        status: event.nextStatus,
        stepKey: event.nextStepKey,
        lastError: undefined,
      };
    case "FETCH_START":
      return { ...state, status: "fetchingModels", lastError: undefined };
    case "FETCH_DONE":
      return {
        ...state,
        status: event.partial ? "modelFetchPartial" : "modelsFetched",
        stepKey: "prepare",
        fetchSummary: event.summary,
      };
    case "PROBE_START":
      return {
        ...state,
        status: "probingConnectivity",
        stepKey: "prepare",
        lastError: undefined,
      };
    case "PROBE_DONE":
      return {
        ...state,
        status: event.canContinue
          ? event.hasWarnings
            ? "connectivityPartial"
            : "connectivityPassed"
          : "connectivityFailed",
        stepKey: event.canContinue ? "review" : "prepare",
        connectivitySummary: event.summary,
      };
    case "SAVE_START":
      return { ...state, status: "savingPlan", lastError: undefined };
    case "SAVE_SUCCESS":
      return { ...state, status: "published", stepKey: "activate" };
    case "SAVE_ERROR":
      return {
        ...state,
        status: "saveFailed",
        stepKey: "activate",
        lastError: event.error,
      };
    case "ENABLE_START":
      return { ...state, status: "enabling", lastError: undefined };
    case "ENABLE_SUCCESS":
      return { ...state, status: "enabled", stepKey: "activate" };
    case "ENABLE_ERROR":
      return {
        ...state,
        status: "enableFailed",
        stepKey: "activate",
        lastError: event.error,
      };
    case "DISMISS":
      return { ...state, status: "dismissed" };
    case "COMPLETE":
      return { ...state, status: "completed" };
    default:
      return state;
  }
}

// 将模型源的模型目录数量转成人可扫读的摘要，避免向导卡片暴露底层 JSON。
function modelSourceSummary(provider: Provider): string {
  const models = readWizardModelCatalog(provider);
  if (models.length === 0) return "尚未获取模型";
  return `${models.length} 个模型`;
}

// 生成模型目录对比签名；只比较会影响路由、展示、上下文和多模态能力的字段。
function modelCatalogSignature(model: CodexCatalogModel): string {
  const displayName = model.displayName?.trim() || model.model;
  return JSON.stringify({
    upstreamModel: model.upstreamModel ?? model.upstream_model ?? model.model,
    displayName,
    contextWindow:
      model.contextWindow === undefined ? null : String(model.contextWindow),
    inputModalities: model.inputModalities ?? model.input_modalities ?? [],
    textOnly: model.textOnly ?? model.text_only ?? null,
    supportsImage: model.supportsImage ?? model.supports_image ?? null,
    vision: model.vision ?? null,
  });
}

// 比较刷新前后的目录，用于在 provider 卡片上标注“有更新/无更新”。
function diffWizardModelCatalog(
  beforeModels: CodexCatalogModel[],
  afterModels: CodexCatalogModel[],
): ModelFetchDiff {
  const beforeByModel = new Map(
    beforeModels.map((model) => [model.model, modelCatalogSignature(model)]),
  );
  const afterByModel = new Map(
    afterModels.map((model) => [model.model, modelCatalogSignature(model)]),
  );
  const added = afterModels
    .map((model) => model.model)
    .filter((model) => !beforeByModel.has(model));
  const removed = beforeModels
    .map((model) => model.model)
    .filter((model) => !afterByModel.has(model));
  const changed = afterModels
    .map((model) => model.model)
    .filter(
      (model) =>
        beforeByModel.has(model) &&
        beforeByModel.get(model) !== afterByModel.get(model),
    );
  return { added, removed, changed };
}

// 判断一次 /models 读取是否实际改变了目录内容。
function hasModelFetchDiff(diff: ModelFetchDiff): boolean {
  return (
    diff.added.length > 0 || diff.removed.length > 0 || diff.changed.length > 0
  );
}

// 只展示少量变化样例，避免 provider 卡片被很长的模型列表撑高。
function formatModelFetchDiff(diff?: ModelFetchDiff): string | null {
  if (!diff || !hasModelFetchDiff(diff)) return null;
  const parts: string[] = [];
  if (diff.added.length > 0) {
    parts.push(
      `新增 ${diff.added.length}: ${diff.added.slice(0, 3).join(", ")}`,
    );
  }
  if (diff.removed.length > 0) {
    parts.push(
      `移除 ${diff.removed.length}: ${diff.removed.slice(0, 3).join(", ")}`,
    );
  }
  if (diff.changed.length > 0) {
    parts.push(
      `更新 ${diff.changed.length}: ${diff.changed.slice(0, 3).join(", ")}`,
    );
  }
  return parts.join("；");
}

// 给未刷新过的 provider 卡片提供稳定默认状态。
function defaultModelFetchCardState(provider: Provider): ModelFetchCardState {
  return {
    status: "idle",
    message: "等待读取模型列表",
    modelCount: readWizardModelCatalog(provider).length,
  };
}

// 模型读取状态的 badge 统一在这里收口，保证顶部按钮和卡片语义一致。
function modelFetchStatusLabel(status: ModelFetchCardStatus): string {
  switch (status) {
    case "loading":
      return "正在读取";
    case "updated":
      return "有模型列表更新";
    case "unchanged":
      return "无模型列表更新";
    case "skipped":
      return "无法在线读取";
    case "error":
      return "获取失败";
    case "idle":
    default:
      return "等待读取";
  }
}

// 根据结果选择 badge 风格；失败用 destructive，其它状态保持低干扰。
function modelFetchBadgeVariant(
  status: ModelFetchCardStatus,
): "outline" | "secondary" | "destructive" {
  if (status === "error") return "destructive";
  if (status === "updated" || status === "unchanged") return "secondary";
  return "outline";
}

// 把模型列表抓取参数格式化成安全摘要，不展示真实 API Key 或 AK/SK。
function fetchConfigSummary(config: WizardModelFetchConfig | null): string {
  if (!config) return "缺少 Base URL 或 API Key";
  if (config.volcengineModelListAction) {
    return `火山 OpenAPI ${config.volcengineModelListAction} (${config.baseUrl})`;
  }
  return `${config.baseUrl}${config.isFullUrl ? " (完整 URL)" : ""}`;
}

// 将 route 的缓存能力转换成向导里的说明，强调缓存验证看真实 usage，而不是基础连通性。
function cacheCapabilitySummary(cache?: CodexCacheConfig): string {
  switch (cache?.cacheMode) {
    case "openai_prompt_cache":
      return "OpenAI Prompt Cache：保留原生前缀缓存；支持时会透传 prompt_cache_key/retention，真实命中看 cached_tokens。";
    case "deepseek_context_cache":
      return "DeepSeek Context Cache：不注入 OpenAI 私有参数；真实命中看 prompt_cache_hit_tokens / miss_tokens。";
    case "glm_context_cache":
    case "zai_context_cache":
      return "GLM/Z.AI 自动上下文缓存：保持稳定前缀，不注入 OpenAI 私有参数；真实命中看 cached_tokens。";
    case "qwen_context_cache":
      return "Qwen/DashScope 上下文缓存：按协议保持请求形态；真实命中看 cached_tokens / cache_creation_input_tokens。";
    case "anthropic_cache_control":
      return "Anthropic cache_control：只适用于 Anthropic/Bedrock 风格消息块。";
    case "auto_prefix_cache":
      return "自动前缀缓存：保持稳定前缀，不额外注入 OpenAI cache 参数。";
    default:
      return "缓存能力未知：只做基础连通性与路由验证，真实命中需看上游 usage。";
  }
}

// 生成官方 Codex OAuth 动态目录读取文案；失败时保留最后一次成功目录，不清空用户配置。
function codexOAuthModelFetchMessage(
  hasModelCatalog: boolean,
  hasCodexOauthAccount: boolean,
) {
  const catalogText = hasModelCatalog
    ? "已保留官方 Codex 内置模型目录"
    : "当前没有可用模型目录";
  const authText = hasCodexOauthAccount
    ? "已检测到 ChatGPT OAuth 账号"
    : "尚未检测到 ChatGPT OAuth 账号，请先在配置步骤登录";
  return `官方 Codex OAuth 将通过 ChatGPT 专用模型接口在线刷新；${catalogText}，${authText}。`;
}

// 生成 Plan provider 在线模型列表不可用时的回退文案，避免把火山缺 AK/SK 误写成永久不支持。
function catalogOnlyPlanMessage(provider: Provider, hasModelCatalog: boolean) {
  return codexCatalogOnlyPlanModelFetchMessage(hasModelCatalog, {
    baseUrl: readWizardProviderBaseUrl(provider),
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    accessKeyId: provider.meta?.usage_script?.accessKeyId,
    secretAccessKey: provider.meta?.usage_script?.secretAccessKey,
  });
}

// 将内部状态机状态转换为用户能理解的短句，便于在向导顶部持续暴露当前进度。
function wizardStatusText(state: WizardFlowState): string {
  switch (state.status) {
    case "needSources":
      return "等待添加至少一个模型源。";
    case "configIncomplete":
      return "部分模型源不能自动获取模型，可补全配置或继续使用已有目录。";
    case "readyToFetchModels":
      return "配置已就绪，可以自动获取模型列表。";
    case "fetchingModels":
      return "正在读取各 provider 的模型列表。";
    case "modelFetchPartial":
      return "模型列表部分成功，请检查失败或跳过的 provider。";
    case "modelsFetched":
      return "模型列表已刷新，下一步处理重名模型。";
    case "probingConnectivity":
      return "正在对每个 provider/model 发起最小 /v1/responses 探测。";
    case "connectivityPassed":
      return "所有已测试模型都能直接响应 /v1/responses。";
    case "connectivityPartial":
      return "连通性测试存在可继续警告，请确认 Chat-only 或跳过项符合预期。";
    case "connectivityFailed":
      return "连通性测试存在阻塞项，请修复 provider 或模型后再保存发布。";
    case "collisionReviewRequired":
      return "检测到重名模型，需要确认别名策略。";
    case "routePreview":
      return "路由预览已生成，可以继续保存发布。";
    case "savingPlan":
      return "正在保存 MultiRouter provider。";
    case "saveFailed":
      return "保存失败，请修正后重试。";
    case "published":
      return "MultiRouter provider 已保存。";
    case "enabling":
      return "正在启用这个多路路由。";
    case "enableFailed":
      return "启用失败，请重试或检查本地代理状态。";
    case "enabled":
      return "已启用，状态页会等待最近一次 Codex 请求转发成功。";
    case "completed":
      return "向导已完成。";
    case "dismissed":
      return "向导已跳过。";
    case "opened":
    case "enablePrompt":
    default:
      return "按步骤完成多路模型配置。";
  }
}

// 把异常转换成面向用户的短文本，同时保留 console 中的详细错误对象。
function formatWizardError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// 生成稳定但不依赖后端的异常 ID，方便 React 渲染和后续按阶段清理。
function createWizardIssueId(stage: WizardStepKey, title: string): string {
  return `${stage}:${title}:${Date.now()}:${Math.random().toString(36).slice(2)}`;
}

// 在有序列表中移动一项，供模型汇总列表和子 Agent 候选列表复用。
function moveOrderedItem(items: string[], item: string, direction: -1 | 1) {
  const index = items.indexOf(item);
  const targetIndex = index + direction;
  if (index < 0 || targetIndex < 0 || targetIndex >= items.length) {
    return items;
  }
  const next = [...items];
  [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
  return next;
}

// 用最新可用模型校正用户草稿顺序；未显式编辑时保留完整模型列表，显式编辑后不自动加回已剔除模型。
function resolveActiveCatalogModelOrder(
  availableModels: CodexCatalogModel[],
  draftOrder: string[] | null,
) {
  const availableNames = availableModels.map((model) => model.model);
  if (draftOrder === null) return availableNames;
  const availableSet = new Set(availableNames);
  return draftOrder.filter((model) => availableSet.has(model));
}

// 保存子 Agent 候选时必须先按最终模型池过滤，避免引用已经剔除的模型。
function resolveActiveSpawnAgentModels(
  draftModels: string[],
  catalogModelOrder: string[],
) {
  const catalogModelSet = new Set(catalogModelOrder);
  return draftModels.filter((model) => catalogModelSet.has(model)).slice(0, 5);
}

// 刷新模型列表后保留用户已经勾选的模型，只把真正新增的模型追加进去。
function reconcileCatalogModelOrderAfterFetch(
  currentOrder: string[] | null,
  previousAvailableModels: string[],
  nextAvailableModels: string[],
) {
  if (currentOrder === null) return null;
  const nextAvailableSet = new Set(nextAvailableModels);
  const previousAvailableSet = new Set(previousAvailableModels);
  const retained = currentOrder.filter((model) => nextAvailableSet.has(model));
  const added = nextAvailableModels.filter(
    (model) => !previousAvailableSet.has(model),
  );
  return [...retained, ...added];
}

export function CodexMultiRouterWizard({
  open,
  providers,
  mode,
  planId,
  onOpenChange,
  onCreateProvider,
  onOpenProviderConfig,
  onOpenWorkspace,
  onEnablePlan,
}: CodexMultiRouterWizardProps) {
  const queryClient = useQueryClient();
  const {
    accounts: codexOauthAccounts,
    hasAnyAccount: hasCodexOauthAccount,
    isLoadingStatus: isCodexOauthStatusLoading,
  } = useCodexOauth();
  const [flowState, dispatchFlow] = useReducer(
    wizardFlowReducer,
    INITIAL_FLOW_STATE,
  );
  const [draftSources, setDraftSources] = useState<Provider[]>([]);
  const [selectedSourceIds, setSelectedSourceIds] = useState<string[]>([]);
  const [draftPlanName, setDraftPlanName] = useState(
    CODEX_MULTI_ROUTER_DEFAULT_NAME,
  );
  const [draftOfficialAuth, setDraftOfficialAuth] =
    useState<CodexOfficialAuthConfig>(DEFAULT_CODEX_OFFICIAL_AUTH);
  const [webSearchEnabled, setWebSearchEnabled] = useState(true);
  const [imageGenerationEnabled, setImageGenerationEnabled] = useState(true);
  const [catalogModelOrder, setCatalogModelOrder] = useState<string[] | null>(
    null,
  );
  const [draftSpawnAgentModels, setDraftSpawnAgentModels] = useState<string[]>(
    [],
  );
  const [savedPlan, setSavedPlan] = useState<Provider | null>(null);
  const [connectivityResults, setConnectivityResults] = useState<
    WizardConnectivityResult[]
  >([]);
  const [isConnectivityConfirmOpen, setIsConnectivityConfirmOpen] =
    useState(false);
  const [wizardIssues, setWizardIssues] = useState<WizardIssue[]>([]);
  const [modelFetchCards, setModelFetchCards] = useState<
    Record<string, ModelFetchCardState>
  >({});
  const initializedOpenRef = useRef(false);

  const resolvedMode =
    mode ??
    (planId || providers.some((provider) => isCodexMultiRouterPlan(provider))
      ? "edit"
      : "create");
  const existingPlan = useMemo(() => {
    if (resolvedMode !== "edit") return undefined;
    return planId
      ? providers.find(
          (provider) =>
            provider.id === planId && isCodexMultiRouterPlan(provider),
        )
      : providers.find((provider) => isCodexMultiRouterPlan(provider));
  }, [planId, providers, resolvedMode]);
  const editingTargetMissing = resolvedMode === "edit" && !existingPlan;
  const providerModelSources = useMemo(
    () => defaultWizardModelSources(providers),
    [providers],
  );
  const hasCodexOAuthSources = useMemo(
    () => draftSources.some((provider) => isWizardCodexOAuthSource(provider)),
    [draftSources],
  );
  const selectedSourceIdSet = useMemo(
    () => new Set(selectedSourceIds),
    [selectedSourceIds],
  );
  const hasUnauthenticatedCodexOAuthSources =
    hasCodexOAuthSources && !isCodexOauthStatusLoading && !hasCodexOauthAccount;
  const stepIndex = STEPS.findIndex((step) => step.key === flowState.stepKey);
  // 防御旧状态或异常跳转写入未知步骤，确保向导始终有可渲染的首步。
  const currentStep = STEPS[stepIndex] ?? STEPS[0];
  const CurrentStepIcon = currentStep.icon;
  const configIssues = useMemo(
    () => getWizardConfigIssues(draftSources),
    [draftSources],
  );
  const modelCollisions = useMemo(
    () => collectWizardModelNameCollisions(draftSources),
    [draftSources],
  );
  const routeReadySources = applyWizardConnectivityApiFormatOverrides(
    draftSources,
    connectivityResults,
  );
  const availableCatalogModels = buildWizardModelCatalog(
    resolveWizardModelNameCollisions(routeReadySources),
  ).models;
  const activeCatalogModelOrder = resolveActiveCatalogModelOrder(
    availableCatalogModels,
    catalogModelOrder,
  );
  const activeSpawnAgentModels = resolveActiveSpawnAgentModels(
    draftSpawnAgentModels,
    activeCatalogModelOrder,
  );
  const isRefreshingModels = flowState.status === "fetchingModels";
  const isProbingConnectivity = flowState.status === "probingConnectivity";
  const isSavingPlan = flowState.status === "savingPlan";
  const isEnablingPlan = flowState.status === "enabling";

  // 每次打开向导只初始化一次。父组件 rerender 会传入新的 providers 数组，不能因此把用户从第 2 步重置回第 1 步。
  useEffect(() => {
    if (!open) {
      initializedOpenRef.current = false;
      return;
    }
    if (initializedOpenRef.current) return;

    initializedOpenRef.current = true;
    setSavedPlan(existingPlan ?? null);
    setDraftSources(providerModelSources);
    setSelectedSourceIds(providerModelSources.map((provider) => provider.id));
    setDraftPlanName(existingPlan?.name ?? CODEX_MULTI_ROUTER_DEFAULT_NAME);
    setDraftOfficialAuth(
      inferCodexOfficialAuth(existingPlan?.settingsConfig?.codexRouting) ??
        DEFAULT_CODEX_OFFICIAL_AUTH,
    );
    const hostedTools = existingPlan
      ? readHostedToolsConfig(existingPlan)
      : DEFAULT_HOSTED_TOOLS_CONFIG;
    setWebSearchEnabled(hostedTools.webSearch.enabled);
    setImageGenerationEnabled(hostedTools.imageGeneration.enabled);
    // 复用统一的安全目录读取，历史方案中混入 null/原始值时不能让整个窗口白屏。
    setCatalogModelOrder(
      existingPlan
        ? readWizardModelCatalog(existingPlan).map((model) => model.model)
        : null,
    );
    setDraftSpawnAgentModels(
      existingPlan?.settingsConfig?.modelCatalog?.spawnAgentModels?.slice(
        0,
        5,
      ) ?? [],
    );
    setConnectivityResults([]);
    setWizardIssues([]);
    setModelFetchCards(
      Object.fromEntries(
        providerModelSources.map((provider) => [
          provider.id,
          defaultModelFetchCardState(provider),
        ]),
      ),
    );
    dispatchFlow({
      type: "INIT",
      hasSources: providerModelSources.length > 0,
    });
  }, [existingPlan, open, planId, providerModelSources, resolvedMode]);

  // 向导打开后只同步 source 的增删；已存在 source 必须保留向导草稿。
  // 父层 provider 查询可能在用户手动选择协议、刷新模型或处理别名后完成 refetch，
  // 如果优先采用 refetch 快照，就会把这些尚未发布的草稿改动还原成数据库旧值。
  // provider 配置页会先关闭向导，重新打开时由初始化 effect 读取最新数据库配置，
  // 因此打开期间不需要用父层快照覆盖当前草稿。
  useEffect(() => {
    if (!open || !initializedOpenRef.current) return;
    setSavedPlan(existingPlan ?? null);
    setDraftSources((currentSources) => {
      const nextSourceById = new Map(
        providerModelSources.map((provider) => [provider.id, provider]),
      );
      const currentById = new Map(
        currentSources.map((provider) => [provider.id, provider]),
      );
      return selectedSourceIds
        .map(
          (providerId) =>
            currentById.get(providerId) ?? nextSourceById.get(providerId),
        )
        .filter((provider): provider is Provider => Boolean(provider));
    });
    setSelectedSourceIds((currentIds) => {
      const nextIds = currentIds.filter((providerId) =>
        providerModelSources.some((provider) => provider.id === providerId),
      );
      return nextIds.length === currentIds.length ? currentIds : nextIds;
    });
    setModelFetchCards((currentCards) =>
      Object.fromEntries(
        providerModelSources.map((provider) => [
          provider.id,
          currentCards[provider.id] ?? defaultModelFetchCardState(provider),
        ]),
      ),
    );
  }, [existingPlan, open, providerModelSources, selectedSourceIds]);

  // 选择只影响本次 MultiRouter 草稿，不修改 provider 数据库或其它已有路由方案。
  const toggleSourceProvider = (provider: Provider, checked: boolean) => {
    setSelectedSourceIds((currentIds) => {
      if (checked) {
        return currentIds.includes(provider.id)
          ? currentIds
          : [...currentIds, provider.id];
      }
      return currentIds.filter((providerId) => providerId !== provider.id);
    });
    setDraftSources((currentSources) => {
      if (checked) {
        return currentSources.some((source) => source.id === provider.id)
          ? currentSources
          : [...currentSources, provider];
      }
      return currentSources.filter((source) => source.id !== provider.id);
    });
    setConnectivityResults([]);
  };

  // 所有异步 catch 都进入同一个问题列表，让 toast 之外的 UI 也能长期展示异常和继续策略。
  const recordWizardIssue = (issue: Omit<WizardIssue, "id">) => {
    setWizardIssues((current) => [
      ...current,
      {
        ...issue,
        id: createWizardIssueId(issue.stage, issue.title),
      },
    ]);
  };

  // 重新执行某个阶段时只清理该阶段旧问题，避免旧错误误导当前判断。
  const clearWizardIssuesForStage = (stage: WizardStepKey) => {
    setWizardIssues((current) =>
      current.filter((issue) => issue.stage !== stage),
    );
  };

  // 切换最终模型池里的保留状态；第一次编辑时从当前完整列表复制一份显式顺序。
  const toggleCatalogModel = (model: string, checked: boolean) => {
    setCatalogModelOrder((current) => {
      const base = current ?? availableCatalogModels.map((item) => item.model);
      if (checked) {
        return base.includes(model) ? base : [...base, model];
      }
      setDraftSpawnAgentModels((spawnModels) =>
        spawnModels.filter((item) => item !== model),
      );
      return base.filter((item) => item !== model);
    });
  };

  // 调整最终模型列表顺序；这个顺序会写入 MultiRouter modelCatalog。
  const moveCatalogModel = (model: string, direction: -1 | 1) => {
    setCatalogModelOrder((current) =>
      moveOrderedItem(
        current ?? availableCatalogModels.map((item) => item.model),
        model,
        direction,
      ),
    );
  };

  // 关闭/跳过时记录 dismissed；首页按钮仍可再次显式打开。
  const closeWizard = (dismissed = true) => {
    if (dismissed) {
      localStorage.setItem(CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY, "true");
      dispatchFlow({ type: "DISMISS" });
    } else {
      dispatchFlow({ type: "COMPLETE" });
    }
    onOpenChange(false);
  };

  // 下一步按钮按状态机 gate 推进；配置不完整时停在当前状态并给出可操作提示。
  const advanceWizard = () => {
    switch (currentStep.key) {
      case "sources":
        if (draftSources.length === 0) {
          dispatchFlow({
            type: "NEXT",
            nextStatus: "needSources",
            nextStepKey: "sources",
          });
          toast.info("请先添加至少一个 Codex provider 作为模型源。", {
            closeButton: true,
          });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            configIssues.length > 0 ? "configIncomplete" : "readyToFetchModels",
          nextStepKey: "prepare",
        });
        if (hasUnauthenticatedCodexOAuthSources) {
          toast.warning(
            "检测到官方 Codex OAuth 源尚未登录 ChatGPT。你可以继续整理第三方模型，但官方 GPT/O 路由需要先完成 OAuth 才能真实转发。",
            {
              closeButton: true,
            },
          );
        }
        if (configIssues.length > 0) {
          toast.warning(
            "部分 provider 不能自动获取模型，将使用已有 modelCatalog 或等待你补全配置。",
            {
              closeButton: true,
            },
          );
        }
        return;
      case "prepare":
        if (
          connectivityResults.length > 0 &&
          !canContinueAfterConnectivity(connectivityResults)
        ) {
          dispatchFlow({
            type: "NEXT",
            nextStatus: "connectivityFailed",
            nextStepKey: "prepare",
          });
          recordWizardIssue({
            stage: "prepare",
            severity: "error",
            title: "Responses 连通性存在阻塞项",
            detail:
              "至少一个 Responses 直连 provider 的 /v1/responses 探测失败，继续保存会让 Codex 请求命中不可用上游。",
            canContinue: false,
          });
          toast.error(
            "连通性测试仍有阻塞项，请先修复失败的 Responses provider。",
            {
              closeButton: true,
            },
          );
          return;
        }
        setDraftSources(resolveWizardModelNameCollisions(draftSources));
        dispatchFlow({
          type: "NEXT",
          nextStatus: "routePreview",
          nextStepKey: "review",
        });
        return;
      case "review":
        if (!draftPlanName.trim()) {
          toast.error("请先填写 MultiRouter 名称。", { closeButton: true });
          return;
        }
        if (activeCatalogModelOrder.length === 0) {
          toast.error("请至少保留一个模型。", { closeButton: true });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "published",
          nextStepKey: "activate",
        });
        return;
      case "activate":
        if (savedPlan) {
          closeWizard(false);
          return;
        }
        toast.info("请点击“保存并发布”写入 MultiRouter provider。", {
          closeButton: true,
        });
        return;
      default:
        return;
    }
  };

  // 上一步只改变教程步骤和对应状态，不回滚已经抓取/保存的草稿数据。
  const retreatWizard = () => {
    const previousStep = STEPS[Math.max(0, stepIndex - 1)];
    dispatchFlow({ type: "GOTO_STEP", stepKey: previousStep.key });
  };

  // 顺序抓取所有可抓模型源；失败不阻塞其它 provider，最终由保存页继续使用已成功目录。
  const refreshModelSources = async () => {
    dispatchFlow({ type: "FETCH_START" });
    clearWizardIssuesForStage("prepare");
    const previousAvailableModels = availableCatalogModels.map(
      (model) => model.model,
    );
    let successCount = 0;
    let skippedCount = 0;
    let failedCount = 0;
    setModelFetchCards(
      Object.fromEntries(
        draftSources.map((provider) => {
          const config = getWizardModelFetchConfig(provider);
          const existingCount = readWizardModelCatalog(provider).length;
          const isCatalogOnlyPlan = isWizardCatalogOnlyModelSource(provider);
          const isCodexOAuth = isWizardCodexOAuthSource(provider);
          return [
            provider.id,
            (config && !isCatalogOnlyPlan) || isCodexOAuth
              ? {
                  status: "loading",
                  message: isCodexOAuth
                    ? "正在读取 ChatGPT OAuth 模型列表并刷新本地目录"
                    : config?.volcengineModelListAction
                      ? "正在读取火山 OpenAPI 模型列表并刷新保留目录"
                      : "正在读取 /models 并刷新保留目录",
                  modelCount: existingCount,
                }
              : {
                  status: "skipped",
                  message: isCodexOAuth
                    ? codexOAuthModelFetchMessage(
                        existingCount > 0,
                        hasCodexOauthAccount,
                      )
                    : isCatalogOnlyPlan
                      ? catalogOnlyPlanMessage(provider, existingCount > 0)
                      : "缺少 Base URL 或 API Key，无法在线读取；已保留现有模型目录。",
                  modelCount: existingCount,
                },
          ];
        }),
      ),
    );
    try {
      const nextSources: Provider[] = [];
      for (const provider of draftSources) {
        const config = getWizardModelFetchConfig(provider);
        const beforeModels = readWizardModelCatalog(provider);
        const isCatalogOnlyPlan = isWizardCatalogOnlyModelSource(provider);
        const isCodexOAuth = isWizardCodexOAuthSource(provider);
        if (isCodexOAuth) {
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: "loading",
              message: "正在读取 ChatGPT OAuth 专用模型列表...",
              modelCount: beforeModels.length,
            },
          }));
          try {
            const fetchedModels = await fetchCodexOauthModels(
              readWizardCodexOAuthAccountId(provider),
            );
            if (fetchedModels.length === 0) {
              throw new Error("ChatGPT OAuth 模型接口返回空列表");
            }
            // OAuth 上游会持续发布新模型，因此成功响应必须追加新条目；已有别名、能力和子 Agent 选择仍由合并函数保留。
            const nextProvider = mergeFetchedModelsIntoWizardProvider(
              provider,
              fetchedModels,
            );
            const afterModels = readWizardModelCatalog(nextProvider);
            const diff = diffWizardModelCatalog(beforeModels, afterModels);
            const hasDiff = hasModelFetchDiff(diff);
            await providersApi.update(nextProvider, "codex");
            nextSources.push(nextProvider);
            successCount += 1;
            setModelFetchCards((current) => ({
              ...current,
              [provider.id]: {
                status: hasDiff ? "updated" : "unchanged",
                message: hasDiff
                  ? `OAuth 目录读取成功，已写入 ${afterModels.length} 个模型。`
                  : `OAuth 目录读取成功，无模型列表更新，仍为 ${afterModels.length} 个模型。`,
                modelCount: afterModels.length,
                diff,
              },
            }));
          } catch (error) {
            const message = formatWizardError(error);
            let cacheFailureMessage: string | null = null;
            try {
              const cachedModels = await fetchCodexOauthCachedModels();
              if (cachedModels.length > 0) {
                // 在线 OAuth 目录失败时使用 Codex 本地官方缓存兜底，避免新建 official 源被写成 0 模型。
                const nextProvider = mergeFetchedModelsIntoWizardProvider(
                  provider,
                  cachedModels,
                );
                const afterModels = readWizardModelCatalog(nextProvider);
                const diff = diffWizardModelCatalog(beforeModels, afterModels);
                const hasDiff = hasModelFetchDiff(diff);
                await providersApi.update(nextProvider, "codex");
                nextSources.push(nextProvider);
                successCount += 1;
                recordWizardIssue({
                  stage: "prepare",
                  severity: "warning",
                  title: "OAuth 在线模型列表获取失败，已使用本地缓存",
                  detail: `ChatGPT OAuth 在线模型列表暂时不可用，已用本地 Codex 模型缓存恢复 ${afterModels.length} 个模型。在线错误：${message}`,
                  canContinue: true,
                  providerName: provider.name,
                });
                setModelFetchCards((current) => ({
                  ...current,
                  [provider.id]: {
                    status: hasDiff ? "updated" : "unchanged",
                    message: hasDiff
                      ? `OAuth 在线读取失败，已使用本地 Codex 模型缓存写入 ${afterModels.length} 个模型。`
                      : `OAuth 在线读取失败，已使用本地 Codex 模型缓存；无模型列表更新，仍为 ${afterModels.length} 个模型。`,
                    modelCount: afterModels.length,
                    diff,
                  },
                }));
                continue;
              }
            } catch (cacheError) {
              cacheFailureMessage = formatWizardError(cacheError);
            }
            failedCount += 1;
            nextSources.push(provider);
            recordWizardIssue({
              stage: "prepare",
              severity: "warning",
              title: "OAuth 模型列表获取失败",
              detail: `获取 ChatGPT OAuth 模型列表失败，已保留现有目录：${message}${
                cacheFailureMessage
                  ? `；本地缓存读取也失败：${cacheFailureMessage}`
                  : "；本地缓存没有可恢复的官方模型目录"
              }`,
              canContinue: true,
              providerName: provider.name,
            });
            setModelFetchCards((current) => ({
              ...current,
              [provider.id]: {
                status: "error",
                message: `OAuth 模型列表获取失败，已保留现有目录：${message}${
                  beforeModels.length === 0
                    ? "；本地缓存也没有可恢复的官方模型目录，请检查 CCSwitchMulti 全局代理或先启动 Codex 官方连接生成缓存。"
                    : ""
                }`,
                modelCount: beforeModels.length,
              },
            }));
          }
          continue;
        }
        if (isCatalogOnlyPlan) {
          skippedCount += 1;
          nextSources.push(provider);
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: "skipped",
              message: catalogOnlyPlanMessage(
                provider,
                beforeModels.length > 0,
              ),
              modelCount: beforeModels.length,
            },
          }));
          continue;
        }
        if (!config) {
          skippedCount += 1;
          nextSources.push(provider);
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: "skipped",
              message:
                "缺少 Base URL 或 API Key，无法在线读取；已保留现有模型目录。",
              modelCount: beforeModels.length,
            },
          }));
          continue;
        }
        setModelFetchCards((current) => ({
          ...current,
          [provider.id]: {
            status: "loading",
            message: `正在读取 ${fetchConfigSummary(config)}`,
            modelCount: beforeModels.length,
          },
        }));
        try {
          const fetchedModels = await fetchModelsForConfig(
            config.baseUrl,
            config.apiKey,
            config.isFullUrl,
            config.modelsUrl,
            config.customUserAgent,
            config.volcengineModelListAction
              ? {
                  action: config.volcengineModelListAction,
                  accessKeyId: config.volcengineAccessKeyId ?? "",
                  secretAccessKey: config.volcengineSecretAccessKey ?? "",
                }
              : undefined,
          );
          const nextProvider = mergeFetchedModelsIntoWizardProvider(
            provider,
            fetchedModels,
            { preserveExistingSelection: true },
          );
          const afterModels = readWizardModelCatalog(nextProvider);
          const diff = diffWizardModelCatalog(beforeModels, afterModels);
          const hasDiff = hasModelFetchDiff(diff);
          await providersApi.update(nextProvider, "codex");
          nextSources.push(nextProvider);
          successCount += 1;
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: hasDiff ? "updated" : "unchanged",
              message: hasDiff
                ? `读取成功，已写入 ${afterModels.length} 个模型。`
                : `读取成功，无模型列表更新，仍为 ${afterModels.length} 个模型。`,
              modelCount: afterModels.length,
              diff,
            },
          }));
        } catch (error) {
          console.error("[CodexMultiRouterWizard] fetch models failed", error);
          const message = formatWizardError(error);
          recordWizardIssue({
            stage: "prepare",
            severity: "warning",
            title: "模型列表获取失败",
            detail: `获取模型列表失败，请检查当前 provider 配置：${message}`,
            canContinue: true,
            providerName: provider.name,
          });
          failedCount += 1;
          nextSources.push(provider);
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: "error",
              message: `获取模型列表失败，请检查当前 provider 配置：${message}`,
              modelCount: beforeModels.length,
            },
          }));
        }
      }
      setDraftSources(nextSources);
      const nextAvailableModels = buildWizardModelCatalog(
        resolveWizardModelNameCollisions(nextSources),
      ).models.map((model) => model.model);
      setCatalogModelOrder((current) =>
        reconcileCatalogModelOrderAfterFetch(
          current,
          previousAvailableModels,
          nextAvailableModels,
        ),
      );
      setDraftSpawnAgentModels((current) => {
        const nextAvailableSet = new Set(nextAvailableModels);
        return current
          .filter((model) => nextAvailableSet.has(model))
          .slice(0, 5);
      });
      setConnectivityResults([]);
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      dispatchFlow({
        type: "FETCH_DONE",
        partial: failedCount > 0 || skippedCount > 0,
        summary: { successCount, skippedCount, failedCount },
      });
      toast.success(
        `模型列表读取完成：${successCount} 个成功，${skippedCount} 个无法读取，${failedCount} 个失败。`,
        { closeButton: true },
      );
    } catch (error) {
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "prepare",
        severity: "error",
        title: "模型列表刷新中断",
        detail: message,
        canContinue: false,
      });
      dispatchFlow({
        type: "FETCH_DONE",
        partial: true,
        summary: { successCount, skippedCount, failedCount },
      });
      toast.error(`模型列表刷新中断：${message}`, {
        closeButton: true,
      });
    }
  };

  // 对每个 provider 的每个可见模型发起 Responses + Chat 双协议探测；这是用户确认后的真实上游请求。
  const probeResponsesConnectivity = async () => {
    setIsConnectivityConfirmOpen(false);
    dispatchFlow({ type: "PROBE_START" });
    clearWizardIssuesForStage("prepare");
    const results: WizardConnectivityResult[] = [];
    for (const provider of draftSources) {
      const config = getWizardModelFetchConfig(provider);
      const models = getWizardConnectivityProbeModels(provider);
      if (isWizardCodexOAuthSource(provider)) {
        results.push(
          skippedWizardConnectivityResult(
            provider,
            hasCodexOauthAccount
              ? "官方 Codex OAuth 使用 ChatGPT 托管 token，不走普通 API Key；跳过 Chat / Responses 双协议探测，启用后到状态页用真实 Codex 请求验证"
              : "官方 Codex OAuth 尚未登录 ChatGPT，跳过普通 API Key 探测；请先在配置步骤完成 OAuth 后再验证官方路由",
          ),
        );
        continue;
      }
      if (!config || !config.apiKey) {
        results.push(
          skippedWizardConnectivityResult(
            provider,
            "缺少 Base URL 或 API Key，跳过 Chat / Responses 双协议探测",
          ),
        );
        continue;
      }
      if (models.length === 0) {
        results.push(
          skippedWizardConnectivityResult(
            provider,
            "没有可探测模型，跳过 Chat / Responses 双协议探测",
          ),
        );
        continue;
      }
      for (const model of models) {
        try {
          const responsesProbe = await probeCodexResponsesForConfig(
            config.baseUrl,
            config.apiKey,
            model,
            config.isFullUrl,
            config.customUserAgent,
          );
          const chatProbe = await probeCodexChatForConfig(
            config.baseUrl,
            config.apiKey,
            model,
            config.isFullUrl,
            config.customUserAgent,
          );
          results.push(
            classifyWizardDualProtocolConnectivityResult({
              provider,
              model,
              responses: {
                ok: responsesProbe.ok,
                detail: responsesProbe.detail,
                url: responsesProbe.url,
                httpStatus: responsesProbe.status,
              },
              chat: {
                ok: chatProbe.ok,
                detail: chatProbe.detail,
                url: chatProbe.url,
                httpStatus: chatProbe.status,
              },
            }),
          );
        } catch (error) {
          const message = formatWizardError(error);
          const classified = classifyWizardConnectivityResult({
            provider,
            model,
            ok: false,
            detail: message,
          });
          recordWizardIssue({
            stage: "prepare",
            severity: classified.canContinue ? "warning" : "error",
            title: "连通性探测命令异常",
            detail: message,
            canContinue: classified.canContinue,
            providerName: provider.name,
          });
          results.push(classified);
        }
      }
    }

    const summary = {
      passCount: results.filter((result) => result.status === "pass").length,
      warnCount: results.filter((result) => result.status === "warn").length,
      skippedCount: results.filter((result) => result.status === "skipped")
        .length,
      failCount: results.filter((result) => result.status === "fail").length,
    };
    setConnectivityResults(results);
    dispatchFlow({
      type: "PROBE_DONE",
      canContinue: canContinueAfterConnectivity(results),
      hasWarnings: summary.warnCount > 0 || summary.skippedCount > 0,
      summary,
    });
    toast.success(
      `连通性测试完成：通过 ${summary.passCount}，警告 ${summary.warnCount}，跳过 ${summary.skippedCount}，失败 ${summary.failCount}。`,
      { closeButton: true },
    );
  };

  // 保存 MultiRouter provider；这里才真正写入 DB，不会静默切换当前 Codex provider。
  const saveMultiRouterPlan = async () => {
    dispatchFlow({ type: "SAVE_START" });
    clearWizardIssuesForStage("activate");
    try {
      const routeReadySources = applyWizardConnectivityApiFormatOverrides(
        draftSources,
        connectivityResults,
      );
      const result = buildCodexMultiRouterWizardPlan(
        providers,
        routeReadySources,
        existingPlan,
        {
          planName: draftPlanName,
          catalogModelOrder: activeCatalogModelOrder,
          spawnAgentModels: activeSpawnAgentModels,
          officialAuth: draftOfficialAuth,
          hostedTools: {
            webSearch: { enabled: webSearchEnabled },
            imageGeneration: { enabled: imageGenerationEnabled },
          },
        },
      );
      let savedProvider = result.plan;
      if (existingPlan) {
        await providersApi.update(result.plan, "codex");
      } else {
        await providersApi.add(result.plan, "codex", false);
        savedProvider = await codexSubagentV2Api.initializeProviderConfig(
          result.plan.id,
        );
      }
      setSavedPlan(savedProvider);
      setDraftSources(result.sourceProviders);
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      toast.success("MultiRouter 方案已保存。", { closeButton: true });
      dispatchFlow({ type: "SAVE_SUCCESS" });
    } catch (error) {
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "activate",
        severity: "error",
        title: "MultiRouter 保存失败",
        detail: message,
        canContinue: false,
      });
      dispatchFlow({ type: "SAVE_ERROR", error: message });
      toast.error(`MultiRouter 保存失败：${message}`, { closeButton: true });
    }
  };

  // 启用动作复用 App 里的 switchProvider 路径，保证 Codex 接管和 OAuth 保留逻辑保持一致。
  const enableSavedPlan = async () => {
    if (!savedPlan) return;
    dispatchFlow({ type: "ENABLE_START" });
    clearWizardIssuesForStage("activate");
    try {
      await onEnablePlan(savedPlan);
      dispatchFlow({ type: "ENABLE_SUCCESS" });
      toast.success(
        "已启用多路模型，状态页已打开。请在 Codex 里发送一次请求；当前链路、监听、Codex 接管、路由入口和最近转发均成功后，状态页会显示真实请求验证结果。",
        {
          closeButton: true,
          duration: 12000,
        },
      );
      closeWizard(false);
    } catch (error) {
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "activate",
        severity: "error",
        title: "启用多路路由失败",
        detail: message,
        canContinue: false,
      });
      dispatchFlow({ type: "ENABLE_ERROR", error: message });
      toast.error(`启用多路路由失败：${message}`, { closeButton: true });
    }
  };

  if (!open) return null;

  const planPreview = buildCodexMultiRouterWizardPlan(
    providers,
    routeReadySources,
    existingPlan,
    {
      planName: draftPlanName,
      catalogModelOrder: activeCatalogModelOrder,
      spawnAgentModels: activeSpawnAgentModels,
      officialAuth: draftOfficialAuth,
    },
  ).plan;
  const previewRoutes = (planPreview.settingsConfig.codexRouting?.routes ??
    []) as CodexRoutingRoute[];
  const previewModels = (planPreview.settingsConfig.modelCatalog?.models ??
    []) as CodexCatalogModel[];
  const availableModelByName = new Map(
    availableCatalogModels.map((model) => [model.model, model]),
  );
  const selectModelRows = [
    ...activeCatalogModelOrder
      .map((model) => availableModelByName.get(model))
      .filter((model): model is CodexCatalogModel => Boolean(model)),
    ...availableCatalogModels.filter(
      (model) => !activeCatalogModelOrder.includes(model.model),
    ),
  ];

  return createPortal(
    <div className="fixed inset-0 z-[120] flex items-center justify-center overflow-hidden bg-black/70 p-3 text-foreground backdrop-blur-sm sm:p-4">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="codex-multirouter-wizard-title"
        data-testid="codex-multirouter-wizard-shell"
        className="flex max-h-full w-[min(96vw,1280px)] min-h-0 flex-col overflow-hidden rounded-2xl border border-border/60 bg-background shadow-2xl"
      >
        <div className="flex shrink-0 items-start justify-between border-b border-border/60 bg-gradient-to-r from-blue-500/10 via-background to-violet-500/10 px-5 py-4">
          <div className="flex items-start gap-3">
            <div className="rounded-md bg-primary/10 p-2 text-primary">
              <CurrentStepIcon className="h-5 w-5" />
            </div>
            <div>
              <div className="text-sm text-muted-foreground">
                第 {stepIndex + 1} / {STEPS.length} 步
              </div>
              <h2
                id="codex-multirouter-wizard-title"
                className="text-xl font-semibold"
              >
                {currentStep.title}
              </h2>
              <p className="mt-1 text-sm text-muted-foreground">
                {currentStep.description}
              </p>
            </div>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => closeWizard(true)}
            aria-label="关闭多路模型配置向导"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        <div
          data-testid="codex-multirouter-wizard-body"
          className="grid min-h-0 flex-1 grid-cols-[15rem_minmax(0,1fr)] overflow-hidden"
        >
          <div className="space-y-1 overflow-y-auto border-r border-border/60 bg-gradient-to-b from-blue-500/8 via-muted/25 to-violet-500/8 p-3">
            {STEPS.map((step, index) => {
              const StepIcon = step.icon;
              return (
                <button
                  key={step.key}
                  type="button"
                  className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm ${
                    index === stepIndex
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-muted"
                  }`}
                  onClick={() =>
                    dispatchFlow({ type: "GOTO_STEP", stepKey: step.key })
                  }
                >
                  <StepIcon className="h-4 w-4 shrink-0" />
                  <span className="truncate">{step.title}</span>
                </button>
              );
            })}
          </div>

          <div className="min-h-0 overflow-y-auto p-5">
            <div
              role="status"
              aria-atomic="true"
              className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border/60 bg-gradient-to-r from-blue-500/10 via-background to-violet-500/10 px-4 py-3"
            >
              <div>
                <div className="text-sm font-semibold">
                  {resolvedMode === "edit" && existingPlan
                    ? `正在编辑：${existingPlan.name}`
                    : "正在创建：新的 MultiRouter 配置"}
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {resolvedMode === "edit" && existingPlan
                    ? existingPlan.id
                    : "新配置不会覆盖已有 MultiRouter；保存后将生成独立方案。"}
                </div>
              </div>
              <Badge variant="outline">
                {resolvedMode === "edit" ? "编辑旧配置" : "创建新配置"}
              </Badge>
            </div>
            {editingTargetMissing ? (
              <div
                role="alert"
                className="mb-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
              >
                目标配置已不存在或尚未刷新。请关闭向导后重新选择要编辑的
                MultiRouter。
              </div>
            ) : null}
            <div role="status" aria-live="polite" className="sr-only">
              <span>状态机：{flowState.status}</span>
              <span>{wizardStatusText(flowState)}</span>
            </div>
            {flowState.lastError && wizardIssues.length === 0 ? (
              <div
                role="alert"
                className="mb-4 rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
              >
                {flowState.lastError}
              </div>
            ) : null}
            {wizardIssues.length > 0 && (
              <div className="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
                <div className="font-medium text-foreground">
                  已捕获问题与处理状态
                </div>
                <div className="mt-2 space-y-2">
                  {wizardIssues.map((issue) => (
                    <div
                      key={issue.id}
                      className="rounded-md border bg-background/80 p-2"
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge
                          variant={
                            issue.severity === "error"
                              ? "destructive"
                              : "outline"
                          }
                        >
                          {issue.severity === "error" ? "错误" : "警告"}
                        </Badge>
                        <span className="font-medium">{issue.title}</span>
                        {issue.providerName && (
                          <span className="text-xs text-muted-foreground">
                            {issue.providerName}
                          </span>
                        )}
                        <span className="text-xs text-muted-foreground">
                          {issue.canContinue ? "可继续" : "需处理后继续"}
                        </span>
                      </div>
                      <div className="mt-1 break-words text-xs text-muted-foreground">
                        {issue.detail}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
            {currentStep.key === "sources" && (
              <div className="space-y-4">
                <div className="rounded-xl border border-border/60 bg-gradient-to-r from-sky-500/10 via-background to-cyan-500/10 p-4 text-sm leading-6">
                  <div className="font-medium">这里只选择模型源</div>
                  <p className="mt-1 text-muted-foreground">
                    凭据、模型目录、API 协议、推理能力和工具兼容性都在各自
                    Provider 页面维护。向导只读取就绪结果并组合路由。
                  </p>
                </div>
              </div>
            )}

            {currentStep.key === "sources" && (
              <div className="space-y-4">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm text-muted-foreground">
                    已选择 {draftSources.length} / {providerModelSources.length}{" "}
                    个 Codex provider 作为本次模型源；取消选择不会删除
                    provider。
                  </p>
                  <Button onClick={onCreateProvider}>
                    <Server className="mr-2 h-4 w-4" />
                    添加 Provider
                  </Button>
                </div>
                <div className="max-h-[min(42vh,28rem)] overflow-y-auto pr-2">
                  <div className="grid gap-3 md:grid-cols-2">
                    {providerModelSources.map((provider) => (
                      <div
                        key={provider.id}
                        className="rounded-xl border border-border/60 bg-card/70 p-3 shadow-sm"
                      >
                        <label className="flex cursor-pointer items-start gap-3">
                          <input
                            type="checkbox"
                            className="mt-1 h-4 w-4"
                            checked={selectedSourceIdSet.has(provider.id)}
                            onChange={(event) =>
                              toggleSourceProvider(
                                provider,
                                event.target.checked,
                              )
                            }
                            aria-label={`使用 ${provider.name} 作为模型源`}
                          />
                          <span className="min-w-0">
                            <span className="block font-medium">
                              {provider.name}
                            </span>
                            <span className="mt-1 block text-xs text-muted-foreground">
                              {provider.id}
                            </span>
                          </span>
                        </label>
                        <div className="mt-3 flex items-center justify-between gap-3">
                          <Badge variant="outline">
                            {modelSourceSummary(provider)}
                          </Badge>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            aria-label={`配置 ${provider.name}`}
                            onClick={() => onOpenProviderConfig?.(provider)}
                          >
                            配置 Provider
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
                {draftSources.length === 0 && (
                  <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
                    状态机当前停在 NeedSources。请先添加一个普通 Codex
                    provider，或关闭向导后从已有配置导入。
                  </div>
                )}
              </div>
            )}

            {currentStep.key === "review" && (
              <div className="space-y-4">
                <div className="rounded-lg border p-4">
                  <label className="text-sm font-medium" htmlFor="plan-name">
                    MultiRouter 名称
                  </label>
                  <Input
                    id="plan-name"
                    className="mt-2"
                    value={draftPlanName}
                    onChange={(event) => setDraftPlanName(event.target.value)}
                    placeholder="例如：Codex MultiRouter - 工作主路由"
                  />
                  <p className="mt-2 text-xs leading-5 text-muted-foreground">
                    这个名称会保存到 provider
                    列表、状态页和后续启用提示里。重命名只影响 MultiRouter
                    方案本身，不会改动单个上游 provider 的名称。
                  </p>
                </div>
              </div>
            )}

            {currentStep.key === "prepare" && (
              <div className="space-y-4">
                <div className="flex flex-wrap gap-3">
                  <Button
                    onClick={refreshModelSources}
                    disabled={
                      isRefreshingModels ||
                      isProbingConnectivity ||
                      draftSources.length === 0
                    }
                  >
                    <RefreshCw
                      className={`mr-2 h-4 w-4 ${
                        isRefreshingModels ? "animate-spin" : ""
                      }`}
                    />
                    自动获取并写入模型列表
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => setIsConnectivityConfirmOpen(true)}
                    disabled={
                      isRefreshingModels ||
                      isProbingConnectivity ||
                      draftSources.length === 0
                    }
                  >
                    <Route
                      className={`mr-2 h-4 w-4 ${
                        isProbingConnectivity ? "animate-pulse" : ""
                      }`}
                    />
                    测试 Chat / Responses 连通性
                  </Button>
                </div>
                <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-900 dark:text-amber-200">
                  连通性测试会对每个 provider 的每个可见模型分别发送
                  /v1/responses 与 /v1/chat/completions 真实请求，输出上限为
                  1024。测试结果会用来判断该 provider 应走 Responses 还是 Chat
                  转换路径；通过只代表基础协议入口可用，不代表工具调用、流式输出、长上下文、多模态或真实
                  Codex 会话一定完整正常。
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                  {draftSources.map((provider) => {
                    const cardState =
                      modelFetchCards[provider.id] ??
                      defaultModelFetchCardState(provider);
                    const diffText = formatModelFetchDiff(cardState.diff);
                    return (
                      <button
                        key={provider.id}
                        type="button"
                        className="rounded-lg border p-3 text-left transition hover:border-primary/60 hover:bg-muted/40 focus:outline-none focus:ring-2 focus:ring-primary/40"
                        onClick={() => onOpenProviderConfig?.(provider)}
                        aria-label={`打开 ${provider.name} 配置页`}
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="truncate font-medium">
                              {provider.name}
                            </div>
                            <div className="mt-2 text-sm text-muted-foreground">
                              {cardState.modelCount} 个模型
                            </div>
                          </div>
                          <Badge
                            variant={modelFetchBadgeVariant(cardState.status)}
                            className="shrink-0 gap-1"
                          >
                            {cardState.status === "loading" && (
                              <RefreshCw className="h-3 w-3 animate-spin" />
                            )}
                            {modelFetchStatusLabel(cardState.status)}
                          </Badge>
                        </div>
                        <div className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">
                          {cardState.message}
                        </div>
                        {diffText && (
                          <div className="mt-2 line-clamp-2 rounded-md bg-primary/10 px-2 py-1 text-xs leading-5 text-primary">
                            {diffText}
                          </div>
                        )}
                        <div className="mt-2 text-xs text-muted-foreground">
                          点击打开 provider 配置页
                        </div>
                      </button>
                    );
                  })}
                </div>
                {connectivityResults.length > 0 && (
                  <div className="max-h-80 overflow-auto rounded-lg border">
                    {connectivityResults.map((result, index) => (
                      <div
                        key={`${result.providerId}:${result.model}:${index}`}
                        className="grid grid-cols-[7rem_1fr] gap-3 border-b px-3 py-2 text-sm last:border-b-0"
                      >
                        <Badge
                          variant={
                            result.status === "fail" ? "destructive" : "outline"
                          }
                          className="h-fit justify-center"
                        >
                          {result.status}
                        </Badge>
                        <div>
                          <div className="font-medium">
                            {result.providerName} / {result.model}
                          </div>
                          <div className="mt-1 text-xs text-muted-foreground">
                            {result.detail}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {currentStep.key === "prepare" && (
              <div className="space-y-4">
                <Button
                  variant="outline"
                  onClick={() =>
                    setDraftSources(
                      resolveWizardModelNameCollisions(draftSources),
                    )
                  }
                >
                  <ShieldAlert className="mr-2 h-4 w-4" />
                  重新计算重名别名
                </Button>
                <div className="rounded-lg border p-4 text-sm text-muted-foreground">
                  同名策略：官方/订阅模型保留原名；中转站或第三方模型显示成
                  gpt-5.4-mini-relay 这类别名，upstreamModel
                  仍指向真实上游模型名。
                </div>
                {modelCollisions.length > 0 && (
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-900 dark:text-amber-200">
                    检测到 {modelCollisions.length}{" "}
                    组上游模型重名。点击下一步时会先应用别名策略，再生成路由。
                  </div>
                )}
                <div className="max-h-72 overflow-auto rounded-lg border">
                  {previewModels.slice(0, 80).map((model) => (
                    <div
                      key={`${model.model}:${model.upstreamModel ?? ""}`}
                      className="flex items-center justify-between border-b px-3 py-2 text-sm last:border-b-0"
                    >
                      <span>{model.model}</span>
                      <span className="text-muted-foreground">
                        {model.upstreamModel &&
                        model.upstreamModel !== model.model
                          ? `上游 ${model.upstreamModel}`
                          : "原名"}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {currentStep.key === "review" && (
              <div className="space-y-4">
                <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                  这里决定最终写入 MultiRouter modelCatalog 和各 route
                  的模型。取消勾选会把旧模型或不想暴露的模型从最终路由里剔除。V4
                  Pro / Flash 子 Agent
                  角色会从可路由目录自动注册，无需在向导中手工选模型。
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() =>
                      setCatalogModelOrder(
                        availableCatalogModels.map((model) => model.model),
                      )
                    }
                  >
                    全部保留
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => {
                      setCatalogModelOrder([]);
                      setDraftSpawnAgentModels([]);
                    }}
                  >
                    全部取消
                  </Button>
                  <Badge variant="outline">
                    已保留 {activeCatalogModelOrder.length} /{" "}
                    {availableCatalogModels.length}
                  </Badge>
                </div>
                <div className="max-h-[min(50vh,34rem)] overflow-auto rounded-lg border">
                  {selectModelRows.map((model) => {
                    const kept = activeCatalogModelOrder.includes(model.model);
                    const orderIndex = activeCatalogModelOrder.indexOf(
                      model.model,
                    );
                    return (
                      <div
                        key={`${model.model}:${model.upstreamModel ?? ""}`}
                        className="grid grid-cols-[2rem_minmax(0,1fr)_8rem_5rem] items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0"
                      >
                        <input
                          type="checkbox"
                          className="h-4 w-4"
                          checked={kept}
                          onChange={(event) =>
                            toggleCatalogModel(
                              model.model,
                              event.target.checked,
                            )
                          }
                          aria-label={`保留 ${model.model}`}
                        />
                        <div className="min-w-0">
                          <div className="truncate font-medium">
                            {model.model}
                          </div>
                          <div className="truncate text-xs text-muted-foreground">
                            {model.upstreamModel &&
                            model.upstreamModel !== model.model
                              ? `上游 ${model.upstreamModel}`
                              : model.displayName || "原名"}
                          </div>
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {model.contextWindow
                            ? `${model.contextWindow} ctx`
                            : "未标注上下文"}
                        </div>
                        <div className="flex items-center gap-1">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            disabled={!kept || orderIndex <= 0}
                            onClick={() => moveCatalogModel(model.model, -1)}
                            title="上移"
                          >
                            <ArrowUp className="h-4 w-4" />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            disabled={
                              !kept ||
                              orderIndex < 0 ||
                              orderIndex >= activeCatalogModelOrder.length - 1
                            }
                            onClick={() => moveCatalogModel(model.model, 1)}
                            title="下移"
                          >
                            <ArrowDown className="h-4 w-4" />
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {currentStep.key === "review" && (
              <div className="space-y-3">
                <div className="grid gap-3 rounded-lg border bg-muted/30 p-4 md:grid-cols-2">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">
                      官方 ChatGPT 认证方式
                    </label>
                    <Select
                      value={draftOfficialAuth.mode}
                      onValueChange={(value) =>
                        setDraftOfficialAuth({
                          mode: value as CodexOfficialAuthMode,
                        })
                      }
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="desktop_current_login">
                          Codex Desktop 当前登录
                        </SelectItem>
                        <SelectItem value="managed_oauth">
                          CCSM OAuth
                        </SelectItem>
                        <SelectItem value="account_pool">
                          OAuth 账号池
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {draftOfficialAuth.mode === "managed_oauth" ? (
                    <div className="space-y-2">
                      <label className="text-sm font-medium">
                        CCSM OAuth 账号
                      </label>
                      <Select
                        value={draftOfficialAuth.accountId ?? "__default__"}
                        onValueChange={(value) =>
                          setDraftOfficialAuth({
                            mode: "managed_oauth",
                            ...(value !== "__default__"
                              ? { accountId: value }
                              : {}),
                          })
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__default__">
                            CCSM 默认账号
                          </SelectItem>
                          {draftOfficialAuth.accountId &&
                          !codexOauthAccounts.some(
                            (account) =>
                              account.id === draftOfficialAuth.accountId,
                          ) ? (
                            <SelectItem value={draftOfficialAuth.accountId}>
                              已保存账号 ({draftOfficialAuth.accountId})
                            </SelectItem>
                          ) : null}
                          {codexOauthAccounts.map((account) => (
                            <SelectItem key={account.id} value={account.id}>
                              {account.login}
                              {account.is_default ? "（默认）" : ""}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                  ) : null}
                  <div className="text-xs leading-5 text-muted-foreground md:col-span-2">
                    {draftOfficialAuth.mode === "account_pool"
                      ? "这个 MultiRouter 会按设置 > OAuth 中已启用账号池的顺序、保留额度和冷却状态选择账号。"
                      : draftOfficialAuth.mode === "managed_oauth"
                        ? "官方 route 使用 CCSM 保存的 OAuth 账号。"
                        : "官方 route 复用 Codex Desktop 当前登录。三种方式都通过 CCSM 的 HTTP Responses 接管链路，WebSocket 不参与选路。"}
                  </div>
                  {existingPlan &&
                  !existingPlan.settingsConfig?.codexRouting?.officialAuth ? (
                    <div className="rounded-md border border-amber-300 bg-amber-50 p-2 text-xs leading-5 text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/30 dark:text-amber-100 md:col-span-2">
                      这是升级前的方案，当前选择由原 route
                      绑定推断。只有点击“保存并发布”后才会写入 Router 级策略。
                    </div>
                  ) : null}
                </div>
                {previewRoutes.map((route) => (
                  <div key={route.id} className="rounded-lg border p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div className="font-medium">{route.label}</div>
                      <Badge variant="outline">
                        {route.upstream.apiFormat}
                      </Badge>
                    </div>
                    <div className="mt-2 text-sm text-muted-foreground">
                      模型 {route.match.models?.length ?? 0} 个；前缀{" "}
                      {(route.match.prefixes ?? []).join(", ") || "无"}
                    </div>
                    <div className="mt-2 text-xs leading-5 text-muted-foreground">
                      认证：
                      {route.upstream.auth.source === "native_codex_auth"
                        ? "Codex Desktop 当前登录"
                        : route.upstream.auth.source === "account_pool"
                          ? "OAuth 账号池"
                          : route.upstream.auth.source === "managed_codex_oauth"
                            ? "CCSM OAuth"
                            : "模型源凭据"}
                      ；客户端传输：HTTP Responses
                    </div>
                    <div className="mt-2 rounded-md bg-muted px-3 py-2 text-xs leading-5 text-muted-foreground">
                      {cacheCapabilitySummary(route.capabilities?.codexCache)}
                    </div>
                  </div>
                ))}
              </div>
            )}

            {currentStep.key === "activate" && (
              <div className="space-y-4">
                <div className="rounded-lg border p-4 text-sm text-muted-foreground">
                  将保存 {previewRoutes.length} 条路由和 {previewModels.length}{" "}
                  个可见模型到{" "}
                  {existingPlan ? existingPlan.name : "新的 MultiRouter"}。
                </div>
                <Button
                  onClick={saveMultiRouterPlan}
                  disabled={
                    isSavingPlan ||
                    editingTargetMissing ||
                    draftSources.length === 0 ||
                    (connectivityResults.length > 0 &&
                      !canContinueAfterConnectivity(connectivityResults))
                  }
                >
                  <Database className="mr-2 h-4 w-4" />
                  {isSavingPlan ? "正在保存..." : "保存并发布"}
                </Button>
              </div>
            )}

            {currentStep.key === "activate" && (
              <div className="space-y-4">
                <div className="rounded-lg border p-4 text-sm leading-6 text-muted-foreground">
                  保存完成后，请显式启用这个多路路由。启用成功后向导会自动关闭，并露出
                  MultiRouter 状态页；保持 CCSwitchMulti 运行，去 Codex
                  里发送一次请求。状态页会持续展示当前链路、监听、Codex
                  接管、路由入口和最近转发；五项成功后会在原地显示真实请求验证通过。
                </div>
                <div className="flex flex-wrap gap-3">
                  <Button
                    onClick={enableSavedPlan}
                    disabled={!savedPlan || isEnablingPlan}
                  >
                    <CheckCircle2 className="mr-2 h-4 w-4" />
                    启用这个多路路由
                  </Button>
                  <Button
                    variant="outline"
                    disabled={!savedPlan}
                    onClick={() => {
                      if (!savedPlan) return;
                      closeWizard(false);
                      onOpenWorkspace(savedPlan, "status");
                    }}
                  >
                    <Route className="mr-2 h-4 w-4" />
                    打开状态页继续验证
                  </Button>
                </div>
              </div>
            )}
          </div>
        </div>

        <Dialog
          open={isConnectivityConfirmOpen}
          onOpenChange={setIsConnectivityConfirmOpen}
        >
          <DialogContent className="max-w-lg" zIndex="top">
            <DialogHeader>
              <DialogTitle>确认开始连通性测试</DialogTitle>
              <DialogDescription className="space-y-2 text-left">
                <span className="block">
                  这个流程需要确认每个 provider/model 到底应该使用 Responses
                  还是 Chat
                  Completions。测试会向上游发送真实请求，可能产生少量额度或流量消耗，也可能触发限流。
                </span>
                <span className="block">
                  每个模型会分别测试 /v1/responses 和
                  /v1/chat/completions，输出上限为
                  1024。都不通时通常不是协议问题，而是 API Key、Base
                  URL、模型权限、额度、网络或上游故障。
                </span>
                <span className="block">
                  注意：Responses 通过只证明最小非流式请求能返回成功，不等于完整
                  Codex 功能验证。保存启用后仍需要在状态页和真实 Codex
                  会话里确认路由、流式响应和工具调用。
                </span>
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setIsConnectivityConfirmOpen(false)}
              >
                取消
              </Button>
              <Button type="button" onClick={probeResponsesConnectivity}>
                确认测试
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <div className="flex shrink-0 items-center justify-between border-t px-5 py-4">
          <Button variant="ghost" onClick={() => closeWizard(true)}>
            跳过
          </Button>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              onClick={retreatWizard}
              disabled={stepIndex === 0}
            >
              <ArrowLeft className="mr-2 h-4 w-4" />
              上一步
            </Button>
            <Button onClick={advanceWizard}>
              {stepIndex === STEPS.length - 1 ? "关闭" : "下一步"}
              {stepIndex !== STEPS.length - 1 && (
                <ArrowRight className="ml-2 h-4 w-4" />
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

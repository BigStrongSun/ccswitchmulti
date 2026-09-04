import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Bot,
  CheckCircle2,
  Database,
  GitBranch,
  GripVertical,
  History,
  RefreshCw,
  Route,
  Server,
  ShieldAlert,
  SlidersHorizontal,
  Wand2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type {
  CodexCatalogModel,
  CodexOfficialAuthConfig,
  CodexOfficialAuthMode,
  CodexRoutingRouteV2,
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
import { restoreCodexProviderProtocolEvidence } from "@/lib/api/protocol-compatibility";
import type { CodexMultiRouterMigrationPreview } from "@/lib/api/providers";
import {
  fetchCodexOauthCachedModels,
  fetchCodexOauthModels,
  fetchModelsForConfig,
} from "@/lib/api/model-fetch";
import {
  type CodexProviderProtocolPreflightOutcome,
  type CodexProviderSetBatchPreview,
} from "@/lib/api/protocol-compatibility";
import {
  codexProviderModelsRequiringProtocolProbe,
  createBatchCodexProtocolLabAdapter,
  providerHasAutomaticCodexModels,
  type CodexProviderSetBatchDraft,
  type CodexProviderSetBatchProbeOutcome,
} from "@/lib/protocol-lab/codex-adapters";
import {
  ProtocolLabCancelled,
  useProtocolLabWorkflow,
} from "@/lib/protocol-lab/useProtocolLabWorkflow";
import {
  CODEX_MULTI_ROUTER_DEFAULT_NAME,
  CODEX_MULTI_ROUTER_DEFAULT_ID,
  CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY,
  DEFAULT_CODEX_OFFICIAL_AUTH,
  buildCodexMultiRouterWizardPlan,
  initialWizardCatalogModelOrder,
  initialWizardSelectedSourceIds,
  buildWizardModelCatalog,
  canContinueAfterConnectivity,
  collectWizardModelNameCollisions,
  collectWizardRouteAliasSelectionIssues,
  defaultWizardModelSources,
  getWizardConnectivityProbeModels,
  getWizardConfigIssues,
  getWizardModelFetchConfig,
  isWizardCatalogOnlyModelSource,
  isWizardCodexOAuthSource,
  inferCodexOfficialAuth,
  inferWizardApiFormat,
  isCodexMultiRouterPlan,
  mergeFetchedModelsIntoWizardProvider,
  readWizardCodexOAuthAccountId,
  readWizardModelCatalog,
  readWizardProviderBaseUrl,
  resolveWizardModelNameCollisions,
  wizardRouteDisplayLabel,
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
import { CodexProtocolProbeProgressDialog } from "@/components/providers/forms/CodexProtocolProbeProgressDialog";
import { CodexProviderSetPreviewDialog } from "@/components/providers/forms/CodexProviderSetPreviewDialog";
import {
  buildWizardPageSequence,
  canEnterWizardPage,
  requiredWizardPrerequisite,
  type WizardFlowContext,
  type WizardPageKey,
} from "@/lib/codexMultiRouterWizardFlow";

interface CodexMultiRouterWizardProps {
  open: boolean;
  providers: Provider[];
  mode?: "create" | "edit";
  planId?: string;
  newlyCreatedProviderIds?: string[];
  onOpenChange: (open: boolean) => void;
  onCreateProvider: () => void;
  onOpenProviderConfig?: (provider: Provider) => void;
  onOpenHistoryRepair?: () => void;
  onOpenWorkspace: (provider: Provider, tab: WorkspaceTab) => void;
  onEnablePlan: (provider: Provider) => void | Promise<void>;
}

interface WizardStep {
  key: WizardPageKey;
  title: string;
  description: string;
  icon: typeof Wand2;
}

interface WizardIssue {
  id: string;
  stage: WizardPageKey;
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

const ALL_STEPS: WizardStep[] = [
  {
    key: "welcome",
    title: "开始配置",
    description:
      "了解向导会检查什么、何时发送测试请求，以及什么时候才会真正启用。",
    icon: Wand2,
  },
  {
    key: "inventory",
    title: "环境检查",
    description: "检查已有 Provider、MultiRouter 和当前配置范围。",
    icon: Database,
  },
  {
    key: "first-provider",
    title: "接入第一个模型源",
    description: "当前没有可用 Provider，先接入一个模型源再返回向导验证。",
    icon: Server,
  },
  {
    key: "readiness",
    title: "模型源就绪",
    description: "逐项确认认证、模型目录、协议档案、能力和可修复问题。",
    icon: ShieldAlert,
  },
  {
    key: "sources",
    title: "选择模型源",
    description: "选择已经接入的 Provider，或先添加一个新的 Codex 模型源。",
    icon: Server,
  },
  {
    key: "catalog",
    title: "同步模型目录",
    description: "读取各模型源的目录并展示差异；最终确认前只保存在本次草稿。",
    icon: RefreshCw,
  },
  {
    key: "protocol",
    title: "协议深探测",
    description:
      "完整测试 Responses 与 Chat 的流式、推理、工具调用和续接能力。",
    icon: Route,
  },
  {
    key: "models",
    title: "选择模型",
    description: "选择对 Codex 可见的模型，并处理不同模型源之间的重名。",
    icon: GitBranch,
  },
  {
    key: "model-order",
    title: "模型顺序",
    description: "单独调整 Codex 模型菜单中的显示顺序。",
    icon: ArrowDown,
  },
  {
    key: "reasoning",
    title: "推理设置",
    description: "查看推理能力与默认强度；高级覆盖仍由 Provider 配置维护。",
    icon: Wand2,
  },
  {
    key: "subagents-tools",
    title: "Sub-Agent 与工具",
    description:
      "配置基础候选模型和 Hosted Tools；高级规则进入 Sub-Agent 工作台。",
    icon: Bot,
  },
  {
    key: "routing-review",
    title: "路由确认",
    description: "只读确认路由、认证所有权、协议选择和所有待写变更。",
    icon: GitBranch,
  },
  {
    key: "save-enable",
    title: "保存并启用",
    description: "原子保存并启用 MultiRouter；成功后继续真实请求验收。",
    icon: CheckCircle2,
  },
  {
    key: "acceptance",
    title: "真实请求验收",
    description: "在 Codex 发起一次请求，并在状态页确认收到合法终止事件。",
    icon: Route,
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
  | "saveConfirmation"
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
  stepKey: WizardPageKey;
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
  | { type: "GOTO_STEP"; stepKey: WizardPageKey }
  | { type: "NEXT"; nextStatus: WizardFlowStatus; nextStepKey: WizardPageKey }
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
  stepKey: "welcome",
};

// reducer 是向导的状态机核心；所有异步动作只发事件，不直接改流程状态。
function wizardFlowReducer(
  state: WizardFlowState,
  event: WizardFlowEvent,
): WizardFlowState {
  switch (event.type) {
    case "INIT":
      return {
        status: event.hasSources ? "opened" : "needSources",
        stepKey: "welcome",
      };
    case "GOTO_STEP":
      return {
        ...state,
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
        stepKey: "catalog",
        fetchSummary: event.summary,
      };
    case "PROBE_START":
      return {
        ...state,
        status: "probingConnectivity",
        stepKey: "protocol",
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
        stepKey: event.canContinue ? "models" : "protocol",
        connectivitySummary: event.summary,
      };
    case "SAVE_START":
      return { ...state, status: "savingPlan", lastError: undefined };
    case "SAVE_SUCCESS":
      return { ...state, status: "published", stepKey: "save-enable" };
    case "SAVE_ERROR":
      return {
        ...state,
        status: "saveFailed",
        stepKey: "save-enable",
        lastError: event.error,
      };
    case "ENABLE_START":
      return { ...state, status: "enabling", lastError: undefined };
    case "ENABLE_SUCCESS":
      return { ...state, status: "enabled", stepKey: "acceptance" };
    case "ENABLE_ERROR":
      return {
        ...state,
        status: "enableFailed",
        stepKey: "save-enable",
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

function modelSourceStatusDetails(provider: Provider): string[] {
  const models = readWizardModelCatalog(provider);
  const fetchConfig = getWizardModelFetchConfig(provider);
  const auth = isWizardCodexOAuthSource(provider)
    ? "OAuth 已绑定"
    : fetchConfig?.apiKey
      ? "API Key 已配置"
      : "凭据待补全";
  const protocol = inferWizardApiFormat(provider);
  const capabilityCount = models.filter(
    (model) =>
      model.contextWindow !== undefined ||
      model.supportsImage === true ||
      model.vision === true ||
      model.textOnly !== undefined,
  ).length;
  const tools = provider.settingsConfig?.hostedTools
    ? "工具配置已声明"
    : "工具配置由 Provider 维护";
  const projection = provider.settingsConfig?.codexRouting
    ? "已有路由投影"
    : "待写入 Route 投影";
  return [
    `认证：${auth}`,
    `模型目录：${models.length} 个`,
    `协议：${protocol}`,
    `能力：${capabilityCount}/${models.length} 个模型有能力摘要`,
    `OAuth：${isWizardCodexOAuthSource(provider) ? "是" : "否"}`,
    `工具/投影：${tools}；${projection}`,
  ];
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
      return "请取消勾选失败模型，或修复后重新探测，再继续下一步。";
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
function createWizardIssueId(stage: WizardPageKey, title: string): string {
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

function protocolProbeProviderSignature(provider: Provider): string {
  return JSON.stringify({
    id: provider.id,
    category: provider.category ?? null,
    settingsConfig: provider.settingsConfig,
    meta: provider.meta ?? null,
  });
}

function skipsWizardDeepProtocolProbe(provider: Provider): boolean {
  return !providerHasAutomaticCodexModels(provider);
}

function skippedWizardDeepProbeResult(
  provider: Provider,
  detail: string,
): WizardConnectivityResult {
  const hasModels = readWizardModelCatalog(provider).some(
    (model) => model.enabled !== false,
  );
  return {
    providerId: provider.id,
    providerName: provider.name,
    model: "*",
    status: hasModels ? "skipped" : "fail",
    canContinue: hasModels,
    detail: hasModels
      ? detail
      : `${detail}；当前模型目录为空，不能生成可用路由。`,
  };
}

function deepProbeConnectivityResults(
  provider: Provider,
  outcome: CodexProviderProtocolPreflightOutcome,
): WizardConnectivityResult[] {
  if (outcome.records.length === 0) {
    return [
      skippedWizardDeepProbeResult(
        provider,
        "后端没有生成需要验证的普通 Codex 模型目标。",
      ),
    ];
  }
  return outcome.records.map((record) => {
    const verified = record.result.readiness === "verified";
    const selected = record.result.selected_transport;
    return {
      providerId: provider.id,
      providerName: provider.name,
      model: record.target.public_model,
      status: verified ? "pass" : "fail",
      canContinue: verified,
      recommendedApiFormat:
        selected === "open_ai_responses"
          ? "openai_responses"
          : selected === "open_ai_chat"
            ? "openai_chat"
            : undefined,
      detail: verified
        ? `完整 Codex 事务已验证，选择 ${
            selected === "open_ai_chat" ? "Chat Completions" : "Responses"
          }。`
        : `深度探测结果为 ${record.result.readiness}；基础响应、流式 SSE、工具调用和工具续轮没有全部通过。`,
    };
  });
}

export function buildWizardConnectivityResultsFromBatchOutcome(
  expectedProviders: Provider[],
  outcome: CodexProviderSetBatchProbeOutcome,
  hasCodexOauthAccount: boolean,
): WizardConnectivityResult[] {
  const outcomeByProviderId = new Map(
    outcome.outcomes.map((entry) => [entry.providerId, entry.outcome]),
  );
  const outcomeSourceByProviderId = new Map(
    outcome.sources.map((source) => [source.provider.id, source.provider]),
  );
  const results: WizardConnectivityResult[] = [];
  // 以用户选中的预期来源为权威集合。后端少返回一个 source 时必须形成具名失败，
  // 不能因为遍历空响应而得到 Verified/Partial/Failed 全为 0 的矛盾终态。
  for (const expectedProvider of expectedProviders) {
    const provider =
      outcomeSourceByProviderId.get(expectedProvider.id) ?? expectedProvider;
    const models = getWizardConnectivityProbeModels(provider);
    if (skipsWizardDeepProtocolProbe(provider)) {
      results.push(
        skippedWizardDeepProbeResult(
          provider,
          provider.meta?.codexProtocolMode === "manual"
            ? "该 Provider 已在高级模式锁定协议，后端不会用自动探测覆盖用户选择。"
            : hasCodexOauthAccount
              ? "官方或托管账号源固定使用 Responses，由账号管理器提供认证，不发送额外计费探测请求。"
              : "官方或托管账号源固定使用 Responses；当前尚未登录对应账号，保存后仍需先完成账号绑定。",
        ),
      );
      continue;
    }
    if (models.length === 0) {
      results.push(
        skippedWizardDeepProbeResult(
          provider,
          "没有可探测模型，不能生成深度探测请求。",
        ),
      );
      continue;
    }
    const probeOutcome = outcomeByProviderId.get(provider.id);
    if (probeOutcome) {
      results.push(...deepProbeConnectivityResults(provider, probeOutcome));
    } else {
      results.push({
        providerId: provider.id,
        providerName: provider.name,
        model: "*",
        status: "fail",
        canContinue: false,
        detail: "后端没有返回该模型源的兼容性探测结果。",
      });
    }
  }
  return results;
}

export function CodexMultiRouterWizard({
  open,
  providers,
  mode,
  planId,
  newlyCreatedProviderIds = [],
  onOpenChange,
  onCreateProvider,
  onOpenProviderConfig,
  onOpenHistoryRepair,
  onOpenWorkspace,
  onEnablePlan,
}: CodexMultiRouterWizardProps) {
  const queryClient = useQueryClient();
  const batchProtocolLabAdapter = useMemo(
    () => createBatchCodexProtocolLabAdapter(),
    [],
  );
  const batchProtocolLab = useProtocolLabWorkflow(batchProtocolLabAdapter);
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
  const [probeDialogOpen, setProbeDialogOpen] = useState(false);
  const [restoredEvidence, setRestoredEvidence] =
    useState<CodexProviderSetBatchProbeOutcome | null>(null);
  const [isRestoringEvidence, setIsRestoringEvidence] = useState(false);
  const [excludedProbeModels, setExcludedProbeModels] = useState<string[]>([]);
  const [wizardIssues, setWizardIssues] = useState<WizardIssue[]>([]);
  const [modelFetchCards, setModelFetchCards] = useState<
    Record<string, ModelFetchCardState>
  >({});
  const [migratedPlanOverride, setMigratedPlanOverride] =
    useState<Provider | null>(null);
  const [migrationPreview, setMigrationPreview] =
    useState<CodexMultiRouterMigrationPreview | null>(null);
  const [migrationError, setMigrationError] = useState<string | null>(null);
  const [isLoadingMigration, setIsLoadingMigration] = useState(false);
  const [isApplyingMigration, setIsApplyingMigration] = useState(false);
  const initializedOpenRef = useRef(false);
  const initializedTargetRef = useRef<string | null>(null);
  const draftGenerationRef = useRef(0);
  const targetGenerationRef = useRef(0);
  const latestProvidersRef = useRef(providers);
  latestProvidersRef.current = providers;
  const sourceFactsRef = useRef(new Map<string, string>());
  const committedSourceFactsRef = useRef(new Map<string, string>());
  const handledCreatedProviderIdsRef = useRef(new Set<string>());
  const createPlanIdRef = useRef<string | null>(null);
  const saveInFlightRef = useRef<Promise<void> | null>(null);
  const consumedReceiptIdsRef = useRef(new Set<string>());
  const lastProtocolLabIssueRef = useRef<string | null>(null);

  const resolvedMode =
    mode ??
    (planId || providers.some((provider) => isCodexMultiRouterPlan(provider))
      ? "edit"
      : "create");
  const storedExistingPlan = useMemo(() => {
    if (resolvedMode !== "edit") return undefined;
    return planId
      ? providers.find(
          (provider) =>
            provider.id === planId && isCodexMultiRouterPlan(provider),
        )
      : providers.find((provider) => isCodexMultiRouterPlan(provider));
  }, [planId, providers, resolvedMode]);
  const existingPlan =
    resolvedMode === "edit" &&
    migratedPlanOverride?.id === storedExistingPlan?.id
      ? (migratedPlanOverride ?? storedExistingPlan)
      : storedExistingPlan;
  const activePlan = savedPlan ?? existingPlan;
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
  const configIssues = useMemo(
    () => getWizardConfigIssues(draftSources),
    [draftSources],
  );
  const modelCollisions = useMemo(
    () => collectWizardModelNameCollisions(draftSources),
    [draftSources],
  );
  const protocolLabOutcomeById = new Map(
    [
      ...(restoredEvidence?.outcomes ?? []),
      ...(batchProtocolLab.probeOutcome?.outcomes ?? []),
    ].map((entry) => [entry.providerId, entry]),
  );
  const excludedProbeModelSet = new Set(excludedProbeModels);
  const probeModelKey = (providerId: string, model: string) =>
    `${providerId}\u0000${model}`;
  const isProbeModelExcluded = (providerId: string, model: string) =>
    excludedProbeModelSet.has(probeModelKey(providerId, model)) ||
    excludedProbeModelSet.has(probeModelKey(providerId, "*"));
  const retainedConnectivityResults = connectivityResults.filter(
    (result) => !isProbeModelExcluded(result.providerId, result.model),
  );
  const routeReadySources = draftSources
    .map((provider) => {
      const entry = protocolLabOutcomeById.get(provider.id);
      return entry &&
        protocolProbeProviderSignature(entry.inputProvider) ===
          protocolProbeProviderSignature(provider)
        ? {
            ...provider,
            settingsConfig: entry.outcome.provider.settingsConfig,
            meta: entry.outcome.provider.meta,
          }
        : provider;
    })
    .filter((provider) => !isProbeModelExcluded(provider.id, "*"))
    .map((provider) => ({
      ...provider,
      settingsConfig: {
        ...provider.settingsConfig,
        modelCatalog: {
          ...provider.settingsConfig.modelCatalog,
          models: readWizardModelCatalog(provider).map((model) =>
            isProbeModelExcluded(provider.id, model.model)
              ? { ...model, enabled: false }
              : model,
          ),
        },
      },
    }))
    .filter((provider) =>
      readWizardModelCatalog(provider).some((model) => model.enabled !== false),
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
  const flowContext: WizardFlowContext = {
    providerCount: providerModelSources.length,
    selectedSourceCount: draftSources.length,
    allSelectedSourcesReady:
      draftSources.length > 0 && configIssues.length === 0,
    catalogPrepared:
      flowState.status === "modelsFetched" ||
      flowState.status === "modelFetchPartial" ||
      availableCatalogModels.length > 0,
    protocolProbeComplete:
      retainedConnectivityResults.length > 0 &&
      canContinueAfterConnectivity(retainedConnectivityResults),
    hasVisibleModels: activeCatalogModelOrder.length > 0,
    planSaved: Boolean(savedPlan),
    planEnabled:
      flowState.status === "enabled" ||
      flowState.status === "completed" ||
      activePlan?.settingsConfig?.codexRouting?.enabled === true,
    acceptanceStatus: "waiting",
  };
  const pageSequence = buildWizardPageSequence(flowContext);
  const steps = ALL_STEPS.filter((step) => pageSequence.includes(step.key));
  const stepIndex = steps.findIndex((step) => step.key === flowState.stepKey);
  // 防御旧状态或条件页消失后的异常跳转，确保向导始终有可渲染页面。
  const currentStep = steps[stepIndex] ?? steps[0];
  const CurrentStepIcon = currentStep.icon;
  const currentStepPrerequisiteKey = requiredWizardPrerequisite(
    currentStep.key,
    flowContext,
  );
  const currentStepPrerequisite = currentStepPrerequisiteKey
    ? steps.find((step) => step.key === currentStepPrerequisiteKey)
    : undefined;
  const isRefreshingModels = flowState.status === "fetchingModels";
  const isProbingConnectivity = batchProtocolLab.state.phase === "probing";
  const isSavingPlan = flowState.status === "savingPlan";
  const isEnablingPlan = flowState.status === "enabling";

  useEffect(() => {
    if (!open) {
      setMigratedPlanOverride(null);
      setMigrationPreview(null);
      setMigrationError(null);
      return;
    }
    if (
      resolvedMode !== "edit" ||
      !storedExistingPlan ||
      storedExistingPlan.settingsConfig?.codexRouting?.schemaVersion === 2 ||
      migratedPlanOverride
    ) {
      return;
    }
    let cancelled = false;
    setIsLoadingMigration(true);
    setMigrationError(null);
    void providersApi
      .getCodexMultiRouterRevision(storedExistingPlan.id)
      .then((revision) =>
        providersApi.previewCodexMultiRouterMigration(
          storedExistingPlan.id,
          revision,
        ),
      )
      .then((preview) => {
        if (!cancelled) setMigrationPreview(preview);
      })
      .catch((error) => {
        if (!cancelled) setMigrationError(formatWizardError(error));
      })
      .finally(() => {
        if (!cancelled) setIsLoadingMigration(false);
      });
    return () => {
      cancelled = true;
    };
  }, [migratedPlanOverride, open, resolvedMode, storedExistingPlan]);

  const applyLegacyMigration = async () => {
    if (!migrationPreview || !storedExistingPlan) return;
    setIsApplyingMigration(true);
    setMigrationError(null);
    try {
      await providersApi.applyCodexMultiRouterMigration(
        storedExistingPlan.id,
        migrationPreview.expectedRevision,
        migrationPreview.planToken,
      );
      const refreshed = await providersApi.getAll("codex");
      const migrated = refreshed[storedExistingPlan.id];
      if (migrated?.settingsConfig?.codexRouting?.schemaVersion !== 2) {
        throw new Error("migration_readback_failed");
      }
      initializedOpenRef.current = false;
      setMigratedPlanOverride(migrated);
      setMigrationPreview(null);
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    } catch (error) {
      setMigrationError(formatWizardError(error));
    } finally {
      setIsApplyingMigration(false);
    }
  };

  // 每次打开向导只初始化一次。父组件 rerender 会传入新的 providers 数组，不能因此把用户从第 2 步重置回第 1 步。
  useEffect(() => {
    if (!open) {
      return;
    }
    const targetKey = `${resolvedMode}:${planId ?? storedExistingPlan?.id ?? "new"}`;
    if (initializedTargetRef.current !== targetKey)
      initializedOpenRef.current = false;
    if (initializedOpenRef.current) return;

    initializedOpenRef.current = true;
    draftGenerationRef.current += 1;
    targetGenerationRef.current += 1;
    initializedTargetRef.current = targetKey;
    handledCreatedProviderIdsRef.current.clear();
    committedSourceFactsRef.current.clear();
    sourceFactsRef.current = new Map(
      providerModelSources.map((provider) => [
        provider.id,
        protocolProbeProviderSignature(provider),
      ]),
    );
    setExcludedProbeModels([]);
    if (existingPlan) {
      createPlanIdRef.current = existingPlan.id;
    } else {
      const defaultId = CODEX_MULTI_ROUTER_DEFAULT_ID;
      createPlanIdRef.current = providers.some(
        (provider) => provider.id === defaultId,
      )
        ? `${defaultId}-${Date.now()}`
        : defaultId;
    }
    const initialSourceIds = initialWizardSelectedSourceIds(
      existingPlan,
      providerModelSources,
    );
    const initialSourceIdSet = new Set(initialSourceIds);
    setSavedPlan(existingPlan ?? null);
    setDraftSources(
      providerModelSources.filter((provider) =>
        initialSourceIdSet.has(provider.id),
      ),
    );
    setSelectedSourceIds(initialSourceIds);
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
      initialWizardCatalogModelOrder(existingPlan, providerModelSources),
    );
    setDraftSpawnAgentModels(
      existingPlan?.settingsConfig?.codexRouting?.schemaVersion === 2
        ? (existingPlan.settingsConfig.codexRouting.spawnAgentModels?.slice(
            0,
            5,
          ) ?? [])
        : (existingPlan?.settingsConfig?.modelCatalog?.spawnAgentModels?.slice(
            0,
            5,
          ) ?? []),
    );
    setConnectivityResults([]);
    setRestoredEvidence(null);
    batchProtocolLab.reset();
    if (existingPlan?.settingsConfig?.codexRouting?.schemaVersion === 2) {
      const generation = draftGenerationRef.current;
      const restoreTargetGeneration = targetGenerationRef.current;
      const initialSources = providerModelSources.filter((source) =>
        initialSourceIdSet.has(source.id),
      );
      setIsRestoringEvidence(true);
      void Promise.all(
        initialSources.map(async (provider) => {
          if (skipsWizardDeepProtocolProbe(provider)) return null;
          try {
            const outcome =
              await restoreCodexProviderProtocolEvidence(provider);
            return outcome
              ? { providerId: provider.id, inputProvider: provider, outcome }
              : null;
          } catch {
            // Missing/old evidence must leave the gate closed, never trigger paid requests.
            return null;
          }
        }),
      ).then((entries) => {
        if (targetGenerationRef.current === restoreTargetGeneration)
          setIsRestoringEvidence(false);
        if (draftGenerationRef.current !== generation) return;
        const outcomes = entries.filter(
          (entry): entry is NonNullable<typeof entry> => entry !== null,
        );
        const restored: CodexProviderSetBatchProbeOutcome = {
          outcomes,
          sources: initialSources.map((provider) => ({
            provider,
            receiptIds:
              outcomes.find((entry) => entry.providerId === provider.id)
                ?.outcome.receiptIds ?? [],
          })),
        };
        setRestoredEvidence(restored);
        setConnectivityResults(
          buildWizardConnectivityResultsFromBatchOutcome(
            initialSources,
            restored,
            hasCodexOauthAccount,
          ).map((result) =>
            result.canContinue
              ? result
              : {
                  ...result,
                  detail:
                    "未找到可复用的完整证据（可能缺失、过期或配置已改变），请重新探测该来源。",
                },
          ),
        );
        setIsRestoringEvidence(false);
      });
    } else {
      setIsRestoringEvidence(false);
    }
    setProbeDialogOpen(false);
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
      hasSources: initialSourceIds.length > 0,
    });
  }, [existingPlan, open, planId, providerModelSources, resolvedMode]);

  // Add Provider 对话框关闭后，App 会把本次新增 ID 传回。向导只选中真实新增项，
  // 不根据名称或数组顺序猜测，并回到就绪检查展示下一步。
  useEffect(() => {
    if (!open || newlyCreatedProviderIds.length === 0) return;
    const createdIdSet = new Set(
      newlyCreatedProviderIds.filter(
        (id) => !handledCreatedProviderIdsRef.current.has(id),
      ),
    );
    const createdProviders = providerModelSources.filter((provider) =>
      createdIdSet.has(provider.id),
    );
    if (createdProviders.length === 0) return;
    // Integrating a newly created source changes the probe target set too.
    // A pending restore for the previous set must not reopen the later steps.
    draftGenerationRef.current += 1;
    setIsRestoringEvidence(false);
    createdProviders.forEach((provider) =>
      handledCreatedProviderIdsRef.current.add(provider.id),
    );
    setSelectedSourceIds((currentIds) => [
      ...new Set([
        ...currentIds,
        ...createdProviders.map((provider) => provider.id),
      ]),
    ]);
    setDraftSources((currentSources) => {
      const byId = new Map(
        [...currentSources, ...createdProviders].map((provider) => [
          provider.id,
          provider,
        ]),
      );
      return [...byId.values()];
    });
    setConnectivityResults([]);
    batchProtocolLab.reset();
    dispatchFlow({ type: "GOTO_STEP", stepKey: "readiness" });
  }, [newlyCreatedProviderIds, open, providerModelSources]);

  // Provider 是模型事实的唯一来源。向导打开期间也必须采用父层查询的最新快照，
  // 否则 Provider 新增模型、上下文或能力变化只会在关闭并重开向导后出现。
  // 保留未保存的目录草稿，但外部事实变化必须使对应证据失效。
  useEffect(() => {
    if (!open || !initializedOpenRef.current) return;
    setSavedPlan((currentPlan) => existingPlan ?? currentPlan);
    const changedIds = new Set(
      providerModelSources
        .filter((provider) => {
          const previous = sourceFactsRef.current.get(provider.id);
          const next = protocolProbeProviderSignature(provider);
          if (previous === next) return false;
          // Only the exact atomic-save response is an expected change. Keep the
          // observed baseline until refetch arrives so old query renders remain safe.
          const committed = committedSourceFactsRef.current.get(provider.id);
          committedSourceFactsRef.current.delete(provider.id);
          return previous !== undefined && next !== committed;
        })
        .map((provider) => provider.id),
    );
    sourceFactsRef.current = new Map(
      providerModelSources.map((provider) => [
        provider.id,
        protocolProbeProviderSignature(provider),
      ]),
    );
    if (changedIds.size > 0) {
      draftGenerationRef.current += 1;
      if (isProbingConnectivity) batchProtocolLab.reset();
      if (isRefreshingModels) {
        dispatchFlow({
          type: "FETCH_DONE",
          partial: true,
          summary: {
            successCount: 0,
            skippedCount: 0,
            failedCount: changedIds.size,
          },
        });
      }
      setConnectivityResults((current) =>
        current
          .filter((result) => !changedIds.has(result.providerId))
          .concat(
            providerModelSources
              .filter(
                (provider) =>
                  changedIds.has(provider.id) &&
                  selectedSourceIds.includes(provider.id),
              )
              .map((provider) => ({
                providerId: provider.id,
                providerName: provider.name,
                model: "*",
                status: "fail" as const,
                canContinue: false,
                detail:
                  "模型源配置已修改，旧探测结果已失效；请重新探测该来源或取消保留该来源。",
              })),
          ),
      );
    }
    setDraftSources((currentSources) => {
      const nextSourceById = new Map(
        providerModelSources.map((provider) => [provider.id, provider]),
      );
      return selectedSourceIds
        .map((providerId) => {
          const latest = nextSourceById.get(providerId);
          const retained = currentSources.find(
            (provider) => provider.id === providerId,
          );
          if (!latest || !retained || changedIds.has(providerId)) return latest;
          return {
            ...latest,
            settingsConfig: retained.settingsConfig,
            meta: retained.meta,
          };
        })
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
    draftGenerationRef.current += 1;
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
    batchProtocolLab.reset();
  };

  // 所有异步 catch 都进入同一个问题列表，让 toast 之外的 UI 也能长期展示异常和继续策略。
  const recordWizardIssue = (issue: Omit<WizardIssue, "id">) => {
    setWizardIssues((current) => [
      ...current.filter(
        (existing) =>
          existing.stage !== issue.stage ||
          existing.title !== issue.title ||
          existing.providerName !== issue.providerName,
      ),
      {
        ...issue,
        id: createWizardIssueId(issue.stage, issue.title),
      },
    ]);
  };

  // 重新执行某个阶段时只清理该阶段旧问题，避免旧错误误导当前判断。
  const clearWizardIssuesForStage = (stage: WizardPageKey) => {
    setWizardIssues((current) =>
      current.filter((issue) => issue.stage !== stage),
    );
  };

  useEffect(() => {
    if (
      batchProtocolLab.state.phase !== "failed" &&
      batchProtocolLab.state.phase !== "blocked"
    ) {
      lastProtocolLabIssueRef.current = null;
      return;
    }
    const message =
      batchProtocolLab.state.phase === "blocked"
        ? "至少一个模型源尚未通过完整 Codex 事务验证。"
        : batchProtocolLab.state.errorDetail || "兼容性工作流执行失败。";
    const signature = `${batchProtocolLab.state.phase}:${batchProtocolLab.state.pendingIntent}:${message}`;
    if (lastProtocolLabIssueRef.current === signature) return;
    lastProtocolLabIssueRef.current = signature;

    if (batchProtocolLab.state.pendingIntent === "validate") {
      const failedResults = (
        batchProtocolLab.state.draft?.sources ?? []
      ).map<WizardConnectivityResult>((source) => ({
        providerId: source.provider.id,
        providerName: source.provider.name,
        model: "*",
        status: "fail",
        canContinue: false,
        detail: `兼容性深度探测中断：${message}`,
      }));
      setConnectivityResults(failedResults);
      dispatchFlow({
        type: "PROBE_DONE",
        canContinue: false,
        hasWarnings: false,
        summary: {
          passCount: 0,
          warnCount: 0,
          skippedCount: 0,
          failCount: Math.max(1, failedResults.length),
        },
      });
      recordWizardIssue({
        stage: "protocol",
        severity: "error",
        title: "兼容性深度探测中断",
        detail: message,
        canContinue: false,
      });
      return;
    }

    dispatchFlow({ type: "SAVE_ERROR", error: message });
    recordWizardIssue({
      stage: "save-enable",
      severity: "error",
      title: "MultiRouter 保存失败",
      detail: message,
      canContinue: false,
    });
    if (batchProtocolLab.state.phase === "failed") {
      toast.error(`MultiRouter 保存失败：${message}`, { closeButton: true });
    }
  }, [
    batchProtocolLab.state.draft,
    batchProtocolLab.state.errorDetail,
    batchProtocolLab.state.pendingIntent,
    batchProtocolLab.state.phase,
  ]);

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

  // 调整最终模型选择与顺序；schema v2 只把 all/include 策略写入 Router。
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
    } else {
      dispatchFlow({ type: "COMPLETE" });
    }
    onOpenChange(false);
  };

  const openWizardStep = (step: WizardStep) => {
    dispatchFlow({ type: "GOTO_STEP", stepKey: step.key });
    const prerequisiteKey = requiredWizardPrerequisite(step.key, flowContext);
    if (!prerequisiteKey) return;
    const prerequisite = steps.find((item) => item.key === prerequisiteKey);
    toast.info(
      `“${step.title}”暂不可编辑，请先完成“${prerequisite?.title ?? "前置步骤"}”。`,
      { closeButton: true },
    );
  };

  // 下一步按钮按状态机 gate 推进；配置不完整时停在当前状态并给出可操作提示。
  const advanceWizard = () => {
    switch (currentStep.key) {
      case "welcome":
        dispatchFlow({
          type: "NEXT",
          nextStatus: "opened",
          nextStepKey: "inventory",
        });
        return;
      case "inventory":
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            providerModelSources.length > 0
              ? "reviewProviderConfig"
              : "needSources",
          nextStepKey:
            providerModelSources.length > 0 ? "readiness" : "first-provider",
        });
        return;
      case "first-provider":
        if (providerModelSources.length === 0) {
          toast.info("请先添加至少一个 Codex 模型源。", { closeButton: true });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "reviewProviderConfig",
          nextStepKey: "readiness",
        });
        return;
      case "readiness":
        if (providerModelSources.length === 0) {
          dispatchFlow({ type: "GOTO_STEP", stepKey: "first-provider" });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "reviewProviderConfig",
          nextStepKey: "sources",
        });
        return;
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
          nextStepKey: "catalog",
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
      case "catalog":
        if (availableCatalogModels.length === 0) {
          toast.error("请先同步出至少一个可用模型。", { closeButton: true });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "readyToFetchModels",
          nextStepKey: "protocol",
        });
        return;
      case "protocol":
        if (
          flowState.connectivitySummary === undefined ||
          connectivityResults.length === 0
        ) {
          toast.info(
            "请先运行协议深探测；已有协议配置不能代替本次向导的真实事务测试。",
            { closeButton: true },
          );
          return;
        }
        if (
          (connectivityResults.length > 0 &&
            !canContinueAfterConnectivity(retainedConnectivityResults)) ||
          retainedConnectivityResults.length === 0
        ) {
          dispatchFlow({
            type: "NEXT",
            nextStatus: "connectivityFailed",
            nextStepKey: "protocol",
          });
          recordWizardIssue({
            stage: "protocol",
            severity: "error",
            title: "兼容性深度探测存在阻塞项",
            detail:
              "仍保留了未通过校验的模型。请取消勾选失败项，或修复后重新探测；至少保留一个通过校验的模型。",
            canContinue: false,
          });
          toast.error("请取消勾选失败模型，或修复后重新探测，再继续。", {
            closeButton: true,
          });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "routePreview",
          nextStepKey: "models",
        });
        return;
      case "models":
        if (activeCatalogModelOrder.length === 0) {
          toast.error("请至少保留一个模型。", { closeButton: true });
          return;
        }
        dispatchFlow({ type: "GOTO_STEP", stepKey: "model-order" });
        return;
      case "model-order":
        dispatchFlow({ type: "GOTO_STEP", stepKey: "reasoning" });
        return;
      case "reasoning":
        dispatchFlow({ type: "GOTO_STEP", stepKey: "subagents-tools" });
        return;
      case "subagents-tools":
        dispatchFlow({ type: "GOTO_STEP", stepKey: "routing-review" });
        return;
      case "routing-review":
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
          nextStepKey: "save-enable",
        });
        return;
      case "save-enable":
        if (flowState.status === "enabled") {
          dispatchFlow({ type: "GOTO_STEP", stepKey: "acceptance" });
          return;
        }
        toast.info("请先保存方案并启用 MultiRouter。", {
          closeButton: true,
        });
        return;
      case "acceptance":
        toast.info("请在 Codex 发送一次真实请求，并在状态页完成验收。", {
          closeButton: true,
        });
        return;
      default:
        return;
    }
  };

  // 上一步只改变教程步骤和对应状态，不回滚已经抓取/保存的草稿数据。
  const retreatWizard = () => {
    const previousStep = steps[Math.max(0, stepIndex - 1)];
    dispatchFlow({ type: "GOTO_STEP", stepKey: previousStep.key });
  };

  // 顺序抓取所有可抓模型源；失败不阻塞其它 provider，最终由保存页继续使用已成功目录。
  const refreshModelSources = async () => {
    const isCurrent = captureDraftOperation();
    dispatchFlow({ type: "FETCH_START" });
    clearWizardIssuesForStage("catalog");
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
            if (!isCurrent()) return;
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
            nextSources.push(nextProvider);
            successCount += 1;
            setModelFetchCards((current) => ({
              ...current,
              [provider.id]: {
                status: hasDiff ? "updated" : "unchanged",
                message: hasDiff
                  ? `OAuth 目录读取成功，已载入草稿 ${afterModels.length} 个模型。`
                  : `OAuth 目录读取成功，无模型列表更新，仍为 ${afterModels.length} 个模型。`,
                modelCount: afterModels.length,
                diff,
              },
            }));
          } catch (error) {
            if (!isCurrent()) return;
            const message = formatWizardError(error);
            let cacheFailureMessage: string | null = null;
            try {
              const cachedModels = await fetchCodexOauthCachedModels();
              if (!isCurrent()) return;
              if (cachedModels.length > 0) {
                // 在线 OAuth 目录失败时使用 Codex 本地官方缓存兜底，避免新建 official 源被写成 0 模型。
                const nextProvider = mergeFetchedModelsIntoWizardProvider(
                  provider,
                  cachedModels,
                );
                const afterModels = readWizardModelCatalog(nextProvider);
                const diff = diffWizardModelCatalog(beforeModels, afterModels);
                const hasDiff = hasModelFetchDiff(diff);
                nextSources.push(nextProvider);
                successCount += 1;
                recordWizardIssue({
                  stage: "catalog",
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
                      ? `OAuth 在线读取失败，已使用本地 Codex 模型缓存载入草稿 ${afterModels.length} 个模型。`
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
              stage: "catalog",
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
          if (!isCurrent()) return;
          const nextProvider = mergeFetchedModelsIntoWizardProvider(
            provider,
            fetchedModels,
            { preserveExistingSelection: true },
          );
          const afterModels = readWizardModelCatalog(nextProvider);
          const diff = diffWizardModelCatalog(beforeModels, afterModels);
          const hasDiff = hasModelFetchDiff(diff);
          nextSources.push(nextProvider);
          successCount += 1;
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: hasDiff ? "updated" : "unchanged",
              message: hasDiff
                ? `读取成功，已载入草稿 ${afterModels.length} 个模型。`
                : `读取成功，无模型列表更新，仍为 ${afterModels.length} 个模型。`,
              modelCount: afterModels.length,
              diff,
            },
          }));
        } catch (error) {
          if (!isCurrent()) return;
          console.error("[CodexMultiRouterWizard] fetch models failed", error);
          const message = formatWizardError(error);
          recordWizardIssue({
            stage: "catalog",
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
      if (!isCurrent()) return;
      setDraftSources(
        nextSources.map((source) => ({
          ...latestProvidersRef.current.find(
            (provider) => provider.id === source.id,
          ),
          ...source,
          name:
            latestProvidersRef.current.find(
              (provider) => provider.id === source.id,
            )?.name ?? source.name,
        })),
      );
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
      batchProtocolLab.reset();
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
      if (!isCurrent()) return;
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "catalog",
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

  const buildBatchDraft = (
    reuseReceipts: boolean,
  ): CodexProviderSetBatchDraft => {
    const result = buildCodexMultiRouterWizardPlan(
      providers,
      routeReadySources,
      activePlan,
      {
        planId: activePlan?.id ?? createPlanIdRef.current ?? undefined,
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
    const previousSources = new Map(
      [
        ...(restoredEvidence?.sources ?? []),
        ...(batchProtocolLab.state.draft?.sources ?? []),
      ].map((source) => [source.provider.id, source]),
    );
    const currentInputSources = new Map(
      draftSources.map((source) => [source.id, source]),
    );
    return {
      sources: routeReadySources.map((source) => ({
        provider: source,
        receiptIds:
          reuseReceipts &&
          protocolLabOutcomeById.has(source.id) &&
          protocolProbeProviderSignature(
            protocolLabOutcomeById.get(source.id)!.inputProvider,
          ) ===
            protocolProbeProviderSignature(
              currentInputSources.get(source.id) ?? source,
            )
            ? (previousSources.get(source.id)?.receiptIds ?? []).filter(
                (_, index) => {
                  const record = protocolLabOutcomeById.get(source.id)?.outcome
                    .records[index];
                  return (
                    record &&
                    !consumedReceiptIdsRef.current.has(
                      (previousSources.get(source.id)?.receiptIds ?? [])[index],
                    ) &&
                    !isProbeModelExcluded(source.id, record.target.public_model)
                  );
                },
              )
            : [],
      })),
      router: result.plan,
    };
  };

  const applyConnectivityProbeOutcome = (
    outcome: CodexProviderSetBatchProbeOutcome,
  ) => {
    const results = buildWizardConnectivityResultsFromBatchOutcome(
      draftSources,
      outcome,
      hasCodexOauthAccount,
    );
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

  function captureDraftOperation() {
    const generation = draftGenerationRef.current;
    const inputFacts = new Map(
      draftSources.map((source) => [
        source.id,
        providers.find((provider) => provider.id === source.id),
      ]),
    );
    return () =>
      draftGenerationRef.current === generation &&
      [...inputFacts].every(([id, original]) => {
        const latest = latestProvidersRef.current.find(
          (provider) => provider.id === id,
        );
        return (
          original &&
          latest &&
          protocolProbeProviderSignature(original) ===
            protocolProbeProviderSignature(latest)
        );
      });
  }

  const requestConnectivityProbe = () => {
    draftGenerationRef.current += 1;
    setIsRestoringEvidence(false);
    const isCurrent = captureDraftOperation();
    clearWizardIssuesForStage("protocol");
    const validation = batchProtocolLab.validate(buildBatchDraft(false));
    void validation
      .then((outcome) => {
        if (isCurrent()) applyConnectivityProbeOutcome(outcome);
      })
      .catch((error) => {
        if (!(error instanceof ProtocolLabCancelled)) {
          console.error("Wizard protocol validation failed", error);
        }
      });
  };

  const confirmConnectivityProbe = () => {
    if (batchProtocolLab.state.pendingIntent === "validate") {
      dispatchFlow({ type: "PROBE_START" });
    }
    setProbeDialogOpen(
      batchProtocolLab.state.draft
        ? batchProtocolLabAdapter.requiresProbe(
            batchProtocolLab.state.draft,
            [],
          )
        : false,
    );
    void batchProtocolLab.confirmProbe();
  };

  // 保存只调用统一 batch workflow：自动 Single/Split 都由后端规划并以 accept_auto
  // 原子提交；Blocked 保留草稿和预览，不再进入第二套 Split 确认状态。
  const saveMultiRouterPlan = () => {
    if (saveInFlightRef.current) return;
    const targetGeneration = targetGenerationRef.current;
    dispatchFlow({ type: "SAVE_START" });
    clearWizardIssuesForStage("save-enable");
    let draft: CodexProviderSetBatchDraft;
    try {
      draft = buildBatchDraft(true);
    } catch (error) {
      const message = formatWizardError(error);
      dispatchFlow({ type: "SAVE_ERROR", error: message });
      recordWizardIssue({
        stage: "save-enable",
        severity: "error",
        title: "MultiRouter 保存失败",
        detail: message,
        canContinue: false,
      });
      return;
    }
    const saveOperation = (async () => {
      // A successful commit consumes its receipts. Re-editing the same draft
      // must restore persisted evidence instead of spending tokens again.
      draft.sources = await Promise.all(
        draft.sources.map(async (source) => {
          if (
            source.receiptIds.length ||
            skipsWizardDeepProtocolProbe(source.provider)
          )
            return source;
          const evidence = await restoreCodexProviderProtocolEvidence(
            source.provider,
          );
          return evidence
            ? { ...source, receiptIds: evidence.receiptIds }
            : source;
        }),
      );
      if (targetGeneration !== targetGenerationRef.current)
        throw new ProtocolLabCancelled();
      return batchProtocolLab.save(
        draft,
        draft.sources.flatMap((source) => source.receiptIds),
      );
    })()
      .then(async (outcome) => {
        draft.sources
          .flatMap((source) => source.receiptIds)
          .forEach((id) => consumedReceiptIdsRef.current.add(id));
        if (targetGeneration !== targetGenerationRef.current) return;
        setSavedPlan(outcome.router);
        committedSourceFactsRef.current = new Map(
          outcome.sourceSnapshots.map((snapshot) => [
            snapshot.logicalProvider.id,
            protocolProbeProviderSignature(snapshot.logicalProvider),
          ]),
        );
        setDraftSources(
          outcome.sourceSnapshots.length > 0
            ? outcome.sourceSnapshots.map(
                (snapshot) => snapshot.logicalProvider,
              )
            : draft.sources.map((source) => source.provider),
        );
        await queryClient.invalidateQueries({
          queryKey: ["providers", "codex"],
        });
        if (targetGeneration !== targetGenerationRef.current) return;
        if (outcome.status === "committed_with_projection_error") {
          toast.warning(
            "模型源和 MultiRouter 已原子保存，但当前 Codex 投影刷新失败；请在状态页重新激活。",
            { closeButton: true },
          );
        } else {
          toast.success("模型源与 MultiRouter 已一次性保存。", {
            closeButton: true,
          });
        }
        dispatchFlow({ type: "SAVE_SUCCESS" });
      })
      .catch((error) => {
        if (targetGeneration !== targetGenerationRef.current) return;
        if (error instanceof ProtocolLabCancelled) return;
        const message = formatWizardError(error);
        dispatchFlow({ type: "SAVE_ERROR", error: message });
      });
    saveInFlightRef.current = saveOperation.finally(() => {
      saveInFlightRef.current = null;
    });
  };

  const returnFromBlockedBatch = () => {
    batchProtocolLab.cancel();
    dispatchFlow({ type: "GOTO_STEP", stepKey: "routing-review" });
  };

  const retryBlockedBatchProbe = () => {
    setConnectivityResults([]);
    dispatchFlow({ type: "GOTO_STEP", stepKey: "protocol" });
    setProbeDialogOpen(true);
    batchProtocolLab.retry();
  };

  // 启用动作复用 App 里的 switchProvider 路径，保证 Codex 接管和 OAuth 保留逻辑保持一致。
  const enableSavedPlan = async () => {
    if (!savedPlan) return;
    const targetGeneration = targetGenerationRef.current;
    dispatchFlow({ type: "ENABLE_START" });
    clearWizardIssuesForStage("save-enable");
    try {
      await onEnablePlan(savedPlan);
      if (targetGeneration !== targetGenerationRef.current) return;
      dispatchFlow({ type: "ENABLE_SUCCESS" });
      toast.success(
        "已启用多路模型。你可以继续修复历史记录，或进入各自独立的模型、Sub-Agent 与排序设置。",
        {
          closeButton: true,
          duration: 12000,
        },
      );
    } catch (error) {
      if (targetGeneration !== targetGenerationRef.current) return;
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "save-enable",
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

  const planPreviewResult = buildCodexMultiRouterWizardPlan(
    providers,
    routeReadySources,
    activePlan,
    {
      planId: activePlan?.id ?? createPlanIdRef.current ?? undefined,
      planName: draftPlanName,
      catalogModelOrder: activeCatalogModelOrder,
      spawnAgentModels: activeSpawnAgentModels,
      officialAuth: draftOfficialAuth,
    },
  );
  const planPreview = planPreviewResult.plan;
  const previewRoutes = (planPreview.settingsConfig.codexRouting?.routes ??
    []) as CodexRoutingRouteV2[];
  const previewModels = buildWizardModelCatalog(
    resolveWizardModelNameCollisions(planPreviewResult.sourceProviders),
    { catalogModelOrder: activeCatalogModelOrder },
  ).models;
  const aliasSelectionIssues = collectWizardRouteAliasSelectionIssues(
    previewRoutes,
    routeReadySources,
  );
  const pendingSourcePreview =
    batchProtocolLab.state.phase === "blocked"
      ? ((
          batchProtocolLab.state
            .preparePreview as CodexProviderSetBatchPreview | null
        )?.sourcePreviews.find((preview) => preview.plan.kind === "blocked") ??
        null)
      : null;
  const previewProvidersById = new Map(
    routeReadySources.map((provider) => [provider.id, provider]),
  );
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
                第 {stepIndex + 1} / {steps.length} 步
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
            {steps.map((step, index) => {
              const StepIcon = step.icon;
              const canEditStep = canEnterWizardPage(step.key, flowContext);
              return (
                <button
                  key={step.key}
                  type="button"
                  className={`flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm ${
                    index === stepIndex
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-muted"
                  }`}
                  onClick={() => openWizardStep(step)}
                  aria-current={index === stepIndex ? "step" : undefined}
                  data-read-only={canEditStep ? undefined : "true"}
                  title={
                    canEditStep ? undefined : "可以查看；完成前置步骤后才能编辑"
                  }
                >
                  <StepIcon className="h-4 w-4 shrink-0" />
                  <span className="truncate">{step.title}</span>
                  {!canEditStep && (
                    <span
                      aria-hidden="true"
                      className="ml-auto shrink-0 text-[10px] opacity-70"
                    >
                      只读
                    </span>
                  )}
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
                  {activePlan
                    ? `正在编辑：${activePlan.name}`
                    : "正在创建：新的 MultiRouter 配置"}
                </div>
                <div className="mt-1 text-xs text-muted-foreground">
                  {activePlan
                    ? activePlan.id
                    : "新配置不会覆盖已有 MultiRouter；保存后将生成独立方案。"}
                </div>
              </div>
              <Badge variant="outline">
                {activePlan ? "编辑当前方案" : "创建新配置"}
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
            {currentStepPrerequisite && (
              <div
                role="status"
                className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-amber-300 bg-amber-50 p-3 text-sm text-amber-950 dark:border-amber-700/60 dark:bg-amber-950/30 dark:text-amber-100"
              >
                <div>
                  <div className="font-medium">当前步骤暂不可编辑</div>
                  <div className="mt-1 text-xs leading-5 opacity-90">
                    请先完成“{currentStepPrerequisite.title}
                    ”。本页可以查看，完成前置步骤后会自动恢复编辑。
                  </div>
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => openWizardStep(currentStepPrerequisite)}
                >
                  去完成{currentStepPrerequisite.title}
                </Button>
              </div>
            )}
            <fieldset
              disabled={Boolean(currentStepPrerequisite)}
              aria-disabled={Boolean(currentStepPrerequisite)}
              className="m-0 min-w-0 border-0 p-0"
            >
              {currentStep.key === "welcome" && (
                <div className="space-y-4">
                  <div className="rounded-xl border border-blue-500/25 bg-gradient-to-r from-blue-500/10 via-background to-violet-500/10 p-5">
                    <div className="text-lg font-semibold">
                      MultiRouter 自动配置向导
                    </div>
                    <p className="mt-2 text-sm leading-6 text-muted-foreground">
                      向导会先检查本机配置，再读取模型目录，并向你选择的上游发送少量真实请求，分别验证
                      Responses、Chat、流式、推理、工具调用和续接能力。
                    </p>
                  </div>
                  <div className="grid gap-3 md:grid-cols-3">
                    <div className="rounded-lg border p-4 text-sm">
                      <div className="font-medium">不会提前生效</div>
                      <p className="mt-2 leading-6 text-muted-foreground">
                        最终确认前不会启用或覆盖当前 Codex 配置。
                      </p>
                    </div>
                    <div className="rounded-lg border p-4 text-sm">
                      <div className="font-medium">测试可能计费</div>
                      <p className="mt-2 leading-6 text-muted-foreground">
                        深探测会调用真实模型，可能产生少量额度和限流占用。
                      </p>
                    </div>
                    <div className="rounded-lg border p-4 text-sm">
                      <div className="font-medium">真实请求再验收</div>
                      <p className="mt-2 leading-6 text-muted-foreground">
                        保存启用后仍需在 Codex 完成一次真实请求才算配置完成。
                      </p>
                    </div>
                  </div>
                </div>
              )}

              {currentStep.key === "inventory" && (
                <div className="grid gap-3 md:grid-cols-2">
                  <div className="rounded-lg border p-4">
                    <div className="text-sm font-medium">Codex Provider</div>
                    <div className="mt-2 text-2xl font-semibold">
                      {providerModelSources.length}
                    </div>
                    <p className="mt-2 text-sm text-muted-foreground">
                      {providerModelSources.length > 0
                        ? "已有模型源，下一步检查每个源的就绪状态。"
                        : "尚无模型源，向导会先带你接入一个 Provider。"}
                    </p>
                  </div>
                  <div className="rounded-lg border p-4">
                    <div className="text-sm font-medium">现有 MultiRouter</div>
                    <div className="mt-2 text-2xl font-semibold">
                      {providers.filter(isCodexMultiRouterPlan).length}
                    </div>
                    <p className="mt-2 text-sm text-muted-foreground">
                      {activePlan
                        ? `当前编辑 ${activePlan.name}`
                        : "将创建独立的新方案，不覆盖已有路由。"}
                    </p>
                  </div>
                </div>
              )}

              {currentStep.key === "first-provider" && (
                <div className="rounded-xl border border-dashed p-6 text-center">
                  <Server className="mx-auto h-10 w-10 text-primary" />
                  <div className="mt-3 font-semibold">先接入一个可用模型源</div>
                  <p className="mx-auto mt-2 max-w-xl text-sm leading-6 text-muted-foreground">
                    添加页会引导填写模型服务地址、凭据并测试连接。保存后会返回本向导，并自动识别和选中新模型源。
                  </p>
                  <Button className="mt-4" onClick={onCreateProvider}>
                    打开模型源添加页
                  </Button>
                </div>
              )}

              {currentStep.key === "readiness" && (
                <div className="space-y-3">
                  {providerModelSources.map((provider) => {
                    const details = modelSourceStatusDetails(provider);
                    const issues = getWizardConfigIssues([provider]);
                    return (
                      <div key={provider.id} className="rounded-lg border p-4">
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div>
                            <div className="font-medium">{provider.name}</div>
                            <div className="mt-1 text-xs text-muted-foreground">
                              {provider.id}
                            </div>
                          </div>
                          <Badge
                            variant={
                              issues.length === 0 ? "secondary" : "outline"
                            }
                          >
                            {issues.length === 0 ? "可继续" : "需要补全"}
                          </Badge>
                        </div>
                        <div className="mt-3 grid gap-1 text-xs leading-5 text-muted-foreground md:grid-cols-2">
                          {details.map((detail) => (
                            <div key={detail}>{detail}</div>
                          ))}
                        </div>
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          className="mt-3"
                          onClick={() => onOpenProviderConfig?.(provider)}
                        >
                          {issues.length === 0
                            ? "查看高级配置"
                            : "补全 Provider 配置"}
                        </Button>
                      </div>
                    );
                  })}
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
                      已选择 {draftSources.length} /{" "}
                      {providerModelSources.length} 个 Codex provider
                      作为本次模型源；取消选择不会删除 provider。
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

              {currentStep.key === "reasoning" && (
                <div className="space-y-4">
                  <div className="rounded-lg border bg-muted/30 p-4 text-sm leading-6 text-muted-foreground">
                    协议页只确认上游实际返回的推理语义；这里展示当前能力和默认强度。模型级覆盖由各
                    Provider 的高级配置维护，不与协议自动选择混在一起。
                  </div>
                  {draftSources.map((provider) => (
                    <div
                      key={provider.id}
                      className="space-y-3 rounded-lg border p-4"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div>
                          <div className="font-medium">{provider.name}</div>
                          <div className="mt-1 text-xs text-muted-foreground">
                            {inferWizardApiFormat(provider)} ·{" "}
                            {readWizardModelCatalog(provider).length} 个模型
                          </div>
                        </div>
                        <Button
                          type="button"
                          variant="outline"
                          aria-label={`编辑 ${provider.name} 的推理高级配置`}
                          onClick={() => onOpenProviderConfig?.(provider)}
                        >
                          编辑推理高级配置
                        </Button>
                      </div>
                      <div className="grid gap-2 md:grid-cols-2">
                        {readWizardModelCatalog(provider).map((model) => {
                          const reasoning = model.reasoning;
                          const support =
                            reasoning?.supportStatus === "confirmed_supported"
                              ? "已确认支持推理"
                              : reasoning?.supportStatus ===
                                  "confirmed_unsupported"
                                ? "已确认不支持推理"
                                : "能力未知，使用服务端默认";
                          return (
                            <div
                              key={model.model}
                              className="rounded-md border bg-muted/20 p-3 text-xs"
                            >
                              <div className="font-medium text-foreground">
                                {model.displayName ?? model.model}
                              </div>
                              <div className="mt-2 space-y-1 text-muted-foreground">
                                <div>{support}</div>
                                <div>
                                  可用强度：
                                  {reasoning?.supportedEfforts?.join(" / ") ||
                                    "未声明"}
                                </div>
                                <div>
                                  默认强度：
                                  {reasoning?.defaultEffort ?? "模型默认"}
                                </div>
                              </div>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              )}

              {currentStep.key === "subagents-tools" && (
                <div className="space-y-4">
                  <div className="rounded-lg border bg-muted/30 p-4 text-sm leading-6 text-muted-foreground">
                    基础模式设置最多五个可路由 Sub-Agent 候选和 Hosted
                    Tools。角色、Provider 继承及 V2
                    规则由独立工作台维护，不会改变主模型推理强度。
                  </div>
                  <div className="max-h-72 overflow-auto rounded-lg border">
                    {activeCatalogModelOrder.map((model) => {
                      const selected = activeSpawnAgentModels.includes(model);
                      return (
                        <label
                          key={model}
                          className="flex items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0"
                        >
                          <input
                            type="checkbox"
                            checked={selected}
                            disabled={
                              !selected && activeSpawnAgentModels.length >= 5
                            }
                            onChange={(event) =>
                              setDraftSpawnAgentModels((current) =>
                                event.target.checked
                                  ? [
                                      ...current.filter(
                                        (item) => item !== model,
                                      ),
                                      model,
                                    ].slice(0, 5)
                                  : current.filter((item) => item !== model),
                              )
                            }
                            aria-label={`使用 ${model} 作为 Sub-Agent 候选`}
                          />
                          <span>{model}</span>
                        </label>
                      );
                    })}
                  </div>
                  <div className="grid gap-3 md:grid-cols-2">
                    <label className="flex items-center gap-3 rounded-lg border p-3 text-sm">
                      <input
                        type="checkbox"
                        checked={webSearchEnabled}
                        onChange={(event) =>
                          setWebSearchEnabled(event.target.checked)
                        }
                      />
                      Web Search Hosted Tool
                    </label>
                    <label className="flex items-center gap-3 rounded-lg border p-3 text-sm">
                      <input
                        type="checkbox"
                        checked={imageGenerationEnabled}
                        onChange={(event) =>
                          setImageGenerationEnabled(event.target.checked)
                        }
                      />
                      Image Generation Hosted Tool
                    </label>
                  </div>
                  <Button
                    type="button"
                    variant="outline"
                    disabled={!activePlan}
                    onClick={() =>
                      activePlan && onOpenWorkspace(activePlan, "subagents")
                    }
                  >
                    打开 Sub-Agent V2 高级工作台
                  </Button>
                </div>
              )}

              {currentStep.key === "routing-review" && (
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

              {currentStep.key === "catalog" && (
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
                      自动获取模型列表
                    </Button>
                  </div>
                  <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                    这里只同步和比较模型目录，结果保存在本次向导草稿；不会在本页改写协议，也不会启用当前配置。
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
                              <div className="mt-2 space-y-0.5 text-xs leading-5 text-muted-foreground">
                                {modelSourceStatusDetails(provider).map(
                                  (detail) => (
                                    <div key={detail}>{detail}</div>
                                  ),
                                )}
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
                </div>
              )}

              {currentStep.key === "protocol" && (
                <div className="space-y-4">
                  <div className="rounded-lg border p-3 text-sm leading-6">
                    探测完成后无需在这里保存。取消勾选失败模型后点击“下一步”，最后到“保存并启用”页保存方案。
                    取消项会在最终保存时停用该模型源中的对应模型；若该来源全部取消，则不保存该来源。
                    未保存草稿在本次应用会话内保留。编辑已保存方案时自动恢复仍有效的探测证据，无需每次重测；也可随时主动重新探测。证据缺失、过期或模型源发生相关变更时，需要重新探测。
                  </div>
                  {isRestoringEvidence && (
                    <p role="status">
                      正在读取已保存的兼容性证据，不发送模型请求…
                    </p>
                  )}
                  {restoredEvidence && (
                    <p role="status">
                      已检查保存记录：复用 {restoredEvidence.outcomes.length}{" "}
                      个模型源的有效证据。未恢复的来源仍需探测。
                    </p>
                  )}
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm leading-6 text-amber-900 dark:text-amber-200">
                    每个普通 Provider/模型会分别验证 Responses 与 Chat
                    Completions 的基础响应、流式
                    SSE、推理语义、强制工具调用和工具结果续接。401/403/429、网络错误及
                    5xx 会标记为认证或上游可用性问题，不会误判成协议不支持。
                  </div>
                  <Button
                    onClick={requestConnectivityProbe}
                    disabled={
                      isRefreshingModels ||
                      isProbingConnectivity ||
                      isRestoringEvidence ||
                      draftSources.length === 0
                    }
                  >
                    <Route
                      className={`mr-2 h-4 w-4 ${
                        isProbingConnectivity ? "animate-pulse" : ""
                      }`}
                    />
                    {isProbingConnectivity
                      ? "正在运行协议深探测"
                      : restoredEvidence?.outcomes.length
                        ? "重新进行兼容性深度探测"
                        : "开始兼容性深度探测"}
                  </Button>
                  {connectivityResults.length > 0 && (
                    <Button
                      variant="outline"
                      disabled={isProbingConnectivity}
                      onClick={() => {
                        setExcludedProbeModels((current) => [
                          ...new Set([
                            ...current,
                            ...connectivityResults
                              .filter((result) => !result.canContinue)
                              .map((result) =>
                                probeModelKey(result.providerId, result.model),
                              ),
                          ]),
                        ]);
                        clearWizardIssuesForStage("protocol");
                      }}
                    >
                      取消所有失败模型
                    </Button>
                  )}
                  {connectivityResults.length > 0 && (
                    <div className="max-h-80 overflow-auto rounded-lg border">
                      {connectivityResults.map((result, index) => (
                        <div
                          key={`${result.providerId}:${result.model}:${index}`}
                          className="grid grid-cols-[7rem_1fr] gap-3 border-b px-3 py-2 text-sm last:border-b-0"
                        >
                          <Badge
                            variant={
                              result.status === "fail"
                                ? "destructive"
                                : "outline"
                            }
                            className="h-fit justify-center"
                          >
                            {result.status}
                          </Badge>
                          <div>
                            <label className="flex items-center gap-2">
                              <Checkbox
                                aria-label={`保留 ${result.providerName} / ${result.model}`}
                                checked={
                                  !isProbeModelExcluded(
                                    result.providerId,
                                    result.model,
                                  )
                                }
                                disabled={isProbingConnectivity}
                                onCheckedChange={(checked) => {
                                  const key = probeModelKey(
                                    result.providerId,
                                    result.model,
                                  );
                                  setExcludedProbeModels((current) =>
                                    checked
                                      ? current.filter((item) => item !== key)
                                      : [...new Set([...current, key])],
                                  );
                                  clearWizardIssuesForStage("protocol");
                                }}
                              />
                              {isProbeModelExcluded(
                                result.providerId,
                                result.model,
                              )
                                ? "已排除（不改变探测结果）"
                                : "保留此模型"}
                            </label>
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

              {currentStep.key === "models" && (
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

              {(currentStep.key === "models" ||
                currentStep.key === "model-order") && (
                <div className="space-y-4">
                  {currentStep.key === "models" && (
                    <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                      默认自动跟随 Provider
                      的全部可用模型；只有取消某个模型时才进入固定筛选。模型排序、推理设置和
                      Sub-Agent 分别在后续独立页面处理。
                    </div>
                  )}
                  <div className="flex flex-wrap items-center gap-2">
                    {currentStep.key === "models" && (
                      <>
                        <Button
                          type="button"
                          variant="outline"
                          onClick={() => setCatalogModelOrder(null)}
                        >
                          自动跟随全部模型
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
                      </>
                    )}
                    <Badge variant="outline">
                      已保留 {activeCatalogModelOrder.length} /{" "}
                      {availableCatalogModels.length}
                    </Badge>
                    <Badge
                      className={
                        catalogModelOrder === null
                          ? "border-emerald-300 bg-emerald-50 text-emerald-800 dark:border-emerald-500/50 dark:bg-emerald-500/10 dark:text-emerald-100"
                          : "border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-500/50 dark:bg-amber-500/10 dark:text-amber-100"
                      }
                    >
                      {catalogModelOrder === null
                        ? "自动跟随 Provider"
                        : "固定模型筛选"}
                    </Badge>
                  </div>
                  <div className="max-h-[min(50vh,34rem)] overflow-auto rounded-lg border">
                    {(currentStep.key === "model-order"
                      ? selectModelRows.filter((model) =>
                          activeCatalogModelOrder.includes(model.model),
                        )
                      : selectModelRows
                    ).map((model) => {
                      const kept = activeCatalogModelOrder.includes(
                        model.model,
                      );
                      const orderIndex = activeCatalogModelOrder.indexOf(
                        model.model,
                      );
                      return (
                        <div
                          key={`${model.model}:${model.upstreamModel ?? ""}`}
                          className={`grid items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0 ${
                            currentStep.key === "models"
                              ? "grid-cols-[2rem_minmax(0,1fr)_8rem]"
                              : "grid-cols-[minmax(0,1fr)_8rem_5rem]"
                          }`}
                        >
                          {currentStep.key === "models" && (
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
                          )}
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
                          {currentStep.key === "model-order" && (
                            <div className="flex items-center gap-1">
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                className="h-8 w-8"
                                disabled={!kept || orderIndex <= 0}
                                onClick={() =>
                                  moveCatalogModel(model.model, -1)
                                }
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
                                  orderIndex >=
                                    activeCatalogModelOrder.length - 1
                                }
                                onClick={() => moveCatalogModel(model.model, 1)}
                                title="下移"
                              >
                                <ArrowDown className="h-4 w-4" />
                              </Button>
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {currentStep.key === "routing-review" && (
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
                    existingPlan.settingsConfig?.codexRouting?.schemaVersion !==
                      2 ? (
                      <div className="rounded-md border border-amber-300 bg-amber-50 p-2 text-xs leading-5 text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/30 dark:text-amber-100 md:col-span-2">
                        这是升级前的方案，当前选择由原 route
                        绑定推断。编辑前需要先预览并显式应用 schema v2 迁移。
                      </div>
                    ) : null}
                  </div>
                  {previewRoutes.map((route) => (
                    <div key={route.id} className="rounded-lg border p-4">
                      <div className="flex items-center justify-between gap-3">
                        <div className="font-medium">
                          {wizardRouteDisplayLabel(
                            route,
                            previewProvidersById.get(route.targetProviderId)
                              ?.name,
                          )}
                        </div>
                        <Badge
                          variant="outline"
                          title={`Provider ID: ${route.targetProviderId}`}
                        >
                          {previewProvidersById.get(route.targetProviderId)
                            ?.name ?? route.targetProviderId}
                        </Badge>
                      </div>
                      <div className="mt-2 text-sm text-muted-foreground">
                        模型范围：
                        {route.modelSelection?.mode === "all"
                          ? "目标 Provider 的全部模型"
                          : `${(route.modelSelection?.models ?? []).length} 个 canonical 模型`}
                        ；前缀 {(route.matchPrefixes ?? []).join(", ") || "无"}
                      </div>
                      <div className="mt-2 text-xs leading-5 text-muted-foreground">
                        认证：
                        {route.authPolicy?.source === "native_codex_auth"
                          ? "Codex Desktop 当前登录"
                          : route.authPolicy?.source === "account_pool"
                            ? "OAuth 账号池"
                            : route.authPolicy?.source === "managed_codex_oauth"
                              ? "CCSM OAuth"
                              : "模型源凭据"}
                        ；客户端传输：HTTP Responses
                      </div>
                      <div className="mt-2 rounded-md bg-muted px-3 py-2 text-xs leading-5 text-muted-foreground">
                        协议、连接地址、凭据和模型能力始终读取目标
                        Provider/模型条目的最新配置；Route 不保存这些字段。
                      </div>
                    </div>
                  ))}
                  {aliasSelectionIssues.length > 0 ? (
                    <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
                      <div className="font-medium">别名需要处理</div>
                      <div className="mt-1 space-y-1 text-xs leading-5">
                        {aliasSelectionIssues.map((issue) => (
                          <div key={`${issue.routeId}:${issue.alias}`}>
                            Route {issue.routeLabel || issue.routeId}
                            {issue.routeLabel &&
                            issue.routeLabel !== issue.routeId
                              ? `（${issue.routeId}）`
                              : ""}
                            {issue.providerName
                              ? ` / Provider ${issue.providerName}`
                              : ""}
                            的“{issue.alias}”→“{issue.canonicalModel}”：
                            {issue.reason}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>
              )}

              {currentStep.key === "save-enable" &&
                flowState.status !== "enabled" && (
                  <div className="space-y-4">
                    <div className="rounded-lg border p-4 text-sm text-muted-foreground">
                      将保存 {previewRoutes.length} 条路由和{" "}
                      {previewModels.length} 个可见模型到{" "}
                      {activePlan ? activePlan.name : "新的 MultiRouter"}。
                    </div>
                    {draftSources.length === 0 ? (
                      <div className="rounded-lg border border-dashed border-amber-500/40 bg-amber-500/10 p-4 text-sm leading-6 text-amber-900 dark:text-amber-100">
                        尚未选择模型源，保存入口仍保留；请回到“选择模型源”添加并配置至少一个
                        Provider 后再保存。
                      </div>
                    ) : null}
                    <Button
                      onClick={saveMultiRouterPlan}
                      disabled={
                        isSavingPlan ||
                        editingTargetMissing ||
                        draftSources.length === 0 ||
                        aliasSelectionIssues.length > 0 ||
                        (connectivityResults.length > 0 &&
                          !canContinueAfterConnectivity(
                            retainedConnectivityResults,
                          ))
                      }
                    >
                      <Database className="mr-2 h-4 w-4" />
                      {isSavingPlan ? "正在保存..." : "保存并发布"}
                    </Button>
                  </div>
                )}

              {currentStep.key === "save-enable" &&
                flowState.status !== "enabled" && (
                  <div className="space-y-4">
                    <div className="rounded-lg border p-4 text-sm leading-6 text-muted-foreground">
                      保存完成后，请显式启用这个多路路由。启用成功后可继续完成独立设置，
                      也可以稍后关闭向导，再到 MultiRouter
                      状态页完成真实请求验证。
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

              {currentStep.key === "acceptance" && savedPlan && (
                <div className="space-y-4">
                  <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-4">
                    <div className="flex items-start gap-3">
                      <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-emerald-600" />
                      <div>
                        <div className="font-medium text-emerald-900 dark:text-emerald-100">
                          MultiRouter 已启用，等待真实请求验收
                        </div>
                        <div className="mt-1 text-sm leading-6 text-emerald-900/80 dark:text-emerald-100/80">
                          请在 Codex
                          中发送一次真实请求；只有路由命中、上游返回且下游收到合法终止事件才算完成。
                          HTTP 200 或只收到响应头不代表请求完整结束。
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="flex flex-wrap gap-3">
                    <Button
                      type="button"
                      onClick={() => onOpenWorkspace(savedPlan, "status")}
                    >
                      <Route className="mr-2 h-4 w-4" />
                      打开状态页完成验收
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      onClick={() => closeWizard(false)}
                    >
                      稍后验收
                    </Button>
                  </div>

                  <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                    验收与下面的历史修复、推理强度、Sub-Agent、模型顺序入口彼此独立，不会重新执行协议探测或拆分
                    Provider。
                  </div>

                  <div className="grid gap-3 sm:grid-cols-2">
                    <Button
                      type="button"
                      variant="outline"
                      className="h-auto justify-start gap-3 px-4 py-3 text-left"
                      disabled={!onOpenHistoryRepair}
                      onClick={() => {
                        onOpenChange(false);
                        onOpenHistoryRepair?.();
                      }}
                    >
                      <History className="h-5 w-5 shrink-0" />
                      <span>
                        <span className="block font-medium">
                          修复 Codex 历史记录
                        </span>
                        <span className="block text-xs text-muted-foreground">
                          打开历史修复页，处理 Provider 与模型切换记录
                        </span>
                      </span>
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      className="h-auto justify-start gap-3 px-4 py-3 text-left"
                      onClick={() => {
                        onOpenChange(false);
                        onOpenWorkspace(savedPlan, "sources");
                      }}
                    >
                      <SlidersHorizontal className="h-5 w-5 shrink-0" />
                      <span>
                        <span className="block font-medium">
                          设置模型推理强度
                        </span>
                        <span className="block text-xs text-muted-foreground">
                          回到各模型源，单独维护推理能力和档位
                        </span>
                      </span>
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      className="h-auto justify-start gap-3 px-4 py-3 text-left"
                      onClick={() => {
                        onOpenChange(false);
                        onOpenWorkspace(savedPlan, "subagents");
                      }}
                    >
                      <Bot className="h-5 w-5 shrink-0" />
                      <span>
                        <span className="block font-medium">
                          配置 Sub-Agent
                        </span>
                        <span className="block text-xs text-muted-foreground">
                          进入独立的 Sub-Agent V1/V2 配置区
                        </span>
                      </span>
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      className="h-auto justify-start gap-3 px-4 py-3 text-left"
                      onClick={() => {
                        onOpenChange(false);
                        onOpenWorkspace(savedPlan, "model-order");
                      }}
                    >
                      <GripVertical className="h-5 w-5 shrink-0" />
                      <span>
                        <span className="block font-medium">调整模型顺序</span>
                        <span className="block text-xs text-muted-foreground">
                          调整 Codex 模型选择器中的展示顺序
                        </span>
                      </span>
                    </Button>
                  </div>
                </div>
              )}
            </fieldset>
          </div>
        </div>

        <CodexProtocolProbeProgressDialog
          open={probeDialogOpen}
          running={batchProtocolLab.state.phase === "probing"}
          expectedTargets={(
            batchProtocolLab.state.draft?.sources ?? []
          ).flatMap((source) =>
            codexProviderModelsRequiringProtocolProbe(source.provider).map(
              (model) => ({
                providerId: source.provider.id,
                providerName: source.provider.name,
                model,
              }),
            ),
          )}
          events={batchProtocolLab.state.progress}
          outcome={
            batchProtocolLab.probeOutcome?.outcomes.at(-1)?.outcome ?? null
          }
          outcomes={(batchProtocolLab.probeOutcome?.outcomes ?? []).map(
            (entry) => entry.outcome,
          )}
          error={batchProtocolLab.state.errorDetail ?? ""}
          onOpenChange={setProbeDialogOpen}
          onRetry={() => {
            setProbeDialogOpen(true);
            batchProtocolLab.retry();
          }}
        />

        <CodexProviderSetPreviewDialog
          open={pendingSourcePreview !== null}
          preview={pendingSourcePreview}
          pending={isSavingPlan}
          onBack={returnFromBlockedBatch}
          onConfirmSplit={() => undefined}
          onRetry={retryBlockedBatchProbe}
        />

        <Dialog
          open={
            batchProtocolLab.state.phase === "awaiting_probe_consent" ||
            batchProtocolLab.state.phase === "stale_retry"
          }
          onOpenChange={(nextOpen) => {
            if (!nextOpen) batchProtocolLab.cancel();
          }}
        >
          <DialogContent className="max-w-lg" zIndex="top">
            <DialogHeader>
              <DialogTitle>确认开始兼容性深度探测</DialogTitle>
              <DialogDescription className="space-y-2 text-left">
                <span className="block">
                  每个普通 provider/model 都会分别验证 Responses 与 Chat
                  Completions。探测会向上游发送真实请求，会消耗模型 Token
                  并可能产生费用，也可能触发限流；完成后会显示每个来源的 Token
                  和费用估算。
                </span>
                <span className="block">
                  每条协议都要依次通过基础响应、流式
                  SSE、推理字段、强制工具调用和工具结果续轮；只有完整事务成功才会生成可保存
                  receipt。
                </span>
                <span className="block">
                  401/403/429、网络错误和 5xx
                  会显示为认证、额度或上游可用性问题，不会被误判成协议不兼容。官方和托管账号源固定走
                  Responses，不发送这组额外探测请求。
                </span>
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => batchProtocolLab.cancel()}
              >
                取消
              </Button>
              <Button type="button" onClick={confirmConnectivityProbe}>
                确认测试
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog
          open={
            resolvedMode === "edit" &&
            Boolean(storedExistingPlan) &&
            storedExistingPlan?.settingsConfig?.codexRouting?.schemaVersion !==
              2 &&
            !migratedPlanOverride
          }
          onOpenChange={(nextOpen) => {
            if (!nextOpen && !isApplyingMigration) closeWizard(false);
          }}
        >
          <DialogContent className="max-w-2xl" zIndex="top">
            <DialogHeader>
              <DialogTitle>编辑前迁移旧 MultiRouter</DialogTitle>
              <DialogDescription>
                schema v1
                保持只读兼容。继续编辑或启用前，需要先检查迁移预览并显式应用；预览不会展示密钥或
                Token。
              </DialogDescription>
            </DialogHeader>
            {isLoadingMigration ? (
              <div className="rounded-md border p-3 text-sm text-muted-foreground">
                正在生成迁移预览…
              </div>
            ) : migrationPreview ? (
              <div className="space-y-3 text-sm">
                <div className="grid gap-2 sm:grid-cols-3">
                  <div className="rounded-md border p-3">
                    删除冗余字段：
                    {migrationPreview.diff.removedRouteFields.length}
                  </div>
                  <div className="rounded-md border p-3">
                    引用变化：{migrationPreview.diff.changedRouteIds.length}
                  </div>
                  <div className="rounded-md border p-3">
                    新建 Provider：{migrationPreview.generatedProviders.length}
                  </div>
                </div>
                {migrationPreview.generatedProviders.map((provider) => (
                  <div key={provider.id} className="rounded-md border p-3">
                    {provider.name} ({provider.id})，来源{" "}
                    {provider.sourceProviderId}
                  </div>
                ))}
                {migrationPreview.warnings.map((warning) => (
                  <div
                    key={warning}
                    className="rounded-md border border-amber-300 bg-amber-50 p-3 text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/30 dark:text-amber-100"
                  >
                    {warning}
                  </div>
                ))}
              </div>
            ) : null}
            {migrationError ? (
              <div
                role="alert"
                className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
              >
                {migrationError}
              </div>
            ) : null}
            <DialogFooter>
              <Button
                variant="outline"
                disabled={isApplyingMigration}
                onClick={() => closeWizard(false)}
              >
                取消编辑
              </Button>
              <Button
                disabled={!migrationPreview || isApplyingMigration}
                onClick={() => void applyLegacyMigration()}
              >
                {isApplyingMigration ? "正在应用…" : "应用迁移并继续编辑"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <div className="flex shrink-0 items-center justify-between border-t px-5 py-4">
          <Button variant="ghost" onClick={() => closeWizard(true)}>
            暂存并关闭
          </Button>
          <Button
            variant="ghost"
            disabled={
              isProbingConnectivity ||
              isSavingPlan ||
              isRefreshingModels ||
              isEnablingPlan
            }
            onClick={() => {
              initializedOpenRef.current = false;
              draftGenerationRef.current += 1;
              batchProtocolLab.reset();
              onOpenChange(false);
            }}
          >
            丢弃草稿
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
              {stepIndex === steps.length - 1 ? "关闭" : "下一步"}
              {stepIndex !== steps.length - 1 && (
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

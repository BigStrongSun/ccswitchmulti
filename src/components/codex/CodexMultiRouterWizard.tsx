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
  KeyRound,
  RefreshCw,
  Route,
  Server,
  ShieldAlert,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type {
  CodexApiFormat,
  CodexCacheConfig,
  CodexCatalogModel,
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
import {
  fetchModelsForConfig,
  probeCodexChatForConfig,
  probeCodexResponsesForConfig,
} from "@/lib/api/model-fetch";
import {
  CODEX_MULTI_ROUTER_DEFAULT_NAME,
  CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY,
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
  hasWizardModelCatalog,
  isWizardCatalogOnlyModelSource,
  isWizardCodexOAuthSource,
  isWizardVolcengineAgentPlanModelSource,
  inferWizardApiFormat,
  isCodexMultiRouterPlan,
  mergeFetchedModelsIntoWizardProvider,
  readWizardModelCatalog,
  readWizardProviderBaseUrl,
  resolveWizardModelNameCollisions,
  skippedWizardConnectivityResult,
  type WizardConnectivityResult,
  type WizardModelFetchConfig,
} from "@/lib/codexMultiRouterWizard";
import type { WorkspaceTab } from "@/components/codex/CodexRouterWorkspacePage";
import { codexCatalogOnlyPlanModelFetchMessage } from "@/utils/codexPlanModelFetch";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import { useCodexOauth } from "@/components/providers/forms/hooks/useCodexOauth";

interface CodexMultiRouterWizardProps {
  open: boolean;
  providers: Provider[];
  onOpenChange: (open: boolean) => void;
  onCreateProvider: () => void;
  onOpenProviderConfig?: (provider: Provider) => void;
  onOpenWorkspace: (provider: Provider, tab: WorkspaceTab) => void;
  onEnablePlan: (provider: Provider) => void | Promise<void>;
}

type WizardStepKey =
  | "intro"
  | "sources"
  | "rename"
  | "providerConfig"
  | "fetchModels"
  | "collisions"
  | "selectModels"
  | "routes"
  | "spawnAgent"
  | "publish"
  | "finish";

interface WizardStep {
  key: WizardStepKey;
  title: string;
  description: string;
  icon: typeof Wand2;
}

interface WizardStepRule {
  errors: string[];
  canContinue: string;
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

interface CodexOAuthWizardStatus {
  isLoading: boolean;
  hasAnyAccount: boolean;
  accountCount: number;
}

const STEPS: WizardStep[] = [
  {
    key: "intro",
    title: "理解 MultiRouter",
    description:
      "Codex 仍连接本地 15721，CCSwitchMulti 按 model 分发到不同上游。",
    icon: Wand2,
  },
  {
    key: "sources",
    title: "创建模型源",
    description:
      "逐个添加 OpenAI/中转站、DeepSeek、Qwen、本地 vLLM/Ollama 等 Codex provider。",
    icon: Server,
  },
  {
    key: "rename",
    title: "命名方案",
    description:
      "给这次新配置的 MultiRouter 起一个清晰名称，后续状态页和 provider 列表都会使用它。",
    icon: Wand2,
  },
  {
    key: "providerConfig",
    title: "配置核心参数",
    description:
      "检查 API Key、Base URL、Responses / Chat Completions、Codex 菜单映射和多模型路由。",
    icon: KeyRound,
  },
  {
    key: "fetchModels",
    title: "获取模型列表",
    description:
      "自动调用 /models，空目录用于初始化，已有目录只刷新保留模型的元数据。",
    icon: RefreshCw,
  },
  {
    key: "collisions",
    title: "处理重名模型",
    description:
      "官方模型保留原名，中转站或第三方同名模型会在模型名后追加 provider 名称并保留 upstreamModel。",
    icon: ShieldAlert,
  },
  {
    key: "selectModels",
    title: "整理模型",
    description: "汇总所有模型后，排序并剔除旧模型或不想暴露给 Codex 的模型。",
    icon: Database,
  },
  {
    key: "routes",
    title: "生成路由规则",
    description:
      "按 provider 分组生成规则，gpt/o、deepseek、qwen 等前缀自动命中对应上游。",
    icon: GitBranch,
  },
  {
    key: "spawnAgent",
    title: "子 Agent 候选",
    description: "从最终模型列表里选择并排序最多 5 个子 Agent 候选模型。",
    icon: Route,
  },
  {
    key: "publish",
    title: "保存并发布",
    description:
      "创建或更新带 codexRouting 和 modelCatalog 的 MultiRouter provider。",
    icon: Database,
  },
  {
    key: "finish",
    title: "启用并测试",
    description:
      "显式启用多路路由，进入状态页等待真实转发成功，再自动跳到历史修复。",
    icon: CheckCircle2,
  },
];

const STEP_RULES: Record<WizardStepKey, WizardStepRule> = {
  intro: {
    errors: ["本地代理未运行或 15721 被其它进程占用时，后续启用会失败。"],
    canContinue: "这是说明步骤，总是可以继续。",
  },
  sources: {
    errors: ["没有普通 Codex provider 时，不能生成任何路由。"],
    canContinue: "至少识别到一个普通 Codex provider 后可以继续。",
  },
  rename: {
    errors: ["名称为空会让后续 provider 列表和状态页难以区分。"],
    canContinue: "填写 MultiRouter 名称后可以继续。",
  },
  providerConfig: {
    errors: [
      "缺少 Base URL/API Key 时无法自动获取模型，也无法做真实连通性测试。",
      "apiFormat 未显式设置时会按模型源和探测结果推断：官方 GPT/O 优先 Responses，未知第三方保守走 Chat Completions。",
    ],
    canContinue:
      "有可用 modelCatalog 时可继续；没有 modelCatalog 且缺配置会停在配置缺口状态。",
  },
  fetchModels: {
    errors: [
      "/models 失败会保留已有目录，不会清空用户配置。",
      "Responses 直连 provider 的 /v1/responses 探测失败是阻塞项。",
      "Chat Completions provider 的 /v1/responses 探测失败是可继续警告。",
    ],
    canContinue:
      "无阻塞连通性失败即可继续；未测试时允许继续但状态条会提示风险。",
  },
  collisions: {
    errors: [
      "多个 provider 暴露同一 upstreamModel 时，后面的同名模型会被路由顺序遮蔽。",
    ],
    canContinue: "接受自动别名策略后可以继续，upstreamModel 会保留真实模型名。",
  },
  selectModels: {
    errors: ["未保留任何模型时，MultiRouter 不会有可路由模型。"],
    canContinue: "至少保留一个模型后可以继续生成路由。",
  },
  routes: {
    errors: ["没有 match.models/prefixes 的 route 不会稳定命中模型请求。"],
    canContinue: "至少生成一条 route 且没有连通性阻塞项时可以继续保存。",
  },
  spawnAgent: {
    errors: ["子 Agent 候选最多 5 个；不选择时会默认使用最终模型列表前 5 个。"],
    canContinue: "候选为空或不超过 5 个都可以继续；保存时会过滤掉已剔除模型。",
  },
  publish: {
    errors: ["数据库写入失败或 provider id 冲突会进入 saveFailed。"],
    canContinue: "点击保存并发布成功后进入完成页；保存失败必须重试或返回修改。",
  },
  finish: {
    errors: [
      "本地代理未运行、端口冲突或切换 provider 失败会进入 enableFailed。",
      "启用后如果 Codex 没有发出真实请求，状态页会停在待请求验证，不会提前跳历史修复。",
    ],
    canContinue:
      "显式启用成功后会自动打开状态页；最近一次转发成功后，App 会提示配置成功并进入历史修复。",
  },
};

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
  stepKey: "intro",
};

// 将左侧教程步骤映射到业务状态；手动跳步也会进入对应的状态分支，避免 UI 步骤和流程状态脱节。
function statusForStep(stepKey: WizardStepKey): WizardFlowStatus {
  switch (stepKey) {
    case "sources":
      return "reviewProviderConfig";
    case "rename":
      return "reviewProviderConfig";
    case "providerConfig":
      return "reviewProviderConfig";
    case "fetchModels":
      return "readyToFetchModels";
    case "collisions":
      return "collisionReviewRequired";
    case "selectModels":
      return "routePreview";
    case "routes":
      return "routePreview";
    case "spawnAgent":
      return "routePreview";
    case "publish":
      return "published";
    case "finish":
      return "enablePrompt";
    case "intro":
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
        stepKey: "intro",
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
        stepKey: "fetchModels",
        fetchSummary: event.summary,
      };
    case "PROBE_START":
      return {
        ...state,
        status: "probingConnectivity",
        stepKey: "fetchModels",
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
        stepKey: event.canContinue ? "collisions" : "fetchModels",
        connectivitySummary: event.summary,
      };
    case "SAVE_START":
      return { ...state, status: "savingPlan", lastError: undefined };
    case "SAVE_SUCCESS":
      return { ...state, status: "published", stepKey: "finish" };
    case "SAVE_ERROR":
      return {
        ...state,
        status: "saveFailed",
        stepKey: "publish",
        lastError: event.error,
      };
    case "ENABLE_START":
      return { ...state, status: "enabling", lastError: undefined };
    case "ENABLE_SUCCESS":
      return { ...state, status: "enabled", stepKey: "finish" };
    case "ENABLE_ERROR":
      return {
        ...state,
        status: "enableFailed",
        stepKey: "finish",
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

// 将协议枚举转成用户能理解的名称，避免在配置页直接暴露 openai_chat 这种内部值。
function apiFormatDisplayName(format: CodexApiFormat): string {
  return format === "openai_responses" ? "Responses API" : "Chat Completions";
}

// 判断旧配置里是否有可识别的显式协议值；未知字符串只用于说明，不参与 route 生成。
function normalizeApiFormat(value: unknown): CodexApiFormat | null {
  return value === "openai_responses" || value === "openai_chat" ? value : null;
}

// 配置页统一调用向导的数据层推断协议，保证 UI 文案和最终 route 保存结果一致。
function providerApiFormatSummary(provider: Provider): string {
  const inferredFormat = inferWizardApiFormat(provider);
  const explicitFormat = normalizeApiFormat(
    provider.meta?.apiFormat ??
      provider.settingsConfig?.apiFormat ??
      provider.settingsConfig?.api_format,
  );
  if (explicitFormat === inferredFormat) {
    return `${apiFormatDisplayName(inferredFormat)}（已显式设置）`;
  }
  if (explicitFormat) {
    return `${apiFormatDisplayName(inferredFormat)}（向导推断；已覆盖旧配置里的 ${apiFormatDisplayName(explicitFormat)}）`;
  }
  return `${apiFormatDisplayName(inferredFormat)}（向导推断；官方 GPT/O 优先 Responses，未知第三方默认 Chat）`;
}

// 判断协议是否由用户在向导里锁定；锁定后保存阶段不会再被连通性探测推荐覆盖。
function providerApiFormatSourceLabel(provider: Provider): string {
  return provider.meta?.apiFormatSource === "manual"
    ? "用户已锁定"
    : "可由探测推荐更新";
}

// 向导只暴露 Codex 当前支持的两个上游协议；历史 openai_messages 配置按 Chat 路径展示和保存。
function selectableApiFormat(
  provider: Provider,
): Extract<CodexApiFormat, "openai_responses" | "openai_chat"> {
  return inferWizardApiFormat(provider) === "openai_responses"
    ? "openai_responses"
    : "openai_chat";
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

// 给配置页提供三态说明：能在线读取、官方 OAuth 需登录、已有目录可继续或确实需要补全。
function providerConfigStatus(
  provider: Provider,
  codexOAuthStatus: CodexOAuthWizardStatus,
): {
  badge: string;
  badgeVariant: "outline" | "secondary" | "destructive";
  summary: string;
} {
  if (isWizardCodexOAuthSource(provider)) {
    if (codexOAuthStatus.isLoading) {
      return {
        badge: "正在检查 OAuth",
        badgeVariant: "outline",
        summary:
          "官方 Codex 使用 ChatGPT OAuth，不需要 API Key/Base URL；正在读取本机已托管的 ChatGPT 账号状态。",
      };
    }
    if (codexOAuthStatus.hasAnyAccount) {
      return {
        badge: "OAuth 已配置",
        badgeVariant: "secondary",
        summary: `已检测到 ${codexOAuthStatus.accountCount} 个可用 ChatGPT 账号；向导会使用官方 Codex 内置模型目录，启用后由 CCSwitchMulti 托管 OAuth 转发。`,
      };
    }
    return {
      badge: "需要 ChatGPT OAuth",
      badgeVariant: "destructive",
      summary:
        "官方 Codex 不使用 API Key/Base URL。请先在这里登录 ChatGPT 账号，否则保存后官方 GPT/O 路由会因为缺少 OAuth token 而无法真实转发。",
    };
  }
  const config = getWizardModelFetchConfig(provider);
  const isCatalogOnlyPlan = isWizardCatalogOnlyModelSource(provider);
  const isVolcengineAgentPlan =
    isWizardVolcengineAgentPlanModelSource(provider);
  if (isCatalogOnlyPlan && hasWizardModelCatalog(provider)) {
    return {
      badge: isVolcengineAgentPlan
        ? "缺在线凭据，使用内置目录"
        : "使用内置模型目录",
      badgeVariant: "secondary",
      summary: isVolcengineAgentPlan
        ? "火山 AgentPlan 当前没有推理 API Key 或 AK/SK，向导会保留已有 modelCatalog 继续生成路由。"
        : "当前 Plan 的模型枚举不走 OpenAI /models，向导会保留已有 modelCatalog 继续生成路由。",
    };
  }
  if (config) {
    return {
      badge: config.volcengineModelListAction
        ? "可通过火山 OpenAPI 获取模型"
        : "可自动获取模型",
      badgeVariant: "outline",
      summary: fetchConfigSummary(config),
    };
  }
  if (hasWizardModelCatalog(provider)) {
    return {
      badge: "已有模型目录，可继续",
      badgeVariant: "secondary",
      summary:
        "已有 modelCatalog，可以继续生成路由；进入获取模型列表步骤时仍会重新尝试 /models 在线读取。",
    };
  }
  return {
    badge: "需补全配置",
    badgeVariant: "destructive",
    summary: isCatalogOnlyPlan
      ? "当前 Plan 缺少推理 API Key 或专用模型列表凭据，且没有可用 modelCatalog"
      : "缺少 Base URL/API Key，且没有可用 modelCatalog",
  };
}

// 生成官方 Codex OAuth 跳过普通 /models 的文案；登录状态只影响可用性提示，不改变内置目录。
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
  return `官方 Codex OAuth 不调用普通 /models；${catalogText}，${authText}。`;
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

// 将用户在配置步骤选择的协议写回草稿 provider，并清空旧探测结果避免预览继续使用过期推荐。
function applyManualApiFormatToProvider(
  provider: Provider,
  apiFormat: CodexApiFormat,
): Provider {
  return {
    ...provider,
    meta: {
      ...(provider.meta ?? {}),
      apiFormat,
      apiFormatSource: "manual",
    },
    settingsConfig: {
      ...(provider.settingsConfig ?? {}),
      apiFormat,
    },
  };
}

export function CodexMultiRouterWizard({
  open,
  providers,
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
  const [draftPlanName, setDraftPlanName] = useState(
    CODEX_MULTI_ROUTER_DEFAULT_NAME,
  );
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

  const existingPlan = useMemo(
    () => providers.find((provider) => isCodexMultiRouterPlan(provider)),
    [providers],
  );
  const providerModelSources = useMemo(
    () => defaultWizardModelSources(providers),
    [providers],
  );
  const codexOAuthStatus = useMemo(
    () => ({
      isLoading: isCodexOauthStatusLoading,
      hasAnyAccount: hasCodexOauthAccount,
      accountCount: codexOauthAccounts.length,
    }),
    [
      codexOauthAccounts.length,
      hasCodexOauthAccount,
      isCodexOauthStatusLoading,
    ],
  );
  const hasCodexOAuthSources = useMemo(
    () => draftSources.some((provider) => isWizardCodexOAuthSource(provider)),
    [draftSources],
  );
  const hasUnauthenticatedCodexOAuthSources =
    hasCodexOAuthSources && !isCodexOauthStatusLoading && !hasCodexOauthAccount;
  const stepIndex = STEPS.findIndex((step) => step.key === flowState.stepKey);
  const currentStep = STEPS[stepIndex];
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

  // 用户手动选择协议时立即更新草稿，并废弃旧探测结果，避免保存时再次套用过期推荐。
  const handleProviderApiFormatChange = (
    providerId: string,
    apiFormat: CodexApiFormat,
  ) => {
    setDraftSources((current) =>
      current.map((provider) =>
        provider.id === providerId
          ? applyManualApiFormatToProvider(provider, apiFormat)
          : provider,
      ),
    );
    setConnectivityResults([]);
  };

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
    setDraftPlanName(existingPlan?.name ?? CODEX_MULTI_ROUTER_DEFAULT_NAME);
    setCatalogModelOrder(
      existingPlan?.settingsConfig?.modelCatalog?.models?.map(
        (model: CodexCatalogModel) => model.model,
      ) ?? null,
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
  }, [existingPlan, open, providerModelSources]);

  // 向导打开后仍要吸收用户新建/删除的普通 Codex provider，但不能重新派发 INIT。
  useEffect(() => {
    if (!open || !initializedOpenRef.current) return;
    setSavedPlan(existingPlan ?? null);
    setDraftSources((currentSources) => {
      const nextSourceById = new Map(
        providerModelSources.map((provider) => [provider.id, provider]),
      );
      const retainedSources = currentSources.filter((provider) =>
        nextSourceById.has(provider.id),
      );
      const retainedIds = new Set(
        retainedSources.map((provider) => provider.id),
      );
      const appendedSources = providerModelSources.filter(
        (provider) => !retainedIds.has(provider.id),
      );
      if (
        retainedSources.length === currentSources.length &&
        appendedSources.length === 0
      ) {
        return currentSources;
      }
      return [...retainedSources, ...appendedSources];
    });
    setModelFetchCards((currentCards) =>
      Object.fromEntries(
        providerModelSources.map((provider) => [
          provider.id,
          currentCards[provider.id] ?? defaultModelFetchCardState(provider),
        ]),
      ),
    );
  }, [existingPlan, open, providerModelSources]);

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

  // 添加或移除子 Agent 候选；候选只从最终保留模型中选择，最多 5 个。
  const toggleSpawnAgentModel = (model: string, checked: boolean) => {
    if (!checked) {
      setDraftSpawnAgentModels((current) =>
        current.filter((item) => item !== model),
      );
      return;
    }
    setDraftSpawnAgentModels((current) => {
      if (current.includes(model)) return current;
      if (current.length >= 5) {
        toast.error("子 Agent 候选最多只能选择 5 个模型。", {
          closeButton: true,
        });
        return current;
      }
      return [...current, model];
    });
  };

  // 调整子 Agent 候选顺序；这个顺序会写入 modelCatalog.spawnAgentModels。
  const moveSpawnAgentModel = (model: string, direction: -1 | 1) => {
    setDraftSpawnAgentModels((current) =>
      moveOrderedItem(current, model, direction),
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
      case "intro":
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            draftSources.length > 0 ? "reviewProviderConfig" : "needSources",
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
          nextStatus: "reviewProviderConfig",
          nextStepKey: "rename",
        });
        return;
      case "rename":
        if (!draftPlanName.trim()) {
          toast.error("请先填写 MultiRouter 名称。", { closeButton: true });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            configIssues.length > 0 ? "configIncomplete" : "readyToFetchModels",
          nextStepKey: "providerConfig",
        });
        return;
      case "providerConfig":
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            configIssues.length > 0 ? "configIncomplete" : "readyToFetchModels",
          nextStepKey: "fetchModels",
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
      case "fetchModels":
        if (
          connectivityResults.length > 0 &&
          !canContinueAfterConnectivity(connectivityResults)
        ) {
          dispatchFlow({
            type: "NEXT",
            nextStatus: "connectivityFailed",
            nextStepKey: "fetchModels",
          });
          recordWizardIssue({
            stage: "fetchModels",
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
        dispatchFlow({
          type: "NEXT",
          nextStatus:
            modelCollisions.length > 0
              ? "collisionReviewRequired"
              : "routePreview",
          nextStepKey: modelCollisions.length > 0 ? "collisions" : "routes",
        });
        return;
      case "collisions":
        setDraftSources(resolveWizardModelNameCollisions(draftSources));
        dispatchFlow({
          type: "NEXT",
          nextStatus: "routePreview",
          nextStepKey: "selectModels",
        });
        return;
      case "selectModels":
        if (activeCatalogModelOrder.length === 0) {
          toast.error("请至少保留一个模型。", { closeButton: true });
          return;
        }
        dispatchFlow({
          type: "NEXT",
          nextStatus: "routePreview",
          nextStepKey: "routes",
        });
        return;
      case "routes":
        dispatchFlow({
          type: "NEXT",
          nextStatus: "routePreview",
          nextStepKey: "spawnAgent",
        });
        return;
      case "spawnAgent":
        dispatchFlow({
          type: "NEXT",
          nextStatus: "published",
          nextStepKey: "publish",
        });
        return;
      case "publish":
        toast.info("请点击“保存并发布”写入 MultiRouter provider。", {
          closeButton: true,
        });
        return;
      case "finish":
        closeWizard(false);
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
    clearWizardIssuesForStage("fetchModels");
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
            config && !isCatalogOnlyPlan && !isCodexOAuth
              ? {
                  status: "loading",
                  message: config.volcengineModelListAction
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
          skippedCount += 1;
          nextSources.push(provider);
          setModelFetchCards((current) => ({
            ...current,
            [provider.id]: {
              status: "skipped",
              message: codexOAuthModelFetchMessage(
                beforeModels.length > 0,
                hasCodexOauthAccount,
              ),
              modelCount: beforeModels.length,
            },
          }));
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
            stage: "fetchModels",
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
        stage: "fetchModels",
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
    clearWizardIssuesForStage("fetchModels");
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
            stage: "fetchModels",
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
    clearWizardIssuesForStage("publish");
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
        },
      );
      if (existingPlan) {
        await providersApi.update(result.plan, "codex");
      } else {
        await providersApi.add(result.plan, "codex", false);
      }
      setSavedPlan(result.plan);
      setDraftSources(result.sourceProviders);
      await queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
      toast.success("MultiRouter 方案已保存。", { closeButton: true });
      dispatchFlow({ type: "SAVE_SUCCESS" });
    } catch (error) {
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "publish",
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
    clearWizardIssuesForStage("finish");
    try {
      await onEnablePlan(savedPlan);
      dispatchFlow({ type: "ENABLE_SUCCESS" });
      toast.success(
        "已启用多路模型，状态页已打开。请在 Codex 里发送一次请求，等待当前链路、监听、Codex 接管、路由入口和最近转发都成功后，会自动进入历史修复。",
        {
          closeButton: true,
          duration: 12000,
        },
      );
      closeWizard(false);
    } catch (error) {
      const message = formatWizardError(error);
      recordWizardIssue({
        stage: "finish",
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
        className="flex max-h-full w-[min(96vw,1280px)] min-h-0 flex-col rounded-lg border border-white/15 bg-background shadow-2xl"
      >
        <div className="flex shrink-0 items-start justify-between border-b px-5 py-4">
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
          <div className="space-y-1 overflow-y-auto border-r bg-muted/30 p-3">
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
            <div className="mb-4 rounded-lg border bg-muted/30 p-3 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline">状态机：{flowState.status}</Badge>
                <span className="text-muted-foreground">
                  {wizardStatusText(flowState)}
                </span>
              </div>
              {flowState.fetchSummary && (
                <div className="mt-2 text-xs text-muted-foreground">
                  最近一次获取模型：成功 {flowState.fetchSummary.successCount}
                  ，跳过 {flowState.fetchSummary.skippedCount}，失败{" "}
                  {flowState.fetchSummary.failedCount}
                </div>
              )}
              {flowState.connectivitySummary && (
                <div className="mt-2 text-xs text-muted-foreground">
                  最近一次 Chat / Responses 基础协议测试：通过{" "}
                  {flowState.connectivitySummary.passCount}，警告{" "}
                  {flowState.connectivitySummary.warnCount}，跳过{" "}
                  {flowState.connectivitySummary.skippedCount}，失败{" "}
                  {flowState.connectivitySummary.failCount}
                </div>
              )}
              {flowState.lastError && (
                <div className="mt-2 text-xs text-destructive">
                  {flowState.lastError}
                </div>
              )}
            </div>
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
            <div className="mb-4 rounded-lg border p-3 text-sm">
              <div className="font-medium">本步骤异常与继续条件</div>
              <ul className="mt-2 space-y-1 text-xs text-muted-foreground">
                {STEP_RULES[currentStep.key].errors.map((error) => (
                  <li key={error}>{error}</li>
                ))}
              </ul>
              <div className="mt-2 text-xs text-muted-foreground">
                可继续判断：{STEP_RULES[currentStep.key].canContinue}
              </div>
            </div>

            {currentStep.key === "intro" && (
              <div className="space-y-4">
                <div className="rounded-lg border p-4">
                  <div className="flex items-center gap-2 font-medium">
                    <Wand2 className="h-4 w-4" />
                    这套向导会帮你完成 7 件事
                  </div>
                  <div className="mt-4 grid gap-3 md:grid-cols-2">
                    {[
                      "把 OpenAI、中转站、DeepSeek、Qwen、本地模型接进来",
                      "给新的 MultiRouter 方案命名，方便后续识别",
                      "自动读取模型列表，并处理官方模型和中转模型重名",
                      "汇总所有模型后排序，并剔除旧模型或不想暴露的模型",
                      "从最终模型池里选择并排序 5 个子 Agent 候选",
                      "按模型名称生成分流规则，让 Codex 自动选上游",
                      "启用后等待真实请求成功，再带你修复历史记录",
                    ].map((item, index) => (
                      <div
                        key={item}
                        className="flex gap-3 rounded-md border bg-muted/30 p-3 text-sm"
                      >
                        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-medium text-primary-foreground">
                          {index + 1}
                        </span>
                        <span className="leading-6">{item}</span>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="rounded-lg border p-4 text-sm leading-6 text-muted-foreground">
                  你不用手动改配置文件。向导会先预览模型源、连通性和路由规则，只有点击“保存并发布”后才写入本地
                  providers 数据库；点击“启用这个多路路由”后才会接管当前 Codex。
                </div>
                <div className="rounded-lg border bg-muted/30 p-4 text-xs leading-6 text-muted-foreground">
                  技术备注：Codex 最后仍只连接本机 127.0.0.1:15721，
                  CCSwitchMulti 会根据请求里的 model 把流量发到对应上游。
                </div>
              </div>
            )}

            {currentStep.key === "sources" && (
              <div className="space-y-4">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm text-muted-foreground">
                    当前识别到 {draftSources.length} 个普通 Codex provider
                    可作为模型源。
                  </p>
                  <Button onClick={onCreateProvider}>
                    <Server className="mr-2 h-4 w-4" />
                    添加 Provider
                  </Button>
                </div>
                <div className="max-h-[min(42vh,28rem)] overflow-y-auto pr-2">
                  <div className="grid gap-3 md:grid-cols-2">
                    {draftSources.map((provider) => (
                      <div key={provider.id} className="rounded-lg border p-3">
                        <div className="font-medium">{provider.name}</div>
                        <div className="mt-1 text-xs text-muted-foreground">
                          {provider.id}
                        </div>
                        <Badge variant="outline" className="mt-3">
                          {modelSourceSummary(provider)}
                        </Badge>
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

            {currentStep.key === "rename" && (
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

            {currentStep.key === "providerConfig" && (
              <div className="space-y-3">
                {hasCodexOAuthSources && (
                  <div className="rounded-lg border border-blue-500/30 bg-blue-500/10 p-3 text-sm leading-6 text-blue-950 dark:text-blue-100">
                    官方 Codex / OpenAI Official 源必须先完成 ChatGPT OAuth
                    登录。它不需要 API Key，也不会调用普通
                    /models；向导会使用内置官方模型目录，
                    登录状态决定启用后能否真实转发官方 GPT/O 请求。
                  </div>
                )}
                {configIssues.length > 0 && (
                  <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-900 dark:text-amber-200">
                    {configIssues.length} 个 provider
                    不能自动获取模型。你可以补全 Base URL/API
                    Key，或继续使用已有 modelCatalog。
                  </div>
                )}
                <div className="max-h-[min(46vh,32rem)] space-y-3 overflow-y-auto pr-2">
                  {draftSources.map((provider) => {
                    const status = providerConfigStatus(
                      provider,
                      codexOAuthStatus,
                    );
                    const isCodexOAuth = isWizardCodexOAuthSource(provider);
                    return (
                      <div
                        key={provider.id}
                        className="rounded-lg border p-4 text-sm"
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="font-medium">{provider.name}</div>
                          <Badge variant={status.badgeVariant}>
                            {status.badge}
                          </Badge>
                        </div>
                        <div className="mt-2 text-muted-foreground">
                          {status.summary}
                        </div>
                        <div className="mt-2 text-xs text-muted-foreground">
                          API 格式：{providerApiFormatSummary(provider)}
                        </div>
                        {isCodexOAuth ? (
                          <div className="mt-4 rounded-lg border bg-muted/30 p-3">
                            <div className="mb-3 text-xs leading-5 text-muted-foreground">
                              此 provider 将固定使用 CCSwitchMulti 托管的 Codex
                              OAuth。登录后可继续使用默认账号；需要指定账号时，请先在
                              Provider 配置页绑定账号。
                            </div>
                            <CodexOAuthSection />
                          </div>
                        ) : (
                          <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                            <div className="text-xs text-muted-foreground">
                              协议选择：{providerApiFormatSourceLabel(provider)}
                            </div>
                            <Select
                              value={selectableApiFormat(provider)}
                              onValueChange={(value) =>
                                handleProviderApiFormatChange(
                                  provider.id,
                                  value as CodexApiFormat,
                                )
                              }
                            >
                              <SelectTrigger
                                aria-label={`${provider.name} API 格式`}
                                className="w-full sm:w-[220px]"
                              >
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="openai_responses">
                                  Responses API
                                </SelectItem>
                                <SelectItem value="openai_chat">
                                  Chat Completions
                                </SelectItem>
                              </SelectContent>
                            </Select>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {currentStep.key === "fetchModels" && (
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

            {currentStep.key === "collisions" && (
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

            {currentStep.key === "selectModels" && (
              <div className="space-y-4">
                <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                  这里决定最终写入 MultiRouter modelCatalog 和各 route
                  的模型。取消勾选会把旧模型或不想暴露的模型从最终路由里剔除；子
                  Agent 候选会在下一步单独选择。
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

            {currentStep.key === "routes" && (
              <div className="space-y-3">
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
                    <div className="mt-2 rounded-md bg-muted px-3 py-2 text-xs leading-5 text-muted-foreground">
                      {cacheCapabilitySummary(route.capabilities?.codexCache)}
                    </div>
                  </div>
                ))}
              </div>
            )}

            {currentStep.key === "spawnAgent" && (
              <div className="space-y-4">
                <div className="rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
                  子 Agent 候选只从上一步保留的模型里选，最多 5
                  个。顺序越靠前，越适合放常用或稳定的模型；如果不选，保存时会默认取最终模型列表前
                  5 个。
                </div>
                <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)]">
                  <div className="rounded-lg border">
                    <div className="border-b px-3 py-2 text-sm font-medium">
                      已选候选 {activeSpawnAgentModels.length} / 5
                    </div>
                    <div className="max-h-80 overflow-auto">
                      {activeSpawnAgentModels.length === 0 && (
                        <div className="p-3 text-sm text-muted-foreground">
                          暂未选择，保存时会使用最终模型列表前 5 个。
                        </div>
                      )}
                      {activeSpawnAgentModels.map((model, index) => (
                        <div
                          key={model}
                          className="grid grid-cols-[2rem_minmax(0,1fr)_5rem_2rem] items-center gap-2 border-b px-3 py-2 text-sm last:border-b-0"
                        >
                          <Badge variant="outline">#{index + 1}</Badge>
                          <span className="truncate font-medium">{model}</span>
                          <div className="flex gap-1">
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8"
                              disabled={index === 0}
                              onClick={() => moveSpawnAgentModel(model, -1)}
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
                                index === activeSpawnAgentModels.length - 1
                              }
                              onClick={() => moveSpawnAgentModel(model, 1)}
                              title="下移"
                            >
                              <ArrowDown className="h-4 w-4" />
                            </Button>
                          </div>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-destructive"
                            onClick={() => toggleSpawnAgentModel(model, false)}
                            title="移除"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      ))}
                    </div>
                  </div>
                  <div className="rounded-lg border">
                    <div className="border-b px-3 py-2 text-sm font-medium">
                      最终模型池
                    </div>
                    <div className="max-h-80 overflow-auto">
                      {previewModels.map((model) => {
                        const selected = activeSpawnAgentModels.includes(
                          model.model,
                        );
                        return (
                          <label
                            key={model.model}
                            className="grid grid-cols-[2rem_minmax(0,1fr)] items-center gap-2 border-b px-3 py-2 text-sm last:border-b-0"
                          >
                            <input
                              type="checkbox"
                              className="h-4 w-4"
                              checked={selected}
                              onChange={(event) =>
                                toggleSpawnAgentModel(
                                  model.model,
                                  event.target.checked,
                                )
                              }
                            />
                            <span className="truncate">{model.model}</span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                </div>
              </div>
            )}

            {currentStep.key === "publish" && (
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

            {currentStep.key === "finish" && (
              <div className="space-y-4">
                <div className="rounded-lg border p-4 text-sm leading-6 text-muted-foreground">
                  保存完成后，请显式启用这个多路路由。启用成功后向导会自动关闭，并露出
                  MultiRouter 状态页；保持 CCSwitchMulti 运行，去 Codex
                  里发送一次请求，状态页五项成功后会提示配置成功并跳到历史修复。
                  历史修复会继续指导你按顺序加载历史、预览修复、确认写入、重启
                  Codex，并打开 GitHub 仓库点 Star。
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
                  会话里确认路由、流式响应、工具调用和历史修复流程。
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

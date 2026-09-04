import { isUserCodexRoutingPlan as isCodexMultiRouterPlan } from "./codexProviderKind";
import type {
  CodexApiFormat,
  CodexCacheConfig,
  CodexCatalogModel,
  CodexModelCatalogConfig,
  CodexOfficialAuthConfig,
  CodexRoutingConfig,
  CodexRoutingConfigV2,
  CodexRoutingAuthPolicy,
  CodexRoutingRoute,
  CodexRoutingRouteV2,
  CodexSubagentVersion,
  Provider,
} from "@/types";
import type { CodexSubagentV2Profile } from "@/types/codexSubagentV2";
import {
  normalizeHostedToolsConfig,
  type HostedToolsConfig,
} from "./hostedTools";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import {
  codexPlanModelListAction,
  isCodexCatalogOnlyPlanModelFetch,
  isCodexVolcengineAgentPlanModelFetch,
} from "@/utils/codexPlanModelFetch";
import { normalizeCodexSubagentVersion } from "@/utils/codexSubagentVersion";

export const CODEX_MULTI_ROUTER_WIZARD_DISMISSED_KEY =
  "ccswitchmulti.codexMultiRouterWizard.dismissed";
export const CODEX_MULTI_ROUTER_DEFAULT_ID = "codex-multirouter";
export const CODEX_MULTI_ROUTER_DEFAULT_NAME = "Codex MultiRouter";
export const CODEX_MULTI_ROUTER_PROXY_BASE_URL = "http://127.0.0.1:15721/v1";

export interface WizardModelFetchConfig {
  baseUrl: string;
  apiKey: string;
  isFullUrl?: boolean;
  modelsUrl?: string;
  customUserAgent?: string;
  volcengineModelListAction?: string;
  volcengineAccessKeyId?: string;
  volcengineSecretAccessKey?: string;
}

export interface WizardPlanBuildResult {
  plan: Provider;
  sourceProviders: Provider[];
}

export interface WizardPlanBuildOptions {
  planId?: string;
  planName?: string;
  catalogModelOrder?: string[];
  spawnAgentModels?: string[];
  subagentVersion?: CodexSubagentVersion;
  officialAuth?: CodexOfficialAuthConfig;
  hostedTools?: HostedToolsConfig;
}

export interface WizardConfigIssue {
  providerId: string;
  providerName: string;
  reason: string;
}

export interface WizardModelNameCollision {
  upstreamModel: string;
  providerIds: string[];
  canonicalProviderIds: string[];
}

export type WizardConnectivityStatus = "pass" | "warn" | "fail" | "skipped";

export interface WizardConnectivityResult {
  providerId: string;
  providerName: string;
  model: string;
  status: WizardConnectivityStatus;
  canContinue: boolean;
  detail: string;
  recommendedApiFormat?: CodexApiFormat;
  url?: string;
  httpStatus?: number | null;
}

export interface WizardProtocolProbe {
  ok: boolean;
  detail: string;
  url?: string;
  httpStatus?: number | null;
}

// 读取 provider 上绑定的托管 Codex OAuth 账号；未绑定时交给后端使用默认账号。
export function readWizardCodexOAuthAccountId(
  provider: Provider,
): string | undefined {
  const meta = provider.meta as
    | (Provider["meta"] & {
        auth_binding?: { accountId?: string; account_id?: string };
      })
    | undefined;
  const authBinding = (meta?.authBinding ?? meta?.auth_binding) as
    | { accountId?: string; account_id?: string }
    | undefined;
  const accountId = authBinding?.accountId ?? authBinding?.account_id;
  return typeof accountId === "string" && accountId.trim()
    ? accountId.trim()
    : undefined;
}

// 判断模型源是否是官方 Codex OAuth 路径；这类 provider 使用 ChatGPT 登录，不走 API Key + /models。
export function isWizardCodexOAuthSource(provider: Provider): boolean {
  const config = provider.settingsConfig ?? {};
  const meta = provider.meta as
    | (Provider["meta"] & {
        auth_binding?: {
          source?: string;
          authProvider?: string;
          auth_provider?: string;
        };
      })
    | undefined;
  const providerType = String(
    meta?.providerType ?? config.providerType ?? "",
  ).toLowerCase();
  const authBinding = (meta?.authBinding ?? meta?.auth_binding) as
    | {
        source?: string;
        authProvider?: string;
        auth_provider?: string;
      }
    | undefined;
  const authSource = String(
    authBinding?.source ?? config.auth?.source ?? "",
  ).toLowerCase();
  const authProvider = String(
    authBinding?.authProvider ??
      authBinding?.auth_provider ??
      config.auth?.authProvider ??
      config.auth?.auth_provider ??
      "",
  ).toLowerCase();
  const authMode = String(config.auth?.auth_mode ?? "").toLowerCase();
  const baseUrl = readWizardProviderBaseUrl(provider).toLowerCase();
  const idOrName = `${provider.id} ${provider.name}`.toLowerCase();
  return (
    provider.category === "official" ||
    providerType.includes("codex_oauth") ||
    authSource === "managed_codex_oauth" ||
    authProvider === "codex_oauth" ||
    authMode === "chatgpt" ||
    baseUrl.includes("chatgpt.com/backend-api/codex") ||
    idOrName === "openai openai" ||
    (idOrName.includes("openai") && idOrName.includes("official"))
  );
}

// 判断模型源是否是官方/OAuth 路径；这些 provider 必须通过 ChatGPT 专用接口获取目录。
function isOfficialCodexSource(provider: Provider): boolean {
  return isWizardCodexOAuthSource(provider);
}

// The built-in official seed represents Codex's currently signed-in account.
// Explicit managed-account metadata keeps using CCSM's OAuth manager instead.
function isWizardNativeCodexAuthSource(provider: Provider): boolean {
  if (provider.id !== "codex-official" || provider.category !== "official") {
    return false;
  }
  const config = provider.settingsConfig ?? {};
  const meta = provider.meta ?? {};
  const legacyMeta = meta as typeof meta & {
    auth_binding?: { source?: string; accountId?: string; account_id?: string };
  };
  const binding = (meta.authBinding ?? legacyMeta.auth_binding) as
    | { source?: string; accountId?: string; account_id?: string }
    | undefined;
  const source = String(
    binding?.source ?? config.auth?.source ?? "",
  ).toLowerCase();
  const accountId =
    binding?.accountId ??
    binding?.account_id ??
    readWizardCodexOAuthAccountId(provider);
  const authMode = String(config.auth?.auth_mode ?? "").toLowerCase();
  const providerType = String(
    meta.providerType ?? config.providerType ?? "",
  ).toLowerCase();
  return (
    !accountId &&
    authMode !== "chatgpt" &&
    !providerType.includes("codex_oauth") &&
    source !== "managed_account" &&
    source !== "managed_codex_oauth"
  );
}

// 读取 Codex provider 的真实持久化模型目录；缺失或结构异常时返回空目录，不能伪造 OAuth 模型权限。
export function readWizardModelCatalog(
  provider: Provider,
): CodexCatalogModel[] {
  const models = provider.settingsConfig?.modelCatalog?.models;
  if (!Array.isArray(models)) {
    return [];
  }
  return models.filter(
    (model): model is CodexCatalogModel =>
      typeof model === "object" &&
      model !== null &&
      typeof (model as CodexCatalogModel).model === "string" &&
      Boolean((model as CodexCatalogModel).model.trim()),
  );
}

function isWizardModelEnabled(model: CodexCatalogModel): boolean {
  return model.enabled !== false;
}

// 判断 provider 是否是 MultiRouter 方案；向导只把普通 provider 当作上游模型源。
export { isUserCodexRoutingPlan as isCodexMultiRouterPlan } from "./codexProviderKind";

export const DEFAULT_CODEX_OFFICIAL_AUTH: CodexOfficialAuthConfig = {
  mode: "desktop_current_login",
};

export function codexOfficialAuthRouteBinding(
  officialAuth: CodexOfficialAuthConfig,
): CodexRoutingAuthPolicy {
  switch (officialAuth.mode) {
    case "managed_oauth":
      return {
        source: "managed_codex_oauth",
        ...(officialAuth.accountId
          ? { accountId: officialAuth.accountId }
          : {}),
      };
    case "account_pool":
      return { source: "account_pool" };
    case "desktop_current_login":
    default:
      return { source: "native_codex_auth" };
  }
}

function officialAuthFromRoute(
  route: CodexRoutingRoute | CodexRoutingRouteV2,
): CodexOfficialAuthConfig | undefined {
  const auth =
    "authPolicy" in route
      ? route.authPolicy
      : "upstream" in route
        ? route.upstream?.auth
        : undefined;
  switch (auth?.source) {
    case "native_codex_auth":
      return { mode: "desktop_current_login" };
    case "managed_account":
    case "managed_codex_oauth":
      return {
        mode: "managed_oauth",
        ...(auth.accountId ? { accountId: auth.accountId } : {}),
      };
    case "account_pool":
      return { mode: "account_pool" };
    default:
      return undefined;
  }
}

export function inferCodexOfficialAuth(
  routing?: CodexRoutingConfig | CodexRoutingConfigV2 | null,
): CodexOfficialAuthConfig | undefined {
  if (routing && "officialAuth" in routing && routing.officialAuth) {
    return routing.officialAuth;
  }
  const inferred = (routing?.routes ?? [])
    .map(officialAuthFromRoute)
    .filter((auth): auth is CodexOfficialAuthConfig => Boolean(auth));
  if (inferred.length === 0) return undefined;
  const first = JSON.stringify(inferred[0]);
  return inferred.every((auth) => JSON.stringify(auth) === first)
    ? inferred[0]
    : undefined;
}

// 从 Codex provider 配置里读取模型列表/连通性探测可用的推理 API Key。
function readWizardProviderApiKey(provider: Provider): string {
  const config = provider.settingsConfig ?? {};
  const auth = config.auth ?? {};
  return String(
    auth.OPENAI_API_KEY ??
      config.apiKey ??
      config.api_key ??
      config.experimental_bearer_token ??
      "",
  ).trim();
}

// 从 provider 配置里提取可调用 /models 的参数；官方 OAuth provider 没有普通 Base URL 时会被跳过。
export function getWizardModelFetchConfig(
  provider: Provider,
): WizardModelFetchConfig | null {
  if (isWizardCodexOAuthSource(provider)) return null;
  const config = provider.settingsConfig ?? {};
  const baseUrl = readWizardProviderBaseUrl(provider);
  const accessKeyId = provider.meta?.usage_script?.accessKeyId;
  const secretAccessKey = provider.meta?.usage_script?.secretAccessKey;
  const apiKey = readWizardProviderApiKey(provider);
  const volcengineModelListAction = codexPlanModelListAction({
    baseUrl,
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    apiKey,
    accessKeyId,
    secretAccessKey,
  });
  if (!baseUrl || (!apiKey && !volcengineModelListAction)) return null;
  return {
    baseUrl,
    apiKey,
    isFullUrl: Boolean(provider.meta?.isFullUrl ?? config.isFullUrl),
    modelsUrl:
      typeof config.modelsUrl === "string" ? config.modelsUrl : undefined,
    customUserAgent: provider.meta?.customUserAgent,
    ...(volcengineModelListAction
      ? {
          volcengineModelListAction,
          volcengineAccessKeyId: accessKeyId,
          volcengineSecretAccessKey: secretAccessKey,
        }
      : {}),
  };
}

// 判断 provider 是否已有可用于路由的真实模型目录。
export function hasWizardModelCatalog(provider: Provider): boolean {
  return readWizardModelCatalog(provider).length > 0;
}

// 从 Codex provider 配置中读取 Base URL，兼容已扁平化字段和原始 config.toml 字符串。
export function readWizardProviderBaseUrl(provider: Provider): string {
  const config = provider.settingsConfig ?? {};
  const direct = String(
    config.base_url ?? config.baseURL ?? config.baseUrl ?? "",
  ).trim();
  if (direct) return direct;
  return typeof config.config === "string"
    ? (extractCodexBaseUrl(config.config) ?? "").trim()
    : "";
}

// 判断向导中的模型源是否属于只能使用内置目录的 Plan provider。
export function isWizardCatalogOnlyModelSource(provider: Provider): boolean {
  return isCodexCatalogOnlyPlanModelFetch({
    baseUrl: readWizardProviderBaseUrl(provider),
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
    apiKey: readWizardProviderApiKey(provider),
    accessKeyId: provider.meta?.usage_script?.accessKeyId,
    secretAccessKey: provider.meta?.usage_script?.secretAccessKey,
  });
}

// 判断向导中的模型源是否是火山 AgentPlan，供 UI 展示专用 OpenAPI 路径。
export function isWizardVolcengineAgentPlanModelSource(
  provider: Provider,
): boolean {
  return isCodexVolcengineAgentPlanModelFetch({
    baseUrl: readWizardProviderBaseUrl(provider),
    partnerPromotionKey: provider.meta?.partnerPromotionKey,
    providerName: provider.name,
  });
}

// 给状态机提供配置缺口列表；已有模型目录的 provider 可以继续进入路由预览，不强制要求 /models 可抓。
export function getWizardConfigIssues(
  providers: Provider[],
): WizardConfigIssue[] {
  return providers
    .filter((provider) => {
      if (isWizardCodexOAuthSource(provider)) return false;
      const hasCatalog = hasWizardModelCatalog(provider);
      if (isWizardCatalogOnlyModelSource(provider)) return !hasCatalog;
      return !getWizardModelFetchConfig(provider) && !hasCatalog;
    })
    .map((provider) => ({
      providerId: provider.id,
      providerName: provider.name,
      reason: isWizardCatalogOnlyModelSource(provider)
        ? "当前 Plan 缺少推理 API Key 或专用模型列表凭据，且没有可用 modelCatalog。"
        : "缺少 Base URL/API Key，且当前没有可用 modelCatalog。",
    }));
}

export interface MergeFetchedWizardModelsOptions {
  preserveExistingSelection?: boolean;
}

// 把 /models 返回值合并进 provider modelCatalog；可选择把已有目录当作用户保留列表，只刷新元数据不追加已删除模型。
export function mergeFetchedModelsIntoWizardProvider(
  provider: Provider,
  fetchedModels: FetchedModel[],
  options: MergeFetchedWizardModelsOptions = {},
): Provider {
  const existingModels = readWizardModelCatalog(provider);
  const byModel = new Map<string, CodexCatalogModel>();
  const byFetchedModel = new Map<string, string>();
  for (const model of existingModels) {
    byModel.set(model.model, model);
    const visibleModel = model.model?.trim();
    if (visibleModel) {
      byFetchedModel.set(visibleModel, model.model);
    }
    const upstreamModel = (
      model.upstreamModel ??
      model.upstream_model ??
      model.model
    )?.trim();
    if (upstreamModel) {
      byFetchedModel.set(upstreamModel, model.model);
    }
  }
  const shouldAppendFetchedModels =
    !options.preserveExistingSelection || existingModels.length === 0;
  for (const fetched of fetchedModels) {
    const modelId = fetched.id.trim();
    if (!modelId) continue;
    const visibleModelId = byFetchedModel.get(modelId) ?? modelId;
    const existing = byModel.get(visibleModelId);
    if (!existing && !shouldAppendFetchedModels) continue;
    byModel.set(visibleModelId, {
      ...(existing ?? {}),
      model: visibleModelId,
      upstreamModel:
        existing?.upstreamModel ?? existing?.upstream_model ?? modelId,
      displayName: existing?.displayName ?? visibleModelId,
      ...(fetched.contextWindow
        ? { contextWindow: fetched.contextWindow }
        : {}),
      ...(fetched.inputModalities && fetched.inputModalities.length > 0
        ? {
            inputModalities: fetched.inputModalities as Array<"text" | "image">,
            input_modalities: fetched.inputModalities as Array<
              "text" | "image"
            >,
          }
        : {}),
      ...(fetched.supportsImage !== undefined && fetched.supportsImage !== null
        ? {
            supportsImage: fetched.supportsImage,
            supports_image: fetched.supportsImage,
          }
        : {}),
    });
  }
  const models = Array.from(byModel.values());
  const allowedModels = new Set(models.map((model) => model.model));
  const rawSpawnAgentModels =
    provider.settingsConfig?.modelCatalog?.spawnAgentModels;
  const spawnAgentModels = Array.isArray(rawSpawnAgentModels)
    ? rawSpawnAgentModels
        .filter(
          (model): model is string =>
            typeof model === "string" && allowedModels.has(model),
        )
        .slice(0, 5)
    : undefined;
  return {
    ...provider,
    settingsConfig: {
      ...provider.settingsConfig,
      modelCatalog: {
        ...(provider.settingsConfig?.modelCatalog ?? {}),
        models,
        ...(spawnAgentModels ? { spawnAgentModels } : {}),
      },
    },
  };
}

// 判断某个模型源是否应该优先保留原始可见模型名；官方/订阅源是重名冲突的 canonical 侧。
function isCanonicalModelSource(provider: Provider): boolean {
  return isOfficialCodexSource(provider);
}

// 收集重名模型冲突，供向导进入“重名确认”状态并展示需要用户理解的别名策略。
export function collectWizardModelNameCollisions(
  providers: Provider[],
): WizardModelNameCollision[] {
  const ownersByUpstream = new Map<string, Provider[]>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider).filter(
      isWizardModelEnabled,
    )) {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      if (!upstream) continue;
      const owners = ownersByUpstream.get(upstream) ?? [];
      owners.push(provider);
      ownersByUpstream.set(upstream, owners);
    }
  }
  return Array.from(ownersByUpstream.entries())
    .filter(([, owners]) => owners.length > 1)
    .map(([upstreamModel, owners]) => ({
      upstreamModel,
      providerIds: owners.map((owner) => owner.id),
      canonicalProviderIds: owners
        .filter(isCanonicalModelSource)
        .map((owner) => owner.id),
    }));
}

// 把 provider 展示名清理成可放进模型 ID 的稳定后缀；优先使用用户能看懂的名称，避免泄露自动生成 ID。
function providerNameSuffix(provider: Provider): string {
  const cleanedName = provider.name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (cleanedName) return cleanedName;
  return (
    provider.id
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "provider"
  );
}

// 为非官方重名模型生成稳定别名，保留 upstreamModel 指向真实上游模型名。
function aliasModelName(provider: Provider, modelName: string): string {
  return `${modelName}-${providerNameSuffix(provider)}`;
}

// 从已处理别名的模型目录生成 route 级模型映射；后端物化 targetProviderId
// 时只读取原 provider 配置，必须靠这里把可见别名改回真实上游模型名。
function buildWizardRouteModelMap(
  provider: Provider,
): Record<string, string> | undefined {
  const entries = readWizardModelCatalog(provider)
    .map((model) => {
      const visibleModel = model.model?.trim();
      const upstreamModel = (
        model.upstreamModel ??
        model.upstream_model ??
        model.model
      )?.trim();
      return visibleModel && upstreamModel && visibleModel !== upstreamModel
        ? [visibleModel, upstreamModel]
        : null;
    })
    .filter((entry): entry is [string, string] => Boolean(entry));
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

// 判断模型目录是否主要是 OpenAI 官方 GPT/O 系列；这些模型应优先保持 Responses 原生链路。
function hasOpenAiResponsesNativeModels(provider: Provider): boolean {
  if (!isOfficialCodexSource(provider)) return false;
  return readWizardModelCatalog(provider).some((model) => {
    const upstream = (
      model.upstreamModel ??
      model.upstream_model ??
      model.model ??
      ""
    )
      .trim()
      .toLowerCase();
    return upstream.startsWith("gpt-") || /^o\d/.test(upstream);
  });
}

// 检测多个 provider 暴露的同名模型；官方保留原名，第三方/中转站自动生成可见别名。
export function resolveWizardModelNameCollisions(
  providers: Provider[],
): Provider[] {
  const ownersByUpstream = new Map<string, Provider[]>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider)) {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      if (!upstream) continue;
      const owners = ownersByUpstream.get(upstream) ?? [];
      owners.push(provider);
      ownersByUpstream.set(upstream, owners);
    }
  }

  return providers.map((provider) => {
    const nextModels = readWizardModelCatalog(provider).map((model) => {
      const upstream =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      const owners = ownersByUpstream.get(upstream) ?? [];
      if (owners.length <= 1 || isCanonicalModelSource(provider)) {
        return { ...model, upstreamModel: upstream };
      }
      return {
        ...model,
        model: aliasModelName(provider, upstream),
        displayName: model.displayName ?? aliasModelName(provider, upstream),
        upstreamModel: upstream,
      };
    });
    return {
      ...provider,
      settingsConfig: {
        ...provider.settingsConfig,
        modelCatalog: {
          ...(provider.settingsConfig?.modelCatalog ?? {}),
          models: nextModels,
        },
      },
    };
  });
}

// 按 provider 名称和模型名推断默认前缀；这些前缀只作为向导初始规则，后续可在工作台细调。
export function inferWizardRoutePrefixes(provider: Provider): string[] {
  const text = `${provider.id} ${provider.name} ${provider.category ?? ""} ${
    provider.meta?.providerType ?? ""
  }`.toLowerCase();
  const models = readWizardModelCatalog(provider)
    .filter(isWizardModelEnabled)
    .map((model) => model.model.toLowerCase());
  const has = (value: string) =>
    text.includes(value) || models.some((model) => model.startsWith(value));
  const prefixes = new Set<string>();
  if (has("openai") || has("gpt")) prefixes.add("gpt");
  if (has("openai") || models.some((model) => /^o\d/.test(model))) {
    prefixes.add("o");
  }
  if (has("deepseek")) prefixes.add("deepseek");
  if (has("qwen")) prefixes.add("qwen");
  if (has("ollama") || has("vllm") || has("local")) prefixes.add("local");
  return Array.from(prefixes);
}

// 推断 route 上游协议；官方 GPT/O 模型优先走 Responses，未知第三方默认走 Chat Completions。
export function inferWizardApiFormat(provider: Provider): CodexApiFormat {
  const config = provider.settingsConfig ?? {};
  if (hasOpenAiResponsesNativeModels(provider)) {
    return "openai_responses";
  }
  return (
    provider.meta?.apiFormat ??
    config.apiFormat ??
    config.api_format ??
    "openai_chat"
  );
}

// 推断 provider 的缓存能力；这里只生成安全默认，不会让自动缓存平台收到 OpenAI 私有参数。
export function inferWizardCacheConfig(provider: Provider): CodexCacheConfig {
  const text = `${provider.id} ${provider.name} ${provider.category ?? ""} ${
    provider.meta?.providerType ?? ""
  }`.toLowerCase();
  const models = readWizardModelCatalog(provider).map((model) =>
    String(model.upstreamModel ?? model.upstream_model ?? model.model)
      .trim()
      .toLowerCase(),
  );
  const hasModel = (needle: string) =>
    models.some((model) => model.includes(needle));
  if (
    isOfficialCodexSource(provider) ||
    hasOpenAiResponsesNativeModels(provider)
  ) {
    return {
      cacheMode: "openai_prompt_cache",
      supportsPromptCacheKey: true,
      supportsPromptCacheRetention: true,
      promptCacheKey: provider.meta?.promptCacheKey,
      promptCacheRetention: provider.meta?.promptCacheRetention,
      usageFields: [
        "usage.input_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cached_tokens",
      ],
    };
  }
  if (text.includes("deepseek") || hasModel("deepseek")) {
    return {
      cacheMode: "deepseek_context_cache",
      usageFields: [
        "usage.prompt_cache_hit_tokens",
        "usage.prompt_cache_miss_tokens",
      ],
    };
  }
  if (
    text.includes("z.ai") ||
    text.includes("zai") ||
    text.includes("glm") ||
    hasModel("glm")
  ) {
    return {
      cacheMode: "glm_context_cache",
      usageFields: ["usage.prompt_tokens_details.cached_tokens"],
    };
  }
  if (text.includes("dashscope") || text.includes("qwen") || hasModel("qwen")) {
    return {
      cacheMode: "qwen_context_cache",
      usageFields: [
        "usage.input_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cached_tokens",
        "usage.prompt_tokens_details.cache_creation_input_tokens",
      ],
    };
  }
  return { cacheMode: "unknown" };
}

// 每个 provider 默认探测其 modelCatalog 暴露的全部可见模型；这是用户显式点击的真实请求，不在向导自动执行。
export function getWizardConnectivityProbeModels(provider: Provider): string[] {
  return Array.from(
    new Set(
      readWizardModelCatalog(provider)
        .map((model) => model.model?.trim())
        .filter((model): model is string => Boolean(model)),
    ),
  );
}

// 将真实 `/v1/responses` 探测结果归类为“可继续/阻塞”；Chat-only provider 的 Responses 失败不是阻塞。
export function classifyWizardConnectivityResult(args: {
  provider: Provider;
  model: string;
  ok: boolean;
  detail: string;
  url?: string;
  httpStatus?: number | null;
}): WizardConnectivityResult {
  const apiFormat = inferWizardApiFormat(args.provider);
  if (args.ok) {
    return {
      providerId: args.provider.id,
      providerName: args.provider.name,
      model: args.model,
      status: "pass",
      canContinue: true,
      detail: "直接 /v1/responses 探测通过。",
      url: args.url,
      httpStatus: args.httpStatus,
    };
  }

  const chatOnlyCanContinue = apiFormat === "openai_chat";
  return {
    providerId: args.provider.id,
    providerName: args.provider.name,
    model: args.model,
    status: chatOnlyCanContinue ? "warn" : "fail",
    canContinue: chatOnlyCanContinue,
    detail: chatOnlyCanContinue
      ? `直接 /v1/responses 失败，但该 provider 配置为 Chat Completions；运行时会由 MultiRouter 转换到 /chat/completions。上游返回：${args.detail}`
      : `该 provider 配置为 Responses 直连，/v1/responses 失败会阻塞真实 Codex 请求。上游返回：${args.detail}`,
    url: args.url,
    httpStatus: args.httpStatus,
  };
}

// 结合 Responses 与 Chat 两条真实探测结果，判断 provider 应该用哪个上游协议。
export function classifyWizardDualProtocolConnectivityResult(args: {
  provider: Provider;
  model: string;
  responses: WizardProtocolProbe;
  chat: WizardProtocolProbe;
}): WizardConnectivityResult {
  const base = {
    providerId: args.provider.id,
    providerName: args.provider.name,
    model: args.model,
    url: args.responses.url ?? args.chat.url,
    httpStatus: args.responses.httpStatus ?? args.chat.httpStatus,
  };
  if (args.responses.ok && args.chat.ok) {
    return {
      ...base,
      status: "pass",
      canContinue: true,
      recommendedApiFormat: "openai_responses",
      detail:
        "Responses 和 Chat Completions 的基础请求都可用；这只能证明协议入口、鉴权和模型名可用，Codex 原生工具链仍优先使用 Responses。",
    };
  }
  if (args.responses.ok) {
    return {
      ...base,
      status: "pass",
      canContinue: true,
      recommendedApiFormat: "openai_responses",
      detail: `Responses 基础请求可用，Chat Completions 不通；将使用 Responses。该结果不等于完整 Codex 功能验证，Chat 返回：${args.chat.detail}`,
    };
  }
  if (args.chat.ok) {
    return {
      ...base,
      status: "warn",
      canContinue: true,
      recommendedApiFormat: "openai_chat",
      detail: `Responses 不通但 Chat Completions 可用；将保留 Chat 转换路径。Responses 返回：${args.responses.detail}`,
    };
  }
  return {
    ...base,
    status: "fail",
    canContinue: false,
    detail: `Responses 和 Chat Completions 都不通，更可能是 API Key、Base URL、模型权限、额度、网络或上游可用性问题。Responses 返回：${args.responses.detail}；Chat 返回：${args.chat.detail}`,
  };
}

// 没有可探测配置时生成跳过结果；有模型目录则可继续但有风险，没有目录则阻塞。
export function skippedWizardConnectivityResult(
  provider: Provider,
  reason: string,
): WizardConnectivityResult {
  const hasCatalog = hasWizardModelCatalog(provider);
  return {
    providerId: provider.id,
    providerName: provider.name,
    model: "*",
    status: hasCatalog ? "skipped" : "fail",
    canContinue: hasCatalog,
    detail: hasCatalog
      ? `${reason}；已有 modelCatalog，允许继续但未验证真实响应。`
      : `${reason}；且没有 modelCatalog，不能确认路由可用。`,
  };
}

// 聚合连通性结果：只要存在阻塞项，状态机就不应自动进入保存发布。
export function canContinueAfterConnectivity(
  results: WizardConnectivityResult[],
): boolean {
  return results.length > 0 && results.every((result) => result.canContinue);
}

// 将 Chat / Responses 双协议探测结果反写到向导草稿；用户手动锁定的协议优先于探测推荐。
export function applyWizardConnectivityApiFormatOverrides(
  providers: Provider[],
  results: WizardConnectivityResult[],
): Provider[] {
  if (results.length === 0) return providers;
  const resultsByProvider = new Map<string, WizardConnectivityResult[]>();
  for (const result of results) {
    const bucket = resultsByProvider.get(result.providerId) ?? [];
    bucket.push(result);
    resultsByProvider.set(result.providerId, bucket);
  }

  return providers.map((provider) => {
    const providerResults = resultsByProvider.get(provider.id) ?? [];
    if (provider.meta?.apiFormatSource === "manual") {
      return provider;
    }
    const formatByCanonicalModel = new Map<string, CodexApiFormat>();
    for (const result of providerResults) {
      if (result.status === "fail" || result.status === "skipped") continue;
      const format =
        result.recommendedApiFormat ??
        (result.status === "pass" ? "openai_responses" : undefined);
      if (format) formatByCanonicalModel.set(result.model, format);
    }
    if (formatByCanonicalModel.size === 0) return provider;
    const models = readWizardModelCatalog(provider).map((model) => {
      const canonicalModel =
        model.upstreamModel ?? model.upstream_model ?? model.model;
      const apiFormat =
        formatByCanonicalModel.get(canonicalModel) ??
        formatByCanonicalModel.get(model.model);
      return apiFormat ? { ...model, apiFormat } : model;
    });
    return {
      ...provider,
      settingsConfig: {
        ...(provider.settingsConfig ?? {}),
        modelCatalog: {
          ...(provider.settingsConfig?.modelCatalog ?? {}),
          models,
        },
      },
    };
  });
}

// 按用户最终保留的模型顺序过滤每个 provider 的目录；路由和最终 catalog 必须共享这份过滤结果。
export function filterWizardProvidersByModelOrder(
  providers: Provider[],
  modelOrder?: string[],
): Provider[] {
  if (!modelOrder) return providers;
  const enabledModels = new Set(
    modelOrder.map((model) => model.trim()).filter(Boolean),
  );
  const orderIndex = new Map(modelOrder.map((model, index) => [model, index]));
  return providers
    .map((provider) => {
      const models = readWizardModelCatalog(provider)
        .filter((model) => enabledModels.has(model.model))
        .sort(
          (left, right) =>
            (orderIndex.get(left.model) ?? Number.MAX_SAFE_INTEGER) -
            (orderIndex.get(right.model) ?? Number.MAX_SAFE_INTEGER),
        );
      return {
        ...provider,
        settingsConfig: {
          ...provider.settingsConfig,
          modelCatalog: {
            ...(provider.settingsConfig?.modelCatalog ?? {}),
            models,
          },
        },
      };
    })
    .filter((provider) => readWizardModelCatalog(provider).length > 0);
}

function canonicalWizardModelIds(provider: Provider): string[] {
  return Array.from(
    new Set(
      readWizardModelCatalog(provider)
        .filter(isWizardModelEnabled)
        .map((model) =>
          (model.upstreamModel ?? model.upstream_model ?? model.model).trim(),
        )
        .filter(Boolean),
    ),
  );
}

export interface WizardRouteAliasSelectionIssue {
  routeId: string;
  routeLabel?: string;
  providerName?: string;
  alias: string;
  canonicalModel: string;
  reason: string;
}

export function wizardRouteDisplayLabel(
  route: Pick<CodexRoutingRouteV2, "id" | "label">,
  providerName?: string | null,
): string {
  const label = route.label?.trim();
  const routeId = route.id.trim();
  if (label && label.toLowerCase() !== routeId.toLowerCase()) {
    return label;
  }
  return providerName?.trim() || label || routeId;
}

export function collectWizardRouteAliasSelectionIssues(
  routes: Array<
    Pick<
      CodexRoutingRouteV2,
      "id" | "label" | "targetProviderId" | "modelSelection" | "aliases"
    >
  >,
  providers: Provider[],
): WizardRouteAliasSelectionIssue[] {
  const providersById = new Map(
    providers.map((provider) => [provider.id, provider]),
  );
  const issues: WizardRouteAliasSelectionIssue[] = [];
  for (const route of routes) {
    const provider = providersById.get(route.targetProviderId);
    if (!provider) continue;
    const routeLabel = wizardRouteDisplayLabel(route, provider.name);
    const canonicalIds = canonicalWizardModelIds(provider);
    const canonicalSet = new Set(
      canonicalIds.map((model) => model.toLowerCase()),
    );
    const selectedSet =
      route.modelSelection?.mode === "all"
        ? canonicalSet
        : new Set(
            (route.modelSelection?.models ?? []).map((model) =>
              model.trim().toLowerCase(),
            ),
          );
    for (const [alias, target] of Object.entries(route.aliases ?? {})) {
      const canonicalModel = target.trim();
      if (!canonicalModel) continue;
      const canonicalKey = canonicalModel.toLowerCase();
      if (!canonicalSet.has(canonicalKey)) {
        issues.push({
          routeId: route.id,
          routeLabel,
          providerName: provider.name,
          alias,
          canonicalModel,
          reason: "别名目标已从 Provider 模型目录移除或重命名。",
        });
      } else if (!selectedSet.has(canonicalKey)) {
        issues.push({
          routeId: route.id,
          routeLabel,
          providerName: provider.name,
          alias,
          canonicalModel,
          reason: "别名目标不在当前 Route 的 canonical selection 中。",
        });
      }
    }
  }
  return issues;
}

// 为模型源生成 provider 分组 route；只引用 targetProviderId，不复制第三方 bearer 密钥。
export function buildWizardRoutesFromSources(
  providers: Provider[],
  officialAuth?: CodexOfficialAuthConfig,
  existingRoutes: CodexRoutingRouteV2[] = [],
): CodexRoutingRouteV2[] {
  return providers.map((provider) => {
    const modelMap = buildWizardRouteModelMap(provider);
    const existingRoute = existingRoutes.find(
      (route) => route.targetProviderId === provider.id,
    );
    const canonicalModels = new Set(canonicalWizardModelIds(provider));
    const aliases = { ...(modelMap ?? {}) };
    // A generated collision alias becomes part of the route contract on first
    // save. Keep that persisted spelling when the Provider is renamed later.
    for (const [visible, canonical] of Object.entries(
      existingRoute?.aliases ?? {},
    )) {
      const target = canonical.trim();
      if (!target || !canonicalModels.has(target)) continue;
      for (const [generatedVisible, generatedTarget] of Object.entries(
        aliases,
      )) {
        if (generatedTarget === target) delete aliases[generatedVisible];
      }
      aliases[visible] = target;
    }
    const oauthAccountId = isWizardCodexOAuthSource(provider)
      ? readWizardCodexOAuthAccountId(provider)
      : undefined;
    return {
      id: `router-${provider.id}`,
      label: provider.name,
      enabled: true,
      targetProviderId: provider.id,
      modelSelection: { mode: "all" },
      matchPrefixes: inferWizardRoutePrefixes(provider),
      aliases: Object.fromEntries(
        Object.entries(aliases).filter(
          ([visible, canonical]) =>
            visible.trim() !== canonical.trim() &&
            canonicalModels.has(canonical.trim()),
        ),
      ),
      authPolicy:
        officialAuth && isWizardCodexOAuthSource(provider)
          ? codexOfficialAuthRouteBinding(officialAuth)
          : isWizardNativeCodexAuthSource(provider)
            ? { source: "native_codex_auth" }
            : isWizardCodexOAuthSource(provider)
              ? {
                  source: "managed_codex_oauth",
                  ...(oauthAccountId ? { accountId: oauthAccountId } : {}),
                }
              : { source: "provider_config" },
    };
  });
}

// 从已处理重名的模型源生成 MultiRouter catalog；保留 upstreamModel 供运行时把别名映射回真实模型。
export function buildWizardModelCatalog(
  providers: Provider[],
  options: Pick<
    WizardPlanBuildOptions,
    "catalogModelOrder" | "spawnAgentModels"
  > = {},
): CodexModelCatalogConfig {
  const byModel = new Map<string, CodexCatalogModel>();
  for (const provider of providers) {
    for (const model of readWizardModelCatalog(provider).filter(
      isWizardModelEnabled,
    )) {
      if (!byModel.has(model.model)) {
        byModel.set(model.model, model);
      }
    }
  }
  const baseModels = Array.from(byModel.values());
  const models = options.catalogModelOrder
    ? options.catalogModelOrder
        .map((model) => byModel.get(model))
        .filter((model): model is CodexCatalogModel => Boolean(model))
    : baseModels;
  const modelSet = new Set(models.map((model) => model.model));
  const spawnAgentModels = (options.spawnAgentModels ?? [])
    .map((model) => model.trim())
    .filter((model) => modelSet.has(model))
    .slice(0, 5);
  return {
    models,
    spawnAgentModels:
      spawnAgentModels.length > 0
        ? spawnAgentModels
        : models.map((model) => model.model).slice(0, 5),
  };
}

// 生成官方 OAuth 源的去重键；未绑定账号时都代表后端默认 ChatGPT 账号。
function wizardCodexOAuthDedupKey(provider: Provider): string {
  return `codex-oauth:${readWizardCodexOAuthAccountId(provider) ?? "default"}`;
}

// 为等价官方 OAuth 源打分；优先保留已有真实目录，其次保留稳定官方 seed。
function wizardCodexOAuthSourceScore(provider: Provider): number {
  let score = readWizardModelCatalog(provider).length * 1000;
  if (provider.id === "codex-official") score += 100;
  if (provider.category === "official") score += 50;
  if (readWizardCodexOAuthAccountId(provider)) score += 10;
  return score;
}

// 收敛等价官方 OAuth 源，避免 `default` 和 `codex-official` 对同一默认账号重复取模和重复报错。
function dedupeWizardCodexOAuthSources(providers: Provider[]): Provider[] {
  const result: Provider[] = [];
  const selectedByKey = new Map<string, Provider>();
  const indexByKey = new Map<string, number>();

  for (const provider of providers) {
    if (!isWizardCodexOAuthSource(provider)) {
      result.push(provider);
      continue;
    }

    const key = wizardCodexOAuthDedupKey(provider);
    const current = selectedByKey.get(key);
    if (!current) {
      selectedByKey.set(key, provider);
      indexByKey.set(key, result.length);
      result.push(provider);
      continue;
    }

    if (
      wizardCodexOAuthSourceScore(provider) <=
      wizardCodexOAuthSourceScore(current)
    ) {
      continue;
    }

    const index = indexByKey.get(key);
    if (index !== undefined) {
      result[index] = provider;
      selectedByKey.set(key, provider);
    }
  }

  return result;
}

// 过滤出向导默认可用的普通 Codex provider；空目录 provider 仍保留，便于引导用户先刷新模型。
export function defaultWizardModelSources(providers: Provider[]): Provider[] {
  return dedupeWizardCodexOAuthSources(
    providers.filter((provider) => !isCodexMultiRouterPlan(provider)),
  );
}

export function initialWizardSelectedSourceIds(
  existingPlan: Provider | null | undefined,
  sourceProviders: Provider[],
): string[] {
  const availableIds = sourceProviders.map((provider) => provider.id);
  const routing = existingPlan?.settingsConfig?.codexRouting as
    | CodexRoutingConfigV2
    | undefined;
  if (routing?.schemaVersion !== 2) return availableIds;

  const routedProviderIds = new Set(
    (routing.routes ?? []).map((route) => route.targetProviderId),
  );
  return availableIds.filter((providerId) => routedProviderIds.has(providerId));
}

export function initialWizardCatalogModelOrder(
  existingPlan: Provider | null | undefined,
  sourceProviders: Provider[],
): string[] | null {
  const routing = existingPlan?.settingsConfig?.codexRouting as
    | CodexRoutingConfigV2
    | undefined;
  if (routing?.schemaVersion !== 2) {
    return existingPlan?.settingsConfig?.modelCatalog
      ? readWizardModelCatalog(existingPlan).map((model) => model.model)
      : null;
  }

  const enabledRoutes = (routing.routes ?? []).filter(
    (route) => route.enabled !== false,
  );
  if (
    enabledRoutes.length > 0 &&
    enabledRoutes.every((route) => route.modelSelection?.mode !== "include")
  ) {
    return null;
  }

  // Use the same source scope as the active draft. An unselected source must
  // not rename restored choices into aliases absent from the draft catalog.
  const selectedSourceIds = new Set(
    initialWizardSelectedSourceIds(existingPlan, sourceProviders),
  );
  const sources = resolveWizardModelNameCollisions(
    sourceProviders.filter((source) => selectedSourceIds.has(source.id)),
  );
  const sourceById = new Map(
    sources.map((provider) => [provider.id, provider]),
  );
  const ordered: string[] = [];
  for (const route of routing.routes ?? []) {
    if (route.enabled === false) continue;
    const source = sourceById.get(route.targetProviderId);
    if (!source) continue;
    const selected =
      route.modelSelection?.mode === "include"
        ? new Set(
            route.modelSelection.models.map((model) =>
              model.trim().toLowerCase(),
            ),
          )
        : null;
    for (const model of readWizardModelCatalog(source)) {
      const identities = [
        model.model,
        model.upstreamModel,
        model.upstream_model,
      ]
        .filter((identity): identity is string => Boolean(identity?.trim()))
        .map((identity) => identity.trim().toLowerCase());
      if (selected && !identities.some((identity) => selected.has(identity))) {
        continue;
      }
      if (!ordered.includes(model.model)) ordered.push(model.model);
    }
  }
  return ordered;
}

// 向导重建路由/目录后，为既有 subagentV2 生成“旧可见名 → 新可见名”的重定向器。
// 旧 profile 是可见名快照；重名消歧发生变化时必须先经旧 aliases 找回 upstream
// 身份，再映射到新目录，否则会产生无法被路由命中的孤儿 profile。
function buildSubagentNameRedirector(
  oldRoutes: CodexRoutingRouteV2[],
  newRoutes: CodexRoutingRouteV2[],
  resolvedSources: Provider[],
) {
  const oldVisibleToUpstream = new Map<string, string>();
  for (const route of oldRoutes) {
    for (const [visible, canonical] of Object.entries(route.aliases ?? {})) {
      if (visible.trim() && canonical.trim()) {
        oldVisibleToUpstream.set(
          visible.trim().toLowerCase(),
          canonical.trim(),
        );
      }
    }
  }

  const identityToNewVisible = new Map<string, string>();
  for (const source of resolvedSources) {
    for (const model of readWizardModelCatalog(source)) {
      const upstream = (
        model.upstreamModel ??
        model.upstream_model ??
        model.model ??
        ""
      ).trim();
      const visible = (model.model ?? "").trim();
      const identity = upstream.toLowerCase();
      if (identity && visible && !identityToNewVisible.has(identity)) {
        identityToNewVisible.set(identity, visible);
      }
    }
  }
  for (const route of newRoutes) {
    for (const [visible, canonical] of Object.entries(route.aliases ?? {})) {
      const identity = canonical.trim().toLowerCase();
      if (identity && visible.trim() && !identityToNewVisible.has(identity)) {
        identityToNewVisible.set(identity, visible.trim());
      }
    }
  }

  function redirectVisibleName(oldVisible: string): string | null {
    const trimmed = oldVisible.trim();
    if (!trimmed) return null;
    const identities = [
      oldVisibleToUpstream.get(trimmed.toLowerCase()),
      trimmed,
    ]
      .filter((identity): identity is string => Boolean(identity))
      .map((identity) => identity.toLowerCase());
    for (const identity of identities) {
      const mapped = identityToNewVisible.get(identity);
      if (mapped) return mapped;
    }
    return null;
  }

  function remapSubagentV2(
    current: CodexRoutingConfigV2["subagentV2"],
  ): CodexRoutingConfigV2["subagentV2"] {
    if (!current?.profiles) return current;
    const profiles = current.profiles;
    const next: Record<string, CodexSubagentV2Profile> = {};
    for (const [oldKey, profile] of Object.entries(profiles)) {
      const oldVisible = (
        typeof profile.model === "string" && profile.model.trim()
          ? profile.model
          : oldKey
      ).trim();
      const newVisible = redirectVisibleName(oldVisible);
      if (
        !newVisible ||
        newVisible.toLowerCase() === oldVisible.toLowerCase()
      ) {
        next[oldKey] = profile;
        continue;
      }

      const occupiedByOther =
        Object.prototype.hasOwnProperty.call(next, newVisible) ||
        Object.entries(profiles).some(([otherKey, otherProfile]) => {
          if (otherKey === oldKey) return false;
          const otherVisible = (
            typeof otherProfile.model === "string" && otherProfile.model.trim()
              ? otherProfile.model
              : otherKey
          ).trim();
          return otherVisible.toLowerCase() === newVisible.toLowerCase();
        });
      if (occupiedByOther) {
        next[oldKey] = profile;
        continue;
      }
      next[newVisible] = { ...profile, model: newVisible };
    }
    return { ...current, profiles: next };
  }

  return { redirectVisibleName, remapSubagentV2 };
}

// 创建或更新 MultiRouter provider；草稿只在用户点击保存发布时写入数据库。
export function buildCodexMultiRouterWizardPlan(
  allProviders: Provider[],
  sourceProviders: Provider[],
  existingPlan?: Provider | null,
  options: WizardPlanBuildOptions = {},
): WizardPlanBuildResult {
  const collisionResolvedSources =
    resolveWizardModelNameCollisions(sourceProviders);
  const resolvedSources = filterWizardProvidersByModelOrder(
    collisionResolvedSources,
    options.catalogModelOrder,
  );
  const selectedCanonicalByProvider = new Map(
    resolvedSources.map((provider) => [
      provider.id,
      new Set(canonicalWizardModelIds(provider)),
    ]),
  );
  const canonicalSourceProviders = sourceProviders
    .map((provider) => {
      const selected = selectedCanonicalByProvider.get(provider.id);
      if (!selected) return provider;
      const models = readWizardModelCatalog(provider).filter((model) =>
        selected.has(
          (model.upstreamModel ?? model.upstream_model ?? model.model).trim(),
        ),
      );
      return {
        ...provider,
        settingsConfig: {
          ...provider.settingsConfig,
          modelCatalog: {
            ...(provider.settingsConfig?.modelCatalog ?? {}),
            models,
          },
        },
      };
    })
    .filter((provider) => readWizardModelCatalog(provider).length > 0);
  const existingRouting = existingPlan?.settingsConfig?.codexRouting as
    | CodexRoutingConfig
    | undefined;
  const existingRoutingV2 =
    (existingRouting as { schemaVersion?: unknown } | undefined)
      ?.schemaVersion === 2
      ? (existingRouting as unknown as CodexRoutingConfigV2)
      : undefined;
  const officialAuth =
    options.officialAuth ??
    inferCodexOfficialAuth(existingRouting) ??
    DEFAULT_CODEX_OFFICIAL_AUTH;
  const hostedTools =
    options.hostedTools ??
    normalizeHostedToolsConfig(existingPlan?.settingsConfig?.hostedTools);
  const routes: CodexRoutingRouteV2[] = buildWizardRoutesFromSources(
    resolvedSources,
    officialAuth,
    existingRoutingV2?.routes ?? [],
  ).map((route) => {
    if (!options.catalogModelOrder) return route;
    const selectedSource = resolvedSources.find(
      (provider) => provider.id === route.targetProviderId,
    );
    const fullSource = collisionResolvedSources.find(
      (provider) => provider.id === route.targetProviderId,
    );
    if (!selectedSource || !fullSource) return route;
    const selectedModels = canonicalWizardModelIds(selectedSource);
    const fullModels = canonicalWizardModelIds(fullSource);
    const selectedSet = new Set(selectedModels);
    const includesEveryProviderModel =
      selectedSet.size === fullModels.length &&
      fullModels.every((model) => selectedSet.has(model));
    return {
      ...route,
      modelSelection: includesEveryProviderModel
        ? ({ mode: "all" } as const)
        : ({ mode: "include", models: selectedModels } as const),
    };
  });
  const selectedVisibleModels = new Set(
    resolvedSources.flatMap((provider) =>
      readWizardModelCatalog(provider).map((model) => model.model),
    ),
  );
  const requestedSpawnAgentModels: string[] =
    options.spawnAgentModels ??
    existingRoutingV2?.spawnAgentModels ??
    (existingRoutingV2
      ? []
      : existingPlan?.settingsConfig?.modelCatalog?.spawnAgentModels) ??
    [];
  const subagentVersion = normalizeCodexSubagentVersion(
    options.subagentVersion ?? existingRouting?.subagentVersion,
  );
  const subagentNameRedirector = buildSubagentNameRedirector(
    existingRoutingV2?.routes ?? [],
    routes,
    resolvedSources,
  );
  const routing: CodexRoutingConfigV2 = {
    ...(existingRoutingV2 ?? {}),
    schemaVersion: 2,
    enabled: true,
    subagentVersion,
    subagentV2: subagentNameRedirector.remapSubagentV2(
      existingRouting?.subagentV2,
    ),
    spawnAgentModels: requestedSpawnAgentModels
      .map(
        (model) => subagentNameRedirector.redirectVisibleName(model) ?? model,
      )
      .filter((model) => selectedVisibleModels.has(model)),
    routes,
  };
  const existingIds = new Set(allProviders.map((provider) => provider.id));
  const planId =
    existingPlan?.id ??
    options.planId ??
    (existingIds.has(CODEX_MULTI_ROUTER_DEFAULT_ID)
      ? `${CODEX_MULTI_ROUTER_DEFAULT_ID}-${Date.now()}`
      : CODEX_MULTI_ROUTER_DEFAULT_ID);
  const existingSettings = { ...(existingPlan?.settingsConfig ?? {}) };
  delete existingSettings.modelCatalog;
  delete existingSettings.model_catalog;
  const plan: Provider = {
    ...(existingPlan ?? {
      id: planId,
      name: CODEX_MULTI_ROUTER_DEFAULT_NAME,
      category: "custom",
      createdAt: Date.now(),
    }),
    id: planId,
    name:
      options.planName?.trim() ||
      existingPlan?.name ||
      CODEX_MULTI_ROUTER_DEFAULT_NAME,
    category: existingPlan?.category ?? "custom",
    settingsConfig: {
      ...existingSettings,
      auth: existingPlan?.settingsConfig?.auth ?? {},
      base_url: CODEX_MULTI_ROUTER_PROXY_BASE_URL,
      baseUrl: CODEX_MULTI_ROUTER_PROXY_BASE_URL,
      config: existingPlan?.settingsConfig?.config ?? null,
      codexRouting: routing,
      hostedTools,
    },
  };
  return { plan, sourceProviders: canonicalSourceProviders };
}

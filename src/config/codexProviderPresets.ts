/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  CodexModelReasoningCapability,
  CodexReasoningEffort,
  PromptCacheRoutingMode,
} from "../types";
import type { PresetTheme } from "./claudeProviderPresets";
import {
  openCodeGoModelsFor,
  type OpenCodeGoModel,
  type OpenCodeGoProtocol,
} from "./openCodeGoCatalog";

export interface CodexProviderPreset {
  name: string;
  // Stable persistence identity; unlike the UI's codex-N index this must not
  // change when presets are inserted or reordered.
  presetKey?: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 第三方供应商可提供单独的获取 API Key 链接
  apiKeyUrl?: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory; // 新增：分类
  isCustomTemplate?: boolean; // 标识是否为自定义模板
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 新增：视觉主题配置
  theme?: PresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
  // Codex API 格式
  apiFormat?: CodexApiFormat;
  // 托管账号预设：目前仅 xAI OAuth（Grok 订阅经本地代理注入 token 直连 api.x.ai）
  providerType?: "xai_oauth";
  // OAuth 预设：隐藏 API Key 输入，保存前要求已登录托管账号
  requiresOAuth?: boolean;
  // Codex Chat 本地路由模式下的模型目录
  modelCatalog?: CodexCatalogModel[];
  // Codex Responses -> Chat Completions reasoning capability defaults
  codexChatReasoning?: CodexChatReasoning;
  // Session-based prompt-cache routing override for Chat Completions upstreams
  promptCacheRouting?: PromptCacheRoutingMode;
}

/**
 * 生成第三方供应商的 auth.json
 */
export function generateThirdPartyAuth(apiKey: string): Record<string, any> {
  return {
    OPENAI_API_KEY: apiKey || "",
  };
}

/**
 * 生成第三方供应商的 config.toml
 */
export function generateThirdPartyConfig(
  providerName: string,
  baseUrl: string,
  modelName = "gpt-5.6-sol",
): string {
  const tomlString = (value: string) => JSON.stringify(value);

  return `model_provider = "custom"
model = ${tomlString(modelName)}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = ${tomlString(providerName)}
base_url = ${tomlString(baseUrl)}
wire_api = "responses"`;
}

/**
 * 生成小米 MiMo 官方 Codex 接入配置。
 *
 * MiMo 官方 Codex 文档要求显式声明 reasoning summary 能力和 1M 上下文；
 * 否则 Codex 会把 `model_reasoning_effort` 当作无效配置，并可能使用保守上下文预算。
 */
function generateMiMoCodexConfig(
  providerName: string,
  baseUrl: string,
  modelName = "mimo-v2.5-pro",
): string {
  const tomlString = (value: string) => JSON.stringify(value);

  return `model_provider = "custom"
model = ${tomlString(modelName)}
model_reasoning_effort = "high"
model_supports_reasoning_summaries = true
model_reasoning_summary = "none"
model_context_window = 1048576
web_search = "disabled"
disable_response_storage = true

[model_providers.custom]
name = ${tomlString(providerName)}
base_url = ${tomlString(baseUrl)}
wire_api = "responses"`;
}

// 内置预设是 CCSwitchMulti 维护的能力声明：新写入统一使用 schema v2
// （supportStatus + controlKind），不再写 legacy supported 字段。
const unsupportedBuiltinReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_unsupported",
  controlKind: "none",
  supportedEfforts: [],
  disableAllowed: false,
  upstream: { format: "none", parameter: "none" },
  source: "builtin",
};

function booleanBuiltinReasoning(
  parameter: "thinking" | "enable_thinking" | "reasoning_split",
  outputFormat: "reasoning_content" | "reasoning_details",
): CodexModelReasoningCapability {
  return {
    schemaVersion: 2,
    supportStatus: "confirmed_supported",
    controlKind: "boolean",
    supportedEfforts: [],
    disableAllowed: true,
    upstream: { format: "boolean", parameter },
    outputFormat,
    source: "builtin",
  };
}

const thinkingBooleanReasoning = booleanBuiltinReasoning(
  "thinking",
  "reasoning_content",
);
const enableThinkingBooleanReasoning = booleanBuiltinReasoning(
  "enable_thinking",
  "reasoning_content",
);
const qwenHybridResponsesReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "boolean",
  supportedEfforts: [],
  disableAllowed: true,
  upstream: { format: "boolean", parameter: "enable_thinking" },
  outputFormat: "reasoning",
  source: "builtin",
};
const qwen38ResponsesReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "medium", "xhigh"],
  defaultEffort: "xhigh",
  disableAllowed: true,
  upstream: { format: "reasoning_object", parameter: "reasoning.effort" },
  outputFormat: "reasoning",
  source: "builtin",
};
const unknownBuiltinReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "unknown",
  controlKind: "unknown",
  supportedEfforts: [],
  disableAllowed: false,
  upstream: { format: "none", parameter: "none" },
  source: "builtin",
};
const reasoningSplitBooleanReasoning = booleanBuiltinReasoning(
  "reasoning_split",
  "reasoning_details",
);

function modelCatalog(
  models: Array<
    | string
    | {
        model: string;
        displayName?: string;
        contextWindow?: number;
        inputModalities?: Array<"text" | "image">;
        textOnly?: boolean;
        supportsImage?: boolean;
        vision?: boolean;
        // Native Responses (direct) overrides for the generated
        // model-catalogs.json. Omitted input modalities are inferred by the
        // backend: confirmed text-only models stay text-only; everything else
        // defaults to text+image.
        supportsParallelToolCalls?: boolean;
        // Vendor's OFFICIAL base_instructions; omit to inherit the neutral
        // template default. Required by Codex, so the backend always emits one.
        baseInstructions?: string;
        reasoning?: CodexModelReasoningCapability;
        // Concise seed syntax used by upstream preset tables. These fields are
        // consumed here and projected into CCSwitchMulti's schema-v2
        // `reasoning` capability; they never enter the exported catalog.
        reasoningLevels?: CodexReasoningEffort[];
        defaultReasoningLevel?: CodexReasoningEffort;
      }
  >,
): CodexCatalogModel[] {
  return models.map((entry) =>
    typeof entry === "string"
      ? { model: entry, reasoning: unsupportedBuiltinReasoning }
      : {
          model: entry.model,
          displayName: entry.displayName,
          contextWindow: entry.contextWindow,
          ...(entry.inputModalities
            ? { inputModalities: entry.inputModalities }
            : {}),
          reasoning:
            entry.reasoning ??
            (entry.reasoningLevels
              ? builtinReasoningFromLevels(
                  entry.reasoningLevels,
                  entry.defaultReasoningLevel,
                )
              : unsupportedBuiltinReasoning),
          ...(entry.textOnly !== undefined ? { textOnly: entry.textOnly } : {}),
          ...(entry.supportsImage !== undefined
            ? { supportsImage: entry.supportsImage }
            : {}),
          ...(entry.vision !== undefined ? { vision: entry.vision } : {}),
          ...(entry.supportsParallelToolCalls !== undefined
            ? { supportsParallelToolCalls: entry.supportsParallelToolCalls }
            : {}),
          ...(entry.baseInstructions !== undefined
            ? { baseInstructions: entry.baseInstructions }
            : {}),
        },
  );
}

function builtinReasoningFromLevels(
  levels: CodexReasoningEffort[],
  defaultEffort?: CodexReasoningEffort,
): CodexModelReasoningCapability {
  const supportedEfforts = levels.filter((level) => level !== "none");
  const resolvedDefault = defaultEffort ?? supportedEfforts[0];
  return {
    schemaVersion: 2,
    supportStatus: "confirmed_supported",
    controlKind: supportedEfforts.length > 0 ? "graded" : "boolean",
    supportedEfforts,
    ...(resolvedDefault ? { defaultEffort: resolvedDefault } : {}),
    disableAllowed: levels.includes("none"),
    upstream: { format: "string", parameter: "reasoning_effort" },
    outputFormat: "reasoning_content",
    source: "builtin",
  };
}

function openCodeGoCodexReasoning(
  model: OpenCodeGoModel,
): CodexModelReasoningCapability {
  if (model.reasoning.kind === "unknown" || model.protocol === "messages") {
    return unknownBuiltinReasoning;
  }
  if (model.reasoning.kind === "toggle") {
    return booleanBuiltinReasoning("thinking", "reasoning_content");
  }

  const supportedEfforts = [...model.reasoning.efforts];
  return {
    schemaVersion: 2,
    supportStatus: "confirmed_supported",
    controlKind: "graded",
    supportedEfforts,
    defaultEffort: supportedEfforts.includes("high")
      ? "high"
      : supportedEfforts[0],
    disableAllowed: model.reasoning.disableAllowed ?? false,
    upstream:
      model.protocol === "responses"
        ? { format: "reasoning_object", parameter: "reasoning.effort" }
        : { format: "string", parameter: "reasoning_effort" },
    outputFormat:
      model.protocol === "responses" ? "reasoning" : "reasoning_content",
    source: "builtin",
  };
}

function openCodeGoCodexCatalog(protocol: OpenCodeGoProtocol) {
  return modelCatalog(
    openCodeGoModelsFor(protocol).map((model) => ({
      model: model.id,
      displayName: model.name,
      contextWindow: model.context,
      inputModalities: model.input.some((modality) => modality === "image")
        ? (["text", "image"] as const)
        : (["text"] as const),
      reasoning: openCodeGoCodexReasoning(model),
    })),
  );
}

const deepSeekV4Reasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "high", "max"],
  defaultEffort: "high",
  disableAllowed: true,
  upstream: {
    format: "string",
    parameter: "reasoning_effort",
    effortMap: {
      low: "low",
      medium: "high",
      high: "high",
      xhigh: "high",
      max: "max",
    },
  },
  source: "builtin",
};

const grok45Reasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "medium", "high"],
  defaultEffort: "high",
  disableAllowed: false,
  upstream: { format: "reasoning_object", parameter: "reasoning.effort" },
  source: "builtin",
};

const glm53ResponsesReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "high", "max"],
  defaultEffort: "max",
  disableAllowed: false,
  upstream: { format: "reasoning_object", parameter: "reasoning.effort" },
  outputFormat: "reasoning",
  source: "builtin",
};

const glm5TurboResponsesReasoning: CodexModelReasoningCapability = {
  ...glm53ResponsesReasoning,
  supportedEfforts: ["max"],
};

const stepThreeLevelReasoning: CodexModelReasoningCapability = {
  schemaVersion: 2,
  supportStatus: "confirmed_supported",
  controlKind: "graded",
  supportedEfforts: ["low", "medium", "high"],
  defaultEffort: "medium",
  disableAllowed: false,
  upstream: { format: "string", parameter: "reasoning_effort" },
  outputFormat: "reasoning",
  source: "builtin",
};

const step2603Reasoning: CodexModelReasoningCapability = {
  ...stepThreeLevelReasoning,
  supportedEfforts: ["low", "high"],
  defaultEffort: "high",
};

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "OpenAI Official",
    websiteUrl: "https://chatgpt.com/codex",
    isOfficial: true,
    category: "official",
    auth: {},
    config: ``,
    theme: {
      icon: "codex",
      backgroundColor: "#1F2937", // gray-800
      textColor: "#FFFFFF",
    },
    icon: "openai",
    iconColor: "#00A67E",
  },
  // ===== 赞助商预设：文件顺序 = 应用内展示顺序，与 README 赞助商表对齐 =====
  {
    name: "Kimi",
    primePartner: true,
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "kimi",
      "https://api.moonshot.cn/v1",
      "kimi-k2.7-code",
    ),
    endpointCandidates: ["https://api.moonshot.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        reasoning: thinkingBooleanReasoning,
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        reasoning: thinkingBooleanReasoning,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://www.kimi.com/code/?aff=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "kimi_coding",
      "https://api.kimi.com/coding/v1",
      "kimi-for-coding",
    ),
    endpointCandidates: ["https://api.kimi.com/coding/v1"],
    apiFormat: "openai_chat",
    promptCacheRouting: "enabled",
    modelCatalog: modelCatalog([
      {
        model: "kimi-for-coding",
        displayName: "Kimi For Coding",
        contextWindow: 262144,
        reasoning: thinkingBooleanReasoning,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "packycode",
      "https://www.packyapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://www.packyapi.ai/v1",
      "https://cf.api.fan/v1",
      "https://slb-v1.api.fan/v1",
      "https://www.packyapi.com/v1",
    ],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "zetaapi",
      "https://api.zetaapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.zetaapi.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APINebula"
base_url = "https://apinebula.ai/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://apinebula.ai/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicodemirror",
      "https://api.aicodemirror.ai/api/codex/backend-api/codex",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.aicodemirror.ai/api/codex/backend-api/codex",
    ],
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "PatewayAI",
    websiteUrl: "https://pateway.ai",
    apiKeyUrl: "https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "patewayai",
      "https://api.pateway.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.pateway.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "patewayai",
    icon: "pateway",
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "fenno",
      "https://api.fenno.ai",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.fenno.ai"],
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co/register?aff=iOKB",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "runapi",
      "https://runapi.co/v1",
      "gpt-5.6-sol",
    ),
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "shengsuanyun",
      "https://router.shengsuanyun.com/api/v1",
      "openai/gpt-5.6-sol",
    ),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.app",
    apiKeyUrl: "https://aigocode.app/invite/CC-SWITCH",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aigocode",
      "https://api.aigocode.app",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.aigocode.app"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aigocode", // 促销信息 i18n key
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qiniu",
      "https://api.qnaigc.com/bypass/openai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.qnaigc.com/bypass/openai/v1",
      "https://api.modelink.ai/bypass/openai/v1",
    ],
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicoding",
      "https://api.aicoding.inc",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.aicoding.inc"],
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "subrouter",
      "https://subrouter.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://subrouter.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APIKEY.FUN"
base_url = "https://api.apikey.fun/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: [
      "https://api.apikey.fun/v1",
      "https://slb.apikey.fun/v1",
    ],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "code0",
      "https://code0.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://code0.ai/v1"],
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.com",
    apiKeyUrl:
      "https://teamorouter.com/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "teamorouter",
      "https://api.teamorouter.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.teamorouter.com/v1"],
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "claudecn",
      "https://claudecn.top/v1",
      "gpt-5.6-sol",
    ),
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
  {
    name: "火山Agentplan",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ark_agentplan",
      "https://ark.cn-beijing.volces.com/api/coding/v3",
      "ark-code-latest",
    ),
    // ⚠️ 计费红线（官方 warning）：Coding Plan 必须走 /api/coding/v3；
    // 填按量端点 /api/v3 不消耗套餐额度、按量另计费，绝不能混入候选
    endpointCandidates: ["https://ark.cn-beijing.volces.com/api/coding/v3"],
    // 官方 Codex 文档（volcengine.com/docs/82379/2556056，2026-07 更新）：
    // Coding Plan /api/coding/v3 已支持 Responses API（wire_api=responses），无需路由接管转换
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
  },
  {
    name: "BytePlus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "byteplus",
      "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      "ark-code-latest",
    ),
    endpointCandidates: [
      "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
    ],
    // 国内站 coding/v3 已切原生 Responses（见 火山Agentplan），但 BytePlus
    // 国际站（bytepluses.com）文档未单独核实，暂保持 Chat 路由
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "ark-code-latest",
        displayName: "Ark Code Latest",
        contextWindow: 256000,
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
  },
  {
    name: "DouBaoSeed",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "doubaoseed",
      "https://ark.cn-beijing.volces.com/api/v3",
      "doubao-seed-2-1-pro-260628",
    ),
    endpointCandidates: ["https://ark.cn-beijing.volces.com/api/v3"],
    // 火山方舟主数据面 /api/v3 原生支持 Responses API（/api/v3/responses），无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch），
    // 让 Codex 直连显示模型并避免 custom 工具被网关拒绝
    modelCatalog: modelCatalog([
      {
        model: "doubao-seed-2-1-pro-260628",
        displayName: "Doubao Seed 2.1 Pro",
        contextWindow: 262144,
      },
    ]),
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "siliconflow",
      "https://api.siliconflow.cn/v1",
      "Pro/MiniMaxAI/MiniMax-M2.7",
    ),
    endpointCandidates: ["https://api.siliconflow.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "Pro/MiniMaxAI/MiniMax-M2.7",
        displayName: "Pro / MiniMax M2.7",
        contextWindow: 200000,
        reasoning: enableThinkingBooleanReasoning,
      },
    ]),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
  },
  {
    name: "SiliconFlow en",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "siliconflow_en",
      "https://api.siliconflow.com/v1",
      "MiniMaxAI/MiniMax-M2.7",
    ),
    endpointCandidates: ["https://api.siliconflow.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "MiniMaxAI/MiniMax-M2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 200000,
        reasoning: enableThinkingBooleanReasoning,
      },
    ]),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "a6api",
      "https://api.a6api.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.a6api.com/v1"],
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "zai-org/glm-5.1"
disable_response_storage = true

[model_providers.custom]
name = "AtlasCloud"
base_url = "https://api.atlascloud.ai/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://api.atlascloud.ai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM 5.1",
        contextWindow: 200000,
      },
    ]),
    isPartner: true,
    partnerPromotionKey: "atlascloud",
    icon: "atlascloud",
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare",
      "https://api.modelverse.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.modelverse.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Compshare Coding Plan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare_coding",
      "https://cp.compshare.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://cp.compshare.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key（复用）
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ccsub",
      "https://www.ccsub.net/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.ccsub.net/v1"],
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "sssaicode",
      "https://node-hk.sssaicodeapi.com/api/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://node-hk.sssaicodeapi.com/api/v1",
      "https://node-hk.sssaiapi.com/api/v1",
      "https://node-cf.sssaicodeapi.com/api/v1",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "sssaicode", // 促销信息 i18n key
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "micu",
      "https://www.micuapi.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.micuapi.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "micu", // 促销信息 i18n key
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "rightcode",
      "https://www.rightapi.ai/codex/v1",
      "gpt-5.6-sol",
    ),
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
  },
  {
    name: "ETok.ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "etok",
      "https://api.etok.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.etok.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "etok", // 促销信息 i18n key
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "cubence",
      "https://api.cubence.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://api.cubence.com/v1",
      "https://api-cf.cubence.com/v1",
      "https://api-dmit.cubence.com/v1",
      "https://api-bwg.cubence.com/v1",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "cubence", // 促销信息 i18n key
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "crazyrouter",
      "https://cn.crazyrouter.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://cn.crazyrouter.com/v1"],
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "dmxapi",
      "https://www.dmxapi.cn/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://www.dmxapi.cn/v1"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "dmxapi", // 促销信息 i18n key
  },
  {
    name: "SudoCode.chat",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/sign-up?aff=CC-SWITCH&utm_source=cc-switch&utm_medium=sponsor&utm_campaign=ccswitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "SudoCode"
base_url = "https://api.sudocode.chat/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://api.sudocode.chat/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true
model_verbosity = "high"

[model_providers.custom]
name = "sudocode"
base_url = "https://sudocode.us/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://sudocode.us/v1", "https://sudocode.run/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    icon: "sudocode-us",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "amux",
      "https://api.amux.ai/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: ["https://api.amux.ai/v1"],
    icon: "amux",
  },
  {
    name: "Azure OpenAI",
    websiteUrl:
      "https://learn.microsoft.com/en-us/azure/ai-foundry/openai/how-to/codex",
    category: "third_party",
    isOfficial: true,
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "Azure OpenAI"
base_url = "https://YOUR_RESOURCE_NAME.openai.azure.com/openai"
env_key = "OPENAI_API_KEY"
query_params = { "api-version" = "2025-04-01-preview" }
wire_api = "responses"`,
    endpointCandidates: ["https://YOUR_RESOURCE_NAME.openai.azure.com/openai"],
    theme: {
      icon: "codex",
      backgroundColor: "#0078D4",
      textColor: "#FFFFFF",
    },
    icon: "azure",
    iconColor: "#0078D4",
  },
  {
    name: "DeepSeek",
    presetKey: "deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "deepseek",
      "https://api.deepseek.com",
      "deepseek-v4-flash",
    ),
    endpointCandidates: ["https://api.deepseek.com"],
    // DeepSeek 官方 Responses API 文档明确 base_url=https://api.deepseek.com，
    // 当前仅 deepseek-v4-flash 支持 Codex；pro 仍由 MultiRouter 拆到 Chat 兼容路由。
    // DeepSeek 官方 Codex 文档（api-docs.deepseek.com → agent_integrations/codex）：
    // deepseek-v4-flash 原生 Responses（wire_api=responses 对自家 base_url），无需路由接管转换。
    // 后端按 deepseek.com host 直接镜像官方 models.json（freeform apply_patch +
    // GPT-5 harness + low/high/max 思考档，需 codex >= 0.144.0），这里只保留行清单与展示名。
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        reasoning: deepSeekV4Reasoning,
      },
      // 官方预计 2026-08 初开通 pro 的 Codex 集成（官方 models.json 已含该条目），
      // 在那之前切到 pro 会上游报错
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        reasoning: deepSeekV4Reasoning,
      },
      {
        model: "deepseek-v4-flash-vision-exp",
        displayName: "DeepSeek V4 Flash Vision Exp",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        textOnly: false,
        supportsImage: true,
        reasoning: deepSeekV4Reasoning,
      },
    ]),
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "Zhipu GLM",
    presetKey: "zhipu-glm-cn",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "zhipu_glm",
      "https://open.bigmodel.cn/api/v1",
      "glm-5.3",
    ),
    endpointCandidates: ["https://open.bigmodel.cn/api/v1"],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        supportsParallelToolCalls: true,
        reasoning: glm53ResponsesReasoning,
      },
      {
        model: "glm-5-turbo",
        displayName: "GLM-5-Turbo",
        contextWindow: 204800,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        supportsParallelToolCalls: true,
        reasoning: glm5TurboResponsesReasoning,
      },
    ]),
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Zhipu GLM en",
    presetKey: "zhipu-glm-en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "zhipu_glm_en",
      "https://api.z.ai/api/v1",
      "glm-5.3",
    ),
    endpointCandidates: ["https://api.z.ai/api/v1"],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        textOnly: true,
        supportsImage: false,
        supportsParallelToolCalls: true,
        reasoning: glm53ResponsesReasoning,
      },
    ]),
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Baidu Qianfan Coding Plan",
    websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
    apiKeyUrl:
      "https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qianfan_coding",
      "https://qianfan.baidubce.com/v2/coding",
      "qianfan-code-latest",
    ),
    endpointCandidates: ["https://qianfan.baidubce.com/v2/coding"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "qianfan-code-latest",
        displayName: "Qianfan Code Latest",
        contextWindow: 131072,
      },
    ]),
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
  },
  {
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "bailian",
      "https://dashscope.aliyuncs.com/compatible-mode/v1",
      "qwen3-coder-plus",
    ),
    endpointCandidates: ["https://dashscope.aliyuncs.com/compatible-mode/v1"],
    // 阿里百炼 DashScope 原生支持 OpenAI Responses API（/compatible-mode/v1/responses，同一 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）
    modelCatalog: modelCatalog([
      {
        model: "qwen3-coder-plus",
        displayName: "Qwen3 Coder Plus",
        contextWindow: 1048576,
        reasoning: enableThinkingBooleanReasoning,
      },
    ]),
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
  // QwenCloud's pay-as-you-go, Coding Plan, and Token Plan credentials are
  // isolated. Keep separate presets so a plan key is never sent to the wrong
  // billing endpoint. The Token Plan catalog is the current personal/team
  // coding-model intersection; plan-specific extras remain discoverable by
  // users without advertising them as universally available.
  {
    name: "QwenCloud",
    presetKey: "qwencloud",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud",
      "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      "qwen3.8-max",
    ),
    endpointCandidates: [
      "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    ],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "qwen3.8-max",
        displayName: "Qwen3.8 Max",
        contextWindow: 983616,
        inputModalities: ["text", "image"],
        supportsParallelToolCalls: false,
        reasoning: qwen38ResponsesReasoning,
      },
      {
        model: "qwen3.8-flash",
        displayName: "Qwen3.8 Flash",
        contextWindow: 983616,
        inputModalities: ["text", "image"],
        supportsParallelToolCalls: false,
        reasoning: qwen38ResponsesReasoning,
      },
      {
        model: "qwen3.7-max",
        displayName: "Qwen3.7 Max",
        contextWindow: 1000000,
        inputModalities: ["text"],
        reasoning: qwenHybridResponsesReasoning,
      },
      {
        model: "qwen3.7-plus",
        displayName: "Qwen3.7 Plus",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: qwenHybridResponsesReasoning,
      },
      {
        model: "qwen3.6-plus",
        displayName: "Qwen3.6 Plus",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: qwenHybridResponsesReasoning,
      },
      {
        model: "qwen3.6-flash",
        displayName: "Qwen3.6 Flash",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: qwenHybridResponsesReasoning,
      },
    ]),
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud For Coding",
    presetKey: "qwencloud-coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud_coding",
      "https://coding-intl.dashscope.aliyuncs.com/v1",
      "qwen3.7-plus",
    ),
    endpointCandidates: ["https://coding-intl.dashscope.aliyuncs.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "qwen3.7-plus",
        displayName: "Qwen3.7 Plus",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: enableThinkingBooleanReasoning,
      },
      {
        model: "qwen3.6-plus",
        displayName: "Qwen3.6 Plus",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: enableThinkingBooleanReasoning,
      },
      {
        model: "qwen3.5-plus",
        displayName: "Qwen3.5 Plus",
        inputModalities: ["text", "image"],
        reasoning: enableThinkingBooleanReasoning,
      },
      {
        model: "qwen3-max-2026-01-23",
        displayName: "Qwen3 Max 2026-01-23",
        contextWindow: 262144,
        inputModalities: ["text"],
        reasoning: enableThinkingBooleanReasoning,
      },
      {
        model: "qwen3-coder-next",
        displayName: "Qwen3 Coder Next",
        inputModalities: ["text"],
        reasoning: unknownBuiltinReasoning,
      },
      {
        model: "qwen3-coder-plus",
        displayName: "Qwen3 Coder Plus",
        contextWindow: 131072,
        inputModalities: ["text"],
        reasoning: unknownBuiltinReasoning,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "enable_thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud Token Plan",
    presetKey: "qwencloud-token-plan",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "qwencloud_token_plan",
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
      "qwen3.8-max",
    ),
    endpointCandidates: [
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
    ],
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "qwen3.8-max",
        displayName: "Qwen3.8 Max",
        contextWindow: 983616,
        inputModalities: ["text", "image"],
        supportsParallelToolCalls: false,
        reasoning: qwen38ResponsesReasoning,
      },
      {
        model: "qwen3.8-flash",
        displayName: "Qwen3.8 Flash",
        contextWindow: 983616,
        inputModalities: ["text", "image"],
        supportsParallelToolCalls: false,
        reasoning: qwen38ResponsesReasoning,
      },
      {
        model: "qwen3.7-max",
        displayName: "Qwen3.7 Max",
        contextWindow: 1000000,
        inputModalities: ["text"],
        reasoning: qwenHybridResponsesReasoning,
      },
      {
        model: "qwen3.7-plus",
        displayName: "Qwen3.7 Plus",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: qwenHybridResponsesReasoning,
      },
      {
        model: "qwen3.6-flash",
        displayName: "Qwen3.6 Flash",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoning: qwenHybridResponsesReasoning,
      },
    ]),
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "Tencent Hunyuan",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/apikey",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "hy3_tokenhub",
      "https://tokenhub.tencentmaas.com/v1",
      "hy3",
    ),
    // 官方备用域名 tencentmaas.cn（文档 1823/130078）；国际站 tokenhub-intl
    // 属不同地域，API Key 不跨站通用，不作候选
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/v1",
      "https://tokenhub.tencentmaas.cn/v1",
    ],
    // 腾讯 TokenHub 官方 Codex 文档（cloud.tencent.com/document/product/1823/133532）：
    // hy3 原生 Responses（wire_api=responses；官方硬性要求的
    // disable_response_storage=true 已由 generateThirdPartyConfig 输出）。
    // ⚠️ 须用 TokenHub API Key（创建时范围需勾选 Hy3）；Coding Plan / Token Plan
    // 订阅 Key 只能走各自 chat 端点，对本预设的 /v1 不通。
    // hy3 在带 tools 的请求里会把 reasoning_effort=low 服务端自动升为 high
    // （Codex 恒带 tools），默认 high 即真实行为。
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）
    modelCatalog: modelCatalog([
      {
        model: "hy3",
        displayName: "Hy3",
        contextWindow: 256000,
        // hy3 不在官方多模态理解模型名单（1823/130988），纯文本
        inputModalities: ["text"],
      },
      {
        model: "hy3-preview",
        displayName: "Hy3 Preview",
        contextWindow: 256000,
        inputModalities: ["text"],
      },
    ]),
    category: "cn_official",
    icon: "hunyuan",
    iconColor: "#0055E9",
  },
  {
    name: "StepFun",
    presetKey: "stepfun-cn",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "stepfun",
      "https://api.stepfun.com/step_plan/v1",
      "step-3.7-flash",
    ),
    endpointCandidates: ["https://api.stepfun.com/step_plan/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
        reasoning: stepThreeLevelReasoning,
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
        reasoning: step2603Reasoning,
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
        reasoning: stepThreeLevelReasoning,
      },
    ]),
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "StepFun en",
    presetKey: "stepfun-en",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "stepfun_en",
      "https://api.stepfun.ai/step_plan/v1",
      "step-3.7-flash",
    ),
    endpointCandidates: ["https://api.stepfun.ai/step_plan/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "step-3.7-flash",
        displayName: "Step 3.7 Flash",
        contextWindow: 262144,
        reasoning: stepThreeLevelReasoning,
      },
      {
        model: "step-3.5-flash-2603",
        displayName: "Step 3.5 Flash 2603",
        contextWindow: 262144,
        reasoning: step2603Reasoning,
      },
      {
        model: "step-3.5-flash",
        displayName: "Step 3.5 Flash",
        contextWindow: 262144,
        reasoning: stepThreeLevelReasoning,
      },
    ]),
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "modelscope",
      "https://api-inference.modelscope.cn/v1",
      "ZhipuAI/GLM-5.1",
    ),
    endpointCandidates: ["https://api-inference.modelscope.cn/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "ZhipuAI/GLM-5.1",
        displayName: "ZhipuAI / GLM-5.1",
        contextWindow: 200000,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "longcat",
      "https://api.longcat.chat/openai/v1",
      "LongCat-2.0",
    ),
    endpointCandidates: ["https://api.longcat.chat/openai/v1"],
    // 美团 LongCat 官方 Codex 文档用 wire_api=responses 对自家 base_url，原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 无官方 catalog：合成 MiMo 式（shell_command 编辑、不发 freeform apply_patch）。
    // 注：LongCat 的 /responses 工具类型契约文档化程度最低，建议真机冒烟一次
    modelCatalog: modelCatalog([
      {
        model: "LongCat-2.0",
        displayName: "LongCat 2.0",
        contextWindow: 1048576,
      },
    ]),
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "minimax",
      "https://api.minimaxi.com/v1",
      "MiniMax-M3",
    ),
    endpointCandidates: ["https://api.minimaxi.com/v1"],
    // MiniMax 官方 API 参考已列 /v1/responses 为正式端点（CN/intl 双区，POST /v1/responses），原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（platform.minimaxi.com/docs/token-plan/codex-cli）：
    // shell_command 编辑、并行工具、文本+图像，不声明 freeform apply_patch
    modelCatalog: modelCatalog([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are Codex, a coding agent based on MiniMax-M3. You and the user share the same workspace and collaborate to achieve the user's goals.",
        reasoning: reasoningSplitBooleanReasoning,
      },
    ]),
    category: "cn_official",
    partnerPromotionKey: "minimax_cn",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "minimax_en",
      "https://api.minimax.io/v1",
      "MiniMax-M3",
    ),
    endpointCandidates: ["https://api.minimax.io/v1"],
    // MiniMax 官方 API 参考已列 /v1/responses 为正式端点（CN/intl 双区，POST /v1/responses），原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（platform.minimax.io/docs/token-plan/codex）：
    // shell_command 编辑、并行工具、文本+图像，不声明 freeform apply_patch
    modelCatalog: modelCatalog([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are Codex, a coding agent based on MiniMax-M3. You and the user share the same workspace and collaborate to achieve the user's goals.",
        reasoning: reasoningSplitBooleanReasoning,
      },
    ]),
    category: "cn_official",
    partnerPromotionKey: "minimax_en",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    apiKeyUrl: "https://ling.tbox.cn/open",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "bailing",
      "https://api.tbox.cn/api/llm/v1",
      "Ling-2.6-1T",
    ),
    endpointCandidates: ["https://api.tbox.cn/api/llm/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "Ling-2.6-1T",
        displayName: "Ling-2.6-1T",
        contextWindow: 262144,
      },
    ]),
    category: "cn_official",
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateMiMoCodexConfig(
      "xiaomi_mimo",
      "https://api.xiaomimimo.com/v1",
      "mimo-v2.5-pro",
    ),
    endpointCandidates: ["https://api.xiaomimimo.com/v1"],
    // 小米 MiMo 官方 Codex 文档已声明原生支持 Responses API（wire_api=responses 对自家 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（mimo.mi.com/.../codex-configuration）：
    // shell_command 编辑、不声明 freeform apply_patch
    modelCatalog: modelCatalog([
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
        reasoning: thinkingBooleanReasoning,
      },
      {
        model: "mimo-v2.5",
        displayName: "MiMo V2.5",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
        reasoning: thinkingBooleanReasoning,
      },
    ]),
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    auth: generateThirdPartyAuth(""),
    config: generateMiMoCodexConfig(
      "xiaomi_mimo_token_plan",
      "https://token-plan-cn.xiaomimimo.com/v1",
      "mimo-v2.5-pro",
    ),
    endpointCandidates: ["https://token-plan-cn.xiaomimimo.com/v1"],
    // 小米 MiMo 官方 Codex 文档已声明原生支持 Responses API（wire_api=responses 对自家 base_url），无需路由接管转换
    apiFormat: "openai_responses",
    // 官方 Codex catalog（mimo.mi.com/.../codex-configuration）：
    // shell_command 编辑、不声明 freeform apply_patch
    modelCatalog: modelCatalog([
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
        reasoning: thinkingBooleanReasoning,
      },
      {
        model: "mimo-v2.5",
        displayName: "MiMo V2.5",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        baseInstructions:
          "You are MiMo, an AI assistant developed by Xiaomi. Today's date: {date} {week}. Your knowledge cutoff date is December 2024.",
        reasoning: thinkingBooleanReasoning,
      },
    ]),
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "novita",
      "https://api.novita.ai/openai/v1",
      "zai-org/glm-5.1",
    ),
    endpointCandidates: ["https://api.novita.ai/openai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 202800,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
  },
  {
    name: "xAI (Grok)",
    presetKey: "xai-grok",
    websiteUrl: "https://x.ai/api",
    apiKeyUrl: "https://console.x.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig("xai", "https://api.x.ai/v1", "grok-4.5"),
    endpointCandidates: ["https://api.x.ai/v1"],
    // xAI 官方以 /v1/responses 为一等端点（docs.x.ai api-reference）：Codex 硬依赖的
    // store:false / include=["reasoning.encrypted_content"] / reasoning effort 均支持，
    // 原生 Responses，无需路由接管转换
    apiFormat: "openai_responses",
    modelCatalog: modelCatalog([
      {
        model: "grok-4.5",
        displayName: "Grok 4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        reasoning: grok45Reasoning,
      },
    ]),
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
  },
  {
    name: "xAI (Grok) OAuth",
    presetKey: "xai-grok-oauth",
    websiteUrl: "https://x.ai/grok",
    auth: generateThirdPartyAuth(""),
    // 托管 OAuth：真实 token 由本地代理按请求注入，CodexAdapter 硬定向
    // api.x.ai；这里的 base_url / 空 auth 只是配置快照，转发时不生效。
    config: generateThirdPartyConfig("xai", "https://api.x.ai/v1", "grok-4.5"),
    apiFormat: "openai_responses",
    providerType: "xai_oauth",
    requiresOAuth: true,
    modelCatalog: modelCatalog([
      {
        model: "grok-4.5",
        displayName: "Grok 4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        reasoning: grok45Reasoning,
      },
    ]),
    category: "third_party",
    icon: "xai",
    iconColor: "#000000",
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "nvidia",
      "https://integrate.api.nvidia.com/v1",
      "moonshotai/kimi-k2.5",
    ),
    endpointCandidates: ["https://integrate.api.nvidia.com/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "moonshotai/kimi-k2.5",
        displayName: "Kimi K2.5",
        contextWindow: 262144,
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
  },
  {
    name: "OpenCode Go (Responses)",
    presetKey: "opencode-go-responses",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    partnerPromotionKey: "opencode_go",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "opencode_go",
      "https://opencode.ai/zen/go/v1",
      "gpt-5.6-luna",
    ),
    endpointCandidates: ["https://opencode.ai/zen/go/v1"],
    apiFormat: "openai_responses",
    modelCatalog: openCodeGoCodexCatalog("responses"),
    category: "third_party",
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    name: "OpenCode Go (Messages)",
    presetKey: "opencode-go-messages",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    partnerPromotionKey: "opencode_go",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "opencode_go_messages",
      "https://opencode.ai/zen/go",
      "minimax-m3",
    ),
    endpointCandidates: ["https://opencode.ai/zen/go"],
    apiFormat: "anthropic",
    modelCatalog: openCodeGoCodexCatalog("messages"),
    category: "third_party",
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    name: "OpenCode Go",
    presetKey: "opencode-go-chat",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    partnerPromotionKey: "opencode_go",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "opencode_go_chat",
      "https://opencode.ai/zen/go/v1",
      "glm-5.3",
    ),
    endpointCandidates: ["https://opencode.ai/zen/go/v1"],
    apiFormat: "openai_chat",
    modelCatalog: openCodeGoCodexCatalog("chat"),
    category: "third_party",
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aihubmix",
      "https://aihubmix.com/v1",
      "gpt-5.6-sol",
    ),
    endpointCandidates: [
      "https://aihubmix.com/v1",
      "https://api.aihubmix.com/v1",
    ],
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "cherryin",
      "https://open.cherryin.net/v1",
      "openai/gpt-5.6-sol",
    ),
    endpointCandidates: ["https://open.cherryin.net/v1"],
    category: "aggregator",
    icon: "cherryin",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "dmxapi",
      "https://www.dmxapi.cn/v1",
      "gpt-5.5",
    ),
    endpointCandidates: ["https://www.dmxapi.cn/v1"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "dmxapi", // 促销信息 i18n key
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "packycode",
      "https://www.packyapi.com/v1",
      "gpt-5.5",
    ),
    endpointCandidates: [
      "https://www.packyapi.com/v1",
      "https://api-slb.packyapi.com/v1",
    ],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.5"
review_model = "gpt-5.5"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APIKEY.FUN"
base_url = "https://api.apikey.fun/v1"
wire_api = "responses"`,
    endpointCandidates: [
      "https://api.apikey.fun/v1",
      "https://slb.apikey.fun/v1",
    ],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.com",
    apiKeyUrl: "https://apinebula.com/02rw5X",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.5"
review_model = "gpt-5.5"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "APINebula"
base_url = "https://apinebula.com/v1"
wire_api = "responses"`,
    endpointCandidates: ["https://apinebula.com/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "zai-org/glm-5.1"
disable_response_storage = true

[model_providers.custom]
name = "AtlasCloud"
base_url = "https://api.atlascloud.ai/v1"
wire_api = "responses"`,
    endpointCandidates: ["https://api.atlascloud.ai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "zai-org/glm-5.1",
        displayName: "GLM 5.1",
        contextWindow: 200000,
      },
    ]),
    isPartner: true,
    partnerPromotionKey: "atlascloud",
    icon: "atlascloud",
  },
  {
    name: "SudoCode",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/register?utm_source=ccswitch&utm_medium=partner",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
review_model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "SudoCode"
base_url = "https://api.sudocode.chat/v1"
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://api.sudocode.chat/v1"],
    apiFormat: "openai_responses",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.top/register?aff=ccswitch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "claudecn",
      "https://claudecn.top/v1",
      "gpt-5.5",
    ),
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "runapi",
      "https://runapi.co/v1",
      "gpt-5.5",
    ),
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "RelaxyCode",
    websiteUrl: "https://www.relaxycode.com",
    apiKeyUrl: "https://www.relaxycode.com/register",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "relaxycode",
      "https://www.relaxycode.com/v1",
      "gpt-5.6-sol",
    ),
    icon: "relaxcode",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "high"
disable_response_storage = true
personality = "pragmatic"

[model_providers.custom]
name = "E-FlowCode"
base_url = "https://e-flowcode.cc/v1"
wire_api = "responses"
model_context_window = 1000000
model_auto_compact_token_limit = 9000000`,
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc/v1"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "custom"
model = "gpt-5.6-sol"
model_reasoning_effort = "medium"
disable_response_storage = true

[model_providers.custom]
name = "PIPELLM"
wire_api = "responses"
base_url = "https://cc-api.pipellm.ai/v1"`,
    category: "aggregator",
    endpointCandidates: ["https://cc-api.pipellm.ai/v1"],
    icon: "pipellm",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "openrouter",
      "https://openrouter.ai/api/v1",
      "gpt-5.6-sol",
    ),
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "therouter",
      "https://api.therouter.ai/v1",
      "openai/gpt-5.3-codex",
    ),
    endpointCandidates: ["https://api.therouter.ai/v1"],
    category: "aggregator",
  },
  {
    name: "PPIO",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ppio",
      "https://api.ppio.com/openai/v1",
      "deepseek/deepseek-v4-flash-0731",
    ),
    endpointCandidates: ["https://api.ppio.com/openai/v1"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "deepseek/deepseek-v4-flash-0731",
        displayName: "Deepseek V4 Flash 0731",
        contextWindow: 1048576,
        inputModalities: ["text"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    icon: "ppio",
    iconColor: "#2874FF",
  },
  {
    // 腾讯云 Token Plan 个人版（1823/130060，2026-08-21 版）：通用 + Hy 两
    // 系列共用同一端点与 API Key，catalog 合并两系列；Auto 智能路由的调用
    // ID 是 tc-code-latest。公告优先于旧目录残留：kimi-k2.5 与
    // minimax-m2.5 均已下线，因此不收录。
    // 注意与 TokenHub 按量 API 市场（1823 线，Hunyuan 预设的 /v1 端点）是
    // 两条产品线：订阅 Key 只能走 /plan 端点，TokenHub Key 对 /plan 不通
    name: "Tencent Token Plan",
    presetKey: "tencent-token-plan",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan",
      "https://api.lkeap.cloud.tencent.com/plan/v3",
      "tc-code-latest",
    ),
    endpointCandidates: ["https://api.lkeap.cloud.tencent.com/plan/v3"],
    // Token Plan 仅提供 Chat Completions；Codex 需要本地路由转换
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "tc-code-latest",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5",
        displayName: "GLM-5",
        contextWindow: 200000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 200000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.3-flash",
        displayName: "GLM-5.3 Flash",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "hy3",
        displayName: "Hy3",
        contextWindow: 256000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "hy4-preview",
        displayName: "Hy4 Preview",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
    ]),
    // 真 Key 实测（2026-08-31）：thinking 参数在 /plan 端点真实生效
    // （开/关均验证，thinking 文档 1300/80637 覆盖 /plan）；reasoning_effort
    // 全模型容忍不报错（含默认 high）。effortValueMode 不声明=passthrough，
    // 档位值域已由各模型 reasoningLevels 限定为实测安全集
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站（新加坡地域）个人版（intl 1300/81315，2026-08-20 版）：
    // Auto 调用 ID 是 auto（≠国内个人版 tc-code-latest），阵容与国内不同
    // （无 GLM-5/5.1/Hy3，多 GLM-5.2/MiniMax-M3）。端点用国际站文档钦定的
    // tencentcloudmaas.com 域（DNS 实测解析新加坡节点）；国内站文档对新加坡
    // 地域给的是 tokenhub-intl.tencentmaas.com，Key 按站独立不跨站通用，
    // 故互不作候选
    name: "Tencent Token Plan (Intl)",
    presetKey: "tencent-token-plan-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    endpointCandidates: ["https://tokenhub-intl.tencentcloudmaas.com/plan/v3"],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "glm-5.3-flash",
        displayName: "GLM-5.3 Flash",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.6",
        displayName: "Kimi K2.6",
        contextWindow: 262144,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
    ]),
    // thinking/reasoning_effort 实测同国内个人版（全模型容忍、开关生效）
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版专业套餐（1823/130659，2026-08-25 版，广州地域）：
    // kimi-k2.5 与 minimax-m2.5 均已公告下线，不收录。新加坡地域阵容不同且
    // Key 不跨站，见 (Intl) 预设
    name: "Tencent Token Plan Enterprise Pro",
    presetKey: "tencent-token-plan-enterprise-pro",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_pro",
      "https://tokenhub.tencentmaas.com/plan/v3",
      "auto",
    ),
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/v3",
      "https://tokenhub-intl.tencentmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "glm-5.3-flash",
        displayName: "GLM-5.3 Flash",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5",
        displayName: "GLM-5",
        contextWindow: 200000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5.1",
        displayName: "GLM-5.1",
        contextWindow: 200000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "glm-5-turbo",
        displayName: "GLM-5 Turbo",
        contextWindow: 200000,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code-highspeed",
        displayName: "Kimi K2.7 Code HighSpeed",
        contextWindow: 262144,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.6",
        displayName: "Kimi K2.6",
        contextWindow: 262144,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m2.7",
        displayName: "MiniMax M2.7",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-0731",
        displayName: "DeepSeek V4 Flash 0731 GA",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-0813",
        displayName: "DeepSeek V4 Pro 0813 GA",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek/deepseek-v4-flash-vision-exp",
        displayName: "DeepSeek V4 Flash Vision Exp",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
      },
    ]),
    // reasoning_effort 默认 high 全模型实测容忍；glm-5.3 的 medium/xhigh
    // 会 400，档位值域已由各模型 reasoningLevels 限定为实测安全集
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版专业套餐（intl 1300/81489，2026-08-26 版，新加坡地域）：
    // 阵容为广州地域子集（无 GLM-5/5.1/5-Turbo、Kimi-K2.6、MiniMax-M2.7）
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    presetKey: "tencent-token-plan-enterprise-pro-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_pro_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "https://tokenhub.tencentcloudmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "glm-5.3-flash",
        displayName: "GLM-5.3 Flash",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.3",
        displayName: "GLM-5.3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["low", "high", "max"],
        defaultReasoningLevel: "high",
      },
      {
        model: "glm-5.2",
        displayName: "GLM-5.2",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "minimax-m3",
        displayName: "MiniMax M3",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "kimi-k3",
        displayName: "Kimi K3",
        contextWindow: 1048576,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code",
        displayName: "Kimi K2.7 Code",
        contextWindow: 262144,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
      {
        model: "kimi-k2.7-code-highspeed",
        displayName: "Kimi K2.7 Code HighSpeed",
        contextWindow: 262144,
        inputModalities: ["text", "image"],
        reasoningLevels: ["high"],
      },
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-0731",
        displayName: "DeepSeek V4 Flash 0731 GA",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-0813",
        displayName: "DeepSeek V4 Pro 0813 GA",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-flash-202605",
        displayName: "DeepSeek V4 Flash Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek-v4-pro-202606",
        displayName: "DeepSeek V4 Pro Official",
        contextWindow: 1048576,
        inputModalities: ["text"],
        reasoningLevels: ["none", "high"],
      },
      {
        model: "deepseek/deepseek-v4-flash-vision-exp",
        displayName: "DeepSeek V4 Flash Vision Exp",
        contextWindow: 1000000,
        inputModalities: ["text", "image"],
        reasoningLevels: ["none", "high"],
      },
    ]),
    // reasoning_effort 默认 high 全模型实测容忍；同国内企业专业版
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版轻享套餐（1823/131173，2026-08-28 版）：仅 Auto 模型。
    // 国内 auto 关思考被静默忽略（真 Key 实测 2026-08-31），只列 high
    name: "Tencent Token Plan Enterprise Lite",
    presetKey: "tencent-token-plan-enterprise-lite",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_lite",
      "https://tokenhub.tencentmaas.com/plan/v3",
      "auto",
    ),
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/v3",
      "https://tokenhub-intl.tencentmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版轻享套餐（intl 1300/81490）：新加坡地域（资源调度范围
    // Global），仅 Auto 模型。INTL auto 关思考真实生效（真 Key 实测）
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    presetKey: "tencent-token-plan-enterprise-lite-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "tencent_token_plan_enterprise_lite_intl",
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "auto",
    ),
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      "https://tokenhub.tencentcloudmaas.com/plan/v3",
    ],
    apiFormat: "openai_chat",
    modelCatalog: modelCatalog([
      {
        model: "auto",
        displayName: "Auto",
        contextWindow: 196608,
        inputModalities: ["text"],
        reasoningLevels: ["high"],
      },
    ]),
    codexChatReasoning: {
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      outputFormat: "reasoning_content",
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
  },
];

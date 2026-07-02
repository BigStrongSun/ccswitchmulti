export interface CodexPlanModelFetchSource {
  baseUrl?: string | null;
  partnerPromotionKey?: string | null;
  providerName?: string | null;
  accessKeyId?: string | null;
  secretAccessKey?: string | null;
}

export const VOLCENGINE_AGENT_PLAN_MODEL_LIST_ACTION = "ListArkAgentPlanModel";

const VOLCENGINE_AGENT_PLAN_PROMOTION_KEY = "volcengine_agentplan";
const BYTEPLUS_PROMOTION_KEY = "byteplus";

const VOLCENGINE_AGENT_PLAN_BASE_URL_MARKERS = [
  "ark.cn-beijing.volces.com/api/coding/v3",
];

const BYTEPLUS_PLAN_BASE_URL_MARKERS = [
  "ark.ap-southeast.bytepluses.com/api/coding/v3",
];

// 归一化用于 Plan provider 判定的文本，避免大小写、尾斜杠和空格导致漏判。
function normalizePlanFetchText(value?: string | null): string {
  return String(value ?? "")
    .trim()
    .replace(/\/+$/, "")
    .toLowerCase();
}

// 判断当前 Codex provider 是否是火山 AgentPlan；它不能走 OpenAI `/models`，但有 AK/SK 时可走火山管控面 OpenAPI。
export function isCodexVolcengineAgentPlanModelFetch(
  source: CodexPlanModelFetchSource,
): boolean {
  const promotionKey = normalizePlanFetchText(source.partnerPromotionKey);
  if (promotionKey === VOLCENGINE_AGENT_PLAN_PROMOTION_KEY) {
    return true;
  }

  const baseUrl = normalizePlanFetchText(source.baseUrl);
  if (
    baseUrl &&
    VOLCENGINE_AGENT_PLAN_BASE_URL_MARKERS.some((marker) =>
      baseUrl.includes(marker),
    )
  ) {
    return true;
  }

  const providerName = normalizePlanFetchText(source.providerName);
  return (
    providerName.includes("agentplan") &&
    (providerName.includes("火山") || providerName.includes("volc"))
  );
}

// 判断当前 Codex provider 是否是暂未接入专用模型列表的 BytePlus Plan。
export function isCodexBytePlusPlanModelFetch(
  source: CodexPlanModelFetchSource,
): boolean {
  const promotionKey = normalizePlanFetchText(source.partnerPromotionKey);
  if (promotionKey === BYTEPLUS_PROMOTION_KEY) {
    return true;
  }

  const baseUrl = normalizePlanFetchText(source.baseUrl);
  return Boolean(
    baseUrl &&
      BYTEPLUS_PLAN_BASE_URL_MARKERS.some((marker) => baseUrl.includes(marker)),
  );
}

// 判断火山管控面 OpenAPI 取模型列表所需的 AK/SK 是否已配置。
export function hasCodexPlanModelFetchCredentials(
  source: CodexPlanModelFetchSource,
): boolean {
  return Boolean(source.accessKeyId?.trim() && source.secretAccessKey?.trim());
}

// 返回需要后端走火山管控面 OpenAPI 的模型列表 Action；没有凭据时返回 undefined 让调用方走 catalog 回退。
export function codexPlanModelListAction(
  source: CodexPlanModelFetchSource,
): string | undefined {
  if (
    isCodexVolcengineAgentPlanModelFetch(source) &&
    hasCodexPlanModelFetchCredentials(source)
  ) {
    return VOLCENGINE_AGENT_PLAN_MODEL_LIST_ACTION;
  }
  return undefined;
}

// 判断当前 Codex provider 是否只能使用内置 modelCatalog，而不能在线刷新模型目录。
export function isCodexCatalogOnlyPlanModelFetch(
  source: CodexPlanModelFetchSource,
): boolean {
  if (isCodexBytePlusPlanModelFetch(source)) {
    return true;
  }
  return (
    isCodexVolcengineAgentPlanModelFetch(source) &&
    !hasCodexPlanModelFetchCredentials(source)
  );
}

// 生成 catalog-only Plan 的用户提示；火山 AgentPlan 缺 AK/SK 时提示补管控面凭据。
export function codexCatalogOnlyPlanModelFetchMessage(
  hasModelCatalog: boolean,
  source: CodexPlanModelFetchSource = {},
): string {
  if (isCodexVolcengineAgentPlanModelFetch(source)) {
    return hasModelCatalog
      ? "火山 AgentPlan 模型列表需要火山 AK/SK 管控面 OpenAPI，当前缺少 AK/SK，已保留内置 modelCatalog。"
      : "火山 AgentPlan 模型列表需要火山 AK/SK 管控面 OpenAPI，请在用量查询里配置 AccessKey ID / SecretAccessKey，或重新选择预设恢复 modelCatalog。";
  }
  return hasModelCatalog
    ? "当前 Plan 的模型枚举不开放 OpenAI /models，已保留内置 modelCatalog。"
    : "当前 Plan 的模型枚举不开放 OpenAI /models，请手动添加模型或重新选择预设恢复 modelCatalog。";
}

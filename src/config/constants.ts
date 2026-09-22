// Provider 类型常量
export const PROVIDER_TYPES = {
  GITHUB_COPILOT: "github_copilot",
  CODEX_OAUTH: "codex_oauth",
  XAI_OAUTH: "xai_oauth",
  // Token Exchange（TE）Provider：一次任务一个短时 Proxy Key，由本机注入端点按 Task/session
  // 注入；它不是长期凭据，因此与托管 OAuth 并列但走独立的注入路径。
  TE_PROVIDER: "token_exchange",
} as const;

/** 所有一等 providerType 的联合；各 app 的 preset 接口共用它，避免各写一套字面量。 */
export type ProviderTypeId =
  (typeof PROVIDER_TYPES)[keyof typeof PROVIDER_TYPES];

// 托管 OAuth 供应商类型：真实凭据由本地代理按请求注入，因此无论上游是否
// 需要格式转换，都必须开启路由接管才能通过认证。新增此类预设时只需把
// providerType 加进本数组，needsRouting 判定即自动覆盖，无需逐个特判。
export const OAUTH_PROVIDER_TYPES: readonly string[] = [
  PROVIDER_TYPES.GITHUB_COPILOT,
  PROVIDER_TYPES.CODEX_OAUTH,
  PROVIDER_TYPES.XAI_OAUTH,
];

/** 判断某 providerType 是否为托管 OAuth（凭据由代理注入、必须开启路由）。 */
export function isOAuthProviderType(
  providerType: string | null | undefined,
): boolean {
  return providerType != null && OAUTH_PROVIDER_TYPES.includes(providerType);
}

// TE Provider 类型：凭据（Proxy Key）只在运行时由本机注入端点注入，静态配置里只有公开占位值。
export const TE_PROVIDER_TYPES: readonly string[] = [
  PROVIDER_TYPES.TE_PROVIDER,
];

/** 判断某 providerType 是否为 Token Exchange Provider。 */
export function isTeProviderType(
  providerType: string | null | undefined,
): boolean {
  return providerType != null && TE_PROVIDER_TYPES.includes(providerType);
}

/** 必须经过本机路由/注入才能工作的 providerType（托管 OAuth 与 TE Provider）。 */
export function requiresLocalRoutingByType(
  providerType: string | null | undefined,
): boolean {
  return isOAuthProviderType(providerType) || isTeProviderType(providerType);
}

// 用量脚本模板类型常量
export const TEMPLATE_TYPES = {
  CUSTOM: "custom",
  GENERAL: "general",
  NEW_API: "newapi",
  GITHUB_COPILOT: "github_copilot",
  TOKEN_PLAN: "token_plan",
  BALANCE: "balance",
  OFFICIAL_SUBSCRIPTION: "official_subscription",
} as const;

export type TemplateType = (typeof TEMPLATE_TYPES)[keyof typeof TEMPLATE_TYPES];

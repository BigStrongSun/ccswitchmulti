export type CredentialStatus =
  | "valid"
  | "expired"
  | "reauth_required"
  | "not_found"
  | "parse_error";

export interface QuotaTier {
  name: string;
  utilization: number; // 0-100
  resetsAt: string | null;
  usedValueUsd?: number | null;
  maxValueUsd?: number | null;
  planLabel?: string | null;
}

export interface ExtraUsage {
  isEnabled: boolean;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
  currency: string | null;
}

/** Codex 单个 reset credit 的脱敏展示字段，不包含 credit id 或账号标识。 */
export interface ResetCreditInfo {
  /** 重置额度类型，例如 rate_limit。 */
  resetType: string | null;
  /** 官方状态，例如 available / redeemed / expired。 */
  status: string | null;
  /** ISO 8601 到期时间；官方未返回时为 null。 */
  expiresAt: string | null;
  /** 官方展示标题；仅用于 UI 标签。 */
  title: string | null;
}

/** Codex banked reset credits 汇总。 */
export interface ResetCredits {
  /** 可用 reset 数量，优先使用官方 available_count。 */
  availableCount: number;
  /** 安全展示明细，可能少于 availableCount。 */
  credits: ResetCreditInfo[];
}

export interface SubscriptionQuota {
  tool: string;
  credentialStatus: CredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: QuotaTier[];
  extraUsage: ExtraUsage | null;
  resetCredits: ResetCredits | null;
  resetCreditsError: string | null;
  error: string | null;
  queriedAt: number | null;
}

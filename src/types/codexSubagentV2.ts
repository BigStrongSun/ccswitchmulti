export type CodexSubagentV2SelectionPolicy =
  | "balanced"
  | "official_first"
  | "third_party_first";

export type CodexSubagentTaskStrength =
  | "long_context_reading"
  | "repository_exploration"
  | "evidence_collection"
  | "summarization"
  | "complex_debugging"
  | "architecture_design"
  | "bounded_implementation"
  | "complex_implementation"
  | "testing"
  | "high_risk_review";

export type CodexSubagentOptimization = "speed" | "balanced" | "quality";
export type CodexSubagentWriteScope =
  | "read_only"
  | "bounded_changes"
  | "complex_changes";
export type CodexSubagentPreference = "preferred" | "eligible" | "fallback";
export type CodexSubagentExplicitReasoningEffort =
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "ultra";

export type CodexSubagentReasoningEffort =
  | "none"
  | CodexSubagentExplicitReasoningEffort;

export interface CodexSubagentReasoningCapability {
  supportKind: "effort_levels" | "boolean_only" | "unsupported" | "unknown";
  source?: string | null;
  confidence: "confirmed" | "declared" | "unverified";
  codexSelectableEfforts: CodexSubagentReasoningEffort[];
  providerAcceptedEfforts: CodexSubagentReasoningEffort[];
  providerDefaultEffort?: CodexSubagentReasoningEffort | null;
  disableAllowed: boolean;
  effortMap: Partial<
    Record<CodexSubagentReasoningEffort, CodexSubagentReasoningEffort>
  >;
  /** 来源能力的稳定指纹；无来源（unknown 兜底）时为空串。 */
  fingerprint?: string;
}

export type CodexSubagentReasoningCapabilities = Record<
  string,
  CodexSubagentReasoningCapability
>;

// ---------------------------------------------------------------------------
// P3：单模型推理能力解析结果（与 catalog / 请求路径 / Sub-Agent 同源）。
// 对应后端 CodexModelReasoningResolution（camelCase）。
// ---------------------------------------------------------------------------

/** 三态支持状态（模型推理能力 schema v2）。 */
export type CodexReasoningSupportStatus =
  | "confirmed_supported"
  | "confirmed_unsupported"
  | "unknown";

/** 控制形态（模型推理能力 schema v2），与支持状态相互独立。 */
export type CodexReasoningControlKind =
  | "none"
  | "boolean"
  | "graded"
  | "budget"
  | "unknown";

/** 能力声明的证据等级。 */
export type CodexReasoningCapabilityConfidence =
  | "authoritative"
  | "verified"
  | "maintained"
  | "inferred";

export interface CodexModelReasoningUpstream {
  format: string;
  parameter: string;
  effortMap?: Record<string, string>;
}

/** 模型声明的推理能力（schema v2），即 catalog 行里的 reasoning 字段。 */
export interface CodexModelReasoningDeclaredCapability {
  schemaVersion?: number;
  supportStatus?: CodexReasoningSupportStatus;
  controlKind?: CodexReasoningControlKind;
  /** Legacy 字段：仅用于读取旧数据。 */
  supported?: boolean;
  supportedEfforts: string[];
  defaultEffort?: string | null;
  disableAllowed: boolean;
  upstream: CodexModelReasoningUpstream;
  outputFormat?: string | null;
  source?: string | null;
  confidence?: CodexReasoningCapabilityConfidence | null;
}

/** 只读检测快照的 reasoning 子对象（allowlist 字段，无敏感信息）。 */
export interface CodexModelReasoningDetectionReasoning {
  supportedEfforts: string[];
  defaultEffort?: string | null;
  /** 推理强制开启（不可关闭）。 */
  mandatory: boolean;
  /** 服务端默认是否开启推理。 */
  defaultEnabled?: boolean | null;
}

/** 只读检测快照（TTL 缓存中的候选，供「采用检测结果」动作）。 */
export interface CodexModelReasoningDetection {
  providerKey: string;
  model: string;
  /** Unix 毫秒时间戳。 */
  fetchedAt: number;
  /** 来源标识：openrouter_api / vllm_server / ... */
  source: string;
  reasoning?: CodexModelReasoningDetectionReasoning | null;
}

/** 单模型推理能力解析结果。 */
export interface CodexModelReasoningResolution {
  model: string;
  /** 当前声明的能力（schema v2）；未声明时为 null。 */
  capability: CodexModelReasoningDeclaredCapability | null;
  /** 能力来源：user / detection / library / builtin / official / unknown。 */
  source: string;
  /** 来源能力的稳定指纹。 */
  fingerprint: string;
  /** 最终生效的 Sub-Agent 能力投影。 */
  resolved: CodexSubagentReasoningCapability;
  /** 是否存在有效 TTL 检测候选快照（供「采用检测结果」动作）。 */
  hasDetectionCandidate: boolean;
  detection: CodexModelReasoningDetection | null;
}

/** P4：AI/CLI 只读 inspect/list/validate/export JSON 契约。 */
export interface CodexReasoningDiagnostic {
  level: "info" | "warning" | "error";
  code: string;
  message: string;
}

export interface CodexReasoningProviderSummary {
  id: string;
  name: string;
}

export interface CodexReasoningInspectResponse {
  schemaVersion: 1;
  requestId: string;
  revision: string;
  provider: CodexReasoningProviderSummary;
  model: string;
  persisted: Record<string, unknown>;
  resolved: CodexModelReasoningResolution;
  codexProjection: Record<string, unknown>;
  providerProjection: Record<string, unknown>;
  diagnostics: CodexReasoningDiagnostic[];
}

export interface CodexReasoningListItem {
  model: string;
  source: string;
  fingerprint: string;
  resolved: CodexSubagentReasoningCapability;
}

export interface CodexReasoningProviderList {
  provider: CodexReasoningProviderSummary;
  revision: string;
  items: CodexReasoningListItem[];
  diagnostics: CodexReasoningDiagnostic[];
}

export interface CodexReasoningListResponse {
  schemaVersion: 1;
  requestId: string;
  providers: CodexReasoningProviderList[];
  diagnostics: CodexReasoningDiagnostic[];
}

export interface CodexReasoningValidationResponse {
  schemaVersion: 1;
  requestId: string;
  revision: string;
  provider: CodexReasoningProviderSummary;
  valid: boolean;
  modelCount: number;
  diagnostics: CodexReasoningDiagnostic[];
}

export interface CodexReasoningExportResponse {
  schemaVersion: 1;
  requestId: string;
  revision: string;
  redacted: true;
  provider: CodexReasoningProviderSummary;
  models: Array<Record<string, unknown>>;
  providerReasoning?: Record<string, unknown> | null;
  diagnostics: CodexReasoningDiagnostic[];
}

/** 只读检测适配器结果（对应后端 DiscoveryOutcome，snake_case 外部标签）。 */
export type CodexReasoningDiscoveryOutcome =
  | { found: CodexModelReasoningDetection }
  | "not_advertised"
  | "unavailable"
  | "invalid";

export type CodexSubagentReasoningPolicy =
  | { policy: "delegated" }
  | { policy: "model_default" }
  | { policy: "fixed"; effort: CodexSubagentExplicitReasoningEffort }
  | { policy: "disabled" };

export interface CodexSubagentQuestionnaire {
  taskStrengths: CodexSubagentTaskStrength[];
  optimization: CodexSubagentOptimization;
  writeScope: CodexSubagentWriteScope;
  preference: CodexSubagentPreference;
}

export interface CodexSubagentProfileOverrides {
  roleName?: string;
  description?: string;
  developerInstructions?: string;
  nicknameCandidates?: string[];
}

export interface CodexSubagentV2Profile {
  model: string;
  enabled: boolean;
  inputModalities?: ["text"] | ["text", "image"];
  questionnaire: CodexSubagentQuestionnaire;
  reasoning: CodexSubagentReasoningPolicy;
  overrides?: CodexSubagentProfileOverrides;
}

export interface CodexSubagentV2Config {
  schemaVersion: 2;
  selectionPolicy: CodexSubagentV2SelectionPolicy;
  profiles: Record<string, CodexSubagentV2Profile>;
}

export interface CodexSubagentProfilePreview {
  providerKind: "official" | "third_party";
  requestedRoleName: string;
  effectiveRoleName: string;
  description: string;
  developerInstructions: string;
  nicknameCandidates: string[];
  model: string;
  modelProvider: "codex_model_router_v2";
  modelReasoningEffort?: CodexSubagentExplicitReasoningEffort;
  reasoningPolicy: CodexSubagentReasoningPolicy["policy"];
  reasoningCapability: CodexSubagentReasoningCapability;
  modelContextWindow: number;
  tomlPreview: string;
  warnings: string[];
}

export type CodexSubagentProfileStatusCode =
  | "generated"
  | "disabled"
  | "unroutable"
  | "invalid"
  | "collision"
  | "inactive_v1";
export type CodexSubagentNonGenerationReason = Exclude<
  CodexSubagentProfileStatusCode,
  "generated"
>;
export type CodexSubagentFieldSource = "automatic" | "override";

export type CodexSubagentInputModalitySource =
  | "profile_explicit"
  | "route"
  | "catalog"
  | "name_registry"
  | "unknown";

export interface CodexSubagentModalityDeclaration {
  source: CodexSubagentInputModalitySource;
  declared?: string[];
  adopted: boolean;
}

export interface CodexSubagentInputModalityInfo {
  modalities?: string[];
  source: CodexSubagentInputModalitySource;
  declarations: CodexSubagentModalityDeclaration[];
  conflict?: string;
}

export interface CodexSubagentProfileFieldSources {
  roleName: CodexSubagentFieldSource;
  description: CodexSubagentFieldSource;
  developerInstructions: CodexSubagentFieldSource;
  nicknameCandidates: CodexSubagentFieldSource;
  modelReasoningEffort: CodexSubagentFieldSource;
}

export interface CodexSubagentProfileStatus {
  profileKey?: string;
  model?: string;
  providerKind?: "official" | "third_party";
  enabled?: boolean;
  routable: boolean;
  fieldSources?: CodexSubagentProfileFieldSources;
  inputModality?: CodexSubagentInputModalityInfo;
  requestedRoleName?: string;
  effectiveRoleName?: string;
  roleFilePath?: string;
  modelProvider?: "codex_model_router_v2";
  modelReasoningEffort?: CodexSubagentExplicitReasoningEffort;
  reasoningPolicy?: CodexSubagentReasoningPolicy["policy"];
  reasoningCapability?: CodexSubagentReasoningCapability;
  status: CodexSubagentProfileStatusCode;
  nonGenerationReason?: CodexSubagentNonGenerationReason;
  warnings: string[];
}

export interface CodexSubagentProfileStatuses {
  mode: "v1" | "v2";
  generationSource:
    | "legacy_managed_roles"
    | "configured_profiles"
    | "inactive_v1";
  profiles: CodexSubagentProfileStatus[];
  warnings: string[];
}

export const DEFAULT_CODEX_SUBAGENT_V2: CodexSubagentV2Config = {
  schemaVersion: 2,
  selectionPolicy: "balanced",
  profiles: {
    "deepseek-v4-flash": {
      model: "deepseek-v4-flash",
      enabled: true,
      questionnaire: {
        taskStrengths: [
          "long_context_reading",
          "repository_exploration",
          "evidence_collection",
          "summarization",
          "testing",
        ],
        optimization: "speed",
        writeScope: "read_only",
        preference: "preferred",
      },
      reasoning: { policy: "delegated" },
    },
    "deepseek-v4-pro": {
      model: "deepseek-v4-pro",
      enabled: true,
      questionnaire: {
        taskStrengths: [
          "complex_debugging",
          "architecture_design",
          "complex_implementation",
          "high_risk_review",
          "testing",
        ],
        optimization: "quality",
        writeScope: "complex_changes",
        preference: "preferred",
      },
      reasoning: { policy: "delegated" },
    },
  },
};

/** 为新建 V2 方案和显式 legacy 初始化返回互不共享引用的问卷默认值。 */
export function createDefaultCodexSubagentV2Config(): CodexSubagentV2Config {
  return JSON.parse(JSON.stringify(DEFAULT_CODEX_SUBAGENT_V2));
}

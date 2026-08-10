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
export type CodexSubagentQuestionnaireReasoningEffort =
  | "auto"
  | "low"
  | "medium"
  | "high"
  | "xhigh";
export type CodexSubagentExplicitReasoningEffort = Exclude<
  CodexSubagentQuestionnaireReasoningEffort,
  "auto"
>;

export interface CodexSubagentQuestionnaire {
  taskStrengths: CodexSubagentTaskStrength[];
  optimization: CodexSubagentOptimization;
  writeScope: CodexSubagentWriteScope;
  preference: CodexSubagentPreference;
  reasoningEffort: CodexSubagentQuestionnaireReasoningEffort;
}

export interface CodexSubagentProfileOverrides {
  roleName?: string;
  description?: string;
  developerInstructions?: string;
  nicknameCandidates?: string[];
  modelReasoningEffort?: CodexSubagentExplicitReasoningEffort;
}

export interface CodexSubagentV2Profile {
  model: string;
  enabled: boolean;
  questionnaire: CodexSubagentQuestionnaire;
  overrides?: CodexSubagentProfileOverrides;
}

export interface CodexSubagentV2Config {
  schemaVersion: 1;
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
  modelContextWindow?: number;
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
  requestedRoleName?: string;
  effectiveRoleName?: string;
  roleFilePath?: string;
  modelProvider?: "codex_model_router_v2";
  modelReasoningEffort?: CodexSubagentExplicitReasoningEffort;
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
  schemaVersion: 1,
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
        preference: "eligible",
        reasoningEffort: "medium",
      },
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
        preference: "eligible",
        reasoningEffort: "high",
      },
    },
  },
};

/** 为新建 V2 方案和显式 legacy 初始化返回互不共享引用的问卷默认值。 */
export function createDefaultCodexSubagentV2Config(): CodexSubagentV2Config {
  return JSON.parse(JSON.stringify(DEFAULT_CODEX_SUBAGENT_V2));
}

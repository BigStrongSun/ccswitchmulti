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
}

export type CodexSubagentReasoningCapabilities = Record<
  string,
  CodexSubagentReasoningCapability
>;

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
      inputModalities: ["text"],
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
      inputModalities: ["text"],
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

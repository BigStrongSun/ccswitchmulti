import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "@/types";
import type {
  CodexModelReasoningResolution,
  CodexReasoningDiscoveryOutcome,
  CodexSubagentProfilePreview,
  CodexSubagentProfileStatuses,
  CodexSubagentReasoningCapabilities,
  CodexSubagentV2Config,
  CodexSubagentV2Profile,
} from "@/types/codexSubagentV2";

export type CodexSubagentV2MutationProvider = Provider & {
  projection?: {
    status: "applied" | "not_required" | "pending_retry";
    warning?: {
      code:
        | "codex_live_projection_pending_retry"
        | "codex_current_provider_lookup_pending_retry";
      message: string;
    };
  };
  verification?: {
    databasePersisted: boolean;
    roleFilesStatus: "verified" | "not_required" | "pending_retry" | "failed";
    roleFiles: Array<{
      profileKey: string;
      path: string;
      exists: boolean;
      contentMatches: boolean;
    }>;
    activation: "restart_codex_and_start_new_session";
  };
};

export type CodexSubagentV2ReconcileAction =
  | "sync_catalog"
  | "remove_all_invalid"
  | "recover_all_invalid_from_catalog"
  | "prune_unroutable";

export const codexSubagentV2Api = {
  getReasoningCapabilities(
    settingsConfig: Record<string, unknown>,
  ): Promise<CodexSubagentReasoningCapabilities> {
    return invoke("get_codex_subagent_reasoning_capabilities", {
      settingsConfig,
    });
  },

  /**
   * P3：解析单模型最终生效的推理能力（与 catalog / 请求 / Sub-Agent 同源）。
   * 供模型卡片展示状态 / 来源 / 指纹 / 档位 / 映射 / 最终行为。
   */
  resolveModelReasoningCapability(
    settingsConfig: Record<string, unknown>,
    providerId: string,
    model: string,
  ): Promise<CodexModelReasoningResolution> {
    return invoke("resolve_codex_model_reasoning_capability", {
      settingsConfig,
      providerId,
      model,
    });
  },

  /**
   * P3：触发单模型只读检测（异步）。仅 Found 会写入 TTL 检测缓存；
   * NotAdvertised / Unavailable / Invalid 不写缓存（缺失证据不是不存在的证据）。
   */
  triggerModelReasoningDetection(
    provider: Provider,
    model: string,
  ): Promise<CodexReasoningDiscoveryOutcome> {
    return invoke("trigger_codex_model_reasoning_detection", {
      provider,
      model,
    });
  },

  previewProfile(
    settingsConfig: Record<string, unknown>,
    model: string,
    profile: CodexSubagentV2Profile,
  ): Promise<CodexSubagentProfilePreview> {
    return invoke("preview_codex_subagent_profile", {
      settingsConfig,
      model,
      profile,
    });
  },

  getProfileStatuses(
    settingsConfig: Record<string, unknown>,
  ): Promise<CodexSubagentProfileStatuses> {
    return invoke("get_codex_subagent_profile_statuses", { settingsConfig });
  },

  updateProviderConfig(
    providerId: string,
    subagentV2: CodexSubagentV2Config,
  ): Promise<CodexSubagentV2MutationProvider> {
    return invoke("update_codex_subagent_v2", { providerId, subagentV2 });
  },

  initializeProviderConfig(
    providerId: string,
  ): Promise<CodexSubagentV2MutationProvider> {
    return invoke("initialize_codex_subagent_v2", { providerId });
  },

  reconcileProviderProfiles(
    providerId: string,
    action: CodexSubagentV2ReconcileAction,
    subagentV2: CodexSubagentV2Config,
  ): Promise<CodexSubagentV2MutationProvider> {
    return invoke("reconcile_codex_subagent_v2_profiles", {
      providerId,
      action,
      subagentV2,
    });
  },
};

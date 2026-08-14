import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "@/types";
import type {
  CodexSubagentProfilePreview,
  CodexSubagentProfileStatuses,
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
  | "recover_all_invalid_from_catalog";

export const codexSubagentV2Api = {
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

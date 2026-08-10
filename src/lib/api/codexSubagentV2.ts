import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "@/types";
import type {
  CodexSubagentProfilePreview,
  CodexSubagentProfileStatuses,
  CodexSubagentV2Config,
  CodexSubagentV2Profile,
} from "@/types/codexSubagentV2";

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
  ): Promise<Provider> {
    return invoke("update_codex_subagent_v2", { providerId, subagentV2 });
  },
};

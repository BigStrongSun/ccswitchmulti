import { invoke } from "@tauri-apps/api/core";
import type {
  CodexSubagentProfilePreview,
  CodexSubagentProfileStatuses,
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
};

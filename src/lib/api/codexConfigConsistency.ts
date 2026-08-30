import { invoke } from "@tauri-apps/api/core";

export type CodexConfigConsistencyState =
  | "consistent"
  | "external_drift"
  | "not_applicable"
  | "unavailable";

export type CodexConfigConsistencyAction =
  | "apply_ccsm"
  | "keep_codex"
  | "later";

export interface CodexConfigConsistencyReport {
  state: CodexConfigConsistencyState;
  providerId: string | null;
  expectedFingerprint: string | null;
  actualFingerprint: string | null;
  changedKeys: string[];
  reason: string | null;
}

export const codexConfigConsistencyApi = {
  inspect(): Promise<CodexConfigConsistencyReport> {
    return invoke("inspect_codex_config_consistency");
  },
  resolve(
    expectedFingerprint: string,
    action: CodexConfigConsistencyAction,
  ): Promise<CodexConfigConsistencyReport> {
    return invoke("resolve_codex_config_consistency", {
      expectedFingerprint,
      action,
    });
  },
};

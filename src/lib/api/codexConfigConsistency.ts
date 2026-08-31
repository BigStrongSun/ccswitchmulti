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

export type CodexConfigRuntimeActivationState =
  | "not_running"
  | "current"
  | "restart_required"
  | "unknown";

export interface CodexConfigRuntimeActivation {
  state: CodexConfigRuntimeActivationState;
  appServerStartedAt: string | null;
  configModifiedAt: string | null;
  reason: string | null;
}

export interface CodexConfigConsistencyReport {
  state: CodexConfigConsistencyState;
  providerId: string | null;
  expectedFingerprint: string | null;
  actualFingerprint: string | null;
  changedKeys: string[];
  reason: string | null;
  runtimeActivation: CodexConfigRuntimeActivation;
}

export type CodexRuntimeRefreshStage =
  | "closing"
  | "force_closing"
  | "applying_config"
  | "launching"
  | "verifying"
  | "completed";

export interface CodexRuntimeRefreshProgress {
  stage: CodexRuntimeRefreshStage;
}

export interface CodexRuntimeRefreshPreflight {
  supported: boolean;
  canRefresh: boolean;
  snapshotToken: string;
  desktopProcessCount: number;
  appServerProcessCount: number;
  processCount: number;
  launchTarget: string | null;
  warning: string | null;
}

export interface CodexRuntimeRefreshResult {
  forceTerminated: boolean;
  closedProcessCount: number;
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
  inspectRuntimeRefresh(): Promise<CodexRuntimeRefreshPreflight> {
    return invoke("inspect_codex_runtime_refresh");
  },
  refreshRuntimeState(
    snapshotToken: string,
  ): Promise<CodexRuntimeRefreshResult> {
    return invoke("refresh_codex_runtime_state", { snapshotToken });
  },
};

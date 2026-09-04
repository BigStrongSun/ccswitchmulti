import { invoke } from "@tauri-apps/api/core";

export type CodexTimezoneMatch =
  | "exact"
  | "offset_match"
  | "mismatch"
  | "unknown";

export interface CodexEgressTimezoneDetection {
  targetHost: string;
  dnsAddresses: string[];
  dnsUsesNonPublicAddress: boolean;
  egressIp: string;
  countryCode?: string;
  region?: string;
  city?: string;
  colo?: string;
  egressTimezone: string;
  currentTimezone: string;
  egressUtcOffset: string;
  currentUtcOffset: string;
  timezoneMatch: CodexTimezoneMatch;
  checkedAt: number;
  networkPath: "ccsm_global_proxy" | "system_or_transparent" | string;
}

export interface CodexRuntimeTimezoneInspection {
  runtimeTimezone: string;
  runtimeUtcOffset: string;
  configuredTimezone?: string;
  matchesConfigured?: boolean;
  timezoneMatch: CodexTimezoneMatch;
}

export type CodexEgressMonitorState =
  | "disabled"
  | "not_tested"
  | "checking"
  | "ready"
  | "restart_required"
  | "error";

export interface CodexEgressMonitorStatus {
  state: CodexEgressMonitorState;
  detectedTimezone?: string;
  detectedEgressIp?: string;
  detectedAt?: number;
  lastAttemptAt?: number;
  lastTrigger?: string;
  lastError?: string;
  nextCheckAt?: number;
  monitorIntervalMinutes: number;
  restartRequired: boolean;
}

export const codexEgressTimezoneApi = {
  async detect(): Promise<CodexEgressTimezoneDetection> {
    return invoke("detect_codex_egress_timezone");
  },
  async inspectRuntime(): Promise<CodexRuntimeTimezoneInspection> {
    return invoke("inspect_codex_runtime_timezone");
  },
  async validate(timezone: string): Promise<void> {
    return invoke("validate_codex_egress_timezone", { timezone });
  },
  async monitorStatus(): Promise<CodexEgressMonitorStatus> {
    return invoke("get_codex_egress_timezone_monitor_status");
  },
  async triggerAutomaticProbe(): Promise<CodexEgressMonitorStatus> {
    return invoke("trigger_codex_egress_timezone_probe");
  },
};

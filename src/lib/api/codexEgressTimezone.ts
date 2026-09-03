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
};

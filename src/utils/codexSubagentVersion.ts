import type { CodexSubagentVersion } from "@/types";

export function normalizeCodexSubagentVersion(
  value: unknown,
): CodexSubagentVersion {
  return value === "v1" ? "v1" : "v2";
}

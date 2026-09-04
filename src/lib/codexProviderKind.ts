import type { Provider } from "@/types";

/** Protocol-set ownership is distinct from user-authored routing intent. */
export function isCodexProtocolSetMember(provider: Provider): boolean {
  const marker = provider.settingsConfig?.codexProtocolSet;
  return Boolean(
    marker &&
      typeof marker === "object" &&
      (marker.role === "facade" || marker.role === "leaf"),
  );
}

export function isUserCodexRoutingPlan(provider: Provider): boolean {
  if (isCodexProtocolSetMember(provider)) return false;
  const routing = provider.settingsConfig?.codexRouting;
  return Boolean(
    routing &&
      typeof routing === "object" &&
      (routing.enabled !== false || (routing.routes?.length ?? 0) > 0),
  );
}

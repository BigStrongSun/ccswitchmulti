import type {
  CodexRoutingAuthPolicy,
  CodexRoutingRouteV2,
  Provider,
} from "@/types";

export type CodexRouteAuthSource = NonNullable<
  CodexRoutingAuthPolicy["source"]
>;

export interface ActiveCodexRouteAuth {
  routeId: string;
  routeLabel: string;
  source: CodexRouteAuthSource;
  accountId: string | null;
}

function readRoutes(provider: Provider): CodexRoutingRouteV2[] | null {
  const routing = provider.settingsConfig.codexRouting;
  if (!routing || typeof routing !== "object") return null;
  const routes = (routing as { routes?: unknown }).routes;
  return Array.isArray(routes) ? (routes as CodexRoutingRouteV2[]) : null;
}

/**
 * Summarize the credential owner selected by every enabled route of the active
 * Codex provider.  A router may intentionally use different credentials for
 * different models, so this returns one item per route instead of inventing a
 * single misleading "current account".
 */
export function summarizeActiveCodexRouteAuth(
  provider: Provider,
): ActiveCodexRouteAuth[] {
  const routes = readRoutes(provider);
  if (!routes) {
    return [
      {
        routeId: provider.id,
        routeLabel: provider.name,
        source: "provider_config",
        accountId: null,
      },
    ];
  }

  return routes
    .filter((route) => route.enabled)
    .map((route) => ({
      routeId: route.id,
      routeLabel: route.label || route.id,
      source: route.authPolicy?.source ?? "provider_config",
      accountId: route.authPolicy?.accountId?.trim() || null,
    }));
}

import type {
  CodexRoutingAuthPolicy,
  CodexRoutingRouteV2,
  Provider,
} from "@/types";
import { readCodexOfficialAuth } from "@/lib/codexOfficialAuth";

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
  providersById?: Record<string, Provider>,
): ActiveCodexRouteAuth[] {
  const officialAuth = (candidate: Provider | undefined) => {
    if (
      candidate?.id !== "codex-official" ||
      candidate.category !== "official"
    ) {
      return null;
    }
    const auth = readCodexOfficialAuth(candidate);
    return auth.mode === "desktop_current_login"
      ? { source: "native_codex_auth" as const, accountId: null }
      : auth.mode === "account_pool"
        ? { source: "account_pool" as const, accountId: null }
        : {
            source: "managed_codex_oauth" as const,
            accountId: auth.accountId?.trim() || null,
          };
  };
  const routes = readRoutes(provider);
  if (!routes) {
    const ownedAuth = officialAuth(provider);
    return [
      {
        routeId: provider.id,
        routeLabel: provider.name,
        source: ownedAuth?.source ?? "provider_config",
        accountId: ownedAuth?.accountId ?? null,
      },
    ];
  }

  return routes
    .filter((route) => route.enabled)
    .map((route) => {
      const ownedAuth = officialAuth(providersById?.[route.targetProviderId]);
      return {
        routeId: route.id,
        routeLabel: route.label || route.id,
        source:
          ownedAuth?.source ?? route.authPolicy?.source ?? "provider_config",
        accountId:
          ownedAuth?.accountId ?? route.authPolicy?.accountId?.trim() ?? null,
      };
    });
}

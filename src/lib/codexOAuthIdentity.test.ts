import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { summarizeActiveCodexRouteAuth } from "./codexOAuthIdentity";

describe("summarizeActiveCodexRouteAuth", () => {
  it("keeps Desktop-native and CCSM-managed route identities distinct", () => {
    const provider = {
      id: "router",
      name: "Codex MultiRouter",
      settingsConfig: {
        codexRouting: {
          routes: [
            {
              id: "official",
              label: "OpenAI Official",
              enabled: true,
              authPolicy: { source: "native_codex_auth" },
            },
            {
              id: "managed",
              label: "Managed fallback",
              enabled: true,
              authPolicy: {
                source: "managed_codex_oauth",
                accountId: "managed-johnson",
              },
            },
            {
              id: "disabled",
              label: "Disabled",
              enabled: false,
              authPolicy: { source: "account_pool" },
            },
          ],
        },
      },
    } as Provider;

    expect(summarizeActiveCodexRouteAuth(provider)).toEqual([
      {
        routeId: "official",
        routeLabel: "OpenAI Official",
        source: "native_codex_auth",
        accountId: null,
      },
      {
        routeId: "managed",
        routeLabel: "Managed fallback",
        source: "managed_codex_oauth",
        accountId: "managed-johnson",
      },
    ]);
  });

  it("reports a direct provider as its own non-OAuth route", () => {
    expect(
      summarizeActiveCodexRouteAuth({
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {},
      }),
    ).toEqual([
      {
        routeId: "deepseek",
        routeLabel: "DeepSeek",
        source: "provider_config",
        accountId: null,
      },
    ]);
  });
});

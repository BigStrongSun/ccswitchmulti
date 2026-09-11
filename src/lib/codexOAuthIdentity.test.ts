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

  it("reports canonical Official authentication from Provider state for direct and routed use", () => {
    const official = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: {},
      meta: {
        codexOfficialAuth: {
          mode: "managed_oauth",
          accountId: "managed-johnson",
        },
      },
    } as Provider;
    const router = {
      id: "router",
      name: "Codex MultiRouter",
      settingsConfig: {
        codexRouting: {
          routes: [
            {
              id: "official",
              label: "OpenAI Official",
              enabled: true,
              targetProviderId: "codex-official",
              authPolicy: { source: "provider_config" },
            },
          ],
        },
      },
    } as Provider;

    expect(summarizeActiveCodexRouteAuth(official)).toEqual([
      {
        routeId: "codex-official",
        routeLabel: "OpenAI Official",
        source: "managed_codex_oauth",
        accountId: "managed-johnson",
      },
    ]);
    expect(
      summarizeActiveCodexRouteAuth(router, {
        "codex-official": official,
        router,
      }),
    ).toEqual([
      {
        routeId: "official",
        routeLabel: "OpenAI Official",
        source: "managed_codex_oauth",
        accountId: "managed-johnson",
      },
    ]);
  });
});

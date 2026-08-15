import { describe, expect, it } from "vitest";
import { extractCodexRoutingConfig } from "@/components/providers/forms/hooks/useCodexConfigState";

describe("extractCodexRoutingConfig", () => {
  it("migrates legacy array codexRouting into the object schema", () => {
    const routing = extractCodexRoutingConfig({
      codexRouting: [
        {
          id: "router-codex-official",
          name: "OpenAI Official",
          providerId: "codex-official",
          models: ["gpt-5.5"],
        },
        {
          id: "router-deepseek",
          label: "DeepSeek",
          provider_id: "codex-deepseek",
          model_prefixes: ["deepseek-"],
        },
      ],
    });

    expect(routing.enabled).toBe(true);
    const routes = routing.routes ?? [];
    expect(routes).toHaveLength(2);
    expect(routes[0].id).toBe("router-codex-official");
    expect(routes[0].match.models).toEqual(["gpt-5.5"]);
    expect(routes[1].match.prefixes).toEqual(["deepseek-"]);
  });

  it("preserves the Router-level official authentication policy", () => {
    const routing = extractCodexRoutingConfig({
      codexRouting: {
        enabled: true,
        officialAuth: {
          mode: "managed_oauth",
          accountId: "acct-1",
        },
        routes: [],
      },
    });

    expect(routing.officialAuth).toEqual({
      mode: "managed_oauth",
      accountId: "acct-1",
    });
  });

  it("preserves Sub-Agent V2 fixed fields when a provider form reads an existing plan", () => {
    const subagentV2 = {
      schemaVersion: 2,
      selectionPolicy: "balanced",
      profiles: {
        "deepseek-v4-pro": {
          model: "deepseek-v4-pro",
          enabled: true,
          inputModalities: ["text"],
          questionnaire: {
            taskStrengths: ["complex_debugging"],
            optimization: "quality",
            writeScope: "complex_changes",
            preference: "preferred",
          },
          reasoning: { policy: "fixed", effort: "high" },
        },
      },
    };
    const routing = extractCodexRoutingConfig({
      codexRouting: {
        enabled: true,
        defaultRouteId: "deepseek",
        subagentVersion: "v2",
        subagentV2,
        routes: [],
      },
    });

    expect(routing.subagentVersion).toBe("v2");
    expect(routing.subagentV2).toEqual(subagentV2);
  });
});

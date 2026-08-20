import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { buildCodexMultiRouterWizardPlan } from "./codexMultiRouterWizard";

const deepseekSource: Provider = {
  id: "deepseek-source",
  name: "DeepSeek",
  category: "custom",
  settingsConfig: {
    baseUrl: "https://example.invalid/v1",
    auth: { OPENAI_API_KEY: "test-only" },
    modelCatalog: {
      models: [{ model: "deepseek-v4-flash" }, { model: "deepseek-v4-pro" }],
    },
  },
};

describe("buildCodexMultiRouterWizardPlan subagent version", () => {
  it("persists an explicit V1 selection without dropping its direct model overrides", () => {
    const { plan } = buildCodexMultiRouterWizardPlan(
      [deepseekSource],
      [deepseekSource],
      null,
      {
        subagentVersion: "v1",
        spawnAgentModels: ["deepseek-v4-pro"],
      } as never,
    );

    expect(plan.settingsConfig.codexRouting.subagentVersion).toBe("v1");
    expect(plan.settingsConfig.codexRouting.spawnAgentModels).toEqual([
      "deepseek-v4-pro",
    ]);
  });

  it("writes V2 when a legacy plan has no explicit subagent version", () => {
    const legacyPlan: Provider = {
      id: "legacy-router",
      name: "Legacy Router",
      category: "custom",
      settingsConfig: {
        codexRouting: { enabled: true, routes: [] },
        modelCatalog: {
          models: [{ model: "deepseek-v4-pro" }],
          spawnAgentModels: ["deepseek-v4-pro"],
        },
      },
    };

    const { plan } = buildCodexMultiRouterWizardPlan(
      [deepseekSource, legacyPlan],
      [deepseekSource],
      legacyPlan,
    );

    expect(plan.settingsConfig.codexRouting.subagentVersion).toBe("v2");
    expect(plan.settingsConfig.codexRouting.spawnAgentModels).toEqual([
      "deepseek-v4-pro",
    ]);
  });
});

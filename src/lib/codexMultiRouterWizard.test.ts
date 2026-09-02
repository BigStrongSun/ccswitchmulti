import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  buildCodexMultiRouterWizardPlan,
  initialWizardCatalogModelOrder,
  initialWizardSelectedSourceIds,
} from "./codexMultiRouterWizard";

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
  it("initializes an existing schema-v2 plan from its route Provider ids", () => {
    const unusedSource: Provider = {
      ...deepseekSource,
      id: "unused-source",
      name: "Unused",
    };
    const existingPlan: Provider = {
      id: "router-v2",
      name: "Router V2",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          routes: [
            {
              id: "deepseek",
              enabled: true,
              targetProviderId: deepseekSource.id,
              modelSelection: { mode: "all" },
              authPolicy: { source: "provider_config" },
            },
            {
              id: "missing",
              enabled: true,
              targetProviderId: "missing-source",
              modelSelection: { mode: "all" },
              authPolicy: { source: "provider_config" },
            },
          ],
        },
      },
    };

    expect(
      initialWizardSelectedSourceIds(existingPlan, [
        deepseekSource,
        unusedSource,
      ]),
    ).toEqual([deepseekSource.id]);
  });

  it("selects every available Provider when creating a new plan", () => {
    const secondSource: Provider = {
      ...deepseekSource,
      id: "second-source",
    };

    expect(
      initialWizardSelectedSourceIds(null, [deepseekSource, secondSource]),
    ).toEqual([deepseekSource.id, secondSource.id]);
  });

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

  it("keeps schema-v2 all-selection in automatic-follow mode instead of freezing Provider facts", () => {
    const source: Provider = {
      ...deepseekSource,
      settingsConfig: {
        ...deepseekSource.settingsConfig,
        modelCatalog: {
          models: [
            { model: "deepseek-v4-flash" },
            { model: "deepseek-v4-pro" },
            { model: "deepseek-v4-vision" },
          ],
        },
      },
    };
    const existingPlan: Provider = {
      id: "router-v2",
      name: "Router V2",
      category: "custom",
      settingsConfig: {
        modelCatalog: {
          models: [{ model: "deepseek-v4-flash" }],
          spawnAgentModels: ["stale-router-candidate"],
        },
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          spawnAgentModels: ["deepseek-v4-pro"],
          routes: [
            {
              id: "deepseek",
              enabled: true,
              targetProviderId: source.id,
              modelSelection: { mode: "all" },
              authPolicy: { source: "provider_config" },
            },
          ],
        },
      },
    };

    const order = initialWizardCatalogModelOrder(existingPlan, [source]);
    expect(order).toBeNull();
    const { plan } = buildCodexMultiRouterWizardPlan(
      [source, existingPlan],
      [source],
      existingPlan,
      { catalogModelOrder: order ?? undefined },
    );

    expect(plan.settingsConfig.codexRouting.routes[0].modelSelection).toEqual({
      mode: "all",
    });
    expect(plan.settingsConfig.codexRouting.spawnAgentModels).toEqual([
      "deepseek-v4-pro",
    ]);
    expect(plan.settingsConfig).not.toHaveProperty("modelCatalog");
    expect(plan.settingsConfig).not.toHaveProperty("model_catalog");
  });
});

describe("buildCodexMultiRouterWizardPlan subagent profile rekey", () => {
  const legacyAlias = "shared-model-old-a";
  const nextAlias = "shared-model-vendor-a";

  function source(id: string, name: string): Provider {
    return {
      id,
      name,
      category: "custom",
      settingsConfig: {
        modelCatalog: { models: [{ model: "shared-model" }] },
      },
    };
  }

  function existingPlanWith(
    profiles: Record<string, Record<string, unknown>>,
    spawnAgentModels: string[] = [legacyAlias],
  ): Provider {
    return {
      id: "router-existing",
      name: "Router Existing",
      category: "custom",
      settingsConfig: {
        codexRouting: {
          schemaVersion: 2,
          enabled: true,
          subagentVersion: "v2",
          routes: [
            {
              id: "vendor-a",
              enabled: true,
              targetProviderId: "vendor-a",
              modelSelection: { mode: "all" },
              aliases: { [legacyAlias]: "shared-model" },
              authPolicy: { source: "provider_config" },
            },
          ],
          subagentV2: {
            schemaVersion: 2,
            selectionPolicy: "balanced",
            profiles,
          },
          spawnAgentModels,
        },
      },
    };
  }

  it("rekeys a stale profile and spawn candidate to the rebuilt visible alias", () => {
    const legacyProfile = {
      model: legacyAlias,
      enabled: true,
      questionnaire: {
        taskStrengths: ["testing"],
        optimization: "quality",
        writeScope: "complex_changes",
        preference: "preferred",
      },
      reasoning: { policy: "fixed", effort: "high" },
    };
    const result = buildCodexMultiRouterWizardPlan(
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      existingPlanWith({ [legacyAlias]: legacyProfile }),
    );
    const routing = result.plan.settingsConfig.codexRouting as Record<
      string,
      any
    >;

    expect(routing.subagentV2.profiles[nextAlias]).toMatchObject({
      model: nextAlias,
      enabled: true,
      reasoning: { policy: "fixed", effort: "high" },
    });
    expect(routing.subagentV2.profiles[legacyAlias]).toBeUndefined();
    expect(routing.spawnAgentModels).toContain(nextAlias);
  });

  it("preserves both profiles when the rebuilt alias is already occupied", () => {
    const result = buildCodexMultiRouterWizardPlan(
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      existingPlanWith({
        [legacyAlias]: { model: legacyAlias, enabled: true },
        [nextAlias]: { model: nextAlias, enabled: false },
      }),
    );
    const profiles = (
      result.plan.settingsConfig.codexRouting as Record<string, any>
    ).subagentV2.profiles;

    expect(profiles[legacyAlias]).toMatchObject({ enabled: true });
    expect(profiles[nextAlias]).toMatchObject({ enabled: false });
  });

  it("preserves an unrelated profile for later catalog reconciliation", () => {
    const removedProfile = { model: "removed-model", enabled: false };
    const result = buildCodexMultiRouterWizardPlan(
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      [source("vendor-a", "Vendor A"), source("vendor-b", "Vendor B")],
      existingPlanWith({ "removed-model": removedProfile }),
    );
    const profiles = (
      result.plan.settingsConfig.codexRouting as Record<string, any>
    ).subagentV2.profiles;

    expect(profiles["removed-model"]).toEqual(removedProfile);
  });
});

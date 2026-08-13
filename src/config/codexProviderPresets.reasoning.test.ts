import { describe, expect, it } from "vitest";

import { codexProviderPresets } from "./codexProviderPresets";

function presetModel(providerName: string, modelName: string) {
  const provider = codexProviderPresets.find(
    (candidate) => candidate.name === providerName,
  );
  expect(provider, `missing preset ${providerName}`).toBeDefined();
  const model = provider?.modelCatalog?.find(
    (candidate) => candidate.model === modelName,
  );
  expect(model, `missing ${providerName}/${modelName}`).toBeDefined();
  return model!;
}

describe("Codex preset reasoning capabilities", () => {
  it("declares DeepSeek V4 official efforts", () => {
    expect(presetModel("DeepSeek", "deepseek-v4-flash").reasoning).toEqual(
      expect.objectContaining({
        supported: true,
        supportedEfforts: ["low", "high", "max"],
        defaultEffort: "high",
        disableAllowed: false,
      }),
    );
    expect(presetModel("DeepSeek", "deepseek-v4-pro").reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: ["low", "high", "max"],
        defaultEffort: "high",
      }),
    );
  });

  it("declares Grok 4.5 efforts without a disable value", () => {
    expect(presetModel("xAI (Grok)", "grok-4.5").reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: ["low", "medium", "high"],
        defaultEffort: "high",
        disableAllowed: false,
      }),
    );
  });

  it("declares GLM-5.2 compatibility aliases and max default", () => {
    expect(presetModel("Zhipu GLM", "glm-5.2").reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: [
          "none",
          "minimal",
          "low",
          "medium",
          "high",
          "xhigh",
          "max",
        ],
        defaultEffort: "max",
        disableAllowed: true,
        upstream: expect.objectContaining({
          parameter: "reasoning_effort",
          effortMap: {
            none: "none",
            minimal: "none",
            low: "high",
            medium: "high",
            high: "high",
            xhigh: "max",
            max: "max",
          },
        }),
      }),
    );
  });

  it("keeps Step model effort sets model-specific", () => {
    expect(presetModel("StepFun", "step-3.7-flash").reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: ["low", "medium", "high"],
      }),
    );
    expect(presetModel("StepFun", "step-3.5-flash-2603").reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: ["low", "high"],
      }),
    );
  });
});

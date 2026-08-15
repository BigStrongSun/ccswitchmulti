import { describe, expect, it } from "vitest";
import type { CodexModelReasoningCapability } from "@/types";

import { normalizeCodexCatalogModelsForSave } from "./ProviderForm";
import {
  applyCodexReasoningCapabilitySource,
  removeCodexEffortMappingsTargeting,
  validateCodexReasoningCapabilityDraft,
} from "./CodexFormFields";

describe("Codex catalog reasoning capability persistence", () => {
  it("separates automatic, maintained and manual capability sources", () => {
    const maintained: CodexModelReasoningCapability = {
      supported: true,
      supportedEfforts: ["low", "high", "max"],
      defaultEffort: "high" as const,
      disableAllowed: true,
      upstream: {
        format: "string" as const,
        parameter: "reasoning_effort" as const,
        effortMap: {
          low: "low" as const,
          high: "high" as const,
          max: "max" as const,
        },
      },
      source: "builtin" as const,
    };

    expect(
      applyCodexReasoningCapabilitySource("automatic", undefined, maintained),
    ).toBeUndefined();
    expect(
      applyCodexReasoningCapabilitySource("builtin", undefined, maintained),
    ).toEqual(maintained);
    expect(
      applyCodexReasoningCapabilitySource("manual", maintained, maintained),
    ).toEqual(expect.objectContaining({ source: "user" }));
  });

  it("preserves a valid user model reasoning override", () => {
    const [model] = normalizeCodexCatalogModelsForSave([
      {
        model: "private-model",
        reasoning: {
          supported: true,
          supportedEfforts: ["low", "high"],
          defaultEffort: "high",
          disableAllowed: false,
          upstream: {
            format: "string",
            parameter: "reasoning_effort",
            effortMap: { low: "low", high: "high" },
          },
          source: "user",
        },
      },
    ]);
    expect(model.reasoning).toEqual(
      expect.objectContaining({
        supportedEfforts: ["low", "high"],
        defaultEffort: "high",
        source: "user",
      }),
    );
  });

  it("rejects a default effort outside the supported list", () => {
    expect(() =>
      normalizeCodexCatalogModelsForSave([
        {
          model: "broken-model",
          reasoning: {
            supported: true,
            supportedEfforts: ["low"],
            defaultEffort: "high",
            disableAllowed: false,
            upstream: { format: "string", parameter: "reasoning_effort" },
          },
        },
      ]),
    ).toThrow(/defaultEffort/);
  });

  it("rejects an effort mapping whose target was removed from supported efforts", () => {
    expect(() =>
      normalizeCodexCatalogModelsForSave([
        {
          model: "broken-map-model",
          reasoning: {
            supported: true,
            supportedEfforts: ["low", "high"],
            defaultEffort: "high",
            disableAllowed: false,
            upstream: {
              format: "string",
              parameter: "reasoning_effort",
              effortMap: { low: "low", medium: "max", high: "high" },
            },
          },
        },
      ]),
    ).toThrow(/target "max".*not in supportedEfforts/);
  });

  it("rejects expert JSON mappings before they can mutate the draft", () => {
    expect(() =>
      validateCodexReasoningCapabilityDraft({
        supported: true,
        supportedEfforts: ["low", "high"],
        defaultEffort: "high",
        disableAllowed: false,
        upstream: {
          format: "string",
          parameter: "reasoning_effort",
          effortMap: { medium: "max" },
        },
        source: "user",
      }),
    ).toThrow(/mapping target max/);
  });

  it("removes every orphan mapping that targets an unchecked effort", () => {
    expect(
      removeCodexEffortMappingsTargeting(
        { low: "low", medium: "high", high: "high", max: "max" },
        "high",
      ),
    ).toEqual({ low: "low", max: "max" });
  });
});

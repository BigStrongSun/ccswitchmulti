import { describe, expect, it } from "vitest";
import type { CodexModelReasoningCapability } from "@/types";

import { normalizeCodexCatalogModelsForSave } from "./ProviderForm";
import {
  applyCodexReasoningCapabilitySource,
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

  it("accepts schema v2 expert JSON without the legacy supported field", () => {
    expect(() =>
      validateCodexReasoningCapabilityDraft({
        schemaVersion: 2,
        supportStatus: "confirmed_supported",
        controlKind: "graded",
        supportedEfforts: ["low", "medium", "xhigh"],
        defaultEffort: "medium",
        disableAllowed: false,
        upstream: {
          format: "string",
          parameter: "reasoning_effort",
          effortMap: {
            low: "low",
            medium: "medium",
            xhigh: "xhigh",
          },
        },
        outputFormat: "reasoning_content",
        source: "user",
      }),
    ).not.toThrow();
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
});

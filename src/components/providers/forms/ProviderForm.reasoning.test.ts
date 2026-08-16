import { describe, expect, it } from "vitest";

import { normalizeCodexCatalogModelsForSave } from "./ProviderForm";

describe("Codex catalog reasoning capability persistence", () => {
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
});

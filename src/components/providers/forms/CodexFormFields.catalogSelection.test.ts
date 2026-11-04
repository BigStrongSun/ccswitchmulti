import { describe, expect, it } from "vitest";
import { selectOnlyDiscoveredCodexCatalogRows } from "./CodexFormFields";

describe("selectOnlyDiscoveredCodexCatalogRows", () => {
  it("enables discovered upstream IDs and preserves other row metadata", () => {
    const rows = [
      {
        rowId: "old-model",
        model: "gpt-5.4",
        enabled: true,
        contextWindow: 128000,
      },
      {
        rowId: "terra-alias",
        model: "Terra",
        upstreamModel: " gpt-6.1-terra ",
        enabled: false,
        contextWindow: 256000,
      },
    ];

    expect(
      selectOnlyDiscoveredCodexCatalogRows(rows, [" GPT-6.1-TERRA "]),
    ).toEqual([
      { ...rows[0], enabled: false },
      { ...rows[1], enabled: true },
    ]);
  });
});

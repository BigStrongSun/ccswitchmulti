import { describe, expect, it } from "vitest";
import { cacheHitRatePercent } from "@/components/usage/UsageTrendChart";

describe("UsageTrendChart cache hit rate", () => {
  it("calculates cache read tokens as a percentage of cacheable input", () => {
    expect(cacheHitRatePercent(250, 500, 0)).toBe(33.33);
  });

  it("returns zero when no cacheable input exists", () => {
    expect(cacheHitRatePercent(0, 0, 0)).toBe(0);
  });
});

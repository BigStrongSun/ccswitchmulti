import { describe, expect, it } from "vitest";
import type { CodexModelReasoningResolution } from "@/types/codexSubagentV2";

import {
  describeFinalBehavior,
  reasoningCardStatus,
  reasoningControlKind,
} from "./CodexModelReasoningCard";

function resolution(
  supportKind: CodexModelReasoningResolution["resolved"]["supportKind"],
): CodexModelReasoningResolution {
  return {
    model: "qwen3.8",
    capability: null,
    source: "unknown",
    fingerprint: "",
    resolved: {
      supportKind,
      confidence: "unverified",
      codexSelectableEfforts: [],
      providerAcceptedEfforts: [],
      disableAllowed: false,
      effortMap: {},
    },
    hasDetectionCandidate: false,
    detection: null,
  };
}

describe("CodexModelReasoningCard", () => {
  it("keeps unknown distinct from confirmed unsupported", () => {
    expect(reasoningCardStatus(resolution("unknown"))).toBe("unknown");
    expect(reasoningCardStatus(resolution("unsupported"))).toBe("unsupported");
  });

  it("projects graded capability and explains the final behavior", () => {
    const value = resolution("effort_levels");
    value.capability = {
      supportStatus: "confirmed_supported",
      controlKind: "graded",
      supportedEfforts: ["low", "high"],
      defaultEffort: "high",
      disableAllowed: true,
      upstream: { format: "reasoning_object", parameter: "reasoning.effort" },
    };
    value.resolved.codexSelectableEfforts = ["low", "high"];
    value.resolved.providerAcceptedEfforts = ["low", "high"];
    value.resolved.providerDefaultEffort = "high";
    value.resolved.disableAllowed = true;

    expect(reasoningControlKind(value)).toBe("graded");
    expect(describeFinalBehavior(value)).toContain("reasoning effort");
    expect(describeFinalBehavior(value)).toContain("low / high");
  });
});

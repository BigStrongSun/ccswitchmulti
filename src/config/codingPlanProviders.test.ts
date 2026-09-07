import { describe, expect, it } from "vitest";

import {
  detectCodingPlanProvider,
  injectCodingPlanUsageScript,
} from "./codingPlanProviders";

type TestProvider = {
  settingsConfig?: Record<string, unknown>;
  meta?: Record<string, unknown>;
};

describe("OpenCode Go coding-plan usage", () => {
  it("detects Go roots without matching pay-as-you-go Zen", () => {
    expect(detectCodingPlanProvider("https://opencode.ai/zen/go")).toBe(
      "opencode_go",
    );
    expect(detectCodingPlanProvider("https://opencode.ai/zen/go/v1")).toBe(
      "opencode_go",
    );
    expect(detectCodingPlanProvider("https://opencode.ai/zen/v1")).toBeNull();
  });

  it.each([
    ["claude", { env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" } }],
    [
      "claude-desktop",
      { env: { ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go" } },
    ],
    [
      "codex",
      {
        config: `[model_providers.custom]\nbase_url = "https://opencode.ai/zen/go/v1"`,
      },
    ],
    ["opencode", { options: { baseURL: "https://opencode.ai/zen/go/v1" } }],
    ["pi", { baseUrl: "https://opencode.ai/zen/go/v1" }],
  ])("injects the Go usage query for %s presets", (appId, settingsConfig) => {
    const injected = injectCodingPlanUsageScript(appId, {
      settingsConfig,
    } as TestProvider);
    expect(injected.meta?.usage_script).toMatchObject({
      enabled: true,
      templateType: "token_plan",
      codingPlanProvider: "opencode_go",
    });
  });

  it("does not broaden other coding-plan auto-detection beyond Claude", () => {
    const provider = {
      settingsConfig: {
        options: { baseURL: "https://api.kimi.com/coding/v1" },
      },
    };
    expect(injectCodingPlanUsageScript("opencode", provider)).toBe(provider);
  });
});

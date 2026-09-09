import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { grokBuildProviderPresets } from "@/config/grokBuildProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("SoleAPI upstream presets", () => {
  const groups = {
    claude: providerPresets,
    desktop: claudeDesktopProviderPresets,
    codex: codexProviderPresets,
    gemini: geminiProviderPresets,
    grok: grokBuildProviderPresets,
    hermes: hermesProviderPresets,
    openclaw: openclawProviderPresets,
    opencode: opencodeProviderPresets,
    pi: piProviderPresets,
  };
  it.each(Object.entries(groups))(
    "%s exposes an optional preset without claiming CCSM sponsorship",
    (_app, presets) => {
      const matches = presets.filter((preset) => preset.name === "SoleAPI");
      expect(matches).toHaveLength(1);
      expect(matches[0].websiteUrl).toBe("https://soleapi.com");
      expect(matches[0].isPartner).not.toBe(true);
      expect(JSON.stringify(matches[0])).not.toContain("/r/ccswitch");
    },
  );
  it("keeps each client's protocol endpoint and the dated Haiku model", () => {
    expect(
      codexProviderPresets.find((p) => p.name === "SoleAPI")?.config,
    ).toContain('base_url = "https://soleapi.com/v1"');
    expect(
      opencodeProviderPresets.find((p) => p.name === "SoleAPI")?.settingsConfig,
    ).toMatchObject({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://soleapi.com/v1", apiKey: "" },
    });
    expect(
      hermesProviderPresets.find((p) => p.name === "SoleAPI")?.settingsConfig,
    ).toMatchObject({
      base_url: "https://soleapi.com",
      api_mode: "anthropic_messages",
      api_key: "",
    });
    expect(
      JSON.stringify(
        claudeDesktopProviderPresets.find((p) => p.name === "SoleAPI")
          ?.modelRoutes,
      ),
    ).toContain("claude-haiku-4-5-20251001");
  });
});

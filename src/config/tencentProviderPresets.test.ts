import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { extractCodexBaseUrl } from "../utils/providerConfigUtils";

const domesticPersonalModels = [
  "tc-code-latest",
  "deepseek-v4-flash-202605",
  "deepseek-v4-pro-202606",
  "minimax-m2.7",
  "minimax-m3",
  "glm-5",
  "glm-5.1",
  "glm-5.2",
  "glm-5.3",
  "glm-5.3-flash",
  "kimi-k2.7-code",
  "kimi-k3",
  "hy3",
  "hy4-preview",
] as const;

const internationalPersonalModels = [
  "auto",
  "glm-5.3-flash",
  "glm-5.2",
  "kimi-k3",
  "kimi-k2.6",
  "deepseek-v4-pro-202606",
  "deepseek-v4-flash-202605",
  "minimax-m3",
] as const;

const domesticEnterpriseModels = [
  "auto",
  "glm-5.3-flash",
  "glm-5.3",
  "glm-5.2",
  "glm-5",
  "glm-5.1",
  "glm-5-turbo",
  "kimi-k3",
  "kimi-k2.7-code",
  "kimi-k2.7-code-highspeed",
  "kimi-k2.6",
  "minimax-m2.7",
  "minimax-m3",
  "deepseek-v4-flash",
  "deepseek-v4-pro",
  "deepseek-v4-flash-0731",
  "deepseek-v4-pro-0813",
  "deepseek-v4-flash-202605",
  "deepseek-v4-pro-202606",
  "deepseek/deepseek-v4-flash-vision-exp",
] as const;

const internationalEnterpriseModels = [
  "auto",
  "glm-5.3-flash",
  "glm-5.3",
  "glm-5.2",
  "minimax-m3",
  "kimi-k3",
  "kimi-k2.7-code",
  "kimi-k2.7-code-highspeed",
  "deepseek-v4-flash",
  "deepseek-v4-pro",
  "deepseek-v4-flash-0731",
  "deepseek-v4-pro-0813",
  "deepseek-v4-flash-202605",
  "deepseek-v4-pro-202606",
  "deepseek/deepseek-v4-flash-vision-exp",
] as const;

const tokenPlanProducts = [
  {
    name: "Tencent Token Plan",
    presetKey: "tencent-token-plan",
    endpoint: "https://api.lkeap.cloud.tencent.com/plan/v3",
    anthropic: "https://api.lkeap.cloud.tencent.com/plan/anthropic",
    primary: "tc-code-latest",
    models: domesticPersonalModels,
  },
  {
    name: "Tencent Token Plan (Intl)",
    presetKey: "tencent-token-plan-intl",
    endpoint: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
    anthropic: "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
    primary: "auto",
    models: internationalPersonalModels,
  },
  {
    name: "Tencent Token Plan Enterprise Pro",
    presetKey: "tencent-token-plan-enterprise-pro",
    endpoint: "https://tokenhub.tencentmaas.com/plan/v3",
    anthropic: "https://tokenhub.tencentmaas.com/plan/anthropic",
    primary: "auto",
    models: domesticEnterpriseModels,
  },
  {
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    presetKey: "tencent-token-plan-enterprise-pro-intl",
    endpoint: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
    anthropic: "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
    primary: "auto",
    models: internationalEnterpriseModels,
  },
  {
    name: "Tencent Token Plan Enterprise Lite",
    presetKey: "tencent-token-plan-enterprise-lite",
    endpoint: "https://tokenhub.tencentmaas.com/plan/v3",
    anthropic: "https://tokenhub.tencentmaas.com/plan/anthropic",
    primary: "auto",
    models: ["auto"],
  },
  {
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    presetKey: "tencent-token-plan-enterprise-lite-intl",
    endpoint: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
    anthropic: "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
    primary: "auto",
    models: ["auto"],
  },
] as const;

const retiredModels = [
  "hy3-preview",
  "deepseek-v3.2",
  "minimax-m2.5",
  "kimi-k2.5",
] as const;

function byName<T extends { name: string }>(items: readonly T[], name: string) {
  return items.find((item) => item.name === name);
}

describe("Tencent Token Plan presets", () => {
  it.each(tokenPlanProducts)(
    "registers $name exactly once in every supported app",
    ({ name }) => {
      for (const presets of [
        providerPresets,
        claudeDesktopProviderPresets,
        codexProviderPresets,
        opencodeProviderPresets,
        openclawProviderPresets,
        hermesProviderPresets,
        piProviderPresets,
      ]) {
        expect(presets.filter((item) => item.name === name)).toHaveLength(1);
      }
    },
  );

  it.each(tokenPlanProducts)(
    "keeps $name endpoints and model catalog aligned across apps",
    ({ name, presetKey, endpoint, anthropic, primary, models }) => {
      const claude = byName(providerPresets, name)!;
      const desktop = byName(claudeDesktopProviderPresets, name)!;
      const codex = byName(codexProviderPresets, name)!;
      const opencode = byName(opencodeProviderPresets, name)!;
      const openclaw = byName(openclawProviderPresets, name)!;
      const hermes = byName(hermesProviderPresets, name)!;
      const pi = byName(piProviderPresets, name)!;

      expect(
        (claude.settingsConfig as { env: Record<string, string> }).env,
      ).toMatchObject({
        ANTHROPIC_BASE_URL: anthropic,
        ANTHROPIC_MODEL: primary,
      });
      expect(desktop).toMatchObject({
        baseUrl: anthropic,
        apiFormat: "anthropic",
      });
      expect(extractCodexBaseUrl(codex.config)).toBe(endpoint);
      expect(codex.presetKey).toBe(presetKey);
      expect(codex.modelCatalog?.map((model) => model.model)).toEqual(models);
      expect(opencode.settingsConfig).toMatchObject({
        npm: "@ai-sdk/openai-compatible",
        options: { baseURL: endpoint },
      });
      expect(Object.keys(opencode.settingsConfig.models)).toEqual(models);
      expect(openclaw.settingsConfig).toMatchObject({
        baseUrl: endpoint,
        api: "openai-completions",
      });
      expect(
        (openclaw.settingsConfig.models ?? []).map((model) => model.id),
      ).toEqual(models);
      expect(
        (hermes.settingsConfig.models ?? []).map((model) => model.id),
      ).toEqual(models);
      expect(hermes.settingsConfig).toMatchObject({
        base_url: endpoint,
        api_mode: "chat_completions",
      });
      expect(pi.settingsConfig).toMatchObject({
        baseUrl: endpoint,
        api: "openai-completions",
      });
      expect(pi.settingsConfig.models.map((model) => model.id)).toEqual(models);
    },
  );

  it("projects Tencent reasoning levels into the CCSwitchMulti capability schema", () => {
    const enterprise = byName(
      codexProviderPresets,
      "Tencent Token Plan Enterprise Pro",
    )!;
    const model = (id: string) =>
      enterprise.modelCatalog?.find((entry) => entry.model === id)?.reasoning;

    expect(model("deepseek-v4-pro-202606")).toMatchObject({
      schemaVersion: 2,
      supportStatus: "confirmed_supported",
      controlKind: "graded",
      supportedEfforts: ["high"],
      defaultEffort: "high",
      disableAllowed: true,
    });
    expect(model("kimi-k2.7-code")).toMatchObject({
      supportedEfforts: ["high"],
      disableAllowed: false,
    });
    expect(model("glm-5.3")).toMatchObject({
      supportedEfforts: ["low", "high", "max"],
      defaultEffort: "high",
      disableAllowed: false,
    });
  });

  it("preserves OpenClaw-specific Tencent model metadata", () => {
    const personal = byName(openclawProviderPresets, "Tencent Token Plan")!;
    const enterprise = byName(
      openclawProviderPresets,
      "Tencent Token Plan Enterprise Pro",
    )!;
    const model = (preset: typeof personal, id: string) =>
      preset.settingsConfig.models?.find((entry) => entry.id === id);

    expect(model(personal, "deepseek-v4-pro-202606")).toMatchObject({
      reasoning: false,
      input: ["text"],
      contextWindow: 1_000_000,
      maxTokens: 384_000,
    });
    expect(model(personal, "minimax-m2.7")).toMatchObject({
      reasoning: false,
      input: ["text"],
      contextWindow: 200_000,
      maxTokens: 131_072,
    });
    expect(model(personal, "hy3")).toMatchObject({ reasoning: true });
    expect(model(enterprise, "deepseek-v4-pro-202606")).toMatchObject({
      reasoning: false,
      input: ["text"],
      contextWindow: 1_048_576,
      maxTokens: 393_216,
    });
  });

  it("materializes Tencent DeepSeek switching and Kimi image capability in Pi", () => {
    const enterprise = byName(
      piProviderPresets,
      "Tencent Token Plan Enterprise Pro",
    )!;
    const deepseek = enterprise.settingsConfig.models.find(
      (model) => model.id === "deepseek-v4-pro-202606",
    )!;
    const kimi = enterprise.settingsConfig.models.find(
      (model) => model.id === "kimi-k2.7-code-highspeed",
    )!;

    // Pi uses `off`, not Codex's `none`. Leaving `off` omitted is deliberate:
    // the DeepSeek wire adapter then emits `thinking: { type: "disabled" }`.
    expect(deepseek.thinkingLevelMap).not.toHaveProperty("off");
    expect(deepseek.thinkingLevelMap).toMatchObject({ high: "high" });
    expect(deepseek.compat).toMatchObject({
      thinkingFormat: "deepseek",
      maxTokensField: "max_tokens",
    });
    expect(kimi.thinkingLevelMap).toMatchObject({ off: null });
    expect(kimi.input).toContain("image");
  });

  it("does not seed already retired aliases in any Tencent catalog", () => {
    const names = tokenPlanProducts.map((product) => product.name);
    const ids = names.flatMap((name) => {
      const claude = byName(providerPresets, name)!;
      const desktop = byName(claudeDesktopProviderPresets, name)!;
      const codex = byName(codexProviderPresets, name)!;
      const opencode = byName(opencodeProviderPresets, name)!;
      const openclaw = byName(openclawProviderPresets, name)!;
      const hermes = byName(hermesProviderPresets, name)!;
      const pi = byName(piProviderPresets, name)!;
      const env = (claude.settingsConfig as { env: Record<string, string> })
        .env;
      return [
        env.ANTHROPIC_MODEL,
        ...(desktop.modelRoutes?.map((route) => route.upstreamModel) ?? []),
        ...(codex.modelCatalog?.map((model) => model.model) ?? []),
        ...Object.keys(opencode.settingsConfig.models),
        ...(openclaw.settingsConfig.models ?? []).map((model) => model.id),
        ...(hermes.settingsConfig.models ?? []).map((model) => model.id),
        ...pi.settingsConfig.models.map((model) => model.id),
      ];
    });
    for (const preset of piProviderPresets.filter((item) =>
      item.name.startsWith("Tencent TokenHub"),
    )) {
      ids.push(...preset.settingsConfig.models.map((model) => model.id));
    }
    for (const retired of retiredModels) expect(ids).not.toContain(retired);
  });
});

describe("Tencent TokenHub Pi presets", () => {
  const currentModels = [
    "hy4-preview",
    "hy3",
    "deepseek-v4-flash-202605",
    "deepseek-v4-pro-202606",
    "deepseek/deepseek-v4-flash-vision-exp",
    "deepseek-v4-flash-0731",
    "deepseek-v4-pro-0813",
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "glm-5.3-flash",
    "glm-5.3",
    "glm-5.2",
    "glm-5.1",
    "glm-5v-turbo",
    "glm-5-turbo",
    "glm-5",
    "kimi-k2.7-code-highspeed",
    "kimi-k3",
    "kimi-k2.7-code",
    "kimi-k2.6",
    "minimax-m3",
    "minimax-m2.7",
    "mimo-v2.5-pro",
  ] as const;

  it.each([
    ["Tencent TokenHub", "https://tokenhub.tencentmaas.com/v1"],
    [
      "Tencent TokenHub (Intl)",
      "https://tokenhub-intl.tencentcloudmaas.com/v1",
    ],
  ] as const)("uses the live /v1 catalog for %s", (name, endpoint) => {
    const preset = byName(piProviderPresets, name)!;
    expect(preset.settingsConfig.baseUrl).toBe(endpoint);
    expect(preset.settingsConfig.api).toBe("openai-completions");
    expect(preset.settingsConfig.models.map((model) => model.id)).toEqual(
      currentModels,
    );
  });
});

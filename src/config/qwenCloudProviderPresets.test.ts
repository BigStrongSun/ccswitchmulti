import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { extractCodexBaseUrl } from "../utils/providerConfigUtils";

const products = [
  {
    name: "QwenCloud",
    presetKey: "qwencloud",
    openai: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    anthropic: "https://dashscope-intl.aliyuncs.com/apps/anthropic",
    codexApi: "openai_responses",
    openCodeApi: "@ai-sdk/openai-compatible",
    piApi: "openai-completions",
    primary: "qwen3.8-max",
    models: [
      "qwen3.8-max",
      "qwen3.8-flash",
      "qwen3.7-max",
      "qwen3.7-plus",
      "qwen3.6-plus",
      "qwen3.6-flash",
    ],
  },
  {
    name: "QwenCloud For Coding",
    presetKey: "qwencloud-coding",
    openai: "https://coding-intl.dashscope.aliyuncs.com/v1",
    anthropic: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
    codexApi: "openai_chat",
    openCodeApi: "@ai-sdk/anthropic",
    piApi: "anthropic-messages",
    primary: "qwen3.7-plus",
    models: [
      "qwen3.7-plus",
      "qwen3.6-plus",
      "qwen3.5-plus",
      "qwen3-max-2026-01-23",
      "qwen3-coder-next",
      "qwen3-coder-plus",
    ],
  },
  {
    name: "QwenCloud Token Plan",
    presetKey: "qwencloud-token-plan",
    openai:
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
    anthropic:
      "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic",
    codexApi: "openai_responses",
    openCodeApi: "@ai-sdk/anthropic",
    piApi: "anthropic-messages",
    primary: "qwen3.8-max",
    models: [
      "qwen3.8-max",
      "qwen3.8-flash",
      "qwen3.7-max",
      "qwen3.7-plus",
      "qwen3.6-flash",
    ],
  },
] as const;

function byName<T extends { name: string }>(items: readonly T[], name: string) {
  return items.find((item) => item.name === name);
}

describe("QwenCloud presets", () => {
  it.each(products)(
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

  it.each(products)(
    "keeps $name plan-specific endpoints and protocols paired",
    ({
      name,
      presetKey,
      openai,
      anthropic,
      codexApi,
      openCodeApi,
      piApi,
      primary,
    }) => {
      const claude = byName(providerPresets, name)!;
      const desktop = byName(claudeDesktopProviderPresets, name)!;
      const codex = byName(codexProviderPresets, name)!;
      const opencode = byName(opencodeProviderPresets, name)!;
      const openclaw = byName(openclawProviderPresets, name)!;
      const hermes = byName(hermesProviderPresets, name)!;
      const pi = byName(piProviderPresets, name)!;
      const env = (claude.settingsConfig as { env: Record<string, string> })
        .env;

      expect(env).toMatchObject({
        ANTHROPIC_BASE_URL: anthropic,
        ANTHROPIC_MODEL: primary,
      });
      expect(desktop).toMatchObject({
        baseUrl: anthropic,
        apiFormat: "anthropic",
      });
      expect(extractCodexBaseUrl(codex.config)).toBe(openai);
      expect(codex).toMatchObject({ presetKey, apiFormat: codexApi });
      expect(opencode.settingsConfig).toMatchObject({
        npm: openCodeApi,
        options: {
          baseURL:
            openCodeApi === "@ai-sdk/anthropic" ? `${anthropic}/v1` : openai,
        },
      });
      expect(openclaw.settingsConfig).toMatchObject({
        baseUrl: `${anthropic}/v1`,
        api: "anthropic-messages",
      });
      expect(hermes.settingsConfig).toMatchObject({
        base_url: anthropic,
        api_mode: "anthropic_messages",
      });
      expect(pi.settingsConfig).toMatchObject({
        baseUrl: piApi === "anthropic-messages" ? anthropic : openai,
        api: piApi,
      });
    },
  );

  it.each(products)(
    "keeps the current $name coding catalog aligned across structured apps",
    ({ name, models }) => {
      const codex = byName(codexProviderPresets, name)!;
      const opencode = byName(opencodeProviderPresets, name)!;
      const openclaw = byName(openclawProviderPresets, name)!;
      const hermes = byName(hermesProviderPresets, name)!;
      const pi = byName(piProviderPresets, name)!;

      expect(codex.modelCatalog?.map((model) => model.model)).toEqual(models);
      expect(Object.keys(opencode.settingsConfig.models)).toEqual(models);
      expect(openclaw.settingsConfig.models?.map((model) => model.id)).toEqual(
        models,
      );
      expect(hermes.settingsConfig.models?.map((model) => model.id)).toEqual(
        models,
      );
      expect(pi.settingsConfig.models.map((model) => model.id)).toEqual(models);
    },
  );

  it("projects evidence-bound Codex reasoning without false unsupported claims", () => {
    const paygo = byName(codexProviderPresets, "QwenCloud")!;
    const coding = byName(codexProviderPresets, "QwenCloud For Coding")!;
    const token = byName(codexProviderPresets, "QwenCloud Token Plan")!;
    const model = (preset: typeof paygo, id: string) =>
      preset.modelCatalog?.find((entry) => entry.model === id)!;
    const openclaw = byName(openclawProviderPresets, "QwenCloud Token Plan")!;
    const openclawQwen = openclaw.settingsConfig.models?.find(
      (model) => model.id === "qwen3.8-max",
    );

    for (const preset of [paygo, token]) {
      for (const id of ["qwen3.8-max", "qwen3.8-flash"]) {
        const qwen = model(preset, id);
        expect(qwen).not.toHaveProperty("reasoningLevels");
        expect(qwen).toMatchObject({
          contextWindow: 983_616,
          inputModalities: ["text", "image"],
          supportsParallelToolCalls: false,
          reasoning: {
            schemaVersion: 2,
            supportStatus: "confirmed_supported",
            controlKind: "graded",
            supportedEfforts: ["low", "medium", "xhigh"],
            defaultEffort: "xhigh",
            disableAllowed: true,
            upstream: {
              format: "reasoning_object",
              parameter: "reasoning.effort",
            },
            outputFormat: "reasoning",
          },
        });
      }
    }
    for (const preset of [paygo, token]) {
      for (const id of preset
        .modelCatalog!.map((entry) => entry.model)
        .filter((id) => !id.startsWith("qwen3.8-"))) {
        expect(model(preset, id).reasoning).toMatchObject({
          supportStatus: "confirmed_supported",
          controlKind: "boolean",
          upstream: { format: "boolean", parameter: "enable_thinking" },
          outputFormat: "reasoning",
        });
      }
    }
    for (const id of [
      "qwen3.7-plus",
      "qwen3.6-plus",
      "qwen3.5-plus",
      "qwen3-max-2026-01-23",
    ]) {
      expect(model(coding, id).reasoning).toMatchObject({
        supportStatus: "confirmed_supported",
        controlKind: "boolean",
        upstream: { format: "boolean", parameter: "enable_thinking" },
        outputFormat: "reasoning_content",
      });
    }
    for (const id of ["qwen3-coder-next", "qwen3-coder-plus"]) {
      expect(model(coding, id).reasoning).toMatchObject({
        supportStatus: "unknown",
        controlKind: "unknown",
        upstream: { format: "none", parameter: "none" },
      });
    }
    expect(coding.codexChatReasoning).toMatchObject({
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "enable_thinking",
      effortParam: "none",
      outputFormat: "reasoning_content",
    });
    expect(openclawQwen).toMatchObject({
      reasoning: true,
      input: ["text", "image"],
      contextWindow: 983_616,
      maxTokens: 131_072,
    });
  });
});

import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { extractCodexBaseUrl } from "../utils/providerConfigUtils";

const responsesModels = [
  "grok-4.6",
  "gpt-5.6-luna",
  "muse-spark-1.3-contributor",
  "muse-spark-1.2-contributor",
] as const;

const messagesModels = [
  "minimax-m3",
  "minimax-m2.7",
  "qwen3.8-max",
  "qwen3.8-flash",
  "qwen3.7-max",
  "qwen3.7-plus",
  "qwen3.6-plus",
] as const;

const chatModels = [
  "glm-5.3-flash",
  "glm-5.3",
  "glm-5.2",
  "glm-5.1",
  "kimi-k3",
  "kimi-k2.7-code",
  "kimi-k2.6",
  "longcat-2.0",
  "deepseek-v4-pro",
  "deepseek-v4-flash",
  "deepseek-v4-flash-vision-exp",
  "mimo-v2.5",
  "mimo-v2.5-pro",
  "hy4-preview",
  "hy3",
  "omen-alpha",
] as const;

const allModels = [...responsesModels, ...messagesModels, ...chatModels];

function byName<T extends { name: string }>(items: readonly T[], name: string) {
  const item = items.find((candidate) => candidate.name === name);
  if (!item) throw new Error(`Missing preset: ${name}`);
  return item;
}

describe("OpenCode Go protocol-aware presets", () => {
  it("keeps the maintained catalog complete, unique, and free of deprecated models", () => {
    expect(new Set(allModels).size).toBe(27);
    expect(allModels).not.toEqual(
      expect.arrayContaining([
        "minimax-m2.5",
        "kimi-k2.5",
        "glm-5",
        "qwen3.5-plus",
        "mimo-v2-pro",
        "mimo-v2-omni",
        "hy3-preview",
        "grok-4.5",
        "ox-alpha-free",
      ]),
    );

    const opencode = byName(opencodeProviderPresets, "OpenCode Go");
    expect(Object.keys(opencode.settingsConfig.models)).toEqual(allModels);
  });

  it("uses Anthropic Messages and x-api-key semantics for Claude clients", () => {
    const claude = byName(providerPresets, "OpenCode Go");
    const desktop = byName(claudeDesktopProviderPresets, "OpenCode Go");
    const env = (claude.settingsConfig as { env: Record<string, string> }).env;

    expect(claude).toMatchObject({
      apiKeyField: "ANTHROPIC_API_KEY",
      apiFormat: "anthropic",
      endpointCandidates: ["https://opencode.ai/zen/go"],
    });
    expect(env).toMatchObject({
      ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "minimax-m3",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "minimax-m2.7",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "minimax-m3",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "minimax-m3",
    });
    expect(env).not.toHaveProperty("ANTHROPIC_AUTH_TOKEN");

    expect(desktop).toMatchObject({
      apiKeyField: "ANTHROPIC_API_KEY",
      baseUrl: "https://opencode.ai/zen/go",
      apiFormat: "anthropic",
    });
    expect(desktop.modelRoutes?.map((route) => route.upstreamModel)).toEqual([
      "minimax-m3",
      "minimax-m2.7",
    ]);
  });

  it("routes every OpenCode model through its documented SDK protocol", () => {
    const models = byName(opencodeProviderPresets, "OpenCode Go").settingsConfig
      .models;

    expect(
      Object.fromEntries(
        Object.entries(models).map(([id, model]) => [
          id,
          (model.provider as { npm?: string } | undefined)?.npm ??
            "@ai-sdk/openai-compatible",
        ]),
      ),
    ).toEqual(
      Object.fromEntries([
        ...responsesModels.map((id) => [id, "@ai-sdk/openai"]),
        ...messagesModels.map((id) => [id, "@ai-sdk/anthropic"]),
        ...chatModels.map((id) => [id, "@ai-sdk/openai-compatible"]),
      ]),
    );
  });

  it("splits Codex and Pi presets so global protocol settings never mix models", () => {
    const variants = [
      {
        name: "OpenCode Go (Responses)",
        codexFormat: "openai_responses",
        piFormat: "openai-responses",
        baseUrl: "https://opencode.ai/zen/go/v1",
        models: responsesModels,
      },
      {
        name: "OpenCode Go (Messages)",
        codexFormat: "anthropic",
        piFormat: "anthropic-messages",
        baseUrl: "https://opencode.ai/zen/go",
        models: messagesModels,
      },
      {
        name: "OpenCode Go",
        codexFormat: "openai_chat",
        piFormat: "openai-completions",
        baseUrl: "https://opencode.ai/zen/go/v1",
        models: chatModels,
      },
    ] as const;

    for (const variant of variants) {
      const codex = byName(codexProviderPresets, variant.name);
      const pi = byName(piProviderPresets, variant.name);
      expect(codex.apiFormat).toBe(variant.codexFormat);
      expect(extractCodexBaseUrl(codex.config)).toBe(variant.baseUrl);
      expect(codex.modelCatalog?.map((model) => model.model)).toEqual(
        variant.models,
      );
      expect(pi.settingsConfig).toMatchObject({
        baseUrl: variant.baseUrl,
        api: variant.piFormat,
      });
      expect(pi.settingsConfig.models.map((model) => model.id)).toEqual(
        variant.models,
      );
    }
  });

  it("preserves provider-specific limits, modalities, and reasoning controls", () => {
    const openCodeModels = byName(opencodeProviderPresets, "OpenCode Go")
      .settingsConfig.models;
    expect(openCodeModels["gpt-5.6-luna"]).toMatchObject({
      limit: { context: 1_050_000, output: 128_000 },
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      reasoning: true,
    });
    expect(openCodeModels["qwen3.8-max"]).toMatchObject({
      limit: { context: 1_000_000, output: 131_072 },
      modalities: {
        input: ["text", "image", "video"],
        output: ["text"],
      },
      reasoning: true,
    });
    expect(openCodeModels["deepseek-v4-flash-vision-exp"]).toMatchObject({
      limit: { context: 1_000_000, output: 384_000 },
      modalities: { input: ["text", "image"], output: ["text"] },
      reasoning: true,
    });

    const codexResponses = byName(
      codexProviderPresets,
      "OpenCode Go (Responses)",
    );
    const luna = codexResponses.modelCatalog?.find(
      (model) => model.model === "gpt-5.6-luna",
    );
    expect(luna).toMatchObject({
      contextWindow: 1_050_000,
      inputModalities: ["text", "image"],
      reasoning: {
        supportStatus: "confirmed_supported",
        controlKind: "graded",
        supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
        defaultEffort: "high",
        disableAllowed: true,
      },
    });

    const piMessages = byName(piProviderPresets, "OpenCode Go (Messages)");
    expect(
      piMessages.settingsConfig.models.find(
        (model) => model.id === "qwen3.8-max",
      ),
    ).toMatchObject({
      contextWindow: 1_000_000,
      maxTokens: 131_072,
      input: ["text", "image"],
    });
  });
});

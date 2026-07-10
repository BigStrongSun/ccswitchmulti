import { describe, expect, it } from "vitest";
import {
  normalizeCodexCatalogModelsForSave,
  normalizeCodexChatReasoningForSave,
} from "@/components/providers/forms/ProviderForm";

describe("ProviderForm Codex catalog helpers", () => {
  it("normalizes catalog rows and removes empty or duplicate models", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: " deepseek-v4-flash ",
          upstreamModel: " deepseek-chat ",
          displayName: " DeepSeek ",
        },
        { model: "deepseek-v4-flash", displayName: "Duplicate" },
        { model: "", displayName: "Empty" },
        {
          model: "kimi-k2",
          upstreamModel: "kimi-k2",
          contextWindow: "128000 tokens",
        },
      ]),
    ).toEqual([
      {
        model: "deepseek-v4-flash",
        upstreamModel: "deepseek-chat",
        displayName: "DeepSeek",
      },
      { model: "kimi-k2", contextWindow: 128000 },
    ]);
  });

  it("keeps duplicate upstream models when visible model aliases differ", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: "gpt-5.5-thirdparty", upstreamModel: "gpt-5.5" },
        { model: "gpt-5.5-backup", upstream_model: "gpt-5.5" },
      ]),
    ).toEqual([
      { model: "gpt-5.5-thirdparty", upstreamModel: "gpt-5.5" },
      { model: "gpt-5.5-backup", upstreamModel: "gpt-5.5" },
    ]);
  });

  it("preserves explicit Codex Chat min output tokens", () => {
    expect(
      normalizeCodexChatReasoningForSave({
        supportsThinking: true,
        supportsEffort: false,
        thinkingParam: "thinking",
        effortParam: "none",
        minOutputTokens: 4096,
        defaultOutputTokens: 65536,
        outputFormat: "reasoning_content",
      }),
    ).toMatchObject({
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "thinking",
      effortParam: "none",
      minOutputTokens: 4096,
      defaultOutputTokens: 65536,
      outputFormat: "reasoning_content",
    });
  });

  it("applies Qwen vLLM Codex Chat defaults without implicit output cap", () => {
    expect(
      normalizeCodexChatReasoningForSave(
        {
          supportsThinking: true,
          supportsEffort: false,
          thinkingParam: "thinking",
          effortParam: "none",
          outputFormat: "reasoning_content",
        },
        {
          providerName: "Qwen Local",
          baseUrl: "https://www.matrixminecraft.cn:24443/vllm/v1",
          models: [{ model: "qwen3.6" }],
        },
      ),
    ).toMatchObject({
      supportsThinking: true,
      supportsEffort: false,
      thinkingParam: "enable_thinking",
      effortParam: "none",
      minOutputTokens: 2048,
      outputFormat: "reasoning_content",
    });
    expect(
      normalizeCodexChatReasoningForSave(
        {
          supportsThinking: true,
          supportsEffort: false,
          thinkingParam: "thinking",
          effortParam: "none",
          outputFormat: "reasoning_content",
        },
        {
          providerName: "Qwen Local",
          baseUrl: "https://www.matrixminecraft.cn:24443/vllm/v1",
          models: [{ model: "qwen3.6" }],
        },
      ),
    ).not.toHaveProperty("defaultOutputTokens");
  });

  it("preserves native-profile overrides (parallel tool calls + input modalities + base instructions)", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "MiniMax-M3",
          displayName: "MiniMax-M3",
          contextWindow: 1000000,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          baseInstructions:
            "  You are Codex, a coding agent based on MiniMax-M3.  ",
        },
        // false must be preserved (not dropped as falsy); empty modalities dropped;
        // empty/whitespace baseInstructions dropped
        {
          model: "mimo-v2.5-pro",
          supportsParallelToolCalls: false,
          inputModalities: [],
          baseInstructions: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, a coding agent based on MiniMax-M3.",
      },
      { model: "mimo-v2.5-pro", supportsParallelToolCalls: false },
    ]);
  });
});

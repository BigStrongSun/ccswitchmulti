import { describe, expect, it } from "vitest";

import {
  buildCodexProtocolMeta,
  buildCodexProtocolProbeProviderDraft,
  readCodexProtocolSettings,
} from "./codexProtocolSettings";

describe("Codex protocol settings", () => {
  it("builds a probe draft from the same request-affecting Provider fields as save", () => {
    const provider = buildCodexProtocolProbeProviderDraft({
      providerId: "codex-draft",
      providerName: "Qwen vLLM",
      baseUrl: "https://vllm.example/v1",
      apiKey: "probe-secret",
      apiFormat: "openai_chat",
      isFullUrl: false,
      defaultModel: "qwen3.8",
      models: [{ model: "qwen3.8", upstreamModel: "Qwen/Qwen3.8" }],
      customUserAgent: "ccsm-probe/1",
      localProxyHeadersOverride: '{"X-Relay":"edge"}',
      localProxyBodyOverride: '{"temperature":0}',
      takeoverEnabled: true,
      codexChatReasoning: {
        supportsThinking: true,
        supportsEffort: true,
      },
      promptCacheRouting: "enabled",
      protocolMode: "auto",
      reasoningProjection: "none",
      toolSchemaDialect: "openai",
      historyReplay: "chat_reasoning_content",
    });

    expect(provider.meta).toEqual(
      expect.objectContaining({
        apiFormat: "openai_chat",
        customUserAgent: "ccsm-probe/1",
        localProxyRequestOverrides: {
          headers: { "x-relay": "edge" },
          body: { temperature: 0 },
        },
        promptCacheRouting: "enabled",
        codexChatReasoning: expect.objectContaining({
          thinkingParam: "enable_thinking",
          effortParam: "reasoning_effort",
          minOutputTokens: 2048,
        }),
      }),
    );
    expect(provider.meta).not.toHaveProperty("codexProtocolMode");
    expect(provider.meta).not.toHaveProperty("codexReasoningProjection");
    expect(provider.meta).not.toHaveProperty("codexToolSchemaDialect");
    expect(provider.meta).not.toHaveProperty("codexHistoryReplay");
  });

  it("rejects invalid request overrides before any probe request is sent", () => {
    expect(() =>
      buildCodexProtocolProbeProviderDraft({
        providerName: "Broken",
        baseUrl: "https://example.test/v1",
        apiKey: "secret",
        apiFormat: "openai_responses",
        isFullUrl: false,
        defaultModel: "model",
        models: [{ model: "model" }],
        customUserAgent: "",
        localProxyHeadersOverride: "{not-json",
        localProxyBodyOverride: "",
        takeoverEnabled: false,
        codexChatReasoning: {},
        promptCacheRouting: "auto",
        protocolMode: "auto",
        reasoningProjection: "none",
        toolSchemaDialect: "openai",
        historyReplay: "native_only",
      }),
    ).toThrow(/请求覆盖格式错误/);
  });

  it("clears manual fields in auto mode and writes only legal manual combinations", () => {
    const legacy = {
      codexProtocolMode: "manual" as const,
      codexReasoningProjection: "reasoning_summary" as const,
      codexToolSchemaDialect: "moonshot_mfjs" as const,
      codexHistoryReplay: "omit" as const,
      customUserAgent: "keep-me",
    };
    expect(
      buildCodexProtocolMeta(legacy, {
        protocolMode: "auto",
        apiFormat: "openai_responses",
        reasoningProjection: "none",
        toolSchemaDialect: "openai",
        historyReplay: "native_only",
      }),
    ).toEqual({ customUserAgent: "keep-me" });

    expect(
      buildCodexProtocolMeta(undefined, {
        protocolMode: "manual",
        apiFormat: "openai_chat",
        reasoningProjection: "raw_reasoning_text",
        toolSchemaDialect: "moonshot_mfjs",
        historyReplay: "omit",
      }),
    ).toEqual({
      codexProtocolMode: "manual",
      codexReasoningProjection: "raw_reasoning_text",
      codexToolSchemaDialect: "moonshot_mfjs",
      codexHistoryReplay: "chat_reasoning_content",
    });

    expect(
      buildCodexProtocolMeta(undefined, {
        protocolMode: "manual",
        apiFormat: "openai_responses",
        reasoningProjection: "reasoning_summary",
        toolSchemaDialect: "openai",
        historyReplay: "responses_reasoning_text_content",
      }),
    ).toEqual({
      codexProtocolMode: "manual",
      codexToolSchemaDialect: "openai",
      codexHistoryReplay: "responses_reasoning_text_content",
    });

    expect(
      buildCodexProtocolMeta(legacy, {
        protocolMode: "manual",
        apiFormat: "anthropic",
        reasoningProjection: "raw_reasoning_text",
        toolSchemaDialect: "moonshot_mfjs",
        historyReplay: "omit",
      }),
    ).toEqual({ customUserAgent: "keep-me" });
  });

  it("persists normalized per-model overrides independently from provider mode", () => {
    expect(
      buildCodexProtocolMeta(undefined, {
        protocolMode: "auto",
        protocolOverrides: {
          " Qwen3.8 ": "openai_chat",
          "GPT-5.5": "openai_responses",
        },
        apiFormat: "openai_responses",
        reasoningProjection: "none",
        toolSchemaDialect: "openai",
        historyReplay: "native_only",
      }),
    ).toEqual({
      codexProtocolOverrides: {
        "qwen3.8": "openai_chat",
        "gpt-5.5": "openai_responses",
      },
    });
  });

  it("loads manual settings conservatively and normalizes protocol-specific defaults", () => {
    expect(readCodexProtocolSettings(undefined, "openai_responses")).toEqual({
      protocolMode: "auto",
      reasoningProjection: "none",
      toolSchemaDialect: "openai",
      historyReplay: "native_only",
    });

    expect(
      readCodexProtocolSettings(
        {
          codexProtocolMode: "manual",
          codexReasoningProjection: "reasoning_summary",
          codexToolSchemaDialect: "openai",
          codexHistoryReplay: "chat_reasoning_content",
        },
        "openai_chat",
      ),
    ).toEqual({
      protocolMode: "manual",
      reasoningProjection: "none",
      toolSchemaDialect: "openai",
      historyReplay: "chat_reasoning_content",
    });

    expect(
      readCodexProtocolSettings(
        {
          codexProtocolMode: "manual",
          codexReasoningProjection: "raw_reasoning_text",
          codexToolSchemaDialect: "moonshot_mfjs",
          codexHistoryReplay: "omit",
        },
        "openai_chat",
      ),
    ).toEqual({
      protocolMode: "manual",
      reasoningProjection: "raw_reasoning_text",
      toolSchemaDialect: "moonshot_mfjs",
      historyReplay: "chat_reasoning_content",
    });

    expect(
      readCodexProtocolSettings(
        {
          codexProtocolMode: "manual",
          codexReasoningProjection: "invalid" as never,
          codexToolSchemaDialect: "invalid" as never,
          codexHistoryReplay: "chat_reasoning_content",
        },
        "openai_responses",
      ),
    ).toEqual({
      protocolMode: "manual",
      reasoningProjection: "none",
      toolSchemaDialect: "openai",
      historyReplay: "native_only",
    });
  });
});

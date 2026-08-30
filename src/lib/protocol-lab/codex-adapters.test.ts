import { describe, expect, it, vi } from "vitest";

import {
  createBatchCodexProtocolLabAdapter,
  createSingleCodexProtocolLabAdapter,
  createUniversalCodexProtocolLabAdapter,
} from "./codex-adapters";

const provider = {
  id: "provider-one",
  name: "Provider One",
  settingsConfig: {
    modelCatalog: {
      models: [{ model: "qwen3.8", enabled: true }],
    },
  },
};

describe("single Codex Protocol Lab adapter", () => {
  it("requires a probe only while an enabled model follows automatic selection", () => {
    const adapter = createSingleCodexProtocolLabAdapter({} as never);

    expect(adapter.requiresProbe(provider, [])).toBe(true);
    expect(adapter.requiresProbe(provider, ["receipt-one"])).toBe(false);
    expect(
      adapter.requiresProbe(
        {
          ...provider,
          meta: {
            codexProtocolMode: "manual",
            codexProtocolOverrides: { "qwen3.8": "openai_chat" },
          },
        },
        [],
      ),
    ).toBe(false);
    expect(
      adapter.requiresProbe(
        {
          ...provider,
          meta: { providerType: "xai_oauth" },
        },
        [],
      ),
    ).toBe(false);
  });

  it("maps the backend commit snapshot and projection status without split confirmation", async () => {
    const snapshot = {
      logicalProvider: provider,
      adaptation: {
        persistence: "split" as const,
        status: "ready" as const,
        effectiveTransport: "mixed" as const,
        models: [],
      },
    };
    const commitCodexProviderSet = vi.fn(async () => ({
      preview: {
        digest: "digest-one",
        sourceProviderId: "provider-one",
        responsesModels: ["qwen3.8"],
        chatModels: ["kimi"],
        plan: {
          kind: "split" as const,
          responses_provider_id: "responses-leaf",
          chat_provider_id: "chat-leaf",
        },
      },
      snapshot,
      projections: [],
      status: "committed_with_projection_error" as const,
      projectionErrorCode: "projection_pending",
    }));
    const adapter = createSingleCodexProtocolLabAdapter({
      commitCodexProviderSet,
    } as never);

    const result = await adapter.commit(
      provider,
      ["receipt-one"],
      {
        digest: "digest-one",
        sourceProviderId: "provider-one",
        responsesModels: [],
        chatModels: [],
        plan: { kind: "single", transport: "open_ai_responses" },
      },
      "accept_auto",
    );

    expect(commitCodexProviderSet).toHaveBeenCalledWith(
      provider,
      ["receipt-one"],
      "digest-one",
      "accept_auto",
    );
    expect(result.snapshot).toBe(snapshot);
    expect(result.projectionWarning).toBe(true);
    expect(result.projectionErrorCode).toBe("projection_pending");
  });
});

describe("batch Codex Protocol Lab adapter", () => {
  it("returns normalized sources and receipts as the authoritative probe outcome", async () => {
    const manualProvider = {
      ...provider,
      id: "provider-manual",
      name: "Provider Manual",
      meta: {
        codexProtocolMode: "manual" as const,
        codexProtocolOverrides: { "qwen3.8": "openai_chat" as const },
      },
    };
    const normalizedProvider = { ...provider, name: "Provider One Normalized" };
    const preflightCodexProviderProtocolCompatibility = vi.fn(async () => ({
      provider: normalizedProvider,
      adaptationPreview: {
        persistence: "single" as const,
        status: "ready" as const,
        effectiveTransport: "open_ai_responses" as const,
        models: [],
      },
      records: [],
      observations: [],
      receiptIds: ["receipt-one"],
      protocolApplied: false,
    }));
    const adapter = createBatchCodexProtocolLabAdapter({
      preflightCodexProviderProtocolCompatibility,
    } as never);
    const router = { ...provider, id: "router-one", name: "Router One" };

    const result = await adapter.preflight(
      {
        sources: [
          { provider, receiptIds: [] },
          { provider: manualProvider, receiptIds: [] },
        ],
        router,
      },
      vi.fn(),
    );

    expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalledTimes(
      1,
    );
    expect(result.outcome.sources).toEqual([
      { provider: normalizedProvider, receiptIds: ["receipt-one"] },
      { provider: manualProvider, receiptIds: [] },
    ]);
    expect(result.outcome.outcomes).toEqual([
      {
        providerId: "provider-one",
        inputProvider: provider,
        outcome: expect.objectContaining({ provider: normalizedProvider }),
      },
    ]);
    expect(result.draft).toEqual({
      sources: result.outcome.sources,
      router,
    });
  });
});

describe("Universal Codex Protocol Lab adapter", () => {
  it("maps a committed projection warning without reporting the save as failed", async () => {
    const commitUniversalProviderSet = vi.fn(async () => ({
      preview: {
        digest: "universal-digest",
        universalProviderId: "universal-one",
        codex: null,
      },
      codexSnapshot: null,
      status: "committed_with_projection_error" as const,
      projectionErrorCode: "projection_pending",
    }));
    const adapter = createUniversalCodexProtocolLabAdapter({
      commitUniversalProviderSet,
    } as never);
    const universal = {
      id: "universal-one",
      name: "Universal One",
      providerType: "newapi",
      baseUrl: "https://example.test/v1",
      apiKey: "secret",
      apps: { claude: false, codex: false, gemini: false },
      models: {},
    };

    const result = await adapter.commit(
      universal,
      [],
      {
        digest: "universal-digest",
        universalProviderId: "universal-one",
        codex: null,
      },
      "accept_auto",
    );

    expect(result.projectionWarning).toBe(true);
    expect(result.projectionErrorCode).toBe("projection_pending");
  });
});

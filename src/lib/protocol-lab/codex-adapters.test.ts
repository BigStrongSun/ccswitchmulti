import { describe, expect, it, vi } from "vitest";

import type { CodexProtocolProbeProgressEvent } from "@/lib/api/protocol-compatibility";

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
  it("treats missing or mismatched probe receipts as stale evidence that must be re-probed", () => {
    const adapter = createSingleCodexProtocolLabAdapter({} as never);

    expect(
      adapter.isDependencyChanged(
        new Error("codex_provider_set_probe_required: receipt-old"),
      ),
    ).toBe(true);
    expect(
      adapter.isDependencyChanged(
        new Error("codex_provider_set_probe_target_mismatch"),
      ),
    ).toBe(true);
    expect(
      adapter.isDependencyChanged(
        new Error("codex_provider_set_probe_receipt_in_use: receipt-live"),
      ),
    ).toBe(false);
  });

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
  it("re-probes nested stale receipts when the workflow requests preflight again", async () => {
    const preflight = vi.fn(async () => ({
      provider,
      receiptIds: ["fresh"],
      records: [],
      observations: [],
      protocolApplied: false,
      adaptationPreview: {
        persistence: "single" as const,
        status: "ready" as const,
        effectiveTransport: "open_ai_responses" as const,
        models: [],
      },
    }));
    const adapter = createBatchCodexProtocolLabAdapter({
      preflightCodexProviderProtocolCompatibility: preflight,
      prepareCodexProviderSetBatch: vi.fn(),
      commitCodexProviderSetBatch: vi.fn(),
    });
    const result = await adapter.preflight(
      { router: provider, sources: [{ provider, receiptIds: ["expired"] }] },
      () => {},
    );
    expect(result.receiptIds).toEqual(["fresh"]);
  });
  it("overlaps two providers, refills a free slot and preserves source/receipt order", async () => {
    const started: string[] = [];
    const release = new Map<string, () => void>();
    const sources = ["a", "b", "c", "d"].map((id) => ({
      provider: { ...provider, id },
      receiptIds: [] as string[],
    }));
    const adapter = createBatchCodexProtocolLabAdapter({
      preflightCodexProviderProtocolCompatibility: async (input, progress) => {
        started.push(input.id);
        await new Promise<void>((resolve) => release.set(input.id, resolve));
        progress?.({
          kind: "candidate_finished",
          model: "qwen3.8",
          selectedTransport: "open_ai_chat",
          readiness: "verified",
        });
        return {
          provider: input,
          receiptIds: [`receipt-${input.id}`],
          records: [],
          observations: [],
          protocolApplied: false,
          adaptationPreview: {
            persistence: "single",
            status: "ready",
            effectiveTransport: "open_ai_chat",
            models: [],
          },
        };
      },
      prepareCodexProviderSetBatch: vi.fn(),
      commitCodexProviderSetBatch: vi.fn(),
    });
    const progress = vi.fn();
    const pending = adapter.preflight({ sources, router: provider }, progress);
    expect(started).toEqual(["a", "b"]);
    release.get("b")!();
    await vi.waitFor(() => expect(started).toEqual(["a", "b", "c"]));
    release.get("c")!();
    await vi.waitFor(() => expect(started).toEqual(["a", "b", "c", "d"]));
    release.get("d")!();
    release.get("a")!();
    const result = await pending;
    expect(result.receiptIds).toEqual([
      "receipt-a",
      "receipt-b",
      "receipt-c",
      "receipt-d",
    ]);
    expect(result.outcome.outcomes.map((entry) => entry.providerId)).toEqual([
      "a",
      "b",
      "c",
      "d",
    ]);
    expect(progress.mock.calls[0][0]).toMatchObject({
      providerId: "b",
      model: "qwen3.8",
    });
  });

  it("stops queued providers on error and drains in-flight work before allowing retry", async () => {
    const started: string[] = [];
    let fail!: (error: Error) => void;
    let finish!: () => void;
    const adapter = createBatchCodexProtocolLabAdapter({
      preflightCodexProviderProtocolCompatibility: async (input) => {
        started.push(input.id);
        if (input.id === "a")
          await new Promise<void>((_, reject) => {
            fail = reject;
          });
        else
          await new Promise<void>((resolve) => {
            finish = resolve;
          });
        return {
          provider: input,
          receiptIds: [],
          records: [],
          observations: [],
          protocolApplied: false,
          adaptationPreview: {
            persistence: "single",
            status: "ready",
            effectiveTransport: "open_ai_chat",
            models: [],
          },
        };
      },
      prepareCodexProviderSetBatch: vi.fn(),
      commitCodexProviderSetBatch: vi.fn(),
    });
    let settled = false;
    const pending = adapter
      .preflight(
        {
          router: provider,
          sources: ["a", "b", "c"].map((id) => ({
            provider: { ...provider, id },
            receiptIds: [],
          })),
        },
        () => {},
      )
      .catch((error) => {
        settled = true;
        return error;
      });
    expect(started).toEqual(["a", "b"]);
    fail(new Error("probe failed"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(settled).toBe(false);
    expect(started).toEqual(["a", "b"]);
    finish();
    expect((await pending).message).toBe("probe failed");
    expect(started).toEqual(["a", "b"]);
  });

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
    const preflightCodexProviderProtocolCompatibility = vi.fn(
      async (
        _provider: unknown,
        onProgress: (event: CodexProtocolProbeProgressEvent) => void,
      ) => {
        onProgress({
          kind: "candidate_finished",
          model: "qwen3.8",
          selectedTransport: "open_ai_responses",
          readiness: "verified",
        });
        return {
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
        };
      },
    );
    const adapter = createBatchCodexProtocolLabAdapter({
      preflightCodexProviderProtocolCompatibility,
    } as never);
    const router = { ...provider, id: "router-one", name: "Router One" };

    const onProgress = vi.fn();
    const result = await adapter.preflight(
      {
        sources: [
          { provider, receiptIds: [] },
          { provider: manualProvider, receiptIds: [] },
        ],
        router,
      },
      onProgress,
    );

    expect(preflightCodexProviderProtocolCompatibility).toHaveBeenCalledTimes(
      1,
    );
    expect(onProgress).toHaveBeenCalledWith({
      kind: "candidate_finished",
      providerId: "provider-one",
      providerName: "Provider One",
      model: "qwen3.8",
      selectedTransport: "open_ai_responses",
      readiness: "verified",
    });
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

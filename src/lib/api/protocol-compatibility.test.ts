import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (message: unknown) => void;
  },
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import {
  commitCodexProviderSet,
  commitCodexProviderSetBatch,
  getCodexProviderEditorSnapshot,
  listCodexProviderAdaptationSummaries,
} from "./protocol-compatibility";

const provider = {
  id: "provider-one",
  name: "Provider One",
  settingsConfig: {},
};

describe("Codex Provider adaptation API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({});
  });

  it("loads one editor snapshot by logical Provider id", async () => {
    await getCodexProviderEditorSnapshot("provider-one");

    expect(invokeMock).toHaveBeenCalledWith(
      "get_codex_provider_editor_snapshot",
      { providerId: "provider-one" },
    );
  });

  it("loads adaptation summaries in one backend call", async () => {
    await listCodexProviderAdaptationSummaries();

    expect(invokeMock).toHaveBeenCalledWith(
      "list_codex_provider_adaptation_summaries",
    );
  });

  it("commits automatic Single or Split plans with one accept_auto intent", async () => {
    await commitCodexProviderSet(
      provider,
      ["receipt-one"],
      "digest-one",
      "accept_auto",
    );

    expect(invokeMock).toHaveBeenCalledWith("commit_codex_provider_set", {
      request: {
        provider,
        receiptIds: ["receipt-one"],
        digest: "digest-one",
        intent: "accept_auto",
      },
    });
  });

  it("uses the same automatic intent for wizard batch commits", async () => {
    const sources = [{ provider, receiptIds: ["receipt-one"] }];
    await commitCodexProviderSetBatch(
      sources,
      provider,
      "digest-batch",
      "accept_auto",
    );

    expect(invokeMock).toHaveBeenCalledWith("commit_codex_provider_set_batch", {
      request: {
        sources,
        router: provider,
        digest: "digest-batch",
        intent: "accept_auto",
      },
    });
  });
});

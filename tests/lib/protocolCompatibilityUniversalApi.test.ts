import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UniversalProvider } from "@/types";

const invokeMock = vi.fn();
const channels: Array<{ onmessage?: (event: unknown) => void }> = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage?: (event: unknown) => void;

    constructor() {
      channels.push(this);
    }
  },
}));

import {
  commitUniversalProviderSet,
  preflightUniversalCodexProtocolCompatibility,
  prepareUniversalProviderSet,
} from "@/lib/api/protocol-compatibility";

const provider: UniversalProvider = {
  id: "universal-qwen",
  name: "Universal Qwen",
  providerType: "newapi",
  baseUrl: "https://gateway.example/v1",
  apiKey: "secret",
  apps: { claude: true, codex: true, gemini: true },
  models: { codex: { model: "qwen3.8" } },
};

describe("Universal Provider Set API", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    channels.splice(0);
  });

  it("preflights the unsaved Universal definition and wires progress events", async () => {
    invokeMock.mockResolvedValueOnce(null);
    const onProgress = vi.fn();

    await preflightUniversalCodexProtocolCompatibility(provider, onProgress);

    expect(invokeMock).toHaveBeenCalledWith(
      "preflight_universal_codex_protocol_compatibility",
      { provider, onEvent: channels[0] },
    );
    channels[0].onmessage?.({ kind: "candidate_started", model: "qwen3.8" });
    expect(onProgress).toHaveBeenCalledWith({
      kind: "candidate_started",
      model: "qwen3.8",
    });
  });

  it("prepares and commits one Universal definition through structured requests", async () => {
    invokeMock.mockResolvedValue({ digest: "digest-1" });

    await prepareUniversalProviderSet(provider, ["receipt-1"]);
    await commitUniversalProviderSet(
      provider,
      ["receipt-1"],
      "digest-1",
      "accept_auto",
    );

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "prepare_universal_provider_set",
      { request: { provider, receiptIds: ["receipt-1"] } },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "commit_universal_provider_set",
      {
        request: {
          provider,
          receiptIds: ["receipt-1"],
          digest: "digest-1",
          intent: "accept_auto",
        },
      },
    );
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { codexSubagentV2Api } from "@/lib/api/codexSubagentV2";

describe("codexSubagentV2Api P4 reasoning read-only transport", () => {
  beforeEach(() => invokeMock.mockReset());

  it("uses the inspect command without sending settings or secrets", async () => {
    invokeMock.mockResolvedValueOnce({ schemaVersion: 1 });

    await codexSubagentV2Api.inspectReasoningCapability(
      "provider-qwen",
      "qwen3.8",
    );

    expect(invokeMock).toHaveBeenCalledWith(
      "inspect_codex_reasoning_capability",
      { providerId: "provider-qwen", model: "qwen3.8" },
    );
  });

  it("exposes list, validate, and always-redacted export transports", async () => {
    invokeMock.mockResolvedValue({ schemaVersion: 1 });

    await codexSubagentV2Api.listReasoningCapabilities("provider-qwen");
    await codexSubagentV2Api.validateReasoningProvider("provider-qwen");
    await codexSubagentV2Api.exportReasoningProvider("provider-qwen");

    expect(invokeMock).toHaveBeenNthCalledWith(
      1,
      "list_codex_reasoning_capabilities",
      { providerId: "provider-qwen" },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "validate_codex_reasoning_provider",
      { providerId: "provider-qwen" },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(
      3,
      "export_codex_reasoning_provider",
      { providerId: "provider-qwen", redacted: true },
    );
  });

  it("validates an unsaved provider candidate without a provider id", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    const settingsConfig = {
      codexRouting: {
        subagentV2: {
          schemaVersion: 2,
          selectionPolicy: "balanced",
          profiles: {},
        },
      },
    };

    await codexSubagentV2Api.validateProviderCandidate(settingsConfig);

    expect(invokeMock).toHaveBeenCalledWith(
      "validate_codex_subagent_v2_provider_candidate",
      { settingsConfig },
    );
  });
});

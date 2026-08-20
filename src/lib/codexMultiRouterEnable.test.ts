import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import {
  enableCodexMultiRouterPlan,
  type ProviderSwitchOutcome,
} from "./codexMultiRouterEnable";

const provider = {
  id: "codex-multirouter",
  name: "Test MultiRouter",
  category: "custom",
  settingsConfig: {
    codexRouting: { schemaVersion: 2, enabled: true, routes: [] },
  },
} as Provider;

describe("enableCodexMultiRouterPlan", () => {
  it("returns the target provider switch result without a separate takeover phase", async () => {
    const expected = { warnings: ["test warning"] };
    const switchProvider = vi
      .fn<(provider: Provider) => Promise<ProviderSwitchOutcome>>()
      .mockResolvedValue({ ok: true, result: expected });

    await expect(
      enableCodexMultiRouterPlan(provider, switchProvider),
    ).resolves.toBe(expected);
    expect(switchProvider).toHaveBeenCalledOnce();
    expect(switchProvider).toHaveBeenCalledWith(provider);
  });

  it("throws the original switch failure instead of allowing wizard success", async () => {
    const error = new Error("atomic takeover verification failed");
    const switchProvider = vi
      .fn<(provider: Provider) => Promise<ProviderSwitchOutcome>>()
      .mockResolvedValue({ ok: false, error });

    await expect(
      enableCodexMultiRouterPlan(provider, switchProvider),
    ).rejects.toBe(error);
  });

  it("refuses to enable a schema v1 plan before explicit migration", async () => {
    const legacyProvider = {
      ...provider,
      settingsConfig: {
        codexRouting: {
          enabled: true,
          routes: [{ id: "legacy-route", upstream: { apiKey: "secret" } }],
        },
      },
    } as Provider;
    const switchProvider = vi.fn();

    await expect(
      enableCodexMultiRouterPlan(legacyProvider, switchProvider),
    ).rejects.toThrow("codex_multirouter_migration_required");
    expect(switchProvider).not.toHaveBeenCalled();
  });
});

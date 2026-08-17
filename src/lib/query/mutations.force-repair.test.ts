import { describe, expect, it, vi } from "vitest";

import { createProviderSwitchFailureToastOptions } from "./mutations";

describe("provider switch force repair action", () => {
  it("offers copy and force repair together for a failed Codex switch", async () => {
    const copy = vi.fn();
    const forceRepair = vi.fn().mockResolvedValue(undefined);

    const options = createProviderSwitchFailureToastOptions({
      appId: "codex",
      providerId: "deepseek-provider",
      detail: "duplicate field max_concurrent_threads_per_session",
      copy,
      forceRepair,
      t: (_key, fallback) => fallback,
    });

    expect(options.action?.label).toBe("复制");
    expect(options.cancel?.label).toBe("强制覆盖");
    options.action?.onClick();
    await options.cancel?.onClick();
    expect(copy).toHaveBeenCalledWith(
      "duplicate field max_concurrent_threads_per_session",
    );
    expect(forceRepair).toHaveBeenCalledWith("deepseek-provider");
  });

  it("does not offer force repair for non-Codex providers", () => {
    const options = createProviderSwitchFailureToastOptions({
      appId: "claude",
      providerId: "claude-provider",
      detail: "switch failed",
      copy: vi.fn(),
      forceRepair: vi.fn(),
      t: (_key, fallback) => fallback,
    });

    expect(options.action?.label).toBe("复制");
    expect(options.cancel).toBeUndefined();
  });
});

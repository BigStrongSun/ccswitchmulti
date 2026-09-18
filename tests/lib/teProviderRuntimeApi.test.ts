import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { getTeProviderRuntimeStatus } from "@/lib/api/teProvider";

describe("getTeProviderRuntimeStatus", () => {
  beforeEach(() => invokeMock.mockReset());

  it("passes only the endpoint to the read-only runtime command", async () => {
    invokeMock.mockResolvedValueOnce({
      sidecarUrl: "http://127.0.0.1:9814",
      sidecarReachable: true,
      sidecarStatus: "ok",
      sidecarError: null,
      provider: {
        online: true,
        reason: null,
        checkedAt: "2026-09-18T04:00:00Z",
        httpStatus: 200,
      },
      latencyMs: 4,
      checkedAt: "2026-09-18T04:00:01Z",
      runtimeBindingExposed: false,
    });

    const status = await getTeProviderRuntimeStatus("http://127.0.0.1:9814");

    expect(invokeMock).toHaveBeenCalledWith("te_provider_runtime_status", {
      sidecarUrl: "http://127.0.0.1:9814",
    });
    expect(status.provider?.online).toBe(true);
    expect(status.runtimeBindingExposed).toBe(false);
  });

  it("turns Tauri string rejections into Error so the UI never renders raw values", async () => {
    invokeMock.mockRejectedValueOnce(
      "TE Provider endpoint must be a numeric loopback host",
    );

    await expect(
      getTeProviderRuntimeStatus("https://example.com"),
    ).rejects.toThrow("numeric loopback");
  });
});

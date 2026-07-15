import { afterEach, describe, expect, it, vi } from "vitest";
import { normalizeProvidersPayload } from "@/lib/api/providers";

/** 构造满足渲染层最小契约的 provider IPC 条目。 */
function provider(overrides: Record<string, unknown> = {}) {
  return {
    id: "relay",
    name: "Relay",
    settingsConfig: { base_url: "https://relay.example/v1" },
    ...overrides,
  };
}

describe("normalizeProvidersPayload", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("隔离 null provider，避免它进入后续的 sort 和向导渲染", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    expect(
      normalizeProvidersPayload({ valid: provider(), malformed: null }),
    ).toEqual({ relay: provider() });
  });

  it("将非对象 settingsConfig 归一化为空对象", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    expect(
      normalizeProvidersPayload({ relay: provider({ settingsConfig: null }) }),
    ).toEqual({
      relay: provider({ settingsConfig: {} }),
    });
  });

  it("拒绝非对象的 IPC 根 payload", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    expect(normalizeProvidersPayload(null)).toEqual({});
  });
});

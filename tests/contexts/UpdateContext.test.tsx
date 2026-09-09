import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";

vi.mock("@/lib/updater", () => ({
  checkForUpdate: vi.fn().mockRejectedValue("TLS certificate expired"),
}));

describe("UpdateProvider error state", () => {
  it("preserves plain string errors from the updater plugin", async () => {
    const { result } = renderHook(useUpdate, { wrapper: UpdateProvider });
    await act(async () => {
      await expect(result.current.checkUpdate()).rejects.toBe(
        "TLS certificate expired",
      );
    });
    expect(result.current.error).toBe("TLS certificate expired");
    expect(result.current.isChecking).toBe(false);
  });
});

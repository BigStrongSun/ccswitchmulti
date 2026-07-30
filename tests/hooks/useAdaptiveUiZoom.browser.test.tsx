import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAdaptiveUiZoom } from "@/hooks/useAdaptiveUiZoom";

describe("adaptive UI zoom outside Tauri", () => {
  it("exits silently when native window metadata is unavailable", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);

    expect(() => renderHook(() => useAdaptiveUiZoom())).not.toThrow();
    expect(warn).not.toHaveBeenCalled();

    warn.mockRestore();
  });
});

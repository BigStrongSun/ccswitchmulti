import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  calculateAdaptiveUiZoom,
  useAdaptiveUiZoom,
} from "@/hooks/useAdaptiveUiZoom";

const setZoomMock = vi.fn().mockResolvedValue(undefined);
const innerSizeMock = vi.fn();
const scaleFactorMock = vi.fn();
let resizeListener:
  | ((event: { payload: { width: number; height: number } }) => void)
  | undefined;
let scaleListener:
  | ((event: {
      payload: {
        scaleFactor: number;
        size: { width: number; height: number };
      };
    }) => void)
  | undefined;

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ setZoom: setZoomMock }),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    innerSize: innerSizeMock,
    scaleFactor: scaleFactorMock,
    onResized: vi.fn(async (listener) => {
      resizeListener = listener;
      return vi.fn();
    }),
    onScaleChanged: vi.fn(async (listener) => {
      scaleListener = listener;
      return vi.fn();
    }),
  }),
}));

describe("adaptive UI zoom", () => {
  beforeEach(() => {
    setZoomMock.mockClear();
    innerSizeMock.mockReset();
    scaleFactorMock.mockReset();
    resizeListener = undefined;
    scaleListener = undefined;
    document.documentElement.removeAttribute("data-ui-zoom");
  });

  it.each([
    [1180, 760, 1],
    [1100, 700, 0.9],
    [1000, 650, 0.85],
    [900, 600, 0.8],
    [1800, 1000, 1],
  ])("maps a %sx%s logical viewport to %s zoom", (width, height, expected) => {
    expect(calculateAdaptiveUiZoom(width, height)).toBe(expected);
  });

  it("applies one WebView-wide zoom and updates it from physical resize events", async () => {
    innerSizeMock.mockResolvedValue({ width: 1500, height: 975 });
    scaleFactorMock.mockResolvedValue(1.5);

    renderHook(() => useAdaptiveUiZoom());

    await waitFor(() => expect(setZoomMock).toHaveBeenCalledWith(0.85));
    expect(document.documentElement.dataset.uiZoom).toBe("0.85");

    await act(async () => {
      resizeListener?.({ payload: { width: 1770, height: 1140 } });
    });

    await waitFor(() => expect(setZoomMock).toHaveBeenLastCalledWith(1));
    expect(document.documentElement.dataset.uiZoom).toBe("1");
  });

  it("recalculates logical size when the monitor scale factor changes", async () => {
    innerSizeMock.mockResolvedValue({ width: 1180, height: 760 });
    scaleFactorMock.mockResolvedValue(1);

    renderHook(() => useAdaptiveUiZoom());
    await waitFor(() => expect(setZoomMock).toHaveBeenCalledWith(1));

    await act(async () => {
      scaleListener?.({
        payload: {
          scaleFactor: 1.25,
          size: { width: 1250, height: 812.5 },
        },
      });
    });

    await waitFor(() => expect(setZoomMock).toHaveBeenLastCalledWith(0.85));
  });
});

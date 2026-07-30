import { useEffect } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

const DESIGN_VIEWPORT_WIDTH = 1180;
const DESIGN_VIEWPORT_HEIGHT = 760;
const MIN_UI_ZOOM = 0.8;
const ZOOM_STEP = 0.05;

type PhysicalViewportSize = {
  width: number;
  height: number;
};

/**
 * Keep one readable, app-wide density across normal pages, portals, and full-screen panels.
 * The minimum window remains usable on smaller displays while larger windows stay at 100%.
 */
export function calculateAdaptiveUiZoom(
  logicalWidth: number,
  logicalHeight: number,
): number {
  if (
    !Number.isFinite(logicalWidth) ||
    !Number.isFinite(logicalHeight) ||
    logicalWidth <= 0 ||
    logicalHeight <= 0
  ) {
    return 1;
  }

  const fit = Math.min(
    1,
    logicalWidth / DESIGN_VIEWPORT_WIDTH,
    logicalHeight / DESIGN_VIEWPORT_HEIGHT,
  );
  const stepped = Math.round(fit / ZOOM_STEP) * ZOOM_STEP;
  return Math.max(MIN_UI_ZOOM, Math.min(1, Number(stepped.toFixed(2))));
}

export function useAdaptiveUiZoom(): void {
  useEffect(() => {
    let appWindow: ReturnType<typeof getCurrentWindow>;
    let webview: ReturnType<typeof getCurrentWebview>;

    try {
      appWindow = getCurrentWindow();
      webview = getCurrentWebview();
    } catch {
      // Renderer tests and browser previews do not expose the Tauri runtime.
      return;
    }

    let disposed = false;
    let requestVersion = 0;
    const unlistenCallbacks: Array<() => void> = [];

    const applyZoom = async (
      physicalSize?: PhysicalViewportSize,
      knownScaleFactor?: number,
    ) => {
      const currentRequest = ++requestVersion;

      try {
        const [size, scaleFactor] = await Promise.all([
          physicalSize ? Promise.resolve(physicalSize) : appWindow.innerSize(),
          knownScaleFactor
            ? Promise.resolve(knownScaleFactor)
            : appWindow.scaleFactor(),
        ]);
        if (disposed || currentRequest !== requestVersion) return;

        const safeScaleFactor = scaleFactor > 0 ? scaleFactor : 1;
        const zoom = calculateAdaptiveUiZoom(
          size.width / safeScaleFactor,
          size.height / safeScaleFactor,
        );
        await webview.setZoom(zoom);
        if (disposed || currentRequest !== requestVersion) return;

        document.documentElement.dataset.uiZoom = String(zoom);
      } catch (error) {
        // Browser-only previews do not expose Tauri window APIs. The native app does.
        console.warn("[adaptive-ui-zoom] Failed to update WebView zoom", error);
      }
    };

    const trackUnlisten = (promise: Promise<() => void>) => {
      void promise
        .then((unlisten) => {
          if (disposed) {
            unlisten();
          } else {
            unlistenCallbacks.push(unlisten);
          }
        })
        .catch((error) => {
          console.warn(
            "[adaptive-ui-zoom] Failed to subscribe to window changes",
            error,
          );
        });
    };

    trackUnlisten(
      appWindow.onResized(({ payload }) => {
        void applyZoom(payload);
      }),
    );
    trackUnlisten(
      appWindow.onScaleChanged(({ payload }) => {
        void applyZoom(payload.size, payload.scaleFactor);
      }),
    );
    void applyZoom();

    return () => {
      disposed = true;
      requestVersion += 1;
      unlistenCallbacks.splice(0).forEach((unlisten) => unlisten());
      document.documentElement.removeAttribute("data-ui-zoom");
    };
  }, []);
}

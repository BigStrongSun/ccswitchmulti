import { describe, expect, it } from "vitest";
import tauriConfig from "../../src-tauri/tauri.conf.json";
import defaultCapability from "../../src-tauri/capabilities/default.json";

describe("main window sizing", () => {
  it("opens new installations at the unscaled UI design viewport", () => {
    const mainWindow = tauriConfig.app.windows[0];

    expect(mainWindow.width).toBe(1180);
    expect(mainWindow.height).toBe(760);
    expect(mainWindow.minWidth).toBe(900);
    expect(mainWindow.minHeight).toBe(600);
  });

  it("allows the renderer to apply the shared WebView zoom", () => {
    expect(defaultCapability.permissions).toContain(
      "core:webview:allow-set-webview-zoom",
    );
  });
});

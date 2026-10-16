import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

async function loadPlatform(userAgent: string, platform: string) {
  vi.resetModules();
  vi.stubGlobal("navigator", { userAgent, platform });
  return import("@/lib/platform");
}

describe("platform drag-region behavior", () => {
  it("keeps Windows event drag regions without native CSS drag styling", async () => {
    const platform = await loadPlatform(
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
      "Win32",
    );

    expect(platform.isWindows()).toBe(true);
    expect(platform.DRAG_REGION_ATTR).toEqual({
      "data-tauri-drag-region": true,
    });
    expect(platform.DRAG_REGION_STYLE).toEqual({});
  });

  it("preserves native drag regions on macOS", async () => {
    const platform = await loadPlatform(
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)",
      "MacIntel",
    );

    expect(platform.isMac()).toBe(true);
    expect(platform.DRAG_REGION_ATTR).toEqual({
      "data-tauri-drag-region": true,
    });
    expect(platform.DRAG_REGION_STYLE).toEqual({ WebkitAppRegion: "drag" });
  });

  it("continues to omit drag regions on Linux", async () => {
    const platform = await loadPlatform(
      "Mozilla/5.0 (X11; Linux x86_64)",
      "Linux x86_64",
    );

    expect(platform.isLinux()).toBe(true);
    expect(platform.DRAG_REGION_ATTR).toEqual({});
    expect(platform.DRAG_REGION_STYLE).toEqual({});
  });

  it("limits native CSS drag styling to the macOS root class", () => {
    const css = readFileSync(resolve(process.cwd(), "src/index.css"), "utf8");

    expect(css).toMatch(
      /\.is-mac\s+\[data-tauri-drag-region\]\s*\{\s*-webkit-app-region:\s*drag;/,
    );
    expect(css).not.toMatch(
      /(?:^|})\s*\[data-tauri-drag-region\]\s*\{\s*-webkit-app-region:\s*drag;/m,
    );
  });
});

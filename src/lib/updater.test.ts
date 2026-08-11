import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { checkForUpdate } from "./updater";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "3.19.1-19"),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(async () => {
    throw new Error("frontend updater client must not be used");
  }),
}));

describe("application update checks", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("returns release metadata from the proxy-aware backend check", async () => {
    vi.mocked(invoke).mockResolvedValue({
      currentVersion: "3.19.1-19",
      availableVersion: "3.19.1-20",
      notes: "Fix updater routing",
      pubDate: "2026-08-11T08:00:00Z",
    });

    await expect(checkForUpdate()).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "3.19.1-19",
        availableVersion: "3.19.1-20",
        notes: "Fix updater routing",
        pubDate: "2026-08-11T08:00:00Z",
      },
    });
  });

  it("maps an empty backend result to up-to-date", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
  });
});

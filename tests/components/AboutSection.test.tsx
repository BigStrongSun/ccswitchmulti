import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";

const getToolVersionsMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.20.1-1"),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: false,
    updateInfo: null,
    checkUpdate: vi.fn().mockResolvedValue(false),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getToolVersions: getToolVersionsMock,
    openExternal: vi.fn(),
    checkUpdates: vi.fn(),
    installUpdateAndRestart: vi.fn(),
    probeToolInstallations: vi.fn().mockResolvedValue([]),
    runToolLifecycleAction: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("framer-motion", () => ({
  motion: new Proxy(
    {},
    {
      get: (_target, tag: string) => tag,
    },
  ),
}));

describe("AboutSection", () => {
  beforeEach(() => {
    getToolVersionsMock.mockReset();
    getToolVersionsMock.mockImplementation(async ([name]: [string]) => [
      {
        name,
        version: null,
        latest_version: null,
        error: null,
        installed_but_broken: false,
        env_type: "windows",
        wsl_distro: null,
      },
    ]);
  });

  it("includes Pi in the local tool lifecycle lookup", async () => {
    render(<AboutSection isPortable={false} />);

    await waitFor(() =>
      expect(getToolVersionsMock).toHaveBeenCalledWith(["pi"], {}),
    );
  });
});

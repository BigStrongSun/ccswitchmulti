import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { watchdogApi, type WatchdogStatus } from "@/lib/api/watchdog";
import { WatchdogStatusPanel } from "./WatchdogStatusPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/lib/api/watchdog", () => ({
  watchdogApi: { getStatus: vi.fn() },
}));

const healthy: WatchdogStatus = {
  enabled: true,
  appPid: 4242,
  supervisorPid: 5151,
  supervisorAlive: true,
  supervisorPath: "C:/App/cc-switch.exe",
  restartsInWindow: 1,
  port: 15721,
  listenerPid: 4242,
  listenerPath: "C:/App/cc-switch.exe",
  portReady: true,
  portDiagnosis: "端口 15721 占用诊断：LISTEN ipv4 127.0.0.1:15721 pid=4242",
  recentEvents: [
    { timestamp: "2026-09-15T00:00:00.000+08:00", level: "info", event: "supervisor-watching" },
  ],
};

describe("WatchdogStatusPanel", () => {
  it("shows the supervisor, restarts and ready port", async () => {
    vi.mocked(watchdogApi.getStatus).mockResolvedValue(healthy);
    render(<WatchdogStatusPanel />);

    await waitFor(() =>
      expect(screen.getByText("settings.watchdogStateRunning")).toBeTruthy(),
    );
    expect(screen.getByText("settings.watchdogPortReady")).toBeTruthy();
    expect(screen.getByText("127.0.0.1:15721")).toBeTruthy();
    expect(screen.getByText("supervisor-watching")).toBeTruthy();
  });

  it("surfaces the port diagnosis when the listener is not ours", async () => {
    vi.mocked(watchdogApi.getStatus).mockResolvedValue({
      ...healthy,
      portReady: false,
      supervisorAlive: false,
      supervisorPid: null,
      restartsInWindow: 0,
      recentEvents: [],
    });
    render(<WatchdogStatusPanel />);

    await waitFor(() =>
      expect(screen.getByText("settings.watchdogPortNotReady")).toBeTruthy(),
    );
    expect(screen.getByText("settings.watchdogStateStopped")).toBeTruthy();
    expect(screen.getByText("settings.watchdogNoEvents")).toBeTruthy();
    expect(screen.getByText(/LISTEN ipv4/)).toBeTruthy();
  });
});
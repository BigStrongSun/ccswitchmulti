import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { codexEgressTimezoneApi } from "@/lib/api/codexEgressTimezone";
import { CodexEgressTimezoneStatusCard } from "./CodexEgressTimezoneStatusCard";

vi.mock("@/lib/api/codexEgressTimezone", () => ({
  codexEgressTimezoneApi: {
    monitorStatus: vi.fn(),
    triggerAutomaticProbe: vi.fn(),
  },
}));

describe("CodexEgressTimezoneStatusCard", () => {
  it("explains package activation without offering an ineffective refresh", async () => {
    vi.mocked(codexEgressTimezoneApi.monitorStatus).mockResolvedValue({
      state: "renderer_only",
      monitorIntervalMinutes: 15,
      restartRequired: false,
    });
    render(<CodexEgressTimezoneStatusCard onOpenCodexStatus={vi.fn()} />);
    expect(await screen.findByText("仅支持页面时区同步")).toBeInTheDocument();
    expect(screen.getByText(/重复刷新无法解除此限制/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "打开安全刷新" }),
    ).not.toBeInTheDocument();
  });

  beforeEach(() => {
    vi.mocked(codexEgressTimezoneApi.monitorStatus).mockReset();
    vi.mocked(codexEgressTimezoneApi.triggerAutomaticProbe).mockReset();
  });

  it("keeps automatic egress status visible in the Codex workspace", async () => {
    vi.mocked(codexEgressTimezoneApi.monitorStatus).mockResolvedValue({
      state: "ready",
      detectedTimezone: "Asia/Taipei",
      detectedEgressIp: "2407:cdc0:…",
      detectedAt: 2_000,
      lastAttemptAt: 2_000,
      lastTrigger: "periodic",
      nextCheckAt: 2_900,
      monitorIntervalMinutes: 15,
      restartRequired: false,
    });

    render(<CodexEgressTimezoneStatusCard />);

    expect(
      await screen.findByText("Codex 出口环境自动监测"),
    ).toBeInTheDocument();
    expect(screen.getByText("监测正常")).toBeInTheDocument();
    expect(screen.getByText(/Asia\/Taipei/)).toBeInTheDocument();
    expect(screen.getByText(/每 15 分钟/)).toBeInTheDocument();
  });

  it("shows a safe-refresh warning and can recheck without requiring CDP", async () => {
    const onOpenCodexStatus = vi.fn();
    const restartRequired = {
      state: "restart_required" as const,
      detectedTimezone: "America/Los_Angeles",
      detectedEgressIp: "8.8.8.…",
      detectedAt: 2_000,
      lastAttemptAt: 2_000,
      lastTrigger: "proxy_failure",
      nextCheckAt: 2_900,
      monitorIntervalMinutes: 15,
      restartRequired: true,
    };
    vi.mocked(codexEgressTimezoneApi.monitorStatus).mockResolvedValue(
      restartRequired,
    );
    vi.mocked(codexEgressTimezoneApi.triggerAutomaticProbe).mockResolvedValue(
      restartRequired,
    );

    render(
      <CodexEgressTimezoneStatusCard onOpenCodexStatus={onOpenCodexStatus} />,
    );
    expect(
      await screen.findByText("出口已变化，需要刷新 Codex"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "打开安全刷新" }));
    expect(onOpenCodexStatus).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "立即检测" }));
    await waitFor(() =>
      expect(
        codexEgressTimezoneApi.triggerAutomaticProbe,
      ).toHaveBeenCalledOnce(),
    );
    expect(screen.getByText(/检测本身不依赖 CDP/)).toBeInTheDocument();
  });
});

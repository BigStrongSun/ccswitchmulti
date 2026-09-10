import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useAppProxyConfig,
  useSetCapacityRetryEnabled,
  useUpdateAppProxyConfig,
} from "@/lib/query/proxy";
import { AutoFailoverConfigPanel } from "./AutoFailoverConfigPanel";

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: vi.fn(),
  useSetCapacityRetryEnabled: vi.fn(),
  useUpdateAppProxyConfig: vi.fn(),
}));

const config = {
  appType: "codex",
  enabled: true,
  autoFailoverEnabled: false,
  capacityRetryEnabled: true,
  maxRetries: 3,
  streamingFirstByteTimeout: 60,
  streamingIdleTimeout: 120,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 4,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 60,
  circuitErrorRateThreshold: 0.6,
  circuitMinRequests: 10,
};

describe("AutoFailoverConfigPanel capacity retry", () => {
  const setCapacity = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useAppProxyConfig).mockReturnValue({
      data: config,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useAppProxyConfig>);
    vi.mocked(useUpdateAppProxyConfig).mockReturnValue({
      mutateAsync: vi.fn(),
      isPending: false,
    } as unknown as ReturnType<typeof useUpdateAppProxyConfig>);
    vi.mocked(useSetCapacityRetryEnabled).mockReturnValue({
      mutateAsync: setCapacity.mockResolvedValue(undefined),
      isPending: false,
    } as unknown as ReturnType<typeof useSetCapacityRetryEnabled>);
  });

  it("shows the dedicated enabled switch only for Codex", () => {
    const { rerender } = render(<AutoFailoverConfigPanel appType="codex" />);
    expect(
      screen.getByRole("switch", { name: "模型容量错误自动续跑" }),
    ).toBeChecked();

    vi.mocked(useAppProxyConfig).mockReturnValue({
      data: { ...config, appType: "claude", capacityRetryEnabled: false },
      isLoading: false,
      error: null,
    } as ReturnType<typeof useAppProxyConfig>);
    rerender(<AutoFailoverConfigPanel appType="claude" />);
    expect(
      screen.queryByRole("switch", { name: "模型容量错误自动续跑" }),
    ).not.toBeInTheDocument();
  });

  it("persists only the capacity field immediately", async () => {
    render(<AutoFailoverConfigPanel appType="codex" />);
    fireEvent.change(screen.getByLabelText("最大重试次数"), {
      target: { value: "9" },
    });
    fireEvent.click(
      screen.getByRole("switch", { name: "模型容量错误自动续跑" }),
    );

    await waitFor(() =>
      expect(setCapacity).toHaveBeenCalledWith({ enabled: false }),
    );
  });
});

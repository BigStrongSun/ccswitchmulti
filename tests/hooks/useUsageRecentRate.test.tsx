import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { useUsageRecentRate, usageKeys } from "@/lib/query/usage";

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getUsageSummary: vi.fn(),
  },
}));

const getUsageSummaryMock = vi.mocked(usageApi.getUsageSummary);

function createWrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

const summary = (realTotalTokens: number) => ({
  totalRequests: 1,
  totalCost: "0",
  totalInputTokens: realTotalTokens,
  totalOutputTokens: 0,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
  successRate: 100,
  realTotalTokens,
  cacheHitRate: 0,
});

describe("useUsageRecentRate", () => {
  beforeEach(() => {
    vi.spyOn(Date, "now").mockReturnValue(1_700_000_300_000);
    getUsageSummaryMock.mockResolvedValue(summary(600));
  });

  afterEach(() => vi.restoreAllMocks());

  it("queries a rolling five-minute window and derives tokens per second", async () => {
    const { result } = renderHook(
      () =>
        useUsageRecentRate("claude", "provider", "model", {
          refetchInterval: false,
        }),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.data?.tokensPerSecond).toBe(2));
    expect(getUsageSummaryMock).toHaveBeenCalledWith(
      1_700_000_000,
      1_700_000_300,
      "claude",
      "provider",
      "model",
    );
  });

  it("resolves a fresh rolling window on each refetch", async () => {
    const { result } = renderHook(
      () =>
        useUsageRecentRate(undefined, undefined, undefined, {
          refetchInterval: false,
        }),
      { wrapper: createWrapper() },
    );
    await waitFor(() => expect(result.current.data).toBeDefined());

    vi.mocked(Date.now).mockReturnValue(1_700_000_600_000);
    getUsageSummaryMock.mockResolvedValueOnce(summary(300));
    await result.current.refetch();

    expect(getUsageSummaryMock).toHaveBeenLastCalledWith(
      1_700_000_300,
      1_700_000_600,
      undefined,
      undefined,
      undefined,
    );
    await waitFor(() => expect(result.current.data?.totalTokens).toBe(300));
  });

  it("keeps filters isolated in the query key", () => {
    expect(usageKeys.recentRate("claude", "a", "m")).not.toEqual(
      usageKeys.recentRate("codex", "a", "m"),
    );
    expect(usageKeys.recentRate()).toEqual([
      "usage",
      "recent-rate",
      null,
      null,
      null,
    ]);
  });

  it("uses backend real-total semantics, including cache tokens", async () => {
    getUsageSummaryMock.mockResolvedValueOnce({
      ...summary(0),
      realTotalTokens: 450,
    });
    const { result } = renderHook(
      () =>
        useUsageRecentRate("all", undefined, undefined, {
          refetchInterval: false,
        }),
      { wrapper: createWrapper() },
    );
    await waitFor(() => expect(result.current.data?.totalTokens).toBe(450));
    expect(getUsageSummaryMock).toHaveBeenLastCalledWith(
      1_700_000_000,
      1_700_000_300,
      undefined,
      undefined,
      undefined,
    );
  });

  it("leaves data undefined when the recent query fails", async () => {
    getUsageSummaryMock.mockRejectedValueOnce(new Error("offline"));
    const { result } = renderHook(
      () =>
        useUsageRecentRate(undefined, undefined, undefined, {
          refetchInterval: false,
        }),
      { wrapper: createWrapper() },
    );
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.data).toBeUndefined();
  });

  it("reports zero for an empty window", async () => {
    getUsageSummaryMock.mockResolvedValueOnce(summary(0));
    const { result } = renderHook(() => useUsageRecentRate(), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.data?.tokensPerSecond).toBe(0));
  });
});

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";

const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());
const useUsageRecentRateMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: (...args: unknown[]) =>
    useUsageSummaryByAppMock(...args),
  useUsageRecentRate: (...args: unknown[]) => useUsageRecentRateMock(...args),
}));

const summary = {
  totalRequests: 1,
  totalCost: "0.000001",
  totalInputTokens: 10,
  totalOutputTokens: 5,
  totalCacheCreationTokens: 3,
  totalCacheReadTokens: 2,
  successRate: 100,
  realTotalTokens: 20,
  cacheHitRate: 2 / 15,
};

const renderHero = (appType: string) =>
  render(
    <UsageHero
      range={{ preset: "today" }}
      appType={appType}
      refreshIntervalMs={0}
    />,
  );

describe("UsageHero cache-write availability", () => {
  beforeEach(() => {
    useUsageSummaryByAppMock.mockReset();
    useUsageRecentRateMock.mockReset();
    useUsageRecentRateMock.mockReturnValue({
      data: {
        inputTokens: 0,
        outputTokens: 0,
        inputTokensPerSecond: 0,
        outputTokensPerSecond: 0,
        windowSeconds: 300,
      },
      isLoading: false,
    });
  });

  it("marks mixed-protocol Pi usage as partial", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [{ appType: "pi", summary }],
      isLoading: false,
    });

    renderHero("pi");

    expect(
      screen.getByTitle("部分协议（如 OpenAI）不上报缓存写入，数值可能偏低"),
    ).toBeInTheDocument();
  });

  it("keeps Claude cache writes available", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [{ appType: "claude", summary }],
      isLoading: false,
    });

    renderHero("claude");

    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.queryByText("N/A")).not.toBeInTheDocument();
  });

  it("marks OpenAI-style cache writes unavailable", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [{ appType: "codex", summary }],
      isLoading: false,
    });

    renderHero("codex");

    expect(screen.getByText("N/A")).toBeInTheDocument();
    expect(
      screen.getByTitle("OpenAI 协议不区分缓存写入，仅上报缓存命中"),
    ).toBeInTheDocument();
  });

  it("shows the rolling token rate while historical totals are loading", () => {
    useUsageSummaryByAppMock.mockReturnValue({ data: [], isLoading: true });
    useUsageRecentRateMock.mockReturnValue({
      data: {
        inputTokens: 600,
        outputTokens: 300,
        inputTokensPerSecond: 2,
        outputTokensPerSecond: 1,
        windowSeconds: 300,
      },
      isLoading: false,
      isError: false,
    });

    renderHero("all");

    expect(screen.getByText("2 tok/s")).toBeInTheDocument();
    expect(screen.getByText("1 tok/s")).toBeInTheDocument();
  });

  it("does not show stale rate data after a failed refresh", () => {
    useUsageSummaryByAppMock.mockReturnValue({ data: [], isLoading: false });
    useUsageRecentRateMock.mockReturnValue({
      data: {
        inputTokens: 600,
        outputTokens: 300,
        inputTokensPerSecond: 2,
        outputTokensPerSecond: 1,
        windowSeconds: 300,
      },
      isLoading: false,
      isError: true,
    });

    renderHero("all");

    expect(screen.queryByText("2 tok/s")).not.toBeInTheDocument();
    expect(screen.queryByText("1 tok/s")).not.toBeInTheDocument();
    expect(screen.getAllByText("--").length).toBeGreaterThan(0);
  });
});

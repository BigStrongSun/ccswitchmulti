import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";

const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());

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
});

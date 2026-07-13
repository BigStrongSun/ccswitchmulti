import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexUsagePage } from "./CodexUsagePage";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import { useModelStats, useUsageSummary } from "@/lib/query/usage";

vi.mock("@/lib/query/subscription", () => ({
  useSubscriptionQuota: vi.fn(),
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageSummary: vi.fn(),
  useModelStats: vi.fn(),
}));

const mockedUseSubscriptionQuota = vi.mocked(useSubscriptionQuota);
const mockedUseUsageSummary = vi.mocked(useUsageSummary);
const mockedUseModelStats = vi.mocked(useModelStats);

/** 生成测试用的 quota query 返回值，避免每条用例重复声明 React Query 字段。 */
function mockQuotaResult(data: unknown, isFetching = false) {
  mockedUseSubscriptionQuota.mockReturnValue({
    data,
    isFetching,
    refetch: vi.fn(),
  } as any);
}

/** 提供空的本地统计 query，保证额度页测试只聚焦声明的场景。 */
function mockEmptyLocalUsage() {
  mockedUseUsageSummary.mockReturnValue({
    data: {
      totalRequests: 0,
      totalCost: "0",
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
      successRate: 0,
      realTotalTokens: 0,
      cacheHitRate: 0,
    },
    isFetching: false,
    refetch: vi.fn(),
  } as any);
  mockedUseModelStats.mockReturnValue({
    data: [],
    refetch: vi.fn(),
  } as any);
}

describe("CodexUsagePage", () => {
  beforeEach(() => {
    mockedUseSubscriptionQuota.mockReset();
    mockedUseUsageSummary.mockReset();
    mockedUseModelStats.mockReset();
    mockEmptyLocalUsage();
  });

  it("renders Codex usage windows and banked reset credits", () => {
    mockQuotaResult({
      tool: "codex",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [
        {
          name: "five_hour",
          utilization: 36,
          resetsAt: "2026-07-06T15:00:00.000Z",
        },
        {
          name: "seven_day",
          utilization: 68,
          resetsAt: "2026-07-12T15:00:00.000Z",
        },
      ],
      extraUsage: null,
      resetCredits: {
        availableCount: 2,
        credits: [
          {
            resetType: "rate_limit",
            status: "available",
            expiresAt: "2026-07-08T15:00:00.000Z",
            title: "Banked reset",
          },
        ],
      },
      resetCreditsError: "one credit missing expiry",
      error: null,
      queriedAt: 1783300000000,
    });

    render(<CodexUsagePage />);

    expect(screen.getByText("Codex 用量与重置额度")).toBeInTheDocument();
    expect(screen.getByText("使用引导")).toBeInTheDocument();
    expect(screen.getByText("从 Codex 工具栏进入")).toBeInTheDocument();
    expect(screen.getByText("按窗口和到期时间决策")).toBeInTheDocument();
    expect(screen.getByText("5 小时窗口")).toBeInTheDocument();
    expect(screen.getByText("每周窗口")).toBeInTheDocument();
    expect(screen.getByText("已存 reset 额度")).toBeInTheDocument();
    expect(screen.getByText("2 个可用")).toBeInTheDocument();
    expect(screen.getByText("Banked reset")).toBeInTheDocument();
    expect(screen.getByText(/Reset credit 明细读取不完整/)).toBeInTheDocument();
    expect(screen.getByText("本地消耗节奏")).toBeInTheDocument();
    expect(
      screen.getByText(/暂无可分析的本地 Codex 使用记录/),
    ).toBeInTheDocument();
  });

  it("shows a visible problem state when Codex credentials are unavailable", () => {
    mockQuotaResult({
      tool: "codex",
      credentialStatus: "not_found",
      credentialMessage: "未找到 Codex 登录文件",
      success: false,
      tiers: [],
      extraUsage: null,
      resetCredits: null,
      resetCreditsError: null,
      error: null,
      queriedAt: null,
    });

    render(<CodexUsagePage />);

    expect(screen.getByText("Codex 登录不可用")).toBeInTheDocument();
    expect(screen.getByText("未找到 Codex 登录文件")).toBeInTheDocument();
  });

  it("renders token pace and model distribution without treating it as official quota", () => {
    mockQuotaResult({
      tool: "codex",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [
        {
          name: "five_hour",
          utilization: 92,
          resetsAt: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
        },
      ],
      extraUsage: null,
      resetCredits: null,
      resetCreditsError: null,
      error: null,
      queriedAt: Date.now(),
    });
    mockedUseUsageSummary.mockReturnValue({
      data: {
        totalRequests: 8,
        totalCost: "0",
        totalInputTokens: 72000,
        totalOutputTokens: 18000,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 9000,
        successRate: 100,
        realTotalTokens: 99000,
        cacheHitRate: 0.4,
      },
      isFetching: false,
      refetch: vi.fn(),
    } as any);
    mockedUseModelStats.mockReturnValue({
      data: [
        {
          model: "gpt-5.6-terra",
          requestCount: 5,
          totalTokens: 80000,
          totalCost: "0",
          avgCostPerRequest: "0",
        },
      ],
      refetch: vi.fn(),
    } as any);

    render(<CodexUsagePage />);

    expect(screen.getByText("当前消耗速度")).toBeInTheDocument();
    expect(screen.getByText("近 7 天模型分布")).toBeInTheDocument();
    expect(screen.getByText("gpt-5.6-terra")).toBeInTheDocument();
    expect(screen.getByText("本地日志口径")).toBeInTheDocument();
    expect(screen.getByText(/按当前节奏可能提前耗尽/)).toBeInTheDocument();
  });
});

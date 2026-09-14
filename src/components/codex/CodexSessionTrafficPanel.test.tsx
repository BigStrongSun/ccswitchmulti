import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodexSessionTrafficPanel } from "./CodexSessionTrafficPanel";

describe("CodexSessionTrafficPanel", () => {
  it("keeps session usage separate, exposes token dimensions, and marks missing usage unknown", () => {
    render(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "C:/Users/test/.codex",
          totalAgents: 4,
          scannedHistoryAgents: 4,
          inRangeAgents: 3,
          unknownRangeAgents: 1,
          observedUsageAgents: 2,
          missingUsageAgents: 1,
          historyTruncated: false,
          proxyUsageIncluded: false,
          agents: [],
          modelStats: [
            {
              model: "gpt-5.6-sol",
              agentCount: 3,
              observedUsageAgents: 2,
              missingUsageAgents: 1,
              requestCount: 8,
              inputTokens: 120,
              cacheReadTokens: 30,
              cacheCreationTokens: 0,
              outputTokens: 60,
              totalTokens: 210,
              totalCost: "0",
            },
            {
              model: "unreported-model",
              agentCount: 1,
              observedUsageAgents: 0,
              missingUsageAgents: 1,
              requestCount: 0,
              inputTokens: 0,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 0,
              totalTokens: 0,
              totalCost: "0",
            },
          ],
        }}
        isLoading={false}
        error={null}
        rangeLabel="今日（本地日历日）"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );

    expect(screen.getByText("今日（本地日历日）子 Agent 会话流量")).toBeInTheDocument();
    expect(screen.getByText(/请求统计与会话统计独立，不能相加。/)).toBeInTheDocument();
    expect(screen.getByText("输入 Tokens")).toBeInTheDocument();
    expect(screen.getByText("缓存读取")).toBeInTheDocument();
    expect(screen.getByText("输出 Tokens")).toBeInTheDocument();
    expect(screen.getAllByText("未采集")).toHaveLength(5);
    expect(screen.getByText(/请求耗时：会话历史未采集可信耗时，不显示为 0ms。/)).toBeInTheDocument();
  });
});

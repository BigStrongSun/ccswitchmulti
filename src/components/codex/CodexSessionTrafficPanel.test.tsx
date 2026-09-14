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
          parentGroups: [
            {
              parentSessionId: "observed-parent",
              childSessionCount: 1,
              observedUsageChildren: 1,
              missingUsageChildren: 0,
              childRequestCount: 3,
              childInputTokens: 80,
              childCacheReadTokens: 20,
              childCacheCreationTokens: 0,
              childOutputTokens: 40,
              childTotalTokens: 140,
              parentDirectUsage: {
                requestCount: 2,
                inputTokens: 50,
                cacheReadTokens: 10,
                cacheCreationTokens: 0,
                outputTokens: 30,
                totalTokens: 90,
              },
              parentDirectUsageSource: "session_sync",
            },
            {
              parentSessionId: "missing-parent",
              childSessionCount: 1,
              observedUsageChildren: 0,
              missingUsageChildren: 1,
              childRequestCount: 0,
              childInputTokens: 0,
              childCacheReadTokens: 0,
              childCacheCreationTokens: 0,
              childOutputTokens: 0,
              childTotalTokens: 0,
              parentDirectUsage: null,
              parentDirectUsageSource: "none",
              parentUsageStatus: "unknown_may_overlap",
            },
          ],
          modelStats: [
            {
              model: "gpt-5.6-sol",
              agentCount: 2,
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

    expect(
      screen.getByText("今日（本地日历日）子 Agent 会话流量"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/请求统计与会话统计独立，不能相加。/),
    ).toBeInTheDocument();
    expect(screen.getByText("所选范围内有用量证据")).toBeInTheDocument();
    expect(
      screen.getByText(/范围无法确认.*已包含在未采集用量中/),
    ).toBeInTheDocument();
    expect(screen.getByText("非缓存输入")).toBeInTheDocument();
    expect(screen.getByText("缓存读取")).toBeInTheDocument();
    expect(screen.getByText("输出 Tokens")).toBeInTheDocument();
    expect(screen.getAllByText("未采集")).toHaveLength(5);
    expect(
      screen.getByText(/父直接同步请求 2；非缓存输入 50/),
    ).toBeInTheDocument();
    expect(screen.getByText("子用量未采集")).toBeInTheDocument();
    expect(
      screen.getByText("父同步记录可能包含子用量，暂不计入"),
    ).toBeInTheDocument();
    expect(screen.getByText("gpt-5.6-sol").parentElement).toHaveTextContent(
      "3(1 未采集)",
    );
    expect(
      screen.getByText(/请求耗时：会话历史未采集可信耗时，不显示为 0ms。/),
    ).toBeInTheDocument();
  });
});

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodexSessionTrafficPanel } from "./CodexSessionTrafficPanel";

describe("CodexSessionTrafficPanel", () => {
  it("labels an empty successful collector snapshot without inventing traffic", () => {
    render(
      <CodexSessionTrafficPanel
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
        collectionStatus={{
          revision: 4,
          phase: "idle",
          lastStartedAt: 1_700_000_000,
          lastCompletedAt: 1_700_000_004,
          lastSuccessAt: 1_700_000_004,
          imported: 0,
          deferred: 0,
          errorsCount: 0,
          lastErrorSummary: null,
          nextRunAt: 1_700_000_060,
          intervalSecs: 60,
        }}
      />,
    );

    expect(screen.getByText("后台采集空闲")).toBeInTheDocument();
    expect(screen.getByText(/最近成功.*暂无新增/)).toBeInTheDocument();
    expect(screen.queryByText("采集正常")).not.toBeInTheDocument();
  });

  it("surfaces a degraded collector with its deferred work and error summary", () => {
    render(
      <CodexSessionTrafficPanel
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
        collectionStatus={{
          revision: 8,
          phase: "degraded",
          lastStartedAt: 1_700_000_000,
          lastCompletedAt: 1_700_000_004,
          lastSuccessAt: 1_699_999_940,
          imported: 2,
          deferred: 3,
          errorsCount: 1,
          lastErrorSummary: "rollout 文件仍在写入",
          nextRunAt: 1_700_000_060,
          intervalSecs: 60,
        }}
      />,
    );

    expect(screen.getByText("后台采集降级")).toBeInTheDocument();
    expect(screen.getByText(/待处理 3/)).toBeInTheDocument();
    expect(
      screen.getByText(/错误摘要：rollout 文件仍在写入/),
    ).toBeInTheDocument();
  });

  it("does not present a not-started collector as an empty successful run", () => {
    render(
      <CodexSessionTrafficPanel
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
        collectionStatus={{
          revision: 0,
          phase: "not_started",
          lastStartedAt: null,
          lastCompletedAt: null,
          lastSuccessAt: null,
          imported: 0,
          deferred: 0,
          errorsCount: 0,
          lastErrorSummary: null,
          nextRunAt: null,
          intervalSecs: 60,
        }}
      />,
    );

    expect(screen.getByText("后台采集尚未启动")).toBeInTheDocument();
    expect(
      screen.getByText(/尚无成功采集；可点击立即同步/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/本轮暂无新增/)).not.toBeInTheDocument();
  });

  it("calls the existing manual sync path exactly once per click", () => {
    const onSync = vi.fn();
    render(
      <CodexSessionTrafficPanel
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={onSync}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "立即同步会话用量" }));
    expect(onSync).toHaveBeenCalledTimes(1);
  });

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
      screen.getByText(
        /会话范围无法确认，未计入所选范围；它们不属于未采集用量/,
      ),
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
      screen.getByText(/请求耗时：会话记录未采集可信耗时，不显示为 0ms。/),
    ).toBeInTheDocument();
  });
});

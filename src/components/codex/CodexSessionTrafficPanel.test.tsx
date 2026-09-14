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
          stateDbPath: "C:/private/session-usage.sqlite",
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
    expect(screen.getByText("已观测请求")).toBeInTheDocument();
    expect(
      screen.getByText(
        /会话范围无法确认，未计入所选范围；它们不属于未采集用量/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("非缓存输入")).toBeInTheDocument();
    expect(screen.getByText("缓存读取")).toBeInTheDocument();
    expect(screen.getByText("输出 Tokens")).toBeInTheDocument();
    expect(screen.getAllByText("未采集")).toHaveLength(5);
    fireEvent.mouseDown(screen.getByRole("tab", { name: "按任务" }), {
      button: 0,
    });
    expect(
      screen.getByText(/已记录父直接用量，但不与子用量相加/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/父同步记录可能包含子用量，暂不计入/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/请求耗时：会话记录未采集可信耗时，不显示为 0ms。/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/数据来源：CCSM 本地会话用量账/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/状态库：|未定位/)).not.toBeInTheDocument();
    expect(
      screen.queryByText("C:/private/session-usage.sqlite"),
    ).not.toBeInTheDocument();
  });

  it("shows an observed-only overview and keeps unpriced observed work out of a complete-cost claim", () => {
    render(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "",
          totalAgents: 2,
          agents: [],
          observedUsageAgents: 2,
          missingUsageAgents: 0,
          modelStats: [
            {
              model: "priced-model",
              agentCount: 1,
              observedUsageAgents: 1,
              missingUsageAgents: 0,
              requestCount: 3,
              inputTokens: 120,
              cacheReadTokens: 40,
              cacheCreationTokens: 0,
              outputTokens: 20,
              totalTokens: 180,
              totalCost: "0.0125",
            },
            {
              model: "unpriced-model",
              agentCount: 1,
              observedUsageAgents: 1,
              missingUsageAgents: 0,
              requestCount: 2,
              inputTokens: 80,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 10,
              totalTokens: 90,
              totalCost: "0",
            },
          ],
        }}
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );

    expect(
      screen.getByText("已观测子任务 Tokens").parentElement,
    ).toHaveTextContent("270");
    expect(screen.getByText("已观测请求").parentElement).toHaveTextContent("5");
    expect(screen.getByText("已知价表估算").parentElement).toHaveTextContent(
      "约 $0.0125",
    );
    expect(screen.getByText(/部分可计价.*未定价/)).toBeInTheDocument();
    expect(screen.queryByText("总成本")).not.toBeInTheDocument();
  });

  it("does not turn loading, absent, or missing-only snapshots into observed zero traffic", () => {
    const { rerender } = render(
      <CodexSessionTrafficPanel
        isLoading
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );
    expect(
      screen.getByText("已观测子任务 Tokens").parentElement,
    ).toHaveTextContent("加载中");
    fireEvent.mouseDown(screen.getByRole("tab", { name: "按任务" }), {
      button: 0,
    });
    expect(screen.getByText("正在读取本期任务关系...")).toBeInTheDocument();

    rerender(
      <CodexSessionTrafficPanel
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );
    expect(
      screen.getByText("已观测子任务 Tokens").parentElement,
    ).toHaveTextContent("—");
    expect(screen.getAllByText("暂无已采集快照")).toHaveLength(2);

    rerender(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "",
          totalAgents: 1,
          observedUsageAgents: 0,
          missingUsageAgents: 1,
          agents: [],
          modelStats: [
            {
              model: "waiting-model",
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
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );
    expect(
      screen.getByText("已观测子任务 Tokens").parentElement,
    ).toHaveTextContent("—");
    expect(screen.getAllByText(/仅有未采集会话/)).toHaveLength(2);
  });

  it("sorts and filters the model view without treating missing usage as zero", () => {
    render(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "",
          totalAgents: 3,
          agents: [],
          modelStats: [
            {
              model: "alpha-unpriced",
              agentCount: 1,
              observedUsageAgents: 1,
              missingUsageAgents: 0,
              requestCount: 1,
              inputTokens: 20,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 2,
              totalTokens: 22,
              totalCost: "0",
            },
            {
              model: "zeta-priced",
              agentCount: 1,
              observedUsageAgents: 1,
              missingUsageAgents: 0,
              requestCount: 4,
              inputTokens: 100,
              cacheReadTokens: 20,
              cacheCreationTokens: 0,
              outputTokens: 10,
              totalTokens: 130,
              totalCost: "0.02",
            },
          ],
        }}
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );

    fireEvent.mouseDown(screen.getByRole("tab", { name: "按模型" }), {
      button: 0,
    });
    fireEvent.click(screen.getByRole("button", { name: "按模型名排序" }));
    expect(screen.getByTestId("model-rows").textContent).toMatch(
      /alpha-unpriced[\s\S]*zeta-priced/,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "筛选模型" }));
    fireEvent.click(screen.getByRole("option", { name: "仅未定价" }));
    expect(screen.getByText("alpha-unpriced")).toBeInTheDocument();
    expect(screen.queryByText("zeta-priced")).not.toBeInTheDocument();
    expect(screen.getByText(/未定价\/待核验/)).toBeInTheDocument();
  });

  it("groups only explicit parents and opens a detail sheet with source and unknown attribution", () => {
    render(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "",
          totalAgents: 3,
          agents: [
            {
              sessionId: "child-a",
              title: "可追溯子任务",
              parentThreadId: "explicit-parent",
              primaryModel: "gpt-detail",
              models: ["gpt-detail"],
              requestCount: 2,
              inputTokens: 50,
              cacheReadTokens: 10,
              cacheCreationTokens: 0,
              outputTokens: 20,
              totalTokens: 80,
              totalCost: "0.01",
              usageStatus: "observed",
              usageSource: "session_sync",
            },
            {
              sessionId: "child-b",
              title: "同一父任务的子任务",
              parentThreadId: "explicit-parent",
              models: [],
              requestCount: 1,
              inputTokens: 6,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 4,
              totalTokens: 10,
              totalCost: "0.002",
              usageStatus: "observed",
              usageSource: "rollout",
            },
            {
              sessionId: "unrelated",
              title: "未关联会话",
              models: [],
              requestCount: 0,
              inputTokens: 0,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 0,
              totalTokens: 0,
              totalCost: "0",
              usageStatus: "missing",
              usageSource: "none",
            },
          ],
          modelStats: [],
          parentGroups: [
            {
              parentSessionId: "explicit-parent",
              childSessionCount: 2,
              observedUsageChildren: 2,
              missingUsageChildren: 0,
              childRequestCount: 3,
              childInputTokens: 56,
              childCacheReadTokens: 10,
              childCacheCreationTokens: 0,
              childOutputTokens: 24,
              childTotalTokens: 90,
              parentDirectUsage: null,
              parentDirectUsageSource: "none",
              parentUsageStatus: "unknown_may_overlap",
            },
          ],
        }}
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );

    fireEvent.mouseDown(screen.getByRole("tab", { name: "按任务" }), {
      button: 0,
    });
    expect(screen.getByText(/父 explicit-parent/)).toBeInTheDocument();
    expect(screen.getByText(/未关联会话（1）/)).toBeInTheDocument();
    expect(screen.queryByText(/^父模型：/)).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "查看 explicit-parent 详情" }),
    );

    expect(screen.getByRole("dialog")).toHaveTextContent(
      "本地会话证据（来源混合）",
    );
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "父同步记录可能包含子用量，暂不计入",
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("可追溯子任务");
  });

  it("keeps missing model, child, and parent-group detail values unknown even with a legacy parent direct record", () => {
    render(
      <CodexSessionTrafficPanel
        stats={{
          codexHome: "",
          totalAgents: 1,
          observedUsageAgents: 0,
          missingUsageAgents: 1,
          agents: [
            {
              sessionId: "missing-child",
              title: "未采集子任务",
              parentThreadId: "legacy-parent",
              models: [],
              requestCount: 0,
              inputTokens: 0,
              cacheReadTokens: 0,
              cacheCreationTokens: 0,
              outputTokens: 0,
              totalTokens: 0,
              totalCost: "0",
              usageStatus: "missing",
              usageSource: "none",
            },
          ],
          modelStats: [
            {
              model: "missing-model",
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
          parentGroups: [
            {
              parentSessionId: "legacy-parent",
              childSessionCount: 1,
              observedUsageChildren: 0,
              missingUsageChildren: 1,
              childRequestCount: 0,
              childInputTokens: 0,
              childCacheReadTokens: 0,
              childCacheCreationTokens: 0,
              childOutputTokens: 0,
              childTotalTokens: 0,
              parentDirectUsage: {
                requestCount: 5,
                inputTokens: 10,
                cacheReadTokens: 4,
                cacheCreationTokens: 0,
                outputTokens: 3,
                totalTokens: 17,
              },
              parentDirectUsageSource: "session_sync",
              parentUsageStatus: "unknown_may_overlap",
            },
          ],
        }}
        isLoading={false}
        error={null}
        rangeLabel="今日"
        isSyncing={false}
        onSync={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /missing-model/ }));
    expect(screen.getByRole("dialog")).toHaveTextContent("尚无用量证据");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("Token 构成");
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));

    fireEvent.mouseDown(screen.getByRole("tab", { name: "按任务" }), {
      button: 0,
    });
    expect(screen.getByText(/子任务用量未采集/)).toBeInTheDocument();
    expect(
      screen.getByText(/父同步记录可能包含子用量，暂不计入/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/已记录父直接用量/)).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "查看 legacy-parent 详情" }),
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("尚无用量证据");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("Token 构成");
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    fireEvent.click(
      screen.getByRole("button", { name: "查看 missing-child 详情" }),
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("尚无用量证据");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("Token 构成");
  });
});

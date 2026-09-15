import { useEffect, useState } from "react";
import { CodexSessionTrafficPanel } from "./CodexSessionTrafficPanel";
import { Button } from "@/components/ui/button";
import type {
  CodexSubagentUsageAgent,
  CodexSubagentUsageStats,
  SessionCollectionStatus,
} from "@/types/usage";

// Synthetic counts are deliberately consistent across model, task and overview views.
const sampleAgents: CodexSubagentUsageAgent[] = [
  {
    sessionId: "fixture-terra-implementation",
    title: "实现会话增量采集",
    agentRole: "实现",
    parentThreadId: "fixture-parent-collection",
    primaryModel: "gpt-5.6-terra",
    models: ["gpt-5.6-terra"],
    requestCount: 12,
    inputTokens: 28000,
    cacheReadTokens: 48000,
    cacheCreationTokens: 0,
    outputTokens: 8200,
    totalTokens: 84200,
    totalCost: "0.184",
    usageStatus: "observed",
    usageSource: "session_sync",
  },
  {
    sessionId: "fixture-qwen-tests",
    title: "补齐断点恢复测试",
    agentRole: "测试",
    parentThreadId: "fixture-parent-collection",
    primaryModel: "qwen3.8",
    models: ["qwen3.8"],
    requestCount: 8,
    inputTokens: 18000,
    cacheReadTokens: 12000,
    cacheCreationTokens: 0,
    outputTokens: 4200,
    totalTokens: 34200,
    totalCost: "0.032",
    usageStatus: "observed",
    usageSource: "session_sync",
  },
  {
    sessionId: "fixture-terra-review",
    title: "审查数据边界",
    agentRole: "审查",
    primaryModel: "gpt-5.6-terra",
    models: ["gpt-5.6-terra"],
    requestCount: 4,
    inputTokens: 6000,
    cacheReadTokens: 10000,
    cacheCreationTokens: 0,
    outputTokens: 1600,
    totalTokens: 17600,
    totalCost: "0.037",
    usageStatus: "observed",
    usageSource: "session_sync",
  },
];
const missingAgent: CodexSubagentUsageAgent = {
  sessionId: "fixture-missing-review",
  title: "等待用量记录的子任务",
  parentThreadId: "fixture-parent-review",
  primaryModel: "glm5.3-flash",
  models: ["glm5.3-flash"],
  requestCount: 0,
  inputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  outputTokens: 0,
  totalTokens: 0,
  totalCost: "0",
  usageStatus: "missing",
  usageSource: "none",
};

function sampleStats(withMissing: boolean): CodexSubagentUsageStats {
  return {
    codexHome: "",
    totalAgents: withMissing ? 4 : 3,
    scannedHistoryAgents: withMissing ? 5 : 3,
    inRangeAgents: 3,
    unknownRangeAgents: withMissing ? 1 : 0,
    observedUsageAgents: 3,
    missingUsageAgents: withMissing ? 1 : 0,
    historyTruncated: false,
    proxyUsageIncluded: false,
    agents: withMissing ? [...sampleAgents, missingAgent] : sampleAgents,
    modelStats: [
      {
        model: "gpt-5.6-terra",
        agentCount: 2,
        observedUsageAgents: 2,
        missingUsageAgents: 0,
        requestCount: 16,
        inputTokens: 34000,
        cacheReadTokens: 58000,
        cacheCreationTokens: 0,
        outputTokens: 9800,
        totalTokens: 101800,
        totalCost: "0.221",
      },
      {
        model: "qwen3.8",
        agentCount: 1,
        observedUsageAgents: 1,
        missingUsageAgents: 0,
        requestCount: 8,
        inputTokens: 18000,
        cacheReadTokens: 12000,
        cacheCreationTokens: 0,
        outputTokens: 4200,
        totalTokens: 34200,
        totalCost: "0.032",
      },
      ...(withMissing
        ? [
            {
              model: "glm5.3-flash",
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
          ]
        : []),
    ],
    parentGroups: [
      {
        parentSessionId: "fixture-parent-collection",
        childSessionCount: 2,
        observedUsageChildren: 2,
        missingUsageChildren: 0,
        childRequestCount: 20,
        childInputTokens: 46000,
        childCacheReadTokens: 60000,
        childCacheCreationTokens: 0,
        childOutputTokens: 12400,
        childTotalTokens: 118400,
        parentDirectUsage: null,
        parentDirectUsageSource: "none",
        parentUsageStatus: "unknown_may_overlap",
      },
      ...(withMissing
        ? [
            {
              parentSessionId: "fixture-parent-review",
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
              parentDirectUsageSource: "none" as const,
              parentUsageStatus: "not_observed" as const,
            },
          ]
        : []),
    ],
  };
}

/** Interactive visual-only harness. Never wired into production app navigation. */
export function CodexSessionTrafficPanelFixture() {
  const [scenario, setScenario] = useState("complete");
  const [dark, setDark] = useState(false);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    return () => document.documentElement.classList.remove("dark");
  }, [dark]);
  const [syncs, setSyncs] = useState(0);
  const timestamp = Math.floor(Date.now() / 1000);
  const status: SessionCollectionStatus = {
    revision: syncs + 1,
    phase:
      scenario === "error"
        ? "degraded"
        : scenario === "empty"
          ? "not_started"
          : "idle",
    lastStartedAt: scenario === "empty" ? null : timestamp - 20,
    lastCompletedAt: scenario === "empty" ? null : timestamp - 16,
    lastSuccessAt: scenario === "empty" ? null : timestamp - 16,
    imported: syncs ? 1 : 0,
    deferred: scenario === "missing" ? 1 : 0,
    errorsCount: scenario === "error" ? 1 : 0,
    lastErrorSummary:
      scenario === "error"
        ? "样本：部分记录暂时无法读取，已保留上次成功结果。"
        : null,
    nextRunAt: scenario === "empty" ? null : timestamp + 44,
    intervalSecs: 60,
  };
  const stats =
    scenario === "empty"
      ? {
          ...sampleStats(false),
          totalAgents: 0,
          scannedHistoryAgents: 0,
          inRangeAgents: 0,
          observedUsageAgents: 0,
          agents: [],
          parentGroups: [],
          modelStats: [],
        }
      : sampleStats(scenario !== "complete");
  return (
    <div className={dark ? "dark" : ""}>
      <main className="min-h-screen bg-background p-4 text-foreground sm:p-8">
        <div className="mx-auto max-w-6xl space-y-5">
          <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4">
            <div>
              <p className="text-xs font-medium text-amber-700 dark:text-amber-400">
                交互预览 · 全部为模拟数据，不代表真实费用或已安装界面
              </p>
              <h1 className="mt-2 text-lg font-semibold">
                Codex / 状态 / 流量
              </h1>
            </div>
            <div className="flex flex-wrap gap-2" aria-label="预览样本控制">
              {[
                ["complete", "完整样本"],
                ["missing", "缺失样本"],
                ["error", "异常样本"],
                ["empty", "首次使用"],
              ].map(([key, label]) => (
                <Button
                  key={key}
                  size="sm"
                  variant={scenario === key ? "default" : "outline"}
                  aria-pressed={scenario === key}
                  onClick={() => setScenario(key)}
                >
                  {label}
                </Button>
              ))}
              <Button
                size="sm"
                variant="outline"
                aria-pressed={dark}
                onClick={() => setDark(!dark)}
              >
                {dark ? "切换浅色" : "切换深色"}
              </Button>
            </div>
          </header>
          <CodexSessionTrafficPanel
            rangeLabel="今日"
            stats={stats}
            isLoading={false}
            error={null}
            isSyncing={false}
            onSync={() => setSyncs(syncs + 1)}
            collectionStatus={status}
            syncMessage={
              syncs ? `预览同步 ${syncs} 次；未调用真实采集接口` : undefined
            }
          />
        </div>
      </main>
    </div>
  );
}

export const CodexSessionCollectionStatusFixtureGallery =
  CodexSessionTrafficPanelFixture;

import { CodexSessionTrafficPanel } from "./CodexSessionTrafficPanel";

/** Manual Vite preview fixture: conspicuously synthetic, never wired into app navigation. */
export function CodexSessionTrafficPanelFixture() {
  return (
    <main className="min-h-screen bg-background p-6">
      <p className="mb-3 text-sm font-semibold text-amber-700">
        FIXTURE — synthetic local evidence, not live usage
      </p>
      <CodexSessionTrafficPanel
        rangeLabel="今日（本地日历日）"
        isLoading={false}
        error={null}
        isSyncing={false}
        onSync={() => undefined}
        collectionStatus={{
          revision: 7,
          phase: "idle",
          lastStartedAt: 1_700_000_000,
          lastCompletedAt: 1_700_000_004,
          lastSuccessAt: 1_700_000_004,
          imported: 2,
          deferred: 1,
          errorsCount: 0,
          lastErrorSummary: null,
          nextRunAt: 1_700_000_060,
          intervalSecs: 60,
        }}
        stats={{
          codexHome: "C:/fixture/.codex",
          totalAgents: 3,
          scannedHistoryAgents: 3,
          inRangeAgents: 3,
          unknownRangeAgents: 0,
          observedUsageAgents: 2,
          missingUsageAgents: 1,
          historyTruncated: false,
          proxyUsageIncluded: false,
          agents: [],
          parentGroups: [
            {
              parentSessionId: "parent-observed-fixture",
              childSessionCount: 2,
              observedUsageChildren: 2,
              missingUsageChildren: 0,
              childRequestCount: 6,
              childInputTokens: 1440,
              childCacheReadTokens: 960,
              childCacheCreationTokens: 0,
              childOutputTokens: 384,
              childTotalTokens: 2784,
              parentDirectUsage: {
                requestCount: 3,
                inputTokens: 360,
                cacheReadTokens: 120,
                cacheCreationTokens: 0,
                outputTokens: 96,
                totalTokens: 576,
              },
              parentDirectUsageSource: "session_sync",
            },
            {
              parentSessionId: "parent-missing-fixture",
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
              missingUsageAgents: 0,
              requestCount: 6,
              inputTokens: 1440,
              cacheReadTokens: 960,
              cacheCreationTokens: 0,
              outputTokens: 384,
              totalTokens: 2784,
              totalCost: "0.0123",
            },
            {
              model: "unknown-upstream",
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
      />
    </main>
  );
}

/** Synthetic gallery for visual acceptance of explicit collector states. */
export function CodexSessionCollectionStatusFixtureGallery() {
  const common = {
    isLoading: false,
    error: null,
    rangeLabel: "验收样本",
    isSyncing: false,
    onSync: () => undefined,
  };
  return (
    <main className="min-h-screen space-y-4 bg-background p-6">
      <p className="text-sm font-semibold text-amber-700">
        FIXTURE — synthetic collector states, not live usage
      </p>
      <CodexSessionTrafficPanel
        {...common}
        collectionStatus={{
          revision: 11,
          phase: "idle",
          lastStartedAt: 1_700_000_000,
          lastCompletedAt: 1_700_000_004,
          lastSuccessAt: 1_700_000_004,
          imported: 5,
          deferred: 0,
          errorsCount: 0,
          lastErrorSummary: null,
          nextRunAt: 1_700_000_060,
          intervalSecs: 60,
        }}
      />
      <CodexSessionTrafficPanel
        {...common}
        collectionStatus={{
          revision: 12,
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
      />
      <CodexSessionTrafficPanel
        {...common}
        collectionStatus={{
          revision: 13,
          phase: "degraded",
          lastStartedAt: 1_700_000_000,
          lastCompletedAt: 1_700_000_004,
          lastSuccessAt: 1_699_999_940,
          imported: 1,
          deferred: 3,
          errorsCount: 1,
          lastErrorSummary: "fixture: rollout file is still being written",
          nextRunAt: 1_700_000_060,
          intervalSecs: 60,
        }}
      />
    </main>
  );
}

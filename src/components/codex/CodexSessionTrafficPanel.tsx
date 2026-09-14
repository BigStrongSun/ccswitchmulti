import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { CodexSubagentUsageStats } from "@/types/usage";

type Props = {
  stats?: CodexSubagentUsageStats;
  isLoading: boolean;
  error: unknown;
  rangeLabel: string;
  isSyncing: boolean;
  onSync: () => void;
  syncMessage?: string | null;
};

function formatTokenCount(value: number): string {
  return value.toLocaleString();
}

function formatEstimatedCost(value: string): string {
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return "未定价/待核验";
  return `约 $${parsed.toFixed(parsed > 0 && parsed < 0.01 ? 6 : 4)}`;
}

/**
 * Bounded Codex history/session-sync observability. This intentionally does
 * not merge proxy records: request traffic and local session evidence answer
 * different questions and must never be added together.
 */
export function CodexSessionTrafficPanel({
  stats,
  isLoading,
  error,
  rangeLabel,
  isSyncing,
  onSync,
  syncMessage,
}: Props) {
  const scanned = stats?.scannedHistoryAgents ?? stats?.totalAgents ?? 0;
  const observed = stats?.observedUsageAgents ?? 0;
  const missing = stats?.missingUsageAgents ?? 0;

  return (
    <section className="rounded-lg border border-violet-200 bg-violet-50/70 p-4 dark:border-violet-700/40 dark:bg-violet-950/10">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold text-foreground dark:text-slate-100">
            {rangeLabel}子 Agent 会话流量
          </h3>
          <p className="mt-1 text-sm leading-6 text-muted-foreground">
            只基于本地 Codex 历史与已同步的 <code>codex_session</code>{" "}
            用量；请求统计与会话统计独立，不能相加。
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={onSync}
          disabled={isSyncing}
          className="gap-2 border-violet-300 bg-background/70 text-violet-700 hover:bg-violet-100 dark:border-violet-500/50 dark:bg-violet-500/10 dark:text-violet-100 dark:hover:bg-violet-500/20"
        >
          <RefreshCw className={`h-4 w-4 ${isSyncing ? "animate-spin" : ""}`} />
          同步会话用量
        </Button>
      </div>

      {syncMessage ? (
        <div className="mt-3 rounded-md border border-violet-200 bg-background/70 px-3 py-2 text-xs text-muted-foreground dark:border-violet-700/50 dark:bg-violet-950/30 dark:text-violet-100">
          {syncMessage}
        </div>
      ) : null}
      {error ? (
        <div className="mt-3 rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-700/50 dark:bg-rose-950/30 dark:text-rose-100">
          子 Agent 用量读取失败：
          {error instanceof Error ? error.message : String(error)}
        </div>
      ) : null}
      {stats?.skippedReason ? (
        <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-700/50 dark:bg-amber-950/30 dark:text-amber-100">
          Codex 历史读取跳过：{stats.skippedReason}
        </div>
      ) : null}

      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        <Summary
          label="本次有界扫描"
          value={`${scanned} 个`}
          detail="仅本次有限历史列表，不代表全库"
        />
        <Summary
          label="所选范围内有用量证据"
          value={`${observed} 个`}
          detail="数值来自会话同步或经验证的 rollout 事件"
        />
        <Summary
          label="未采集用量"
          value={`${missing} 个`}
          detail="不把兼容零值当作 0 Token / $0"
        />
      </div>
      {stats?.unknownRangeAgents ? (
        <p className="mt-2 text-xs text-amber-800 dark:text-amber-200">
          有 {stats.unknownRangeAgents}{" "}
          个会话范围无法确认，已包含在未采集用量中，不能与未采集数相加。
        </p>
      ) : null}
      {stats?.historyTruncated ? (
        <p className="mt-2 text-xs text-amber-800 dark:text-amber-200">
          本地历史列表已截断；统计仅覆盖本次有界读取到的会话。
        </p>
      ) : null}

      <div className="mt-3 overflow-x-auto rounded-lg border border-border dark:border-slate-700">
        <div className="min-w-[760px] grid grid-cols-[1.15fr_0.65fr_0.65fr_0.75fr_0.75fr_0.75fr_0.8fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground dark:bg-slate-900/80 dark:text-slate-300">
          <span>模型</span>
          <span className="text-right">会话</span>
          <span className="text-right">请求</span>
          <span className="text-right">非缓存输入</span>
          <span className="text-right">缓存读取</span>
          <span className="text-right">输出 Tokens</span>
          <span className="text-right">费用估算</span>
        </div>
        {isLoading ? (
          <div className="p-4 text-sm text-muted-foreground">
            正在读取本期会话统计...
          </div>
        ) : stats?.modelStats.length ? (
          stats.modelStats.map((row) => {
            const observedRows = row.observedUsageAgents ?? row.agentCount;
            const missingRows = row.missingUsageAgents ?? 0;
            const totalSessions = observedRows + missingRows;
            const fullyMissing = observedRows === 0 && missingRows > 0;
            return (
              <div
                key={row.model}
                className="min-w-[760px] grid grid-cols-[1.15fr_0.65fr_0.65fr_0.75fr_0.75fr_0.75fr_0.8fr] gap-2 border-t border-border px-3 py-2 text-xs text-foreground dark:border-slate-800 dark:text-slate-300"
              >
                <span className="truncate font-mono">{row.model}</span>
                <span className="text-right">
                  {totalSessions}
                  {missingRows ? (
                    <em className="ml-1 not-italic text-amber-700 dark:text-amber-200">
                      ({missingRows} 未采集)
                    </em>
                  ) : null}
                </span>
                <span className="text-right">
                  {fullyMissing ? "未采集" : row.requestCount}
                </span>
                <span className="text-right">
                  {fullyMissing ? "未采集" : formatTokenCount(row.inputTokens)}
                </span>
                <span className="text-right">
                  {fullyMissing
                    ? "未采集"
                    : formatTokenCount(row.cacheReadTokens)}
                </span>
                <span className="text-right">
                  {fullyMissing ? "未采集" : formatTokenCount(row.outputTokens)}
                </span>
                <span className="text-right">
                  {fullyMissing ? "未采集" : formatEstimatedCost(row.totalCost)}
                </span>
              </div>
            );
          })
        ) : (
          <div className="p-4 text-sm leading-6 text-muted-foreground">
            本期暂无已采集的子 Agent
            用量；请在需要立即更新时手动同步。未采集的会话不会显示为 $0。
          </div>
        )}
      </div>

      <div className="mt-3 rounded-lg border border-border bg-background/70 px-3 py-2 text-xs leading-6 text-muted-foreground dark:border-slate-700 dark:bg-slate-950/20 dark:text-slate-300">
        请求耗时：会话历史未采集可信耗时，不显示为
        0ms。费用为本地价表估算，零值显示为待核验，不等同上游账单。状态库：
        {stats?.stateDbPath ?? "未定位"}。
      </div>
      {stats?.parentGroups?.length ? (
        <ParentGroups groups={stats.parentGroups} />
      ) : null}
    </section>
  );
}

function ParentGroups({
  groups,
}: {
  groups: NonNullable<CodexSubagentUsageStats["parentGroups"]>;
}) {
  return (
    <div className="mt-3 rounded-lg border border-sky-200 bg-sky-50/60 p-3 text-xs text-slate-700 dark:border-sky-700/50 dark:bg-sky-950/20 dark:text-slate-200">
      <div className="font-semibold">
        已记录的直接父子协作（独立分层，不与会话/模型总计相加）
      </div>
      <div className="mt-1 text-muted-foreground">
        仅 session_meta 明确记录的直接 parent_thread_id；不把普通 fork
        或嵌套关系推断成树。
      </div>
      {groups.map((group) => {
        const childUsage =
          group.observedUsageChildren > 0
            ? `子请求 ${group.childRequestCount}；非缓存输入 ${formatTokenCount(group.childInputTokens)} / 缓存读取 ${formatTokenCount(group.childCacheReadTokens)} / 缓存写入 ${formatTokenCount(group.childCacheCreationTokens)} / 输出 ${formatTokenCount(group.childOutputTokens)} / 总数 ${formatTokenCount(group.childTotalTokens)}`
            : "子用量未采集";
        const parentUsage = group.parentDirectUsage
          ? `父直接同步请求 ${group.parentDirectUsage.requestCount}；非缓存输入 ${formatTokenCount(group.parentDirectUsage.inputTokens)} / 缓存读取 ${formatTokenCount(group.parentDirectUsage.cacheReadTokens)} / 缓存写入 ${formatTokenCount(group.parentDirectUsage.cacheCreationTokens)} / 输出 ${formatTokenCount(group.parentDirectUsage.outputTokens)} / 总数 ${formatTokenCount(group.parentDirectUsage.totalTokens)}`
          : group.parentUsageStatus === "unknown_may_overlap"
            ? "父同步记录可能包含子用量，暂不计入"
            : "父直接用量未采集";
        return (
          <div
            key={group.parentSessionId}
            className="mt-2 grid gap-2 border-t border-sky-200 pt-2 sm:grid-cols-4 dark:border-sky-800"
          >
            <span className="truncate font-mono" title={group.parentSessionId}>
              父 {group.parentSessionId}
            </span>
            <span>
              子会话 {group.childSessionCount}（已采集{" "}
              {group.observedUsageChildren}，未采集 {group.missingUsageChildren}
              ）
            </span>
            <span>{childUsage}</span>
            <span>{parentUsage}</span>
          </div>
        );
      })}
    </div>
  );
}

function Summary({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="rounded-md border border-border bg-background/70 px-3 py-2 dark:border-slate-700 dark:bg-slate-950/20">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 font-semibold text-foreground dark:text-slate-100">
        {value}
      </div>
      <div className="mt-1 text-xs text-muted-foreground">{detail}</div>
    </div>
  );
}

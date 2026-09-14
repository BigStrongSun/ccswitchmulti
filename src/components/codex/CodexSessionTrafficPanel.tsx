import { useMemo, useState } from "react";
import { ChevronDown, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type {
  CodexSubagentParentUsageGroup,
  CodexSubagentUsageAgent,
  CodexSubagentUsageStats,
  SessionCollectionStatus,
} from "@/types/usage";

type Props = {
  stats?: CodexSubagentUsageStats;
  isLoading: boolean;
  error: unknown;
  rangeLabel: string;
  isSyncing: boolean;
  onSync: () => void;
  syncMessage?: string | null;
  collectionStatus?: SessionCollectionStatus;
  collectionStatusError?: unknown;
};
type SortKey = "tokens" | "model";
type Filter = "all" | "priced" | "unpriced" | "missing";
type Detail =
  | { kind: "model"; row: CodexSubagentUsageStats["modelStats"][number] }
  | { kind: "agent"; agent: CodexSubagentUsageAgent }
  | {
      kind: "task";
      group: CodexSubagentParentUsageGroup;
      children: CodexSubagentUsageAgent[];
    };

const tokens = (value: number) => value.toLocaleString();
const costValue = (value: string) => {
  const n = Number.parseFloat(value);
  return Number.isFinite(n) && n > 0 ? n : null;
};
const cost = (value: string) => {
  const n = costValue(value);
  return n === null ? "未定价/待核验" : `约 $${n.toFixed(n < 0.01 ? 6 : 4)}`;
};
const observed = (row: CodexSubagentUsageStats["modelStats"][number]) =>
  row.observedUsageAgents ?? row.agentCount;
const collectorTime = (value: number | null | undefined) =>
  typeof value === "number" && Number.isFinite(value)
    ? new Date(value * 1000).toLocaleString(undefined, {
        month: "numeric",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "未记录";
const collectorLabel = (phase: SessionCollectionStatus["phase"]) =>
  ({
    idle: "后台采集空闲",
    running: "后台采集中",
    degraded: "后台采集降级",
    not_started: "后台采集尚未启动",
  })[phase];

/** Local bounded session evidence; never add it to proxy request traffic. */
export function CodexSessionTrafficPanel({
  stats,
  isLoading,
  error,
  rangeLabel,
  isSyncing,
  onSync,
  syncMessage,
  collectionStatus,
  collectionStatusError,
}: Props) {
  const [sort, setSort] = useState<SortKey>("tokens");
  const [filter, setFilter] = useState<Filter>("all");
  const [modelQuery, setModelQuery] = useState("");
  const [view, setView] = useState("models");
  const [detail, setDetail] = useState<Detail | null>(null);
  const rows = stats?.modelStats ?? [];
  const agents = stats?.agents ?? [];
  const overview = useMemo(
    () => ({
      requestCount: rows.reduce((sum, row) => sum + row.requestCount, 0),
      tokenCount: rows.reduce((sum, row) => sum + row.totalTokens, 0),
      priced: rows.reduce(
        (sum, row) => sum + (costValue(row.totalCost) ?? 0),
        0,
      ),
      incompletePrice: rows.some(
        (row) => observed(row) > 0 && costValue(row.totalCost) === null,
      ),
    }),
    [rows],
  );
  const visibleRows = useMemo(
    () =>
      rows
        .filter((row) => {
          if (
            !row.model
              .toLocaleLowerCase()
              .includes(modelQuery.trim().toLocaleLowerCase())
          )
            return false;
          if (filter === "priced") return costValue(row.totalCost) !== null;
          if (filter === "unpriced")
            return observed(row) > 0 && costValue(row.totalCost) === null;
          if (filter === "missing") return (row.missingUsageAgents ?? 0) > 0;
          return true;
        })
        .sort((a, b) =>
          sort === "model"
            ? a.model.localeCompare(b.model)
            : b.totalTokens - a.totalTokens || a.model.localeCompare(b.model),
        ),
    [filter, modelQuery, rows, sort],
  );
  const groups = stats?.parentGroups ?? [];
  const parentIds = new Set(groups.map((group) => group.parentSessionId));
  const ungrouped = agents.filter(
    (agent) => !agent.parentThreadId || !parentIds.has(agent.parentThreadId),
  );
  const observedCount =
    stats?.observedUsageAgents ??
    rows.reduce((sum, row) => sum + observed(row), 0);
  const missingCount =
    stats?.missingUsageAgents ??
    rows.reduce((sum, row) => sum + (row.missingUsageAgents ?? 0), 0);
  const hasSnapshot = stats !== undefined;
  const summaryState = isLoading
    ? "loading"
    : !hasSnapshot
      ? "absent"
      : observedCount === 0 && missingCount > 0
        ? "missing"
        : "ready";
  return (
    <section className="rounded-lg border border-border bg-card p-4 text-foreground shadow-sm dark:border-slate-700">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold">
            {rangeLabel}子 Agent 会话流量
          </h3>
          <p className="mt-1 text-sm leading-6 text-muted-foreground">
            只基于本地 Codex 会话索引与已同步的 <code>codex_session</code>{" "}
            用量；请求统计与会话统计独立，不能相加。
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={onSync}
          disabled={isSyncing}
          className="gap-2"
        >
          <RefreshCw className={`h-4 w-4 ${isSyncing ? "animate-spin" : ""}`} />
          立即同步会话用量
        </Button>
      </div>
      {syncMessage ? (
        <div className="mt-3 rounded-md border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          {syncMessage}
        </div>
      ) : null}
      <CollectorStatus
        status={collectionStatus}
        error={collectionStatusError}
      />
      {error ? (
        <div className="mt-3 rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
          子 Agent 用量读取失败：
          {error instanceof Error ? error.message : String(error)}
        </div>
      ) : null}
      {stats?.skippedReason ? (
        <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
          会话用量索引读取跳过：{stats.skippedReason}
        </div>
      ) : null}
      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        <Summary
          label="已观测子任务 Tokens"
          value={
            summaryState === "ready"
              ? tokens(overview.tokenCount)
              : summaryState === "loading"
                ? "加载中"
                : "—"
          }
          detail={
            summaryState === "missing"
              ? "仅有未采集会话，不按 0 Token 计"
              : summaryState === "absent"
                ? "暂无已采集快照"
                : "仅已同步的子任务用量；不是主/子任务总数"
          }
        />
        <Summary
          label="已观测请求"
          value={
            summaryState === "ready"
              ? tokens(overview.requestCount)
              : summaryState === "loading"
                ? "加载中"
                : "—"
          }
          detail={`${observedCount} 个有用量证据；${missingCount} 个未采集不按 0 计`}
        />
        <Summary
          label="已知价表估算"
          value={
            summaryState === "ready"
              ? overview.priced
                ? cost(String(overview.priced))
                : "未定价/待核验"
              : summaryState === "loading"
                ? "加载中"
                : "—"
          }
          detail={
            summaryState === "missing"
              ? "仅有未采集会话，不构成 $0"
              : summaryState === "absent"
                ? "暂无已采集快照"
                : overview.incompletePrice
                  ? "部分可计价；仍有已观测用量未定价"
                  : "本地价表估算，不等同上游账单"
          }
        />
      </div>
      {stats?.unknownRangeAgents ? (
        <p className="mt-2 text-xs text-amber-800 dark:text-amber-200">
          有 {stats.unknownRangeAgents}{" "}
          个会话范围无法确认，未计入所选范围；它们不属于未采集用量。
        </p>
      ) : null}
      {stats?.historyTruncated ? (
        <p className="mt-2 text-xs text-amber-800 dark:text-amber-200">
          本地会话索引已截断；统计仅覆盖当前数据库快照中的会话。
        </p>
      ) : null}
      <Tabs value={view} onValueChange={setView} className="mt-4">
        <TabsList className="w-full justify-start sm:w-auto">
          <TabsTrigger value="models" className="min-w-0">
            按模型
          </TabsTrigger>
          <TabsTrigger value="tasks" className="min-w-0">
            按任务
          </TabsTrigger>
        </TabsList>
        <TabsContent value="models" className="mt-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <p className="text-xs text-muted-foreground">
              模型统计仅汇总已观测子任务；概览不随此处筛选变化。
            </p>
            <div className="flex flex-wrap gap-2">
              <Input
                aria-label="搜索模型"
                value={modelQuery}
                onChange={(event) => setModelQuery(event.target.value)}
                className="h-8 w-[132px] text-xs"
                placeholder="搜索模型"
              />
              <Select
                value={filter}
                onValueChange={(value) => setFilter(value as Filter)}
              >
                <SelectTrigger
                  aria-label="筛选模型"
                  className="h-8 w-[132px] text-xs"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">全部模型</SelectItem>
                  <SelectItem value="priced">有估算价格</SelectItem>
                  <SelectItem value="unpriced">仅未定价</SelectItem>
                  <SelectItem value="missing">含未采集会话</SelectItem>
                </SelectContent>
              </Select>
              <Button
                size="sm"
                className="h-8 text-xs"
                aria-label="按模型名排序"
                aria-pressed={sort === "model"}
                variant={sort === "model" ? "secondary" : "outline"}
                onClick={() => setSort("model")}
              >
                模型名
              </Button>
              <Button
                size="sm"
                className="h-8 text-xs"
                aria-label="按 Tokens 排序"
                aria-pressed={sort === "tokens"}
                variant={sort === "tokens" ? "secondary" : "outline"}
                onClick={() => setSort("tokens")}
              >
                Tokens
              </Button>
            </div>
          </div>
          <ModelRows
            rows={visibleRows}
            isLoading={isLoading}
            onDetail={(row) => setDetail({ kind: "model", row })}
          />
        </TabsContent>
        <TabsContent value="tasks" className="mt-3">
          <TaskRows
            groups={groups}
            agents={agents}
            ungrouped={ungrouped}
            isLoading={isLoading}
            hasSnapshot={hasSnapshot}
            onDetail={(group, children) =>
              setDetail({ kind: "task", group, children })
            }
            onAgentDetail={(agent) => setDetail({ kind: "agent", agent })}
          />
        </TabsContent>
      </Tabs>
      <div className="mt-3 rounded-lg border border-border bg-muted/20 px-3 py-2 text-xs leading-6 text-muted-foreground">
        请求耗时：会话记录未采集可信耗时，不显示为
        0ms。费用为本地价表估算，零值显示为待核验，不等同上游账单。数据来源：CCSM
        本地会话用量账。
      </div>
      <DetailSheet
        detail={detail}
        onOpenChange={(open) => !open && setDetail(null)}
      />
    </section>
  );
}

function CollectorStatus({
  status,
  error,
}: {
  status?: SessionCollectionStatus;
  error: unknown;
}) {
  if (error)
    return (
      <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
        后台采集状态暂不可用：
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  if (!status) return null;
  const notStarted = status.phase === "not_started";
  const alert =
    notStarted || status.phase === "degraded" || status.deferred > 0;
  return (
    <Collapsible
      className={`mt-3 rounded-md border px-3 py-2 text-xs ${alert ? "border-amber-200 bg-amber-50 text-amber-900" : "border-border bg-muted/20 text-muted-foreground"}`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span>
          <strong>{collectorLabel(status.phase)}</strong>
          {notStarted
            ? " · 尚无成功采集；可点击立即同步或等待后台启动。"
            : ` · 最近成功 ${collectorTime(status.lastSuccessAt)} · ${status.imported ? `本轮新增 ${status.imported}` : "本轮暂无新增"} · 待处理 ${status.deferred} · 错误 ${status.errorsCount}`}
        </span>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" size="sm" className="h-6 gap-1 px-1 text-xs">
            采集诊断
            <ChevronDown className="h-3 w-3" />
          </Button>
        </CollapsibleTrigger>
      </div>
      {status.lastErrorSummary ? (
        <div className="mt-1">错误摘要：{status.lastErrorSummary}</div>
      ) : null}
      <CollapsibleContent className="mt-2 border-t border-current/15 pt-2 leading-5">
        最近开始 {collectorTime(status.lastStartedAt)}；最近完成{" "}
        {collectorTime(status.lastCompletedAt)}。
        {status.nextRunAt
          ? `下次约 ${collectorTime(status.nextRunAt)}`
          : "下次运行时间未记录"}
        ；周期 {status.intervalSecs} 秒。
      </CollapsibleContent>
    </Collapsible>
  );
}

function ModelRows({
  rows,
  isLoading,
  onDetail,
}: {
  rows: CodexSubagentUsageStats["modelStats"];
  isLoading: boolean;
  onDetail: (row: CodexSubagentUsageStats["modelStats"][number]) => void;
}) {
  if (isLoading)
    return (
      <div className="rounded-lg border border-border p-4 text-sm text-muted-foreground">
        正在读取本期会话统计...
      </div>
    );
  if (!rows.length)
    return (
      <div className="rounded-lg border border-border p-4 text-sm text-muted-foreground">
        本期暂无已采集的子 Agent 用量；未采集的会话不会显示为 $0。
      </div>
    );
  const max = Math.max(...rows.map((row) => row.totalTokens), 1);
  return (
    <div className="overflow-x-auto rounded-lg border border-border">
      <div className="min-w-[720px] grid grid-cols-[1.35fr_.55fr_.65fr_.8fr_.8fr_.8fr_.8fr] gap-2 bg-muted px-3 py-2 text-xs font-semibold text-muted-foreground">
        <span>模型</span>
        <span className="text-right">会话</span>
        <span className="text-right">请求</span>
        <span className="text-right">非缓存输入</span>
        <span className="text-right">缓存读取</span>
        <span className="text-right">输出 Tokens</span>
        <span className="text-right">估算</span>
      </div>
      <div data-testid="model-rows">
        {rows.map((row) => {
          const seen = observed(row);
          const missing = row.missingUsageAgents ?? 0;
          return (
            <button
              key={row.model}
              type="button"
              className="min-w-[720px] grid w-full grid-cols-[1.35fr_.55fr_.65fr_.8fr_.8fr_.8fr_.8fr] gap-2 border-t border-border px-3 py-2 text-left text-xs hover:bg-muted/50"
              onClick={() => onDetail(row)}
            >
              <span className="min-w-0">
                <span className="block truncate font-mono">{row.model}</span>
                <span className="mt-1 block h-1.5 overflow-hidden rounded bg-muted">
                  <span
                    className="block h-full bg-slate-500"
                    style={{ width: `${(row.totalTokens / max) * 100}%` }}
                  />
                </span>
              </span>
              <span className="text-right tabular-nums">
                {seen + missing}
                {missing ? (
                  <em className="ml-1 not-italic text-amber-700">
                    ({missing} 未采集)
                  </em>
                ) : null}
              </span>
              <span className="text-right tabular-nums">
                {seen ? row.requestCount : "未采集"}
              </span>
              <span className="text-right tabular-nums">
                {seen ? tokens(row.inputTokens) : "未采集"}
              </span>
              <span className="text-right tabular-nums">
                {seen ? tokens(row.cacheReadTokens) : "未采集"}
              </span>
              <span className="text-right tabular-nums">
                {seen ? tokens(row.outputTokens) : "未采集"}
              </span>
              <span className="text-right tabular-nums">
                {seen ? cost(row.totalCost) : "未采集"}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function TaskRows({
  groups,
  agents,
  ungrouped,
  isLoading,
  hasSnapshot,
  onDetail,
  onAgentDetail,
}: {
  groups: CodexSubagentParentUsageGroup[];
  agents: CodexSubagentUsageAgent[];
  ungrouped: CodexSubagentUsageAgent[];
  isLoading: boolean;
  hasSnapshot: boolean;
  onDetail: (
    group: CodexSubagentParentUsageGroup,
    children: CodexSubagentUsageAgent[],
  ) => void;
  onAgentDetail: (agent: CodexSubagentUsageAgent) => void;
}) {
  if (isLoading)
    return (
      <div className="rounded-lg border border-border p-4 text-sm text-muted-foreground">
        正在读取本期任务关系...
      </div>
    );
  if (!hasSnapshot)
    return (
      <div className="rounded-lg border border-border p-4 text-sm text-muted-foreground">
        暂无已采集快照，无法判断任务关系。
      </div>
    );
  return (
    <div className="space-y-2">
      <p className="text-xs leading-5 text-muted-foreground">
        只按 <code>session_meta.parent_thread_id</code>{" "}
        的显式关系分组；不推测父模型，也不把普通 fork 或嵌套关系拼成树。
      </p>
      {groups.map((group) => {
        const children = agents.filter(
          (agent) => agent.parentThreadId === group.parentSessionId,
        );
        return (
          <details
            key={group.parentSessionId}
            className="rounded-lg border border-border bg-muted/10"
            open
          >
            <summary className="flex cursor-pointer list-none flex-wrap items-center justify-between gap-2 px-3 py-2 text-xs">
              <span className="font-mono">父 {group.parentSessionId}</span>
              <span>
                子会话 {group.childSessionCount}（已采集{" "}
                {group.observedUsageChildren}，未采集{" "}
                {group.missingUsageChildren}）
              </span>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 text-xs"
                aria-label={`查看 ${group.parentSessionId} 详情`}
                onClick={(event) => {
                  event.preventDefault();
                  onDetail(group, children);
                }}
              >
                详情
              </Button>
            </summary>
            <div className="border-t border-border px-3 py-2 text-xs text-muted-foreground">
              {group.observedUsageChildren > 0
                ? `子任务仅计入子会话：请求 ${group.childRequestCount}；Tokens ${tokens(group.childTotalTokens)}。`
                : "子任务用量未采集，不以 0 请求或 0 Tokens 表示。"}
              {group.parentUsageStatus === "unknown_may_overlap"
                ? " 父同步记录可能包含子用量，暂不计入。"
                : group.parentDirectUsage
                  ? " 已记录父直接用量，但不与子用量相加。"
                  : " 父直接用量未采集。"}
            </div>
            {children.length ? (
              <div className="space-y-1 border-t border-border px-3 py-2">
                {children.map((child) => (
                  <div
                    key={child.sessionId}
                    className="flex flex-wrap items-center justify-between gap-2 rounded bg-background/60 px-2 py-1.5 text-xs"
                  >
                    <span className="min-w-0 break-all">
                      <strong>{child.title || "未命名子任务"}</strong> ·{" "}
                      {child.primaryModel || child.models[0] || "模型未知"} ·{" "}
                      <span className="tabular-nums">
                        {child.usageStatus === "observed"
                          ? `${tokens(child.totalTokens)} Tokens · ${cost(child.totalCost)}`
                          : "未采集"}
                      </span>
                    </span>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-6 text-xs"
                      aria-label={`查看 ${child.sessionId} 详情`}
                      onClick={() => onAgentDetail(child)}
                    >
                      详情
                    </Button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="border-t border-border px-3 py-2 text-xs text-muted-foreground">
                当前快照只保留该父组汇总，未提供子会话条目。
              </div>
            )}
          </details>
        );
      })}
      {ungrouped.length ? (
        <details className="rounded-lg border border-border bg-muted/10" open>
          <summary className="cursor-pointer px-3 py-2 text-xs font-medium">
            未关联会话（{ungrouped.length}）
          </summary>
          <div className="border-t border-border px-3 py-2 text-xs text-muted-foreground">
            {ungrouped.map((agent) => (
              <div
                key={agent.sessionId}
                className="flex flex-wrap items-center justify-between gap-2 py-1"
              >
                <span>
                  <span className="font-mono">{agent.sessionId}</span> ·{" "}
                  {agent.title || "未命名会话"} ·{" "}
                  {agent.usageStatus === "observed"
                    ? `${tokens(agent.totalTokens)} Tokens · ${cost(agent.totalCost)}`
                    : "未采集"}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 text-xs"
                  aria-label={`查看 ${agent.sessionId} 详情`}
                  onClick={() => onAgentDetail(agent)}
                >
                  详情
                </Button>
              </div>
            ))}
          </div>
        </details>
      ) : null}
      {!groups.length && !ungrouped.length ? (
        <div className="rounded-lg border border-border p-4 text-sm text-muted-foreground">
          暂无可展示的显式任务关系。
        </div>
      ) : null}
    </div>
  );
}

function DetailSheet({
  detail,
  onOpenChange,
}: {
  detail: Detail | null;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={detail !== null} onOpenChange={onOpenChange}>
      <DialogContent className="fixed left-auto right-0 top-0 h-dvh max-h-none w-full max-w-xl translate-x-0 translate-y-0 rounded-none border-l p-0 sm:w-[36rem]">
        <DialogHeader>
          <DialogTitle>
            {detail?.kind === "model"
              ? `${detail.row.model} 用量详情`
              : detail?.kind === "agent"
                ? `${detail.agent.title || "子任务"} 详情`
                : detail
                  ? `父 ${detail.group.parentSessionId} 任务详情`
                  : "用量详情"}
          </DialogTitle>
          <DialogDescription>
            本页只展示当前本地会话证据；未知值不会转换为零值。
          </DialogDescription>
        </DialogHeader>
        {detail?.kind === "model" ? (
          <div className="space-y-4 overflow-y-auto p-5 text-sm">
            {observed(detail.row) ? (
              <Breakdown
                input={detail.row.inputTokens}
                cache={detail.row.cacheReadTokens}
                creation={detail.row.cacheCreationTokens}
                output={detail.row.outputTokens}
              />
            ) : (
              <p className="text-muted-foreground">
                尚无用量证据；未采集不以 0 Tokens 或 $0 表示。
              </p>
            )}
            <p>
              来源：{observed(detail.row) ? "已观测本地会话证据" : "未采集"}
              。费用：
              {cost(detail.row.totalCost)}。
            </p>
            {(detail.row.missingUsageAgents ?? 0) > 0 ? (
              <p className="text-amber-700">
                有 {detail.row.missingUsageAgents} 个会话未采集，不计入
                Tokens、请求或费用。
              </p>
            ) : null}
          </div>
        ) : detail?.kind === "agent" ? (
          <div className="space-y-4 overflow-y-auto p-5 text-sm">
            {detail.agent.usageStatus === "observed" ? (
              <Breakdown
                input={detail.agent.inputTokens}
                cache={detail.agent.cacheReadTokens}
                creation={detail.agent.cacheCreationTokens}
                output={detail.agent.outputTokens}
              />
            ) : (
              <p className="text-muted-foreground">
                尚无用量证据；未采集不以 0 Tokens 或 $0 表示。
              </p>
            )}
            <p>
              来源：
              {detail.agent.usageSource === "session_sync"
                ? "本地会话同步账"
                : detail.agent.usageSource === "rollout"
                  ? "本地 rollout 用量证据"
                  : "未采集"}
              。费用：
              {detail.agent.usageStatus === "observed"
                ? cost(detail.agent.totalCost)
                : "未定价/待核验"}
              。
            </p>
            <p className="break-all text-muted-foreground">
              会话 {detail.agent.sessionId}
            </p>
          </div>
        ) : detail ? (
          <div className="space-y-4 overflow-y-auto p-5 text-sm">
            {detail.group.observedUsageChildren > 0 ? (
              <Breakdown
                input={detail.group.childInputTokens}
                cache={detail.group.childCacheReadTokens}
                creation={detail.group.childCacheCreationTokens}
                output={detail.group.childOutputTokens}
              />
            ) : (
              <p className="text-muted-foreground">
                尚无用量证据；未采集不以 0 Tokens 或 $0 表示。
              </p>
            )}
            <p>
              来源：
              {detail.children.length === 0
                ? "当前快照未提供逐条子会话，来源无法逐条确认"
                : detail.children.every(
                      (child) => child.usageSource === "session_sync",
                    )
                  ? "本地会话同步账"
                  : detail.children.every(
                        (child) => child.usageSource === "rollout",
                      )
                    ? "本地 rollout 用量证据"
                    : detail.children.some(
                          (child) => child.usageSource === "none",
                        )
                      ? "本地会话证据（含未采集）"
                      : "本地会话证据（来源混合）"}
              。
            </p>
            {detail.group.parentUsageStatus === "unknown_may_overlap" ? (
              <p className="text-amber-700">
                父同步记录可能包含子用量，暂不计入。
              </p>
            ) : null}
            <div>
              <h4 className="font-medium">显式子会话</h4>
              {detail.children.length ? (
                detail.children.map((child) => (
                  <div
                    key={child.sessionId}
                    className="mt-2 rounded border border-border p-2"
                  >
                    <div className="font-mono text-xs">{child.sessionId}</div>
                    <div>
                      {child.title || "未命名会话"} ·{" "}
                      {child.usageStatus === "observed" ? "已观测" : "未采集"}
                    </div>
                  </div>
                ))
              ) : (
                <p className="mt-1 text-muted-foreground">
                  当前 DTO 未提供可展开的子会话条目。
                </p>
              )}
            </div>
          </div>
        ) : null}
        <div className="border-t border-border p-3 text-right">
          <DialogClose asChild>
            <Button size="sm" variant="outline">
              关闭
            </Button>
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Breakdown({
  input,
  cache,
  creation,
  output,
}: {
  input: number;
  cache: number;
  creation: number;
  output: number;
}) {
  const total = Math.max(input + cache + creation + output, 1);
  return (
    <div>
      <div className="mb-2 flex justify-between text-xs text-muted-foreground">
        <span>Token 构成</span>
        <span>总数 {tokens(input + cache + creation + output)}</span>
      </div>
      <div
        className="flex h-3 overflow-hidden rounded bg-muted"
        aria-label={`Token 构成：非缓存输入 ${input}，缓存读取 ${cache}，缓存写入 ${creation}，输出 ${output}`}
      >
        <span
          className="bg-slate-500"
          style={{ width: `${(input / total) * 100}%` }}
        />
        <span
          className="bg-slate-300"
          style={{ width: `${(cache / total) * 100}%` }}
        />
        <span
          className="bg-slate-400"
          style={{ width: `${(creation / total) * 100}%` }}
        />
        <span
          className="bg-sky-500"
          style={{ width: `${(output / total) * 100}%` }}
        />
      </div>
      <div className="mt-2 grid grid-cols-2 gap-2 text-xs text-muted-foreground sm:grid-cols-4">
        <span>非缓存输入 {tokens(input)}</span>
        <span>缓存读取 {tokens(cache)}</span>
        <span>缓存写入 {tokens(creation)}</span>
        <span>输出 {tokens(output)}</span>
      </div>
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
    <div className="rounded-md border border-border bg-muted/10 px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 text-lg font-semibold tabular-nums">{value}</div>
      <div className="mt-1 text-xs text-muted-foreground">{detail}</div>
    </div>
  );
}

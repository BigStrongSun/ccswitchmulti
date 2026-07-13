import React, { useEffect, useState } from "react";
import {
  AlertCircle,
  ArrowRight,
  BarChart3,
  CheckCircle2,
  Clock,
  Database,
  Gauge,
  Info,
  LineChart,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
  ShieldAlert,
  Sparkles,
  TimerReset,
  TrendingDown,
  TrendingUp,
  MonitorSmartphone,
  ShieldCheck,
  ExternalLink,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import {
  useModelStats,
  useQuotaCollaborationOverview,
  useSyncQuotaCollaboration,
  useUsageSummary,
} from "@/lib/query/usage";
import type {
  QuotaTier,
  ResetCreditInfo,
  SubscriptionQuota,
} from "@/types/subscription";
import type { ModelStats, UsageSummary } from "@/types/usage";
import type { QuotaCollaborationOverview } from "@/types/usage";

const TRACKED_TIERS = new Set(["five_hour", "seven_day"]);
const FIVE_HOUR_WINDOW_MS = 5 * 60 * 60 * 1000;
const SEVEN_DAY_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;
const TODAY_RANGE = { preset: "today" } as const;
const SEVEN_DAY_RANGE = { preset: "7d" } as const;

const TIER_LABELS: Record<string, string> = {
  five_hour: "5 小时窗口",
  seven_day: "每周窗口",
  weekly_limit: "每周窗口",
};

interface UsageWindowCardProps {
  tier: QuotaTier;
}

interface ResetCreditRowProps {
  credit: ResetCreditInfo;
  index: number;
}

interface GuideStepProps {
  index: number;
  title: string;
  detail: string;
}

interface UsageReadingHintProps {
  title: string;
  detail: string;
  tone: "ok" | "warn" | "info";
}

interface WindowForecast {
  /** 已过周期的平均官方窗口消耗速度，单位为百分点/小时。 */
  percentPerHour: number;
  /** 在当前平均速度不变时的预计耗尽时刻；无法计算时为 null。 */
  depletionAt: Date | null;
  /** 预计耗尽是否发生在本次官方重置前。 */
  depletesBeforeReset: boolean;
  /** 预测的可解释性等级，固定为近似估算。 */
  confidence: "low" | "medium";
}

/** 页面级配色：每个 surface 都显式维护 light 与 dark 两套颜色，避免主题切换时只继承单套颜色。 */
const USAGE_PAGE_COLORS = {
  shell:
    "border-slate-200 bg-white text-slate-950 dark:border-slate-700/80 dark:bg-slate-950/30 dark:text-slate-100",
  header:
    "bg-gradient-to-r from-sky-50 via-white to-emerald-50 dark:from-blue-950/45 dark:via-slate-900 dark:to-emerald-950/30",
  chip: "border-slate-200 bg-white/80 text-slate-600 dark:border-slate-700/80 dark:bg-slate-950/40 dark:text-slate-300",
  guide:
    "border-sky-200 bg-sky-50/80 text-sky-950 dark:border-blue-700/40 dark:bg-blue-950/15 dark:text-blue-100",
  guideStep:
    "border-sky-200 bg-white/90 text-slate-950 dark:border-blue-700/40 dark:bg-slate-950/40 dark:text-slate-100",
  card: "border-slate-200 bg-white text-slate-950 dark:border-slate-700/80 dark:bg-slate-950/30 dark:text-slate-100",
  inset:
    "border-slate-200 bg-slate-50 text-slate-950 dark:border-slate-700/70 dark:bg-slate-950/40 dark:text-slate-100",
  progressTrack: "bg-slate-200 dark:bg-slate-800",
  warning:
    "border-amber-200 bg-amber-50 text-amber-950 dark:border-amber-800 dark:bg-amber-950/30 dark:text-amber-100",
  warningButton:
    "border-amber-300 bg-white text-amber-950 hover:bg-amber-100 dark:border-amber-700 dark:bg-slate-950/40 dark:text-amber-100 dark:hover:bg-amber-900/40",
  resetSummary: "bg-sky-100 text-sky-800 dark:bg-sky-500/10 dark:text-sky-300",
  analytics:
    "border-violet-200 bg-violet-50/60 text-slate-950 dark:border-violet-800/50 dark:bg-violet-950/15 dark:text-slate-100",
  analyticsInset:
    "border-slate-200 bg-white/85 text-slate-950 dark:border-slate-700/70 dark:bg-slate-950/35 dark:text-slate-100",
};

/** 读数提示卡配色：ok/warn/info 各自维护浅色和深色配色。 */
const READING_HINT_COLORS: Record<UsageReadingHintProps["tone"], string> = {
  ok: "border-emerald-200 bg-emerald-50 text-emerald-950 dark:border-emerald-700/40 dark:bg-emerald-950/20 dark:text-emerald-100",
  warn: "border-amber-200 bg-amber-50 text-amber-950 dark:border-amber-700/40 dark:bg-amber-950/20 dark:text-amber-100",
  info: "border-sky-200 bg-sky-50 text-sky-950 dark:border-blue-700/40 dark:bg-blue-950/20 dark:text-blue-100",
};

/** 将百分比限制在进度条可安全渲染的 0-100 区间。 */
function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(Math.max(value, 0), 100);
}

/** 根据已用百分比返回容量状态文案。 */
function describeCapacity(utilization: number): string {
  if (utilization >= 95) return "接近耗尽";
  if (utilization >= 75) return "偏紧";
  if (utilization >= 50) return "正常";
  return "充足";
}

/** 根据已用百分比返回页面上的语义颜色。 */
function capacityTone(utilization: number): string {
  if (utilization >= 95) return "text-red-600 dark:text-red-400";
  if (utilization >= 75) return "text-amber-600 dark:text-amber-400";
  if (utilization >= 50) return "text-blue-600 dark:text-blue-400";
  return "text-emerald-600 dark:text-emerald-400";
}

/** 根据已用百分比返回进度条颜色。 */
function capacityBarTone(utilization: number): string {
  if (utilization >= 95) return "bg-red-500";
  if (utilization >= 75) return "bg-amber-500";
  if (utilization >= 50) return "bg-blue-500";
  return "bg-emerald-500";
}

/** 把 ISO 时间格式化为本地可读时间；无效或缺失时返回兜底文案。 */
function formatDateTime(value: string | null | undefined): string {
  if (!value) return "未返回";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "无法解析";
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** 把毫秒时间戳格式化为最近刷新时间。 */
function formatCheckedAt(value: number | null | undefined): string {
  if (!value) return "尚未刷新";
  return new Date(value).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** 将 token 数量压缩成适合仪表盘阅读的本地化格式。 */
function formatTokens(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  return new Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: value >= 1000000 ? 1 : 0,
  }).format(value);
}

/** 将官方窗口的重置时间换算为剩余小时数；无法解析时返回 null。 */
function getHoursUntil(value: string | null | undefined): number | null {
  if (!value) return null;
  const resetMs = new Date(value).getTime();
  if (!Number.isFinite(resetMs)) return null;
  return Math.max(0, (resetMs - Date.now()) / (60 * 60 * 1000));
}

/** 根据窗口类型返回可用于推算平均速率的理论周期长度。 */
function getWindowDurationMs(tierName: string): number | null {
  if (tierName === "five_hour") return FIVE_HOUR_WINDOW_MS;
  if (tierName === "seven_day" || tierName === "weekly_limit") {
    return SEVEN_DAY_WINDOW_MS;
  }
  return null;
}

/**
 * 根据当前官方百分比与已过周期时间估算窗口耗尽点。
 *
 * 官方接口不会返回窗口总 token 数，因此这里严格只在“官方百分比”维度做
 * 线性外推，不能把本地 token 日志换算为官方配额。
 */
function buildWindowForecast(tier: QuotaTier): WindowForecast | null {
  const durationMs = getWindowDurationMs(tier.name);
  const resetsAtMs = tier.resetsAt ? new Date(tier.resetsAt).getTime() : NaN;
  const used = clampPercent(tier.utilization);
  if (!durationMs || !Number.isFinite(resetsAtMs) || used <= 0) return null;

  const windowStartMs = resetsAtMs - durationMs;
  const elapsedHours = Math.max(
    (Date.now() - windowStartMs) / (60 * 60 * 1000),
    1 / 60,
  );
  const percentPerHour = used / elapsedHours;
  const remaining = Math.max(0, 100 - used);
  const hoursUntilFull = remaining / percentPerHour;
  const depletionAt = new Date(Date.now() + hoursUntilFull * 60 * 60 * 1000);

  return {
    percentPerHour,
    depletionAt,
    depletesBeforeReset: depletionAt.getTime() < resetsAtMs,
    confidence: elapsedHours >= 1 ? "medium" : "low",
  };
}

/** 根据窗口预测生成一条明确但不过度承诺的行动建议。 */
function getWindowRecommendation(
  tier: QuotaTier,
  forecast: WindowForecast | null,
): { title: string; detail: string; tone: "ok" | "warn" | "risk" } {
  const hoursUntilReset = getHoursUntil(tier.resetsAt);
  if (!forecast || hoursUntilReset === null) {
    return {
      title: "等待更多窗口数据",
      detail: "官方尚未提供足够的有效窗口读数，暂不显示耗尽预测。",
      tone: "warn",
    };
  }
  if (forecast.depletesBeforeReset) {
    return {
      title: "按当前节奏可能提前耗尽",
      detail: `预计 ${formatDateTime(forecast.depletionAt?.toISOString())} 用尽，建议降低并发或把轻量任务切到其它模型。`,
      tone: "risk",
    };
  }
  if (forecast.percentPerHour >= 15) {
    return {
      title: "消耗节奏偏快",
      detail: `重置前约剩 ${Math.ceil(hoursUntilReset)} 小时；优先避免大上下文重复提交。`,
      tone: "warn",
    };
  }
  return {
    title: "当前节奏可覆盖至重置",
    detail: `按已过周期平均速度估算，仍会在 ${formatDateTime(tier.resetsAt)} 前保有余量。`,
    tone: "ok",
  };
}

/** 计算今天已过去的小时数，保证新的一天不会除以零。 */
function getElapsedTodayHours(nowMs: number = Date.now()): number {
  const now = new Date(nowMs);
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.max((nowMs - start.getTime()) / (60 * 60 * 1000), 1 / 60);
}

/** 选出 token 消耗最高的若干模型，并稳定处理同值排序。 */
function getTopModels(stats: ModelStats[] | undefined): ModelStats[] {
  return [...(stats ?? [])]
    .filter((item) => item.totalTokens > 0)
    .sort(
      (left, right) =>
        right.totalTokens - left.totalTokens ||
        right.requestCount - left.requestCount ||
        left.model.localeCompare(right.model),
    )
    .slice(0, 4);
}

/** 计算 reset credit 到期紧迫度，用于提示即将过期的额度。 */
function resetCreditUrgency(expiresAt: string | null | undefined): {
  label: string;
  className: string;
} {
  if (!expiresAt) {
    return {
      label: "无到期记录",
      className: "text-muted-foreground",
    };
  }
  const expiresAtMs = new Date(expiresAt).getTime();
  if (Number.isNaN(expiresAtMs)) {
    return {
      label: "到期时间异常",
      className: "text-amber-600 dark:text-amber-400",
    };
  }
  const hoursLeft = (expiresAtMs - Date.now()) / (1000 * 60 * 60);
  if (hoursLeft <= 0) {
    return {
      label: "已过期",
      className: "text-red-600 dark:text-red-400",
    };
  }
  if (hoursLeft <= 24) {
    return {
      label: "今天到期",
      className: "text-red-600 dark:text-red-400",
    };
  }
  if (hoursLeft <= 72) {
    return {
      label: "即将到期",
      className: "text-amber-600 dark:text-amber-400",
    };
  }
  if (hoursLeft <= 24 * 7) {
    return {
      label: "本周到期",
      className: "text-blue-600 dark:text-blue-400",
    };
  }
  return {
    label: "可用",
    className: "text-emerald-600 dark:text-emerald-400",
  };
}

/** 判断 reset credit 是否仍处于官方 available 状态。 */
function isAvailableCredit(credit: ResetCreditInfo): boolean {
  return credit.status?.toLowerCase() === "available";
}

/** 从额度响应中挑出主页面展示的 Codex 速率窗口。 */
function getVisibleTiers(quota: SubscriptionQuota | undefined): QuotaTier[] {
  return (quota?.tiers ?? []).filter(
    (tier) => TRACKED_TIERS.has(tier.name) || tier.name in TIER_LABELS,
  );
}

/** 引导步骤条目：说明从哪里进入页面以及读数顺序。 */
const GuideStep: React.FC<GuideStepProps> = ({ index, title, detail }) => (
  <div
    className={`flex gap-3 rounded-lg border p-3 text-sm ${USAGE_PAGE_COLORS.guideStep}`}
  >
    <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-sky-600 text-xs font-semibold text-white dark:bg-blue-500">
      {index}
    </div>
    <div className="min-w-0">
      <div className="font-medium text-foreground">{title}</div>
      <div className="mt-1 text-xs leading-5 text-muted-foreground">
        {detail}
      </div>
    </div>
  </div>
);

/** 读数提示条目：把颜色和阈值解释成可执行判断。 */
const UsageReadingHint: React.FC<UsageReadingHintProps> = ({
  title,
  detail,
  tone,
}) => {
  return (
    <div className={`rounded-lg border px-3 py-2 ${READING_HINT_COLORS[tone]}`}>
      <div className="text-xs font-semibold">{title}</div>
      <div className="mt-1 text-xs leading-5 opacity-80">{detail}</div>
    </div>
  );
};

/** 页面引导区：补齐入口、刷新和读数判断，避免用户只看到一组静态数字。 */
const UsageGuidePanel: React.FC = () => (
  <section className={`rounded-lg border p-4 ${USAGE_PAGE_COLORS.guide}`}>
    <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
      <Info className="h-4 w-4" />
      使用引导
    </div>
    <div className="grid gap-3 lg:grid-cols-3">
      <GuideStep
        index={1}
        title="从 Codex 工具栏进入"
        detail="主界面先切到 Codex，再点多模型路由旁边的柱状图按钮。"
      />
      <GuideStep
        index={2}
        title="先刷新当前登录"
        detail="页面读取本机 Codex Desktop / CLI 登录；刷新只重新查询，不兑换 reset。"
      />
      <GuideStep
        index={3}
        title="按窗口和到期时间决策"
        detail="先看 5 小时与每周剩余额度，再看 reset 是否临近到期。"
      />
    </div>
    <div className="mt-3 grid gap-2 lg:grid-cols-3">
      <UsageReadingHint
        tone="ok"
        title="绿色 / 蓝色"
        detail="容量还可用，适合继续工作或保留 reset。"
      />
      <UsageReadingHint
        tone="warn"
        title="黄色 / 红色"
        detail="窗口偏紧；如果刷新还远，优先规划 reset 或等待。"
      />
      <UsageReadingHint
        tone="info"
        title="到期提示"
        detail="今天到期、即将到期、本周到期会单独标出，避免 reset 白白过期。"
      />
    </div>
  </section>
);

/** 预测提示使用的单一图标和语义配色，避免分散在调用位置判断。 */
function ForecastStatusIcon({
  tone,
}: {
  tone: "ok" | "warn" | "risk";
}): React.ReactNode {
  if (tone === "risk") {
    return <ShieldAlert className="h-4 w-4 text-red-600 dark:text-red-400" />;
  }
  if (tone === "warn") {
    return (
      <TrendingUp className="h-4 w-4 text-amber-600 dark:text-amber-400" />
    );
  }
  return (
    <TrendingDown className="h-4 w-4 text-emerald-600 dark:text-emerald-400" />
  );
}

/** 渲染单个 5 小时/每周用量窗口。 */
const UsageWindowCard: React.FC<UsageWindowCardProps> = ({ tier }) => {
  const used = clampPercent(tier.utilization);
  const remaining = Math.max(0, 100 - used);
  const label = TIER_LABELS[tier.name] ?? tier.name;
  const forecast = buildWindowForecast(tier);
  const recommendation = getWindowRecommendation(tier, forecast);

  return (
    <section className={`rounded-lg border p-4 ${USAGE_PAGE_COLORS.card}`}>
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-foreground">{label}</div>
          <div className="mt-1 text-xs text-muted-foreground">
            重置时间：{formatDateTime(tier.resetsAt)}
          </div>
        </div>
        <div className={`text-right ${capacityTone(used)}`}>
          <div className="text-2xl font-semibold tabular-nums">
            {Math.round(remaining)}%
          </div>
          <div className="text-xs font-medium">剩余</div>
        </div>
      </div>

      <div
        className={`h-2 overflow-hidden rounded-full ${USAGE_PAGE_COLORS.progressTrack}`}
      >
        <div
          className={`h-full rounded-full transition-all ${capacityBarTone(used)}`}
          style={{ width: `${used}%` }}
        />
      </div>

      <div className="mt-3 flex items-center justify-between text-xs">
        <span className={capacityTone(used)}>{describeCapacity(used)}</span>
        <span className="text-muted-foreground tabular-nums">
          已用 {Math.round(used)}%
        </span>
      </div>

      <div
        className={`mt-4 rounded-md border px-3 py-2.5 ${
          recommendation.tone === "risk"
            ? USAGE_PAGE_COLORS.warning
            : USAGE_PAGE_COLORS.inset
        }`}
      >
        <div className="flex items-start gap-2">
          <ForecastStatusIcon tone={recommendation.tone} />
          <div className="min-w-0">
            <div className="text-xs font-semibold">{recommendation.title}</div>
            <div className="mt-1 text-xs leading-5 text-muted-foreground">
              {recommendation.detail}
            </div>
          </div>
        </div>
        {forecast && (
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-current/10 pt-2 text-[11px] text-muted-foreground">
            <span className="tabular-nums">
              平均 {forecast.percentPerHour.toFixed(1)}% / 小时
            </span>
            <span>
              置信度：{forecast.confidence === "medium" ? "中" : "低"}
            </span>
          </div>
        )}
      </div>
    </section>
  );
};

/**
 * 本地日志分析区：把 token、模型和缓存数据独立于官方窗口展示。
 *
 * 该区域只描述本机已同步的 Codex 会话/代理日志，避免与官方百分比混用。
 */
const LocalUsageAnalytics: React.FC<{
  summary: UsageSummary | undefined;
  modelStats: ModelStats[] | undefined;
  isLoading: boolean;
}> = ({ summary, modelStats, isLoading }) => {
  const topModels = getTopModels(modelStats);
  const tokensPerHour = summary
    ? summary.realTotalTokens / getElapsedTodayHours()
    : 0;
  const cachePercent = summary
    ? Math.round(Math.max(0, Math.min(summary.cacheHitRate, 1)) * 100)
    : 0;
  const heavyModel = topModels[0];
  const highBurn =
    tokensPerHour >= 200000 || (summary?.totalRequests ?? 0) >= 50;
  const topModelsTotalTokens = topModels.reduce(
    (total, item) => total + item.totalTokens,
    0,
  );

  return (
    <section className={`rounded-lg border p-5 ${USAGE_PAGE_COLORS.analytics}`}>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex items-start gap-2">
          <LineChart className="mt-0.5 h-4 w-4 text-violet-700 dark:text-violet-300" />
          <div>
            <h3 className="text-base font-semibold text-foreground">
              本地消耗节奏
            </h3>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              来自本机已同步的 Codex 会话与代理日志，仅用于识别 token
              和模型消耗趋势，不换算官方窗口额度。
            </p>
          </div>
        </div>
        <span className="inline-flex w-fit items-center gap-1 rounded-md border border-violet-200 bg-white/80 px-2 py-1 text-xs text-violet-800 dark:border-violet-800/70 dark:bg-slate-950/35 dark:text-violet-200">
          <Database className="h-3.5 w-3.5" />
          本地日志口径
        </span>
      </div>

      {isLoading && !summary ? (
        <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
          <LoaderCircle className="h-4 w-4 animate-spin" />
          正在汇总本地 Codex 使用记录...
        </div>
      ) : !summary || summary.totalRequests === 0 ? (
        <div
          className={`mt-4 rounded-md border border-dashed px-3 py-4 text-sm text-muted-foreground ${USAGE_PAGE_COLORS.analyticsInset}`}
        >
          暂无可分析的本地 Codex 使用记录。完成一次 Codex
          会话同步或通过本机代理发起请求后，这里会显示 token 速度和模型分布。
        </div>
      ) : (
        <>
          <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <div
              className={`rounded-md border p-3 ${USAGE_PAGE_COLORS.analyticsInset}`}
            >
              <div className="text-xs text-muted-foreground">今日 token</div>
              <div className="mt-1 text-xl font-semibold tabular-nums">
                {formatTokens(summary.realTotalTokens)}
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                {summary.totalRequests} 次请求
              </div>
            </div>
            <div
              className={`rounded-md border p-3 ${USAGE_PAGE_COLORS.analyticsInset}`}
            >
              <div className="text-xs text-muted-foreground">当前消耗速度</div>
              <div className="mt-1 text-xl font-semibold tabular-nums">
                {formatTokens(tokensPerHour)}
                <span className="ml-1 text-xs font-medium text-muted-foreground">
                  token / 小时
                </span>
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                按今日已过时间平均
              </div>
            </div>
            <div
              className={`rounded-md border p-3 ${USAGE_PAGE_COLORS.analyticsInset}`}
            >
              <div className="text-xs text-muted-foreground">缓存命中率</div>
              <div className="mt-1 text-xl font-semibold tabular-nums">
                {cachePercent}%
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                {cachePercent < 30 ? "重复上下文可优先复用" : "上下文复用正常"}
              </div>
            </div>
            <div
              className={`rounded-md border p-3 ${USAGE_PAGE_COLORS.analyticsInset}`}
            >
              <div className="text-xs text-muted-foreground">请求成功率</div>
              <div className="mt-1 text-xl font-semibold tabular-nums">
                {Math.round(summary.successRate)}%
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                已计入成功请求
              </div>
            </div>
          </div>

          <div
            className={`mt-4 rounded-md border p-3 ${USAGE_PAGE_COLORS.analyticsInset}`}
          >
            <div className="flex items-start gap-2">
              <Sparkles className="mt-0.5 h-4 w-4 text-violet-700 dark:text-violet-300" />
              <div className="min-w-0 text-sm">
                <div className="font-semibold text-foreground">
                  {highBurn
                    ? "建议先降低高上下文任务密度"
                    : "本地 token 节奏处于可观察范围"}
                </div>
                <p className="mt-1 leading-5 text-muted-foreground">
                  {highBurn
                    ? `当前约 ${formatTokens(tokensPerHour)} token / 小时。将大任务拆分、避免重复粘贴长上下文，并把轻量任务分配到低成本模型可降低消耗。`
                    : heavyModel
                      ? `${heavyModel.model} 是近 7 天 token 消耗最多的模型，占展示模型总量约 ${Math.round((heavyModel.totalTokens / Math.max(topModelsTotalTokens, 1)) * 100)}%。`
                      : "继续积累本地会话记录后，可获得模型级别的消耗建议。"}
                </p>
              </div>
            </div>
          </div>

          <div className="mt-4">
            <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
              <BarChart3 className="h-4 w-4 text-violet-700 dark:text-violet-300" />
              近 7 天模型分布
            </div>
            {topModels.length > 0 ? (
              <div className="divide-y rounded-md border bg-white/70 dark:divide-slate-700/70 dark:bg-slate-950/20">
                {topModels.map((item) => {
                  const share = Math.min(
                    100,
                    Math.round(
                      (item.totalTokens / Math.max(topModelsTotalTokens, 1)) *
                        100,
                    ),
                  );
                  return (
                    <div
                      key={item.model}
                      className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 px-3 py-2.5 text-sm sm:grid-cols-[minmax(0,1fr)_110px_90px]"
                    >
                      <div className="min-w-0">
                        <div className="truncate font-medium text-foreground">
                          {item.model}
                        </div>
                        <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-violet-100 dark:bg-violet-950/60">
                          <div
                            className="h-full rounded-full bg-violet-500"
                            style={{ width: `${share}%` }}
                          />
                        </div>
                      </div>
                      <div className="text-right text-xs text-muted-foreground tabular-nums">
                        {formatTokens(item.totalTokens)} token
                      </div>
                      <div className="text-right text-xs text-muted-foreground tabular-nums">
                        {item.requestCount} 次请求
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="text-sm text-muted-foreground">
                本地日志没有返回可展示的模型级 token 记录。
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
};

/** 渲染已接入 CCSwitchMulti 的设备汇总，避免将 token 错当官方窗口额度。 */
const QuotaCollaborationPanel: React.FC<{
  overview: QuotaCollaborationOverview | undefined;
  isLoading: boolean;
  onSync: () => void;
  isSyncing: boolean;
}> = ({ overview, isLoading, onSync, isSyncing }) => {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [deviceName, setDeviceName] = useState("");
  const [mode, setMode] = useState<"observe" | "enforce">("observe");
  const [threshold, setThreshold] = useState(20);
  const [confirmEnforceOpen, setConfirmEnforceOpen] = useState(false);
  const [isSaving, setIsSaving] = useState(false);

  /** 以持久化配置为准同步编辑控件，避免概览缓存短暂落后时覆盖用户输入。 */
  useEffect(() => {
    if (!settings?.quotaCollaboration) return;
    const config = settings.quotaCollaboration;
    setDeviceName(config.deviceName ?? "");
    setMode(config.mode ?? "observe");
    setThreshold(
      Math.min(90, Math.max(1, config.enforceRemainingPercent ?? 20)),
    );
  }, [settings?.quotaCollaboration]);

  /** 保存本机协作配置，并让页面同时刷新设置和远端报告缓存。 */
  const saveCollaborationSettings = async (
    nextMode: "observe" | "enforce" = mode,
  ) => {
    if (!settings) {
      toast.error("设置尚未加载完成，请稍后重试。");
      return;
    }
    setIsSaving(true);
    try {
      const current = settings.quotaCollaboration;
      await settingsApi.save({
        ...settings,
        quotaCollaboration: {
          deviceId: current?.deviceId ?? overview?.deviceId ?? "",
          deviceName: deviceName.trim(),
          mode: nextMode,
          enforceRemainingPercent: Math.min(90, Math.max(1, threshold)),
          latestWindowUtilization: current?.latestWindowUtilization ?? {},
          latestWindowCapturedAt: current?.latestWindowCapturedAt ?? null,
        },
      });
      setMode(nextMode);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings"] }),
        queryClient.invalidateQueries({
          queryKey: ["usage", "quota-collaboration"],
        }),
      ]);
      toast.success("多设备协作设置已保存。");
    } catch (error) {
      toast.error(
        `保存多设备协作设置失败：${error instanceof Error ? error.message : "未知错误"}`,
      );
    } finally {
      setIsSaving(false);
    }
  };

  /** 在用户确认边界后才写入 enforce，防止误以为它能控制旁路流量。 */
  const confirmEnforce = () => {
    setConfirmEnforceOpen(false);
    void saveCollaborationSettings("enforce");
  };

  if (isLoading && !overview) {
    return (
      <section className={`rounded-lg border p-5 ${USAGE_PAGE_COLORS.card}`}>
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="h-4 w-4 animate-spin" />
          正在读取多设备协作缓存...
        </div>
      </section>
    );
  }

  const reports = overview?.reports ?? [];
  const todayTokens = reports.reduce(
    (sum, report) => sum + report.todayTokens,
    0,
  );
  const weekTokens = reports.reduce(
    (sum, report) => sum + report.sevenDayTokens,
    0,
  );
  const isEnforcing = overview?.mode === "enforce";
  return (
    <section className={`rounded-lg border p-5 ${USAGE_PAGE_COLORS.card}`}>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex items-start gap-2">
          <MonitorSmartphone className="mt-0.5 h-4 w-4 text-emerald-700 dark:text-emerald-300" />
          <div>
            <h3 className="text-base font-semibold text-foreground">
              多设备额度协作
            </h3>
            <p className="mt-1 text-xs leading-5 text-muted-foreground">
              官方窗口是同一 Codex 账号的总量；下表只汇总已接入 CCSwitchMulti
              的设备 token，不把两种口径相互换算。
            </p>
          </div>
        </div>
        <span
          className={`inline-flex w-fit items-center gap-1 rounded-md border px-2 py-1 text-xs ${
            isEnforcing
              ? "border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-700 dark:bg-amber-950/30 dark:text-amber-200"
              : "border-emerald-200 bg-emerald-50 text-emerald-800 dark:border-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200"
          }`}
        >
          <ShieldCheck className="h-3.5 w-3.5" />
          {isEnforcing ? "约束模式" : "观测模式"}
        </span>
      </div>
      <div className="mt-3">
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="gap-2"
          onClick={onSync}
          disabled={isSyncing}
        >
          <RefreshCw
            className={`h-3.5 w-3.5 ${isSyncing ? "animate-spin" : ""}`}
          />
          同步设备报告
        </Button>
      </div>

      <div className={`mt-4 rounded-md border p-3 ${USAGE_PAGE_COLORS.inset}`}>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <h4 className="text-sm font-semibold text-foreground">协作设置</h4>
            <p className="mt-0.5 text-xs text-muted-foreground">
              每台设备都使用同一 WebDAV 或 S3 目录；设备名称只影响列表显示。
            </p>
          </div>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label="打开多设备协作教程"
            title="打开多设备协作教程"
            onClick={() =>
              void settingsApi.openExternal(
                "https://github.com/BigStrongSun/ccswitchmulti/blob/main/docs/guides/codex-multi-device-quota-collaboration-zh.md",
              )
            }
          >
            <ExternalLink className="h-4 w-4" />
          </Button>
        </div>
        <div className="mt-3 grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
          <div className="space-y-1.5">
            <Label htmlFor="quota-collaboration-device-name">设备名称</Label>
            <Input
              id="quota-collaboration-device-name"
              value={deviceName}
              maxLength={64}
              placeholder="例如：办公室 Windows"
              onChange={(event) => setDeviceName(event.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label>协作模式</Label>
            <div className="flex rounded-md border p-1">
              <Button
                type="button"
                size="sm"
                variant={mode === "observe" ? "default" : "ghost"}
                onClick={() => setMode("observe")}
              >
                观测
              </Button>
              <Button
                type="button"
                size="sm"
                variant={mode === "enforce" ? "default" : "ghost"}
                onClick={() => {
                  if (mode !== "enforce") setConfirmEnforceOpen(true);
                }}
              >
                约束
              </Button>
            </div>
          </div>
        </div>
        {mode === "enforce" ? (
          <div className="mt-3 grid gap-2 md:grid-cols-[minmax(0,1fr)_96px] md:items-end">
            <div className="space-y-1.5">
              <Label htmlFor="quota-collaboration-threshold">
                窗口剩余阈值
              </Label>
              <input
                id="quota-collaboration-threshold"
                className="h-2 w-full cursor-pointer accent-amber-600"
                type="range"
                min="1"
                max="90"
                value={threshold}
                onChange={(event) => setThreshold(Number(event.target.value))}
              />
            </div>
            <Input
              aria-label="窗口剩余阈值百分比"
              type="number"
              min="1"
              max="90"
              value={threshold}
              onChange={(event) =>
                setThreshold(
                  Math.min(90, Math.max(1, Number(event.target.value) || 1)),
                )
              }
            />
          </div>
        ) : null}
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            onClick={() => void saveCollaborationSettings()}
            disabled={!settings || isSaving}
          >
            {isSaving ? "保存中..." : "保存协作设置"}
          </Button>
          {!overview?.configured ? (
            <span className="text-xs text-amber-700 dark:text-amber-300">
              先在设置中启用 WebDAV 或 S3，才能同步其它设备。
            </span>
          ) : null}
        </div>
      </div>

      <ol className="mt-4 grid gap-2 text-xs text-muted-foreground sm:grid-cols-2">
        <li
          className={`rounded-md border px-3 py-2 ${USAGE_PAGE_COLORS.inset}`}
        >
          1. 每台设备启用相同的 WebDAV 或 S3
        </li>
        <li
          className={`rounded-md border px-3 py-2 ${USAGE_PAGE_COLORS.inset}`}
        >
          2. 每台设备刷新一次官方额度
        </li>
        <li
          className={`rounded-md border px-3 py-2 ${USAGE_PAGE_COLORS.inset}`}
        >
          3. 分别点击同步设备报告
        </li>
        <li
          className={`rounded-md border px-3 py-2 ${USAGE_PAGE_COLORS.inset}`}
        >
          4. 返回任意设备确认列表已汇总
        </li>
      </ol>

      {overview?.warning ? (
        <div
          className={`mt-4 rounded-md border px-3 py-3 text-sm ${USAGE_PAGE_COLORS.warning}`}
        >
          {overview.warning}
        </div>
      ) : reports.length === 0 ? (
        <div
          className={`mt-4 rounded-md border border-dashed px-3 py-4 text-sm text-muted-foreground ${USAGE_PAGE_COLORS.inset}`}
        >
          当前只有本机缓存。完成一次官方额度刷新后会生成本机报告；配置协作同步后，其他设备会出现在这里。
        </div>
      ) : (
        <>
          <div className="mt-4 overflow-x-auto rounded-md border">
            <div className="min-w-[620px] divide-y">
              <div className="grid grid-cols-[minmax(150px,1fr)_110px_110px_80px_130px] gap-3 bg-slate-50 px-3 py-2 text-xs font-medium text-muted-foreground dark:bg-slate-900/50">
                <span>设备</span>
                <span className="text-right">今日 token</span>
                <span className="text-right">7 天 token</span>
                <span className="text-right">请求</span>
                <span className="text-right">最后上报</span>
              </div>
              {reports.map((report) => (
                <div
                  key={report.deviceId}
                  className="grid grid-cols-[minmax(150px,1fr)_110px_110px_80px_130px] gap-3 px-3 py-2.5 text-sm"
                >
                  <div className="min-w-0 truncate font-medium">
                    {report.deviceName}
                    {report.deviceId === overview?.deviceId ? "（本机）" : ""}
                  </div>
                  <span className="text-right tabular-nums">
                    {formatTokens(report.todayTokens)}
                  </span>
                  <span className="text-right tabular-nums">
                    {formatTokens(report.sevenDayTokens)}
                  </span>
                  <span className="text-right tabular-nums">
                    {report.sevenDayRequests}
                  </span>
                  <span className="text-right text-xs text-muted-foreground">
                    {formatDateTime(
                      new Date(report.capturedAt * 1000).toISOString(),
                    )}
                  </span>
                </div>
              ))}
            </div>
          </div>
          <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            <span>已覆盖 {reports.length} 台设备</span>
            <span>今日合计 {formatTokens(todayTokens)} token</span>
            <span>7 天合计 {formatTokens(weekTokens)} token</span>
          </div>
        </>
      )}
      <div
        className={`mt-4 rounded-md border px-3 py-2.5 text-xs leading-5 ${isEnforcing ? USAGE_PAGE_COLORS.warning : USAGE_PAGE_COLORS.inset}`}
      >
        {isEnforcing
          ? `窗口剩余不高于 ${Math.round(overview?.enforceRemainingPercent ?? 20)}% 时，本机网关会拒绝继续转发 Codex 请求。未经过 CCSwitchMulti 的原生 Codex App 仍不受此策略控制。`
          : "观测模式不会拦截请求。约束模式只对经过 CCSwitchMulti 网关的实例生效。"}
      </div>
      <ConfirmDialog
        isOpen={confirmEnforceOpen}
        title="启用约束模式？"
        message={
          "当官方窗口剩余不高于阈值时，本机 CCSwitchMulti 网关会拒绝继续转发 Codex 请求。\n\n直接使用原生 Codex App、CLI 或其它未经过 CCSwitchMulti 网关的请求不受此策略控制，仍会消耗同一账号额度。"
        }
        confirmText="我了解，启用约束"
        cancelText="保持观测"
        variant="info"
        onConfirm={confirmEnforce}
        onCancel={() => setConfirmEnforceOpen(false)}
      />
    </section>
  );
};

/** 渲染单条 banked reset credit 到期记录。 */
const ResetCreditRow: React.FC<ResetCreditRowProps> = ({ credit, index }) => {
  const urgency = resetCreditUrgency(credit.expiresAt);

  return (
    <div
      className={`grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-3 rounded-lg border px-3 py-2 text-sm ${USAGE_PAGE_COLORS.inset}`}
    >
      <div className="min-w-0">
        <div className="truncate font-medium text-foreground">
          {credit.title || `Reset ${index + 1}`}
        </div>
        <div className="text-xs text-muted-foreground">
          状态：{credit.status ?? "unknown"}
        </div>
      </div>
      <div className="text-xs text-muted-foreground tabular-nums">
        {formatDateTime(credit.expiresAt)}
      </div>
      <div className={`text-xs font-medium ${urgency.className}`}>
        {urgency.label}
      </div>
    </div>
  );
};

/** 渲染凭据缺失、过期或接口失败时的页面级状态。 */
function renderQuotaProblem(
  quota: SubscriptionQuota | undefined,
  loading: boolean,
  refetch: () => void,
): React.ReactNode {
  if (loading && !quota) {
    return (
      <div
        className={`rounded-lg border p-6 text-sm text-muted-foreground ${USAGE_PAGE_COLORS.card}`}
      >
        正在读取本机 Codex 登录状态...
      </div>
    );
  }

  if (!quota) return null;

  const isCredentialProblem =
    quota.credentialStatus === "not_found" ||
    quota.credentialStatus === "expired" ||
    quota.credentialStatus === "parse_error";

  if (!isCredentialProblem && quota.success) return null;

  const title = isCredentialProblem ? "Codex 登录不可用" : "额度查询失败";
  const detail =
    quota.credentialMessage ||
    quota.error ||
    "请确认 Codex Desktop / CLI 已登录，然后刷新。";

  return (
    <section
      className={`rounded-lg border p-5 text-sm shadow-sm ${USAGE_PAGE_COLORS.warning}`}
    >
      <div className="mb-3 flex items-center gap-2 font-semibold">
        <AlertCircle className="h-4 w-4" />
        {title}
      </div>
      <p className="mb-4 opacity-90">{detail}</p>
      <Button
        type="button"
        onClick={refetch}
        disabled={loading}
        size="sm"
        variant="outline"
        className={`gap-2 ${USAGE_PAGE_COLORS.warningButton}`}
      >
        <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
        重新刷新
      </Button>
    </section>
  );
}

/** Codex 用量与 banked reset credits 的独立工具页。 */
export const CodexUsagePage: React.FC = () => {
  const {
    data: quota,
    isFetching,
    refetch,
  } = useSubscriptionQuota("codex", true, true, 5);
  const {
    data: todayUsage,
    isFetching: isTodayUsageFetching,
    refetch: refetchTodayUsage,
  } = useUsageSummary(TODAY_RANGE, { appType: "codex" });
  const { data: modelStats, refetch: refetchModelStats } = useModelStats(
    SEVEN_DAY_RANGE,
    { appType: "codex" },
  );
  const {
    data: quotaCollaboration,
    isFetching: isQuotaCollaborationFetching,
    refetch: refetchQuotaCollaboration,
  } = useQuotaCollaborationOverview();
  const syncQuotaCollaboration = useSyncQuotaCollaboration();
  const visibleTiers = getVisibleTiers(quota);
  const availableCredits = (quota?.resetCredits?.credits ?? []).filter(
    isAvailableCredit,
  );
  const availableCount = Math.max(quota?.resetCredits?.availableCount ?? 0, 0);
  const missingExpiryCount = Math.max(
    availableCount - availableCredits.length,
    0,
  );
  const problem = renderQuotaProblem(quota, isFetching, () => void refetch());

  return (
    <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 px-6 py-6">
      <section
        className={`overflow-hidden rounded-lg border ${USAGE_PAGE_COLORS.shell}`}
      >
        <div
          className={`flex flex-col gap-4 px-4 py-3 lg:flex-row lg:items-center lg:justify-between ${USAGE_PAGE_COLORS.header}`}
        >
          <div className="min-w-0 space-y-2">
            <div className="flex items-center gap-2 text-base font-semibold">
              <Gauge className="h-4 w-4 text-sky-600 dark:text-blue-300" />
              Codex 用量与重置额度
            </div>
            <p className="max-w-4xl text-xs leading-5 text-muted-foreground dark:text-slate-400">
              这里查看的是当前 Codex 登录账号的速率窗口和已存 reset
              额度。页面只读：不会兑换 reset、不会修改账号，也不会写入 Codex
              配置。
            </p>
            <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span
                className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 ${USAGE_PAGE_COLORS.chip}`}
              >
                Codex 工具栏
                <ArrowRight className="h-3 w-3" />
                柱状图按钮
              </span>
              <span
                className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 ${USAGE_PAGE_COLORS.chip}`}
              >
                只读查询
              </span>
              <span
                className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 ${USAGE_PAGE_COLORS.chip}`}
              >
                自动 5 分钟刷新
              </span>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3 text-sm">
            <div
              className={`inline-flex items-center gap-2 rounded-lg border px-3 py-2 ${USAGE_PAGE_COLORS.chip}`}
            >
              <Clock className="h-4 w-4" />
              {formatCheckedAt(quota?.queriedAt)}
            </div>
            <Button
              type="button"
              onClick={() => {
                void refetch();
                void refetchTodayUsage();
                void refetchModelStats();
                void refetchQuotaCollaboration();
              }}
              disabled={isFetching}
              size="sm"
              className="gap-2 bg-sky-600 hover:bg-sky-500 dark:bg-blue-600 dark:hover:bg-blue-500"
            >
              <RefreshCw
                className={`h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
              />
              刷新
            </Button>
          </div>
        </div>
      </section>

      <UsageGuidePanel />

      {problem}

      {quota?.success && (
        <>
          <div className="grid gap-4 lg:grid-cols-2">
            {visibleTiers.map((tier) => (
              <UsageWindowCard key={tier.name} tier={tier} />
            ))}
            {visibleTiers.length === 0 && (
              <section
                className={`rounded-lg border p-5 text-sm text-muted-foreground lg:col-span-2 ${USAGE_PAGE_COLORS.card}`}
              >
                Codex 没有返回 5 小时或每周用量窗口。
              </section>
            )}
          </div>

          <LocalUsageAnalytics
            summary={todayUsage}
            modelStats={modelStats}
            isLoading={isTodayUsageFetching}
          />

          <QuotaCollaborationPanel
            overview={quotaCollaboration}
            isLoading={isQuotaCollaborationFetching}
            isSyncing={syncQuotaCollaboration.isPending}
            onSync={() => {
              void syncQuotaCollaboration.mutateAsync().catch((error) => {
                toast.error(
                  `同步设备报告失败：${error instanceof Error ? error.message : "请检查 WebDAV 或 S3 配置。"}`,
                );
              });
            }}
          />

          <section
            className={`rounded-lg border p-5 ${USAGE_PAGE_COLORS.card}`}
          >
            <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-center gap-2">
                <RotateCcw className="h-4 w-4 text-sky-600 dark:text-sky-400" />
                <h3 className="text-base font-semibold text-foreground">
                  已存 reset 额度
                </h3>
              </div>
              <div
                className={`inline-flex items-center gap-2 rounded-lg px-3 py-2 text-sm font-semibold ${USAGE_PAGE_COLORS.resetSummary}`}
              >
                <TimerReset className="h-4 w-4" />
                {availableCount} 个可用
              </div>
            </div>

            {availableCount > 0 ? (
              <div className="flex flex-col gap-2">
                {availableCredits.map((credit, index) => (
                  <ResetCreditRow
                    key={`${credit.expiresAt ?? "missing"}-${index}`}
                    credit={credit}
                    index={index}
                  />
                ))}
                {missingExpiryCount > 0 && (
                  <div
                    className={`rounded-lg border border-dashed px-3 py-2 text-sm text-muted-foreground ${USAGE_PAGE_COLORS.inset}`}
                  >
                    还有 {missingExpiryCount} 个可用 reset
                    没有返回可展示的到期明细。
                  </div>
                )}
              </div>
            ) : (
              <div
                className={`flex items-center gap-2 rounded-lg border px-3 py-3 text-sm text-muted-foreground ${USAGE_PAGE_COLORS.inset}`}
              >
                <CheckCircle2 className="h-4 w-4 text-emerald-600 dark:text-emerald-400" />
                当前没有已存 reset 额度。
              </div>
            )}

            {quota.resetCreditsError && (
              <div
                className={`mt-3 rounded-lg border px-3 py-2 text-sm ${USAGE_PAGE_COLORS.warning}`}
              >
                Reset credit 明细读取不完整：{quota.resetCreditsError}
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
};

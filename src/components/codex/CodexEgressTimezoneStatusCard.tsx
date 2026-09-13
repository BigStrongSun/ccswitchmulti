import { useCallback, useEffect, useState } from "react";
import {
  Clock3,
  Loader2,
  Network,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  codexEgressTimezoneApi,
  type CodexEgressMonitorStatus,
} from "@/lib/api/codexEgressTimezone";

function stateLabel(status: CodexEgressMonitorStatus) {
  switch (status.state) {
    case "disabled":
      return "尚未开启";
    case "not_tested":
      return "等待首次检测";
    case "checking":
      return "正在检测出口";
    case "ready":
      return "监测正常";
    case "restart_required":
      return "出口已变化，需要刷新 Codex";
    case "renderer_only":
      return "仅支持页面时区同步";
    case "error":
      return "自动检测失败";
  }
}

function triggerLabel(trigger?: string) {
  switch (trigger) {
    case "startup":
      return "启动前检查";
    case "periodic":
      return "定时检查";
    case "proxy_failure":
      return "转发波动触发";
    case "network_resume":
      return "网络/睡眠恢复触发";
    case "manual":
      return "手动检查";
    default:
      return "尚无记录";
  }
}

interface CodexEgressTimezoneStatusCardProps {
  onOpenCodexStatus?: () => void;
}

export function CodexEgressTimezoneStatusCard({
  onOpenCodexStatus,
}: CodexEgressTimezoneStatusCardProps) {
  const [status, setStatus] = useState<CodexEgressMonitorStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [requestError, setRequestError] = useState("");

  const refresh = useCallback(async () => {
    try {
      setStatus(await codexEgressTimezoneApi.monitorStatus());
      setRequestError("");
    } catch (error) {
      setRequestError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 30_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const probeNow = async () => {
    setLoading(true);
    setRequestError("");
    try {
      setStatus(await codexEgressTimezoneApi.triggerAutomaticProbe());
    } catch (error) {
      setRequestError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  };

  const warning = status?.state === "restart_required";
  const failed = status?.state === "error" || Boolean(requestError);
  return (
    <section
      className={`rounded-lg border px-4 py-3 ${
        warning
          ? "border-amber-500/40 bg-amber-500/10"
          : failed
            ? "border-destructive/40 bg-destructive/10"
            : "border-border bg-card"
      }`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-2">
          <div className="flex items-center gap-2 text-sm font-semibold">
            {warning || failed ? (
              <TriangleAlert className="h-4 w-4 text-amber-500" />
            ) : (
              <Network className="h-4 w-4 text-emerald-500" />
            )}
            Codex 出口环境自动监测
            <span className="rounded-full border bg-background/70 px-2 py-0.5 text-xs font-normal">
              {loading && !status
                ? "读取中"
                : status
                  ? stateLabel(status)
                  : "不可用"}
            </span>
          </div>
          <p className="text-xs leading-5 text-muted-foreground">
            {status?.state === "disabled"
              ? "请在设置 → 常规中开启“跟随出口自动监测”。开启后会在启动、转发波动、网络恢复和定时周期自动检查。"
              : status?.detectedTimezone
                ? `当前出口 ${status.detectedEgressIp ?? "未知"} · ${status.detectedTimezone}；${triggerLabel(status.lastTrigger)}，每 ${status.monitorIntervalMinutes} 分钟兜底检查。`
                : "尚未取得出口时区；自动监测不会阻塞模型请求。"}
          </p>
          {warning && (
            <p className="text-xs font-medium text-amber-800 dark:text-amber-200">
              新结果已经保存，但运行中的 app-server 仍使用旧 TZ。CCSM
              不会强制结束任务，请在没有运行任务时使用 Codex 状态页的安全刷新。
            </p>
          )}
          {status?.state === "renderer_only" && (
            <p className="text-xs font-medium text-amber-800 dark:text-amber-200">
              Windows 应用包启动无法传入进程时区，只会尝试同步页面时区；
              app-server 时区未覆盖，重复刷新无法解除此限制。
            </p>
          )}
          {(requestError || status?.lastError) && (
            <p
              className="text-xs text-destructive"
              role="status"
              aria-live="polite"
            >
              {requestError || status?.lastError}
            </p>
          )}
          <p className="flex items-center gap-1 text-[11px] text-muted-foreground">
            <Clock3 className="h-3 w-3" />
            检测本身不依赖 CDP；CDP 只用于可选的 Codex 页面时区同步和验证。
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {warning && onOpenCodexStatus && (
            <Button type="button" size="sm" onClick={onOpenCodexStatus}>
              打开安全刷新
            </Button>
          )}
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={loading || status?.state === "disabled"}
            onClick={() => void probeNow()}
          >
            {loading ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" />
            )}
            立即检测
          </Button>
        </div>
      </div>
    </section>
  );
}

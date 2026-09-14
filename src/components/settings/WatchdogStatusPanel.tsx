import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  CheckCircle2,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { watchdogApi, type WatchdogStatus } from "@/lib/api/watchdog";

/**
 * 只读的“看门狗与端口自检”面板：守护进程是否在跑、最近自动拉起次数、
 * 当前端口归属与占用诊断。数据来自后端 get_watchdog_status 命令。
 */
export function WatchdogStatusPanel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<WatchdogStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setStatus(await watchdogApi.getStatus());
      setFailed(false);
    } catch (error) {
      console.error("[WatchdogStatusPanel] Failed to load status", error);
      setFailed(true);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const supervisorBadge = !status
    ? null
    : !status.enabled
      ? {
          label: t("settings.watchdogStateDisabled"),
          variant: "secondary" as const,
        }
      : status.supervisorAlive
        ? {
            label: t("settings.watchdogStateRunning"),
            variant: "default" as const,
          }
        : {
            label: t("settings.watchdogStateStopped"),
            variant: "destructive" as const,
          };

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Activity className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.watchdogPanelTitle")}
        </h3>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="ml-auto h-7 w-7"
          title={t("settings.watchdogRefresh")}
          aria-label={t("settings.watchdogRefresh")}
          onClick={() => void load()}
          disabled={loading}
        >
          <RefreshCw
            className={loading ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"}
          />
        </Button>
      </div>

      {failed || !status ? (
        <p className="text-xs text-destructive">
          {t("settings.watchdogLoadFailed")}
        </p>
      ) : (
        <div className="space-y-2 text-xs">
          <div className="flex flex-wrap items-center gap-2">
            <ShieldCheck className="h-3.5 w-3.5 text-emerald-500" />
            <span className="text-muted-foreground">
              {t("settings.watchdogSupervisor")}
            </span>
            {supervisorBadge ? (
              <Badge variant={supervisorBadge.variant}>
                {supervisorBadge.label}
              </Badge>
            ) : null}
            {status.supervisorPid ? (
              <span className="font-mono text-muted-foreground">
                PID {status.supervisorPid}
              </span>
            ) : null}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <span className="text-muted-foreground">
              {t("settings.watchdogRestartsWindow")}
            </span>
            <span className="font-mono">{status.restartsInWindow}</span>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            {status.portReady ? (
              <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
            ) : (
              <ShieldAlert className="h-3.5 w-3.5 text-amber-500" />
            )}
            <span className="text-muted-foreground">
              {t("settings.watchdogPortLabel")}
            </span>
            <span className="font-mono">
              {status.port > 0 ? `127.0.0.1:${status.port}` : "-"}
            </span>
            <Badge variant={status.portReady ? "default" : "destructive"}>
              {status.portReady
                ? t("settings.watchdogPortReady")
                : t("settings.watchdogPortNotReady")}
            </Badge>
            {status.listenerPid ? (
              <span className="font-mono text-muted-foreground">
                PID {status.listenerPid}
              </span>
            ) : null}
          </div>

          {!status.portReady && status.portDiagnosis ? (
            <div className="space-y-1">
              <p className="text-muted-foreground">
                {t("settings.watchdogDiagnosis")}
              </p>
              <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted/50 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
                {status.portDiagnosis}
              </pre>
            </div>
          ) : null}

          <div className="space-y-1">
            <p className="text-muted-foreground">
              {t("settings.watchdogRecentEvents")}
            </p>
            {status.recentEvents.length === 0 ? (
              <p className="text-muted-foreground">
                {t("settings.watchdogNoEvents")}
              </p>
            ) : (
              <ul className="space-y-0.5 font-mono text-[11px] text-muted-foreground">
                {status.recentEvents.map((event, index) => (
                  <li key={`${event.timestamp}-${event.event}-${index}`}>
                    <span>{event.timestamp}</span>{" "}
                    <span
                      className={
                        event.level === "error" ? "text-destructive" : ""
                      }
                    >
                      {event.event}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

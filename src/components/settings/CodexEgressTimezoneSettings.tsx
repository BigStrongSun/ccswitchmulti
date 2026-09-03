import { useState } from "react";
import { Clock3, Loader2, Network, TriangleAlert } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  codexEgressTimezoneApi,
  type CodexEgressTimezoneDetection,
  type CodexRuntimeTimezoneInspection,
} from "@/lib/api/codexEgressTimezone";
import type { CodexEgressTimezoneSettings as CodexEgressTimezoneValue } from "@/types";

interface CodexEgressTimezoneSettingsProps {
  value?: CodexEgressTimezoneValue;
  onChange: (
    value: CodexEgressTimezoneValue,
  ) => boolean | void | Promise<boolean | void>;
}

function comparisonLabel(report: CodexEgressTimezoneDetection) {
  if (report.timezoneMatch === "exact") {
    return "IANA 时区和当前 UTC 偏移都一致。";
  }
  if (report.timezoneMatch === "offset_match") {
    return "IANA 名称不同，但当前 UTC 偏移相同；这不等于已确认存在时区冲突。";
  }
  if (report.timezoneMatch === "mismatch") {
    return "IANA 时区和当前 UTC 偏移均不一致。若要尝试对齐，请显式启用探测结果。";
  }
  return "无法识别当前应用时区，不能自动判断是否一致。";
}

function locationLabel(report: CodexEgressTimezoneDetection) {
  return [report.countryCode, report.region, report.city, report.colo]
    .filter(Boolean)
    .join(" · ");
}

function looksLikeIanaTimezone(value: string) {
  return /^[A-Za-z][A-Za-z0-9_+\-]*(\/[A-Za-z0-9_+\-]+)+$/.test(value.trim());
}

export function CodexEgressTimezoneSettings({
  value = { mode: "off" },
  onChange,
}: CodexEgressTimezoneSettingsProps) {
  const [report, setReport] = useState<CodexEgressTimezoneDetection | null>(
    null,
  );
  const [detecting, setDetecting] = useState(false);
  const [error, setError] = useState("");
  const [runtime, setRuntime] = useState<CodexRuntimeTimezoneInspection | null>(
    null,
  );
  const [inspectingRuntime, setInspectingRuntime] = useState(false);
  const [manualVisible, setManualVisible] = useState(value.mode === "manual");
  const [manualTimezone, setManualTimezone] = useState(
    value.manualTimezone ?? "",
  );

  const detect = async () => {
    setDetecting(true);
    setError("");
    try {
      setReport(await codexEgressTimezoneApi.detect());
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setDetecting(false);
    }
  };

  const applyDetection = async () => {
    if (!report) return;
    await onChange({
      ...value,
      mode: "auto",
      detectedTimezone: report.egressTimezone,
      detectedAt: report.checkedAt,
      detectedEgressIp: report.egressIp,
      detectedCountryCode: report.countryCode,
      detectedRegion: report.region,
      detectedCity: report.city,
      detectedColo: report.colo,
    });
  };

  const inspectRuntime = async () => {
    setInspectingRuntime(true);
    setError("");
    try {
      setRuntime(await codexEgressTimezoneApi.inspectRuntime());
    } catch (caught) {
      setRuntime(null);
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setInspectingRuntime(false);
    }
  };

  const saveManual = async () => {
    const timezone = manualTimezone.trim();
    if (!looksLikeIanaTimezone(timezone)) {
      setError("请输入 IANA 时区，例如 Asia/Taipei 或 America/Los_Angeles。");
      return;
    }
    setError("");
    try {
      await codexEgressTimezoneApi.validate(timezone);
      await onChange({ ...value, mode: "manual", manualTimezone: timezone });
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  };

  const activeTimezone =
    value.mode === "auto"
      ? value.detectedTimezone
      : value.mode === "manual"
        ? value.manualTimezone
        : undefined;

  return (
    <section className="space-y-4 rounded-xl border border-border/60 bg-muted/10 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <Clock3 className="h-4 w-4 text-sky-500" />
            <h3 className="text-sm font-medium">Codex 出口时区（实验）</h3>
          </div>
          <p className="max-w-3xl text-xs text-muted-foreground">
            CCSwitchMulti 启动 Codex 时给子进程注入 TZ，并通过受管 CDP 对页面
            renderer 应用同一 IANA 时区；不会修改 Windows 系统时区。已运行的
            Codex 需要完全退出后再由 CCSM 拉起才会完整生效。
          </p>
        </div>
        <span className="rounded-full border px-2 py-1 text-xs text-muted-foreground">
          {value.mode === "off"
            ? "已关闭"
            : `${value.mode === "auto" ? "跟随探测" : "手动"} · ${activeTimezone ?? "未配置"}`}
        </span>
      </div>

      <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-200">
        <div className="flex gap-2">
          <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
          <p>
            没有官方证据证明时区不一致一定导致模型降级；该开关只用于复现和验证社区报告，默认关闭，不会自动改动你的运行环境。
          </p>
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          onClick={detect}
          disabled={detecting}
        >
          {detecting ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <Network className="mr-2 h-4 w-4" />
          )}
          {detecting ? "探测中" : "探测出口时区"}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => setManualVisible((shown) => !shown)}
        >
          手动设置
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={inspectRuntime}
          disabled={inspectingRuntime}
        >
          {inspectingRuntime && (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          )}
          {inspectingRuntime ? "检查中" : "检查运行中 Codex 页面"}
        </Button>
        {value.mode !== "off" && (
          <Button
            type="button"
            variant="ghost"
            onClick={() => onChange({ ...value, mode: "off" })}
          >
            关闭时区注入
          </Button>
        )}
      </div>

      <p className="text-xs text-muted-foreground">
        探测会访问 chatgpt.com/cdn-cgi/trace，并把该站点实际看到的公网出口 IP
        发送给 ipwho.is 查询 IANA 时区；不会发送 API Key、Cookie 或对话内容。
      </p>

      <p className="text-xs text-muted-foreground">
        “检查运行中 Codex 页面”读取的是当前 renderer 的实际 IANA 时区和 UTC
        偏移。app-server 只有在由 CCSM 直接拉起时才会继承
        TZ；因此完整验证仍需先完全退出 Codex，再由 CCSM 启动。
      </p>

      {error && (
        <p
          className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive"
          role="alert"
        >
          {error}
        </p>
      )}

      {runtime && (
        <p className="rounded-md border border-border-default bg-background/70 p-2 text-xs">
          运行中的 Codex renderer：{runtime.runtimeTimezone} (
          {runtime.runtimeUtcOffset})
          {runtime.matchesConfigured === true
            ? "，与当前配置一致。"
            : runtime.timezoneMatch === "offset_match"
              ? `，IANA 名称与配置 ${runtime.configuredTimezone} 不同，但当前 UTC 偏移相同。`
              : runtime.matchesConfigured === false
                ? `，与当前配置 ${runtime.configuredTimezone ?? "关闭"} 不一致；请完全退出 Codex 后由 CCSM 重新拉起。`
                : "；当前未启用时区注入。"}
        </p>
      )}

      {report && (
        <div className="space-y-3 rounded-lg border bg-background/70 p-3 text-xs">
          <div className="grid gap-2 sm:grid-cols-2">
            <p>
              <span className="text-muted-foreground">
                当前系统/CCSM 时区：
              </span>
              <strong>{report.currentTimezone}</strong> (
              {report.currentUtcOffset})
            </p>
            <p>
              <span className="text-muted-foreground">ChatGPT 出口时区：</span>
              <strong>{report.egressTimezone}</strong> ({report.egressUtcOffset}
              )
            </p>
            <p>
              <span className="text-muted-foreground">出口位置：</span>
              {locationLabel(report) || "未知"}
            </p>
            <p>
              <span className="text-muted-foreground">公网出口：</span>
              {report.egressIp}
            </p>
          </div>
          <p className="font-medium">{comparisonLabel(report)}</p>
          {report.dnsUsesNonPublicAddress && (
            <p className="rounded bg-sky-500/10 p-2 text-sky-800 dark:text-sky-200">
              DNS 返回 {report.dnsAddresses.join("、")}，属于透明代理常见的
              fake-IP；CCSM 不会拿它做地理定位，而是使用 chatgpt.com
              实际看到的公网出口。
            </p>
          )}
          <Button type="button" onClick={applyDetection}>
            使用探测结果
          </Button>
        </div>
      )}

      {manualVisible && (
        <div className="flex flex-col gap-2 rounded-lg border bg-background/70 p-3 sm:flex-row sm:items-end">
          <div className="flex-1 space-y-1">
            <Label htmlFor="codex-manual-timezone">IANA 时区</Label>
            <Input
              id="codex-manual-timezone"
              value={manualTimezone}
              placeholder="Asia/Taipei"
              onChange={(event) => setManualTimezone(event.target.value)}
            />
          </div>
          <Button type="button" onClick={saveManual}>
            保存手动时区
          </Button>
        </div>
      )}
    </section>
  );
}

import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  Loader2,
  RotateCcw,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CodexConfigConsistencyReport } from "@/lib/api/codexConfigConsistency";
import type {
  CodexRuntimeRefreshPhase,
  CodexRuntimeRefreshWorkflow,
} from "@/hooks/useCodexConfigConsistency";

type CodexRuntimeRefreshView = Pick<
  CodexRuntimeRefreshWorkflow,
  "phase" | "preflight" | "progress"
> &
  Partial<
    Pick<
      CodexRuntimeRefreshWorkflow,
      "error" | "result" | "rendererRetryPending"
    >
  >;

interface CodexConfigConsistencyDialogProps {
  report: CodexConfigConsistencyReport | null;
  pending: boolean;
  error: string | null;
  onApply: () => void;
  onKeep: () => void;
  onLater: () => void;
  onRetry: () => void;
  refresh?: CodexRuntimeRefreshView;
  onInspectRefresh?: () => void;
  onConfirmRefresh?: () => void;
  onCancelRefresh?: () => void;
  onRetryRendererCompatibility?: () => void;
}

const REFRESH_STAGE_ORDER = [
  "closing",
  "repairing_history",
  "applying_config",
  "launching",
  "verifying",
] as const;

function refreshStageIndex(stage?: string): number {
  if (stage === "force_closing") return 0;
  if (stage === "completed") return REFRESH_STAGE_ORDER.length;
  return REFRESH_STAGE_ORDER.indexOf(
    stage as (typeof REFRESH_STAGE_ORDER)[number],
  );
}

export function CodexConfigConsistencyDialog({
  report,
  pending,
  error,
  onApply,
  onKeep,
  onLater,
  onRetry,
  refresh = { phase: "idle", preflight: null, progress: null },
  onInspectRefresh,
  onConfirmRefresh,
  onCancelRefresh,
  onRetryRendererCompatibility,
}: CodexConfigConsistencyDialogProps) {
  const { t } = useTranslation();
  const refreshPhase: CodexRuntimeRefreshPhase = refresh.phase;
  if (refreshPhase !== "idle") {
    const inspecting = refreshPhase === "inspecting";
    const showingStatus = refreshPhase === "status";
    const confirming = refreshPhase === "confirm";
    const refreshing = refreshPhase === "refreshing";
    const completed = refreshPhase === "completed";
    const failed = refreshPhase === "failed";
    const completedWithWarnings =
      completed && refresh.result?.outcome === "completed_with_warnings";
    const currentStageIndex = refreshStageIndex(refresh.progress?.stage);
    const stageLabels = [
      t("codexConfigConsistency.stageClosing"),
      t("codexConfigConsistency.stageRepairingHistory"),
      t("codexConfigConsistency.stageApplyingConfig"),
      t("codexConfigConsistency.stageLaunching"),
      t("codexConfigConsistency.stageVerifying"),
    ];

    return (
      <Dialog open>
        <DialogContent className="max-w-lg" zIndex="top">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {failed ? (
                <XCircle className="h-5 w-5 text-destructive" />
              ) : completedWithWarnings || showingStatus ? (
                <AlertTriangle className="h-5 w-5 text-amber-500" />
              ) : completed ? (
                <CheckCircle2 className="h-5 w-5 text-emerald-500" />
              ) : inspecting || refreshing ? (
                <Loader2 className="h-5 w-5 animate-spin text-blue-500" />
              ) : (
                <AlertTriangle className="h-5 w-5 text-amber-500" />
              )}
              {inspecting
                ? t("codexConfigConsistency.refreshInspectingTitle")
                : showingStatus
                  ? t("codexConfigConsistency.statusTitle")
                  : confirming
                    ? t("codexConfigConsistency.refreshConfirmTitle")
                    : refreshing
                      ? t("codexConfigConsistency.refreshingTitle")
                      : completed
                        ? completedWithWarnings
                          ? t(
                              "codexConfigConsistency.refreshCompletedWithWarningsTitle",
                            )
                          : t("codexConfigConsistency.refreshCompletedTitle")
                        : t("codexConfigConsistency.refreshFailedTitle")}
            </DialogTitle>
            <DialogDescription>
              {inspecting
                ? t("codexConfigConsistency.refreshInspectingDescription")
                : showingStatus
                  ? t("codexConfigConsistency.statusDescription")
                  : confirming
                    ? t("codexConfigConsistency.refreshConfirmDescription")
                    : refreshing
                      ? t("codexConfigConsistency.refreshingDescription")
                      : completed
                        ? completedWithWarnings
                          ? t(
                              "codexConfigConsistency.refreshCompletedWithWarningsDescription",
                            )
                          : t(
                              "codexConfigConsistency.refreshCompletedDescription",
                            )
                        : t("codexConfigConsistency.refreshFailedDescription")}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-3 px-6 py-4 text-sm">
            {inspecting ? (
              <div className="flex items-center gap-3 rounded-md border bg-muted/20 p-4">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>{t("codexConfigConsistency.inspectingProcesses")}</span>
              </div>
            ) : null}

            {showingStatus && refresh.preflight ? (
              <div className="space-y-3">
                <div className="rounded-md border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <p className="font-medium">
                      {t("codexConfigConsistency.configStatus")}
                    </p>
                    <span
                      className={
                        report?.state === "external_drift" ||
                        report?.runtimeActivation?.state === "restart_required"
                          ? "text-amber-600 dark:text-amber-300"
                          : "text-emerald-600 dark:text-emerald-300"
                      }
                    >
                      {report?.state === "external_drift" ||
                      report?.runtimeActivation?.state === "restart_required"
                        ? t("codexConfigConsistency.statusWarning")
                        : t("codexConfigConsistency.statusReady")}
                    </span>
                  </div>
                </div>
                <div className="rounded-md border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <p className="font-medium">
                      {t("codexConfigConsistency.paginatedHistoryStatus")}
                    </p>
                    <span
                      className={
                        refresh.preflight.paginatedHistory
                          .affectedRolloutCount > 0 ||
                        refresh.preflight.paginatedHistory.blockedRolloutCount >
                          0
                          ? "text-amber-600 dark:text-amber-300"
                          : "text-emerald-600 dark:text-emerald-300"
                      }
                    >
                      {refresh.preflight.paginatedHistory.affectedRolloutCount >
                        0 ||
                      refresh.preflight.paginatedHistory.blockedRolloutCount > 0
                        ? t("codexConfigConsistency.statusWarning")
                        : t("codexConfigConsistency.statusReady")}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {refresh.preflight.paginatedHistory.affectedRolloutCount > 0
                      ? `${t("codexConfigConsistency.paginatedHistoryFiles")} ${refresh.preflight.paginatedHistory.affectedRolloutCount} · ${t("codexConfigConsistency.duplicateOrdinals")} ${refresh.preflight.paginatedHistory.duplicateOrdinalCount}`
                      : t("codexConfigConsistency.noPaginatedHistoryIssue")}
                  </p>
                </div>
                <div className="rounded-md border bg-muted/20 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <p className="font-medium">
                      {t("codexConfigConsistency.rendererCompatibilityStatus")}
                    </p>
                    <span className="text-muted-foreground">
                      {t("codexConfigConsistency.statusPending")}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t("codexConfigConsistency.rendererIndependentHint")}
                  </p>
                </div>
              </div>
            ) : null}

            {confirming && refresh.preflight ? (
              <>
                <div className="grid grid-cols-2 gap-3">
                  <p className="rounded-md border bg-muted/20 p-3">
                    {t("codexConfigConsistency.desktopProcesses")}{" "}
                    {refresh.preflight.desktopProcessCount}
                  </p>
                  <p className="rounded-md border bg-muted/20 p-3">
                    {t("codexConfigConsistency.appServerProcesses")}{" "}
                    {refresh.preflight.appServerProcessCount}
                  </p>
                </div>
                <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-amber-800 dark:text-amber-200">
                  {t("codexConfigConsistency.activeTasksWarning")}
                </div>
                {refresh.preflight.paginatedHistory.affectedRolloutCount > 0 ? (
                  <div className="rounded-md border border-blue-500/30 bg-blue-500/10 p-3 text-blue-800 dark:text-blue-200">
                    <p className="font-medium">
                      {t("codexConfigConsistency.paginatedHistoryRepair")}
                    </p>
                    <p className="mt-1 text-xs">
                      {t("codexConfigConsistency.paginatedHistoryFiles")}{" "}
                      {refresh.preflight.paginatedHistory.affectedRolloutCount}
                      {" · "}
                      {t("codexConfigConsistency.duplicateOrdinals")}{" "}
                      {refresh.preflight.paginatedHistory.duplicateOrdinalCount}
                    </p>
                  </div>
                ) : null}
                {refresh.preflight.paginatedHistory.blockedRolloutCount > 0 ? (
                  <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-amber-800 dark:text-amber-200">
                    {t("codexConfigConsistency.paginatedHistorySkipped")}{" "}
                    {refresh.preflight.paginatedHistory.blockedRolloutCount}
                  </div>
                ) : null}
                <p className="rounded-md border border-blue-500/30 bg-blue-500/10 p-3 text-blue-800 dark:text-blue-200">
                  {t("codexConfigConsistency.historyCompatibilityCheck")}
                </p>
                <p className="break-all text-xs text-muted-foreground">
                  {t("codexConfigConsistency.launchTarget")}{" "}
                  {refresh.preflight.launchTarget || t("common.unknown")}
                </p>
              </>
            ) : null}

            {refreshing ? (
              <div className="space-y-2">
                {stageLabels.map((label, index) => {
                  const done = index < currentStageIndex;
                  const active = index === currentStageIndex;
                  return (
                    <div
                      key={label}
                      className="flex items-center gap-3 rounded-md border bg-muted/20 p-3"
                    >
                      {done ? (
                        <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                      ) : active ? (
                        <Loader2 className="h-4 w-4 animate-spin text-blue-500" />
                      ) : (
                        <Circle className="h-4 w-4 text-muted-foreground/50" />
                      )}
                      <span className={done ? "text-muted-foreground" : ""}>
                        {label}
                      </span>
                    </div>
                  );
                })}
                {refresh.progress?.stage === "force_closing" ? (
                  <p className="text-xs text-amber-600 dark:text-amber-300">
                    {t("codexConfigConsistency.forceClosingHint")}
                  </p>
                ) : null}
              </div>
            ) : null}

            {completed ? (
              <div className="space-y-2">
                <div className="rounded-md border border-emerald-500/40 bg-emerald-500/10 p-3 text-emerald-800 dark:text-emerald-200">
                  <p>{t("codexConfigConsistency.configApplied")}</p>
                  <p className="mt-1 text-xs">
                    {t("codexConfigConsistency.paginatedHistoryReady")}
                  </p>
                </div>
                {refresh.result?.rendererCompatibilityStatus === "warning" ? (
                  <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-amber-800 dark:text-amber-200">
                    <p className="font-medium">
                      {t("codexConfigConsistency.rendererCompatibilityWarning")}
                    </p>
                    {refresh.result.rendererCompatibilityMessage ? (
                      <p className="mt-1 break-words text-xs">
                        {refresh.result.rendererCompatibilityMessage}
                      </p>
                    ) : null}
                    <p className="mt-1 text-xs">
                      {t("codexConfigConsistency.rendererIndependentHint")}
                    </p>
                  </div>
                ) : null}
                <div className="rounded-md border bg-muted/20 p-3">
                  {t("codexConfigConsistency.newTaskHint")}
                  {refresh.result?.forceTerminated ? (
                    <p className="mt-2 text-xs">
                      {t("codexConfigConsistency.forcedCloseUsed")}
                    </p>
                  ) : null}
                  {(refresh.result?.repairedHistoryRolloutCount ?? 0) > 0 ? (
                    <p className="mt-2 text-xs">
                      {t("codexConfigConsistency.historyRepairCompleted")}{" "}
                      {refresh.result?.repairedHistoryRolloutCount}
                      {" · "}
                      {t("codexConfigConsistency.duplicateOrdinals")}{" "}
                      {refresh.result?.repairedHistoryDuplicateCount}
                    </p>
                  ) : null}
                </div>
              </div>
            ) : null}

            {failed ? (
              <>
                {currentStageIndex >= 0 &&
                currentStageIndex < stageLabels.length ? (
                  <p className="text-sm font-medium">
                    {t("codexConfigConsistency.failedStage")}{" "}
                    {stageLabels[currentStageIndex]}
                  </p>
                ) : null}
                <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-destructive">
                  {refresh.error ||
                    t("codexConfigConsistency.refreshUnknownError")}
                </div>
              </>
            ) : null}
          </div>

          <DialogFooter className="gap-2">
            {showingStatus ? (
              <>
                <Button variant="outline" onClick={onCancelRefresh}>
                  {t("codexConfigConsistency.back")}
                </Button>
                {refresh.preflight?.supported &&
                refresh.preflight.canRefresh ? (
                  <Button onClick={onInspectRefresh}>
                    <RotateCcw className="mr-2 h-4 w-4" />
                    {t("codexConfigConsistency.refreshCodex")}
                  </Button>
                ) : null}
              </>
            ) : confirming ? (
              <>
                <Button variant="outline" onClick={onCancelRefresh}>
                  {t("codexConfigConsistency.cancelRefresh")}
                </Button>
                <Button onClick={onConfirmRefresh}>
                  {t("codexConfigConsistency.confirmRefresh")}
                </Button>
              </>
            ) : failed ? (
              <>
                <Button variant="outline" onClick={onCancelRefresh}>
                  {t("codexConfigConsistency.back")}
                </Button>
                <Button onClick={onInspectRefresh}>
                  <RotateCcw className="mr-2 h-4 w-4" />
                  {t("codexConfigConsistency.retryRefresh")}
                </Button>
              </>
            ) : completed ? (
              <>
                {refresh.result?.rendererCompatibilityStatus === "warning" &&
                onRetryRendererCompatibility ? (
                  <Button
                    variant="outline"
                    onClick={onRetryRendererCompatibility}
                    disabled={refresh.rendererRetryPending}
                  >
                    {refresh.rendererRetryPending ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <RotateCcw className="mr-2 h-4 w-4" />
                    )}
                    {t("codexConfigConsistency.retryRendererCompatibility")}
                  </Button>
                ) : null}
                <Button onClick={onCancelRefresh}>
                  {t("codexConfigConsistency.finish")}
                </Button>
              </>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  if (!report) return null;
  const runtimeRestartRequired =
    report.runtimeActivation?.state === "restart_required";
  const externalDrift = report.state === "external_drift";
  const takeoverProjectionDrift = report.reason === "takeover_projection_drift";
  if (!externalDrift && !runtimeRestartRequired) return null;

  if (!externalDrift && runtimeRestartRequired) {
    return (
      <Dialog
        open
        onOpenChange={(open) => {
          if (!open && !pending) onLater();
        }}
      >
        <DialogContent className="max-w-lg" zIndex="top">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-amber-500" />
              {t("codexConfigConsistency.runtimeTitle")}
            </DialogTitle>
            <DialogDescription>
              {t("codexConfigConsistency.runtimeDescription")}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3 px-6 py-2 text-sm">
            <p>{t("codexConfigConsistency.runtimeRestartHint")}</p>
            {error ? (
              <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
                {error}
              </div>
            ) : null}
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" onClick={onLater} disabled={pending}>
              {t("codexConfigConsistency.later")}
            </Button>
            <Button onClick={onRetry} disabled={pending}>
              {pending ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              {t("codexConfigConsistency.recheck")}
            </Button>
            {onInspectRefresh ? (
              <Button onClick={onInspectRefresh} disabled={pending}>
                {t("codexConfigConsistency.refreshCodex")}
              </Button>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onLater();
      }}
    >
      <DialogContent className="max-w-lg" zIndex="top">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <AlertTriangle className="h-5 w-5 text-amber-500" />
            {t("codexConfigConsistency.title")}
          </DialogTitle>
          <DialogDescription>
            {t("codexConfigConsistency.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 px-6 py-2 text-sm">
          <div>
            <span className="font-medium">
              {t("codexConfigConsistency.provider")}
            </span>{" "}
            <code>{report.providerId || t("common.unknown")}</code>
          </div>
          <div>
            <span className="font-medium">
              {t("codexConfigConsistency.changedKeys")}
            </span>
            <ul className="mt-2 max-h-32 space-y-1 overflow-y-auto rounded-md border bg-muted/20 p-2 font-mono text-xs">
              {report.changedKeys.map((key) => (
                <li key={key}>{key}</li>
              ))}
            </ul>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("codexConfigConsistency.noSecrets")}
          </p>
          {error ? (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              <p>{error}</p>
              <Button
                className="mt-2"
                size="sm"
                variant="outline"
                onClick={onRetry}
                disabled={pending}
              >
                {t("codexConfigConsistency.retry")}
              </Button>
            </div>
          ) : null}
        </div>

        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={onLater} disabled={pending}>
            {t("codexConfigConsistency.later")}
          </Button>
          {!takeoverProjectionDrift ? (
            <Button variant="outline" onClick={onKeep} disabled={pending}>
              {t("codexConfigConsistency.keepCodex")}
            </Button>
          ) : null}
          <Button onClick={onApply} disabled={pending}>
            {pending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("codexConfigConsistency.applyCcsm")}
          </Button>
          {onInspectRefresh ? (
            <Button onClick={onInspectRefresh} disabled={pending}>
              {t("codexConfigConsistency.applyAndRefresh")}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

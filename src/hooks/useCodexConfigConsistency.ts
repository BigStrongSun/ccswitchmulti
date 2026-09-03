import { useCallback, useEffect, useRef, useState } from "react";

import {
  codexConfigConsistencyApi,
  type CodexConfigConsistencyAction,
  type CodexConfigConsistencyReport,
  type CodexRuntimeRefreshPreflight,
  type CodexRuntimeRefreshProgress,
  type CodexRuntimeRefreshResult,
} from "@/lib/api/codexConfigConsistency";
import { extractErrorMessage } from "@/utils/errorUtils";
import { proxyApi } from "@/lib/api/proxy";
import { useTauriEvent } from "./useTauriEvent";

export type CodexRuntimeRefreshPhase =
  | "idle"
  | "inspecting"
  | "status"
  | "confirm"
  | "refreshing"
  | "completed"
  | "failed";

export interface CodexRuntimeRefreshWorkflow {
  phase: CodexRuntimeRefreshPhase;
  preflight: CodexRuntimeRefreshPreflight | null;
  progress: CodexRuntimeRefreshProgress | null;
  result: CodexRuntimeRefreshResult | null;
  error: string | null;
  rendererRetryPending?: boolean;
}

interface CodexConfigConsistencyState {
  report: CodexConfigConsistencyReport | null;
  pending: boolean;
  error: string | null;
  refresh: CodexRuntimeRefreshWorkflow;
  close: () => void;
  resolve: (action: CodexConfigConsistencyAction) => Promise<void>;
  recheck: () => Promise<void>;
  openStatusPanel: () => Promise<void>;
  inspectRuntimeRefresh: () => Promise<void>;
  confirmRuntimeRefresh: () => Promise<void>;
  retryRendererCompatibility: () => Promise<void>;
  cancelRuntimeRefresh: () => void;
}

const EMPTY_RUNTIME_REFRESH: CodexRuntimeRefreshWorkflow = {
  phase: "idle",
  preflight: null,
  progress: null,
  result: null,
  error: null,
};

export function useCodexConfigConsistency(): CodexConfigConsistencyState {
  const [report, setReport] = useState<CodexConfigConsistencyReport | null>(
    null,
  );
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState<CodexRuntimeRefreshWorkflow>(
    EMPTY_RUNTIME_REFRESH,
  );
  const seenFingerprintsRef = useRef(new Set<string>());
  const seenHistoryDamageRef = useRef(new Set<string>());
  const runtimeRefreshActiveRef = useRef(false);

  const acceptReport = useCallback((next: CodexConfigConsistencyReport) => {
    if (runtimeRefreshActiveRef.current) return;
    const attentionKey =
      next.state === "external_drift" && next.actualFingerprint
        ? `drift:${next.actualFingerprint}`
        : next.runtimeActivation?.state === "restart_required"
          ? `runtime:${next.runtimeActivation.appServerStartedAt ?? "unknown"}:${next.runtimeActivation.configModifiedAt ?? "unknown"}`
          : null;
    if (!attentionKey) {
      setReport(null);
      setError(null);
      return;
    }
    if (seenFingerprintsRef.current.has(attentionKey)) return;
    seenFingerprintsRef.current.add(attentionKey);
    setError(null);
    setReport(next);
  }, []);

  useTauriEvent<CodexConfigConsistencyReport>(
    "codex-config-consistency",
    acceptReport,
  );

  useTauriEvent<CodexRuntimeRefreshProgress>(
    "codex-runtime-refresh-progress",
    (progress) => {
      if (!runtimeRefreshActiveRef.current) return;
      setRefresh((current) => ({ ...current, progress }));
    },
  );

  useEffect(() => {
    let active = true;
    let requestPending = false;
    const inspect = async () => {
      if (runtimeRefreshActiveRef.current) return;
      if (requestPending) return;
      requestPending = true;
      try {
        const next = await codexConfigConsistencyApi.inspect();
        if (!active) return;
        acceptReport(next);
        const configNeedsAttention =
          next.state === "external_drift" ||
          next.runtimeActivation?.state === "restart_required";
        if (configNeedsAttention || runtimeRefreshActiveRef.current) return;
        const preflight =
          await codexConfigConsistencyApi.inspectRuntimeRefresh();
        if (!active || runtimeRefreshActiveRef.current) return;
        const history = preflight.paginatedHistory;
        const historyNeedsAttention =
          history.affectedRolloutCount > 0 || history.blockedRolloutCount > 0;
        if (!historyNeedsAttention) return;
        const historyKey = [
          history.duplicateOrdinalCount,
          history.rotatedThreadCount,
          history.rotatedSegmentCount,
          history.blockedRolloutCount,
          history.blockedReason ?? "",
        ].join(":");
        if (seenHistoryDamageRef.current.has(historyKey)) return;
        seenHistoryDamageRef.current.add(historyKey);
        runtimeRefreshActiveRef.current = true;
        setReport(next);
        setRefresh({
          phase: "status",
          preflight,
          progress: null,
          result: null,
          error: null,
        });
      } catch (cause) {
        if (active) console.debug("[CodexConsistency] inspect failed", cause);
      } finally {
        requestPending = false;
      }
    };
    void inspect();
    const interval = window.setInterval(() => void inspect(), 30_000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [acceptReport]);

  const close = useCallback(() => {
    runtimeRefreshActiveRef.current = false;
    setRefresh(EMPTY_RUNTIME_REFRESH);
    setReport(null);
    setError(null);
  }, []);

  const resolve = useCallback(
    async (action: CodexConfigConsistencyAction) => {
      if (!report) return;
      if (action === "later") {
        if (report.actualFingerprint) {
          void codexConfigConsistencyApi
            .resolve(report.actualFingerprint, action)
            .catch((cause) =>
              console.debug("[CodexConsistency] defer failed", cause),
            );
        }
        close();
        return;
      }
      if (!report.actualFingerprint) return;
      setPending(true);
      setError(null);
      try {
        const next = await codexConfigConsistencyApi.resolve(
          report.actualFingerprint,
          action,
        );
        if (
          next.state === "external_drift" ||
          next.runtimeActivation?.state === "restart_required"
        ) {
          setReport(next);
        } else {
          close();
        }
      } catch (cause) {
        setError(extractErrorMessage(cause) || "Codex 配置处理失败");
      } finally {
        setPending(false);
      }
    },
    [close, report],
  );

  const recheck = useCallback(async () => {
    setPending(true);
    setError(null);
    try {
      const next = await codexConfigConsistencyApi.inspect();
      if (
        next.state === "external_drift" ||
        next.runtimeActivation?.state === "restart_required"
      ) {
        setReport(next);
      } else {
        close();
      }
    } catch (cause) {
      setError(extractErrorMessage(cause) || "Codex 配置检测失败");
    } finally {
      setPending(false);
    }
  }, [close]);

  const inspectRuntimeRefresh = useCallback(async () => {
    runtimeRefreshActiveRef.current = true;
    setRefresh({
      phase: "inspecting",
      preflight: null,
      progress: null,
      result: null,
      error: null,
    });
    try {
      const preflight = await codexConfigConsistencyApi.inspectRuntimeRefresh();
      if (!preflight.supported || !preflight.canRefresh) {
        setRefresh({
          phase: "failed",
          preflight,
          progress: null,
          result: null,
          error: preflight.supported
            ? "未找到可用的 Codex Desktop 启动入口"
            : "当前平台暂不支持自动刷新 Codex 状态",
        });
        return;
      }
      setRefresh({
        phase: "confirm",
        preflight,
        progress: null,
        result: null,
        error: null,
      });
    } catch (cause) {
      setRefresh({
        phase: "failed",
        preflight: null,
        progress: null,
        result: null,
        error: extractErrorMessage(cause) || "Codex 运行状态检查失败",
      });
    }
  }, []);

  const openStatusPanel = useCallback(async () => {
    runtimeRefreshActiveRef.current = true;
    setRefresh({
      phase: "inspecting",
      preflight: null,
      progress: null,
      result: null,
      error: null,
    });
    setError(null);
    try {
      const [nextReport, preflight] = await Promise.all([
        codexConfigConsistencyApi.inspect(),
        codexConfigConsistencyApi.inspectRuntimeRefresh(),
      ]);
      setReport(nextReport);
      setRefresh({
        phase: "status",
        preflight,
        progress: null,
        result: null,
        error: null,
      });
    } catch (cause) {
      setRefresh({
        phase: "failed",
        preflight: null,
        progress: null,
        result: null,
        error: extractErrorMessage(cause) || "Codex 状态检查失败",
      });
    }
  }, []);

  const confirmRuntimeRefresh = useCallback(async () => {
    const preflight = refresh.preflight;
    if (!preflight || refresh.phase !== "confirm") return;
    runtimeRefreshActiveRef.current = true;
    setRefresh((current) => ({
      ...current,
      phase: "refreshing",
      progress: null,
      result: null,
      error: null,
    }));
    try {
      const result = await codexConfigConsistencyApi.refreshRuntimeState(
        preflight.snapshotToken,
      );
      setRefresh((current) => ({
        ...current,
        phase: "completed",
        progress: { stage: "completed" },
        result,
        error: null,
      }));
    } catch (cause) {
      setRefresh((current) => ({
        ...current,
        phase: "failed",
        error: extractErrorMessage(cause) || "Codex 状态刷新失败",
      }));
    }
  }, [refresh.phase, refresh.preflight]);

  const cancelRuntimeRefresh = useCallback(() => {
    const completed = refresh.phase === "completed";
    runtimeRefreshActiveRef.current = false;
    setRefresh(EMPTY_RUNTIME_REFRESH);
    if (completed) close();
  }, [close, refresh.phase]);

  const retryRendererCompatibility = useCallback(async () => {
    if (!refresh.result) return;
    setRefresh((current) => ({
      ...current,
      rendererRetryPending: true,
    }));
    try {
      const retry = await proxyApi.unlockCodexModelPicker();
      const ready = Boolean(
        retry.injected &&
          retry.allProviderHistoryPatched &&
          retry.historyRefreshRequested,
      );
      setRefresh((current) => ({
        ...current,
        rendererRetryPending: false,
        result: current.result
          ? {
              ...current.result,
              outcome: ready ? "completed" : "completed_with_warnings",
              rendererCompatibilityStatus: ready ? "ready" : "warning",
              rendererCompatibilityMessage: ready ? null : retry.message,
            }
          : null,
      }));
    } catch (cause) {
      setRefresh((current) => ({
        ...current,
        rendererRetryPending: false,
        result: current.result
          ? {
              ...current.result,
              outcome: "completed_with_warnings",
              rendererCompatibilityStatus: "warning",
              rendererCompatibilityMessage:
                extractErrorMessage(cause) || "Codex 历史查询兼容层重试失败",
            }
          : null,
      }));
    }
  }, [refresh.result]);

  return {
    report,
    pending,
    error,
    refresh,
    close,
    resolve,
    recheck,
    openStatusPanel,
    inspectRuntimeRefresh,
    confirmRuntimeRefresh,
    retryRendererCompatibility,
    cancelRuntimeRefresh,
  };
}

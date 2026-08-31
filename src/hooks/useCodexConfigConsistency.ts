import { useCallback, useEffect, useRef, useState } from "react";

import {
  codexConfigConsistencyApi,
  type CodexConfigConsistencyAction,
  type CodexConfigConsistencyReport,
} from "@/lib/api/codexConfigConsistency";
import { extractErrorMessage } from "@/utils/errorUtils";
import { useTauriEvent } from "./useTauriEvent";

interface CodexConfigConsistencyState {
  report: CodexConfigConsistencyReport | null;
  pending: boolean;
  error: string | null;
  close: () => void;
  resolve: (action: CodexConfigConsistencyAction) => Promise<void>;
  recheck: () => Promise<void>;
}

export function useCodexConfigConsistency(): CodexConfigConsistencyState {
  const [report, setReport] = useState<CodexConfigConsistencyReport | null>(
    null,
  );
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const seenFingerprintsRef = useRef(new Set<string>());

  const acceptReport = useCallback((next: CodexConfigConsistencyReport) => {
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

  useEffect(() => {
    let active = true;
    let requestPending = false;
    const inspect = async () => {
      if (requestPending) return;
      requestPending = true;
      try {
        const next = await codexConfigConsistencyApi.inspect();
        if (active) acceptReport(next);
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

  return { report, pending, error, close, resolve, recheck };
}

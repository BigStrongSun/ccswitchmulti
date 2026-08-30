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
}

export function useCodexConfigConsistency(): CodexConfigConsistencyState {
  const [report, setReport] = useState<CodexConfigConsistencyReport | null>(
    null,
  );
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const seenFingerprintsRef = useRef(new Set<string>());

  const acceptReport = useCallback((next: CodexConfigConsistencyReport) => {
    if (next.state !== "external_drift" || !next.actualFingerprint) return;
    if (seenFingerprintsRef.current.has(next.actualFingerprint)) return;
    seenFingerprintsRef.current.add(next.actualFingerprint);
    setError(null);
    setReport(next);
  }, []);

  useTauriEvent<CodexConfigConsistencyReport>(
    "codex-config-consistency",
    acceptReport,
  );

  useEffect(() => {
    let active = true;
    void codexConfigConsistencyApi
      .inspect()
      .then((next) => {
        if (active) acceptReport(next);
      })
      .catch((cause) => {
        if (active) console.debug("[CodexConsistency] inspect failed", cause);
      });
    return () => {
      active = false;
    };
  }, [acceptReport]);

  const close = useCallback(() => {
    setReport(null);
    setError(null);
  }, []);

  const resolve = useCallback(
    async (action: CodexConfigConsistencyAction) => {
      if (!report?.actualFingerprint) return;
      setPending(true);
      setError(null);
      try {
        await codexConfigConsistencyApi.resolve(
          report.actualFingerprint,
          action,
        );
        close();
      } catch (cause) {
        setError(extractErrorMessage(cause) || "Codex 配置处理失败");
      } finally {
        setPending(false);
      }
    },
    [close, report],
  );

  return { report, pending, error, close, resolve };
}

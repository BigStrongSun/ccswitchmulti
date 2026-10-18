import { useEffect, useRef, useState } from "react";
import { Copy, RefreshCw, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { proxyApi } from "@/lib/api/proxy";
import type { CodexHistoryContentRepairOutcome } from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

type Operation = "preview" | "apply" | null;

interface CodexHistoryContentRepairPanelProps {
  onApplyingChange?: (applying: boolean) => void;
}

export function CodexHistoryContentRepairDialog() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [applying, setApplying] = useState(false);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!applying) setOpen(next);
      }}
    >
      <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
        {t("codexHistoryContentRepair.title")}
      </Button>
      <DialogContent
        className="max-w-3xl overflow-y-auto p-4"
        aria-describedby={undefined}
      >
        <DialogTitle className="sr-only">
          {t("codexHistoryContentRepair.title")}
        </DialogTitle>
        <Button
          className="self-end"
          size="sm"
          variant="ghost"
          disabled={applying}
          onClick={() => setOpen(false)}
        >
          {t("codexHistoryContentRepair.close")}
        </Button>
        <CodexHistoryContentRepairPanel onApplyingChange={setApplying} />
      </DialogContent>
    </Dialog>
  );
}

/** 仅消费独立的正文恢复计划，不读取旧面板的 Provider、项目或会话选择。 */
export function CodexHistoryContentRepairPanel({
  onApplyingChange,
}: CodexHistoryContentRepairPanelProps = {}) {
  const { t } = useTranslation();
  const [codexHome, setCodexHome] = useState("");
  const [result, setResult] = useState<CodexHistoryContentRepairOutcome | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [operation, setOperation] = useState<Operation>(null);
  const requestId = useRef(0);
  const operationRef = useRef<Operation>(null);

  useEffect(() => {
    onApplyingChange?.(operation === "apply");
  }, [operation, onApplyingChange]);

  useEffect(
    () => () => {
      requestId.current += 1;
    },
    [],
  );

  function changeHome(value: string) {
    if (operationRef.current === "apply") return;
    requestId.current += 1;
    operationRef.current = null;
    setOperation(null);
    setCodexHome(value);
    setResult(null);
    setError(null);
  }

  async function previewRepair() {
    if (operationRef.current) return;
    const id = ++requestId.current;
    operationRef.current = "preview";
    setOperation("preview");
    setResult(null);
    setError(null);
    try {
      const preview = await proxyApi.previewCodexHistoryContentRepair(
        codexHome.trim() || null,
      );
      if (id === requestId.current) setResult(preview);
    } catch (cause) {
      if (id === requestId.current) {
        setError(extractErrorMessage(cause) || String(cause));
      }
    } finally {
      if (id === requestId.current) {
        operationRef.current = null;
        setOperation(null);
      }
    }
  }

  const canApply = Boolean(
    !operation &&
      result?.dryRun &&
      result.affectedFiles > 0 &&
      result.previewToken,
  );

  async function applyRepair() {
    if (operationRef.current || !canApply || !result?.previewToken) return;
    const token = result.previewToken;
    const id = ++requestId.current;
    operationRef.current = "apply";
    setOperation("apply");
    // 计划仅可执行一次；错误或成功后均需新预览才能再次写入。
    setResult({ ...result, previewToken: null });
    setError(null);
    try {
      const applied = await proxyApi.applyCodexHistoryContentRepair(token);
      if (id === requestId.current) {
        setResult({ ...applied, previewToken: null });
      }
    } catch (cause) {
      if (id === requestId.current) {
        setResult(null);
        setError(extractErrorMessage(cause) || String(cause));
      }
    } finally {
      if (id === requestId.current) {
        operationRef.current = null;
        setOperation(null);
      }
    }
  }

  return (
    <div className="space-y-4 rounded-lg border bg-card p-4">
      <div>
        <h2 className="flex items-center gap-2 text-base font-semibold">
          <ShieldCheck className="size-5" />
          {t("codexHistoryContentRepair.title")}
        </h2>
        <p className="mt-2 text-sm text-muted-foreground">
          {t("codexHistoryContentRepair.description")}
        </p>
      </div>
      <div className="rounded-md border bg-muted/30 p-3 text-sm leading-6">
        {t("codexHistoryContentRepair.scope")}
      </div>
      <label className="block text-sm font-medium">
        {t("codexHistoryContentRepair.home")}
        <Input
          className="mt-1"
          placeholder={t("codexHistoryContentRepair.homePlaceholder")}
          value={codexHome}
          onChange={(event) => changeHome(event.target.value)}
          disabled={operation === "apply"}
        />
      </label>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          onClick={() => void previewRepair()}
          disabled={Boolean(operation)}
        >
          {operation === "preview" ? (
            <RefreshCw className="mr-2 size-4 animate-spin" />
          ) : null}
          {t("codexHistoryContentRepair.preview")}
        </Button>
        <Button onClick={() => void applyRepair()} disabled={!canApply}>
          {operation === "apply" ? (
            <RefreshCw className="mr-2 size-4 animate-spin" />
          ) : null}
          {t("codexHistoryContentRepair.apply")}
        </Button>
      </div>
      <div role="status" className="text-sm" aria-live="polite">
        {operation === "preview"
          ? t("codexHistoryContentRepair.scanning")
          : operation === "apply"
            ? t("codexHistoryContentRepair.applying")
            : result
              ? result.dryRun
                ? result.affectedFiles === 0
                  ? t("codexHistoryContentRepair.noChanges")
                  : result.requiresCodexExit
                    ? t("codexHistoryContentRepair.waitingForExit")
                    : t("codexHistoryContentRepair.ready")
                : t("codexHistoryContentRepair.completed")
              : t("codexHistoryContentRepair.idle")}
      </div>
      {error ? (
        <div
          role="alert"
          className="whitespace-pre-wrap break-words rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
        >
          {error}
        </div>
      ) : null}
      {result ? (
        <div className="space-y-3">
          {!result.nativeConfig ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm">
              {t("codexHistoryContentRepair.nativeRequired")}
            </p>
          ) : null}
          <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
            {(
              [
                [
                  t("codexHistoryContentRepair.scannedFiles"),
                  result.scannedFiles,
                ],
                [
                  t("codexHistoryContentRepair.affectedFiles"),
                  result.affectedFiles,
                ],
                [
                  t("codexHistoryContentRepair.compressedFiles"),
                  result.compressedFiles,
                ],
                [
                  t(
                    result.dryRun
                      ? "codexHistoryContentRepair.pendingParts"
                      : "codexHistoryContentRepair.repairedParts",
                  ),
                  result.repairedParts,
                ],
              ] as const
            ).map(([label, count]) => (
              <div key={label} className="rounded-md border bg-muted/20 p-2">
                <div className="text-xs text-muted-foreground">{label}</div>
                <div className="font-semibold">{count.toLocaleString()}</div>
              </div>
            ))}
          </div>
          <Badge variant="outline">
            {t("codexHistoryContentRepair.stage", {
              stage: t(`codexHistoryContentRepair.stages.${result.stage}`, {
                defaultValue: result.stage,
              }),
            })}
          </Badge>
          <p className="whitespace-pre-wrap break-words text-sm">
            {result.message}
          </p>
          <div className="break-all text-xs text-muted-foreground">
            {t("codexHistoryContentRepair.resolvedHome", {
              path: result.codexHome,
            })}
          </div>
          {result.planPath ? (
            <RepairPath
              label={t("codexHistoryContentRepair.plan")}
              path={result.planPath}
            />
          ) : null}
          {result.backupDir ? (
            <RepairPath
              label={t("codexHistoryContentRepair.backup")}
              path={result.backupDir}
            />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function RepairPath({ label, path }: { label: string; path: string }) {
  const { t } = useTranslation();
  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
      toast.success(t("codexHistoryContentRepair.copiedPath", { label }));
    } catch (cause) {
      toast.error(
        extractErrorMessage(cause) || t("codexHistoryContentRepair.copyFailed"),
      );
    }
  }
  return (
    <div className="flex items-start justify-between gap-2 rounded-md border p-2 text-xs">
      <div className="min-w-0">
        <div className="text-muted-foreground">{label}</div>
        <div className="break-all font-mono">{path}</div>
      </div>
      <Button
        size="sm"
        variant="ghost"
        aria-label={t("codexHistoryContentRepair.copyPath", { label })}
        onClick={() => void copyPath()}
      >
        <Copy className="size-4" />
      </Button>
    </div>
  );
}

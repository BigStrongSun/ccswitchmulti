import { AlertTriangle, Loader2 } from "lucide-react";
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

interface CodexConfigConsistencyDialogProps {
  report: CodexConfigConsistencyReport | null;
  pending: boolean;
  error: string | null;
  onApply: () => void;
  onKeep: () => void;
  onLater: () => void;
  onRetry: () => void;
}

export function CodexConfigConsistencyDialog({
  report,
  pending,
  error,
  onApply,
  onKeep,
  onLater,
  onRetry,
}: CodexConfigConsistencyDialogProps) {
  const { t } = useTranslation();
  if (!report || report.state !== "external_drift") return null;

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
          <Button variant="outline" onClick={onKeep} disabled={pending}>
            {t("codexConfigConsistency.keepCodex")}
          </Button>
          <Button onClick={onApply} disabled={pending}>
            {pending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("codexConfigConsistency.applyCcsm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

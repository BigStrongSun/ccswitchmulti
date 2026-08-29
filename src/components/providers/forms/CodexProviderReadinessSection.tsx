import type { Ref } from "react";
import { CheckCircle2, Download, Loader2, Route, Server } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { CodexApiFormat, CodexCatalogModel } from "@/types";

type ValidationTone = "muted" | "success" | "warning" | "error";

interface CodexProviderReadinessSectionProps {
  models: CodexCatalogModel[];
  defaultModel?: string;
  apiFormat: CodexApiFormat;
  isMaintainedPreset: boolean;
  isSyncingModels: boolean;
  isValidatingConnection: boolean;
  validationSummary?: string;
  validationTone?: ValidationTone;
  highlightSync?: boolean;
  syncButtonRef?: Ref<HTMLButtonElement>;
  sectionRef?: Ref<HTMLElement>;
  onSyncModels: () => void;
  onValidateConnection: () => void;
}

function apiFormatLabel(apiFormat: CodexApiFormat): string {
  switch (apiFormat) {
    case "openai_responses":
      return "Responses";
    case "anthropic":
      return "Anthropic Messages";
    default:
      return "Chat Completions";
  }
}

export function CodexProviderReadinessSection({
  models,
  defaultModel,
  apiFormat,
  isMaintainedPreset,
  isSyncingModels,
  isValidatingConnection,
  validationSummary = "",
  validationTone = "muted",
  highlightSync = false,
  syncButtonRef,
  sectionRef,
  onSyncModels,
  onValidateConnection,
}: CodexProviderReadinessSectionProps) {
  const configuredModels = models.filter((model) => model.model.trim());
  const enabledModels = configuredModels.filter(
    (model) => model.enabled !== false,
  );
  const disabledModelCount = configuredModels.length - enabledModels.length;
  const configuredDefault = defaultModel?.trim() ?? "";
  const selectedModel =
    enabledModels
      .find((model) => model.model.trim() === configuredDefault)
      ?.model.trim() ||
    enabledModels[0]?.model.trim() ||
    (configuredModels.length > 0 ? "尚未启用" : "尚未选择");
  const hasModels = enabledModels.length > 0;
  const validationPassed = validationTone === "success";
  const ready = hasModels && validationPassed;
  const readinessLabel =
    configuredModels.length === 0
      ? "需要同步模型"
      : !hasModels
        ? "需要启用模型"
        : ready
          ? "可加入 MultiRouter"
          : validationTone === "error"
            ? "连接验证失败"
            : "建议先验证连接";

  return (
    <section
      ref={sectionRef}
      aria-labelledby="codex-model-readiness-title"
      className="space-y-4 rounded-lg border border-border-default bg-muted/10 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <h3
            id="codex-model-readiness-title"
            className="text-sm font-semibold text-foreground"
          >
            模型与兼容性
          </h3>
          <p className="text-xs leading-relaxed text-muted-foreground">
            同步这个模型源可用的模型，并验证它能否被 Codex 和 MultiRouter
            正常使用。
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            ref={syncButtonRef}
            type="button"
            size="sm"
            onClick={onSyncModels}
            disabled={isSyncingModels}
            className={cn(
              "h-8 gap-1 border border-blue-700 bg-blue-600 px-3 text-white shadow-sm hover:bg-blue-700 dark:border-blue-400 dark:bg-blue-500 dark:hover:bg-blue-600",
              highlightSync &&
                "border-blue-500 bg-blue-50 text-blue-700 shadow-[0_0_0_3px_rgba(59,130,246,0.18)] dark:bg-blue-950/40 dark:text-blue-200",
            )}
          >
            {isSyncingModels ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
            同步模型
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-8 gap-1"
            disabled={isValidatingConnection}
            onClick={onValidateConnection}
          >
            {isValidatingConnection ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Route className="h-3.5 w-3.5" />
            )}
            验证连接
          </Button>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-3">
        <div className="rounded-md border border-border-default bg-background/70 p-3">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Server className="h-3.5 w-3.5" />
            模型目录
          </div>
          <p className="mt-1 text-sm font-medium text-foreground">
            {configuredModels.length === 0
              ? "尚未同步"
              : disabledModelCount > 0
                ? `${enabledModels.length} 个已启用，${disabledModelCount} 个已停用`
                : `${enabledModels.length} 个模型`}
          </p>
        </div>
        <div className="rounded-md border border-border-default bg-background/70 p-3">
          <p className="text-xs text-muted-foreground">默认模型</p>
          <p className="mt-1 truncate text-sm font-medium text-foreground">
            {selectedModel}
          </p>
        </div>
        <div className="rounded-md border border-border-default bg-background/70 p-3">
          <p className="text-xs text-muted-foreground">上游协议</p>
          <p className="mt-1 text-sm font-medium text-foreground">
            {apiFormatLabel(apiFormat)}
          </p>
        </div>
      </div>

      <div className="rounded-md border border-border-default bg-background/70 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="space-y-1">
            <p className="text-xs font-medium text-foreground">就绪状态</p>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {isMaintainedPreset
                ? "协议、上下文、推理档位与 /model 目录由 CCSwitchMulti 维护。"
                : "验证连接时会自动检测 Chat 与 Responses；只有自动检测失败时才需要在高级设置中手动覆盖。"}
            </p>
          </div>
          <span
            className={cn(
              "inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-xs font-medium",
              ready
                ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                : validationTone === "error"
                  ? "border-destructive/40 bg-destructive/10 text-destructive"
                  : "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300",
            )}
          >
            {ready && <CheckCircle2 className="h-3.5 w-3.5" />}
            {readinessLabel}
          </span>
        </div>
        {isMaintainedPreset && (
          <p className="mt-2 text-xs font-medium text-blue-700 dark:text-blue-300">
            由 CCSwitchMulti 维护
          </p>
        )}
        {validationSummary && (
          <p
            role={validationTone === "error" ? "alert" : "status"}
            className={cn(
              "mt-2 text-xs leading-relaxed",
              validationTone === "success" &&
                "text-emerald-700 dark:text-emerald-300",
              validationTone === "warning" &&
                "text-amber-700 dark:text-amber-300",
              validationTone === "error" && "text-destructive",
              validationTone === "muted" && "text-muted-foreground",
            )}
          >
            {validationSummary}
          </p>
        )}
      </div>
    </section>
  );
}

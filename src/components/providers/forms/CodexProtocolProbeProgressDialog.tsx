import { useMemo } from "react";
import {
  CheckCircle2,
  Circle,
  Loader2,
  MinusCircle,
  XCircle,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import type {
  CodexProtocolCompatibilityRecord,
  CodexProtocolProbeProgressEvent,
  CodexProtocolProbeFailure,
  CodexProtocolProbeReadiness,
  CodexProtocolProbeStage,
  CodexProtocolProbeStageStatus,
  CodexProtocolTransport,
  CodexProviderProtocolPreflightOutcome,
  CodexReasoningSemantic,
  CodexReasoningSource,
} from "@/lib/api/protocol-compatibility";
import type { CodexHistoryReplay, CodexToolSchemaDialect } from "@/types";

type VisibleStageStatus = CodexProtocolProbeStageStatus | "pending" | "running";

interface BranchProgress {
  touched: boolean;
  stages: Record<CodexProtocolProbeStage, VisibleStageStatus>;
  reasoningSemantic: CodexReasoningSemantic | null;
  reasoningSource: CodexReasoningSource | null;
  readiness: CodexProtocolProbeReadiness | null;
  failures: CodexProtocolProbeFailure[];
  toolSchemaDialect: CodexToolSchemaDialect | null;
  historyReplay: CodexHistoryReplay | null;
}

interface ModelProgress {
  model: string;
  branches: Record<CodexProtocolTransport, BranchProgress>;
  selectedTransport: CodexProtocolTransport | null;
  readiness: CodexProtocolProbeReadiness | null;
}

interface CodexProtocolProbeProgressDialogProps {
  open: boolean;
  running: boolean;
  expectedModels?: string[];
  events: CodexProtocolProbeProgressEvent[];
  outcome: CodexProviderProtocolPreflightOutcome | null;
  error: string;
  onOpenChange: (open: boolean) => void;
  onRetry?: () => void;
}

const STAGES: Array<{ id: CodexProtocolProbeStage; label: string }> = [
  { id: "baseline", label: "基础响应" },
  { id: "streaming", label: "流式 SSE" },
  { id: "reasoning", label: "思考内容" },
  { id: "forced_tool", label: "工具调用" },
  { id: "continuation", label: "工具续轮" },
];

const TRANSPORTS: CodexProtocolTransport[] = [
  "open_ai_responses",
  "open_ai_chat",
];

function emptyBranch(): BranchProgress {
  return {
    touched: false,
    stages: {
      baseline: "pending",
      streaming: "pending",
      reasoning: "pending",
      forced_tool: "pending",
      continuation: "pending",
    },
    reasoningSemantic: null,
    reasoningSource: null,
    readiness: null,
    failures: [],
    toolSchemaDialect: null,
    historyReplay: null,
  };
}

function emptyModel(model: string): ModelProgress {
  return {
    model,
    branches: {
      open_ai_responses: emptyBranch(),
      open_ai_chat: emptyBranch(),
    },
    selectedTransport: null,
    readiness: null,
  };
}

function reasoningStageStatus(
  semantic: CodexReasoningSemantic,
  baseline: VisibleStageStatus,
): VisibleStageStatus {
  if (semantic === "readable" || semantic === "summary") return "passed";
  if (semantic === "opaque") return "unsupported";
  return baseline === "passed" ? "unsupported" : "skipped";
}

function applyRecord(
  model: ModelProgress,
  record: CodexProtocolCompatibilityRecord,
) {
  model.selectedTransport = record.result.selected_transport;
  model.readiness = record.result.readiness;
  for (const branch of record.result.branches) {
    const target = model.branches[branch.assessment.transport];
    target.touched = true;
    target.stages.baseline = branch.assessment.baseline;
    target.stages.streaming = branch.assessment.streaming;
    target.stages.forced_tool = branch.assessment.forced_tool;
    target.stages.continuation = branch.assessment.continuation;
    target.stages.reasoning = reasoningStageStatus(
      branch.reasoning_shape.semantic,
      branch.assessment.baseline,
    );
    target.reasoningSemantic = branch.reasoning_shape.semantic;
    target.reasoningSource = branch.reasoning_shape.source;
    target.toolSchemaDialect = branch.tool_schema_dialect ?? null;
    target.historyReplay = branch.history_replay ?? null;
    target.readiness = [
      branch.assessment.baseline,
      branch.assessment.streaming,
      branch.assessment.forced_tool,
      branch.assessment.continuation,
    ].every((status) => status === "passed")
      ? "verified"
      : branch.assessment.baseline === "passed"
        ? "partial"
        : "unverified";
    target.failures = branch.failures ?? [];
  }
}

function buildProgress(
  expectedModels: string[],
  events: CodexProtocolProbeProgressEvent[],
  outcome: CodexProviderProtocolPreflightOutcome | null,
): ModelProgress[] {
  const models = new Map<string, ModelProgress>();
  const ensure = (model: string) => {
    const current = models.get(model) ?? emptyModel(model);
    models.set(model, current);
    return current;
  };

  for (const model of expectedModels) ensure(model);

  for (const event of events) {
    if (!("model" in event)) continue;
    const model = ensure(event.model);
    if (event.kind === "candidate_started") continue;
    if (event.kind === "candidate_finished") {
      model.selectedTransport = event.selectedTransport;
      model.readiness = event.readiness;
      continue;
    }
    const branch = model.branches[event.transport];
    branch.touched = true;
    if (event.kind === "stage_started") {
      branch.stages[event.stage] = "running";
    } else if (event.kind === "stage_finished") {
      branch.stages[event.stage] = event.stageStatus;
      if (event.failure) {
        branch.failures = [
          ...branch.failures.filter(
            (failure) => failure.stage !== event.failure?.stage,
          ),
          event.failure,
        ];
      }
    } else if (event.kind === "reasoning_classified") {
      branch.reasoningSemantic = event.reasoningSemantic;
      branch.reasoningSource = event.reasoningSource;
      branch.stages.reasoning = reasoningStageStatus(
        event.reasoningSemantic,
        branch.stages.baseline,
      );
    } else if (event.kind === "branch_finished") {
      branch.readiness = event.readiness;
    }
  }

  for (const record of outcome?.records ?? []) {
    const model = ensure(record.target.public_model);
    applyRecord(model, record);
  }
  return [...models.values()];
}

function statusPresentation(status: VisibleStageStatus) {
  if (status === "running") {
    return {
      label: "检测中",
      icon: Loader2,
      className: "text-sky-600 animate-spin",
    };
  }
  if (status === "passed") {
    return { label: "通过", icon: CheckCircle2, className: "text-emerald-600" };
  }
  if (status === "failed") {
    return { label: "失败", icon: XCircle, className: "text-destructive" };
  }
  if (status === "unsupported") {
    return { label: "不支持", icon: MinusCircle, className: "text-amber-600" };
  }
  if (status === "skipped") {
    return {
      label: "已跳过",
      icon: MinusCircle,
      className: "text-muted-foreground",
    };
  }
  return { label: "等待", icon: Circle, className: "text-muted-foreground/60" };
}

function reasoningLabel(
  semantic: CodexReasoningSemantic | null,
  status: VisibleStageStatus,
) {
  if (semantic === "readable") return "原始推理正文";
  if (semantic === "summary") return "上游原生摘要";
  if (semantic === "opaque") return "加密/不透明（Codex 无法展示）";
  if (semantic === "none") return status === "skipped" ? "未检测" : "未返回";
  return "待识别";
}

function failureLabel(failure: CodexProtocolProbeFailure) {
  if (failure.kind === "http_status") {
    if (failure.status_code === 521) return "HTTP 521 · 上游不可达";
    if (failure.status_code === 401) return "HTTP 401 · 认证失败";
    if (failure.status_code === 403) return "HTTP 403 · 当前凭据无权限";
    if (failure.status_code === 429) return "HTTP 429 · 限流或额度不足";
    if ([404, 405, 415].includes(failure.status_code ?? 0)) {
      return `HTTP ${failure.status_code} · 接口不支持`;
    }
    if ((failure.status_code ?? 0) >= 500) {
      return `HTTP ${failure.status_code} · 上游服务异常`;
    }
    return failure.status_code
      ? `HTTP ${failure.status_code} · 上游请求失败`
      : "上游请求失败";
  }
  if (failure.kind === "timeout") return "请求超时";
  if (failure.kind === "network") return "网络连接失败";
  if (failure.kind === "response_too_large") return "响应超过探测上限";
  if (failure.kind === "invalid_request") return "探测地址无效";
  return "响应格式无效";
}

function readinessLabel(readiness: CodexProtocolProbeReadiness | null) {
  if (readiness === "verified") return "Verified";
  if (readiness === "partial") return "Partial";
  if (readiness === "unverified") return "Failed";
  return "待评估";
}

function transportLabel(transport: CodexProtocolTransport) {
  return transport === "open_ai_responses" ? "Responses" : "Chat Completions";
}

function toolSchemaLabel(
  dialect: CodexToolSchemaDialect | null,
  status: VisibleStageStatus,
) {
  if (status !== "passed" || dialect === null) return "未确认";
  return dialect === "moonshot_mfjs" ? "Moonshot MFJS" : "OpenAI";
}

function historyReplayLabel(
  replay: CodexHistoryReplay | null,
  status: VisibleStageStatus,
) {
  if (status !== "passed" || replay === null) return "未确认";
  if (replay === "chat_reasoning_content") return "reasoning_content";
  if (replay === "responses_reasoning_text_content") {
    return "reasoning_text content";
  }
  if (replay === "omit") return "不回放推理项";
  return "原生 Responses";
}

export function CodexProtocolProbeProgressDialog({
  open,
  running,
  expectedModels = [],
  events,
  outcome,
  error,
  onOpenChange,
  onRetry,
}: CodexProtocolProbeProgressDialogProps) {
  const models = useMemo(
    () => buildProgress(expectedModels, events, outcome),
    [expectedModels, events, outcome],
  );
  const completedModels = models.filter((model) => model.readiness !== null);
  const completed = completedModels.length;
  const modelSummary = {
    total: completed,
    verified: completedModels.filter((model) => model.readiness === "verified")
      .length,
    partial: completedModels.filter((model) => model.readiness === "partial")
      .length,
    failed: completedModels.filter((model) => model.readiness === "unverified")
      .length,
  };
  // Batch Protocol Lab forwards one batch_finished event per Provider. The
  // dialog owns the cross-Provider view, so its title must aggregate every
  // batch instead of treating the last Provider as the entire operation.
  const batchSummaries = events.filter(
    (event) => event.kind === "batch_finished",
  );
  const batchSummary = batchSummaries.reduce(
    (summary, batch) => ({
      total: summary.total + batch.total,
      verified: summary.verified + batch.verified,
      partial: summary.partial + batch.partial,
      failed: summary.failed + batch.failed,
    }),
    { total: 0, verified: 0, partial: 0, failed: 0 },
  );
  const completionSummary =
    batchSummaries.length > 0 ? batchSummary : modelSummary;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && running) return;
        onOpenChange(nextOpen);
      }}
    >
      <DialogContent
        className="flex max-h-[88vh] max-w-5xl flex-col"
        zIndex="top"
      >
        <DialogHeader>
          <DialogTitle>Codex 兼容性深度探测</DialogTitle>
          <DialogDescription>
            {running
              ? `正在验证模型 ${completed}/${models.length || "…"}。每个模型会依次检查 Responses 与 Chat。`
              : batchSummaries.length > 0 || completed > 0
                ? `已完成 ${completionSummary.total} 个模型：Verified ${completionSummary.verified}，Partial ${completionSummary.partial}，Failed ${completionSummary.failed}。`
                : "探测已结束。"}
          </DialogDescription>
        </DialogHeader>

        <div
          className="min-h-0 flex-1 space-y-3 overflow-y-auto pr-1"
          role="status"
          aria-live="polite"
        >
          {error && (
            <div
              className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
              role="alert"
            >
              探测中断：{error}
            </div>
          )}
          {models.length === 0 && !error && (
            <div className="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
              正在准备模型和探测请求…
            </div>
          )}
          {models.map((model) => (
            <article
              key={model.model}
              aria-label={`${model.model} 探测进度`}
              className="space-y-3 rounded-lg border border-border-default bg-muted/10 p-4"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="font-medium text-foreground">{model.model}</h3>
                <div className="flex items-center gap-2 text-xs">
                  {model.selectedTransport && (
                    <span className="rounded-full border border-sky-500/30 bg-sky-500/10 px-2 py-0.5 text-sky-700 dark:text-sky-300">
                      选择 {transportLabel(model.selectedTransport)}
                    </span>
                  )}
                  <span className="rounded-full border px-2 py-0.5 text-muted-foreground">
                    {readinessLabel(model.readiness)}
                  </span>
                </div>
              </div>

              <div className="grid gap-3 lg:grid-cols-2">
                {TRANSPORTS.map((transport) => {
                  const branch = model.branches[transport];
                  return (
                    <section
                      key={transport}
                      className="rounded-md border bg-background/70 p-3"
                    >
                      <div className="mb-3 flex items-center justify-between gap-2">
                        <h4 className="text-sm font-medium">
                          {transportLabel(transport)}
                        </h4>
                        <span className="text-xs text-muted-foreground">
                          {readinessLabel(branch.readiness)}
                        </span>
                      </div>
                      {!branch.touched ? (
                        <p className="text-xs text-muted-foreground">
                          等待开始
                        </p>
                      ) : (
                        <div className="space-y-2">
                          {STAGES.map((stage) => {
                            const status = statusPresentation(
                              branch.stages[stage.id],
                            );
                            const Icon = status.icon;
                            return (
                              <div
                                key={stage.id}
                                className="flex items-center justify-between gap-3 text-sm"
                              >
                                <span>{stage.label}</span>
                                <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                                  {stage.id === "reasoning" && (
                                    <span>
                                      {reasoningLabel(
                                        branch.reasoningSemantic,
                                        branch.stages.reasoning,
                                      )}
                                    </span>
                                  )}
                                  <Icon
                                    className={cn("h-4 w-4", status.className)}
                                    aria-hidden
                                  />
                                  <span>{status.label}</span>
                                </span>
                              </div>
                            );
                          })}
                          <div className="space-y-1 border-t pt-2 text-xs text-muted-foreground">
                            <p>
                              工具 Schema：
                              {toolSchemaLabel(
                                branch.toolSchemaDialect,
                                branch.stages.forced_tool,
                              )}
                            </p>
                            <p>
                              历史续轮：
                              {historyReplayLabel(
                                branch.historyReplay,
                                branch.stages.continuation,
                              )}
                            </p>
                          </div>
                          {branch.failures.length > 0 && (
                            <div className="space-y-1 border-t pt-2 text-xs text-destructive">
                              {branch.failures.map((failure) => (
                                <p
                                  key={`${failure.stage}:${failure.kind}:${failure.status_code ?? ""}`}
                                >
                                  {failureLabel(failure)}
                                </p>
                              ))}
                            </div>
                          )}
                        </div>
                      )}
                    </section>
                  );
                })}
              </div>
            </article>
          ))}
        </div>

        <DialogFooter>
          {!running && onRetry && (
            <Button type="button" variant="outline" onClick={onRetry}>
              重新探测
            </Button>
          )}
          <Button
            type="button"
            onClick={() => onOpenChange(false)}
            disabled={running}
          >
            {running ? "探测进行中" : "关闭"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

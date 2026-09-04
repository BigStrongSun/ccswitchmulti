import { useMemo, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
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
  CodexProtocolProbeFailure,
  CodexProtocolProbeReadiness,
  CodexProtocolProbeStage,
  CodexProtocolProbeStageStatus,
  CodexProtocolTransport,
  CodexProviderProtocolPreflightOutcome,
  CodexReasoningSemantic,
  CodexReasoningSource,
} from "@/lib/api/protocol-compatibility";
import type {
  CodexProviderProtocolProbeTarget,
  CodexProviderScopedProtocolProbeProgressEvent,
} from "@/lib/protocol-lab/codex-adapters";
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
  key: string;
  model: string;
  providerId: string | null;
  providerName: string | null;
  branches: Record<CodexProtocolTransport, BranchProgress>;
  selectedTransport: CodexProtocolTransport | null;
  readiness: CodexProtocolProbeReadiness | null;
}

interface CodexProtocolProbeProgressDialogProps {
  open: boolean;
  running: boolean;
  expectedModels?: string[];
  expectedTargets?: CodexProviderProtocolProbeTarget[];
  events: CodexProviderScopedProtocolProbeProgressEvent[];
  outcome: CodexProviderProtocolPreflightOutcome | null;
  outcomes?: CodexProviderProtocolPreflightOutcome[];
  storedRecords?: CodexProtocolCompatibilityRecord[];
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

function progressKey(model: string, providerId?: string | null): string {
  return providerId ? `${providerId}\u0000${model}` : model;
}

function emptyModel(
  model: string,
  providerId?: string | null,
  providerName?: string | null,
): ModelProgress {
  return {
    key: progressKey(model, providerId),
    model,
    providerId: providerId ?? null,
    providerName: providerName ?? null,
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
  expectedTargets: CodexProviderProtocolProbeTarget[],
  events: CodexProviderScopedProtocolProbeProgressEvent[],
  outcomes: CodexProviderProtocolPreflightOutcome[],
  storedRecords: CodexProtocolCompatibilityRecord[],
): ModelProgress[] {
  const models = new Map<string, ModelProgress>();
  const scoped =
    expectedTargets.length > 0 ||
    events.some((event) => Boolean(event.providerId));
  const ensure = (
    model: string,
    providerId?: string | null,
    providerName?: string | null,
  ) => {
    const key = progressKey(model, scoped ? providerId : null);
    const current =
      models.get(key) ??
      emptyModel(model, scoped ? providerId : null, providerName);
    models.set(key, current);
    return current;
  };

  if (expectedTargets.length > 0) {
    for (const target of expectedTargets) {
      ensure(target.model, target.providerId, target.providerName);
    }
  } else {
    for (const model of expectedModels) ensure(model);
  }

  for (const event of events) {
    if (!("model" in event)) continue;
    const model = ensure(event.model, event.providerId, event.providerName);
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

  for (const outcome of outcomes) {
    for (const record of outcome.records) {
      const model = ensure(
        record.target.public_model,
        scoped ? outcome.provider.id : null,
        scoped ? outcome.provider.name : null,
      );
      applyRecord(model, record);
    }
  }
  for (const record of storedRecords) {
    const model = ensure(record.target.public_model);
    applyRecord(model, record);
  }
  return [...models.values()];
}

function aggregateProbeUsage(
  outcomes: CodexProviderProtocolPreflightOutcome[],
) {
  const summaries = outcomes.flatMap((outcome) =>
    outcome.probeUsage ? [outcome.probeUsage] : [],
  );
  if (summaries.length === 0) return undefined;
  const estimatedCosts = summaries
    .flatMap((summary) =>
      summary.estimatedCostUsd === null
        ? []
        : [Number(summary.estimatedCostUsd)],
    )
    .filter(Number.isFinite);
  const estimatedCost = estimatedCosts.reduce((sum, cost) => sum + cost, 0);
  return {
    inputTokens: summaries.reduce(
      (sum, summary) => sum + summary.inputTokens,
      0,
    ),
    outputTokens: summaries.reduce(
      (sum, summary) => sum + summary.outputTokens,
      0,
    ),
    cacheReadTokens: summaries.reduce(
      (sum, summary) => sum + summary.cacheReadTokens,
      0,
    ),
    cacheCreationTokens: summaries.reduce(
      (sum, summary) => sum + summary.cacheCreationTokens,
      0,
    ),
    totalTokens: summaries.reduce(
      (sum, summary) => sum + summary.totalTokens,
      0,
    ),
    reportedResponses: summaries.reduce(
      (sum, summary) => sum + summary.reportedResponses,
      0,
    ),
    successfulResponses: summaries.reduce(
      (sum, summary) => sum + summary.successfulResponses,
      0,
    ),
    estimatedCostUsd:
      estimatedCosts.length > 0
        ? estimatedCost.toFixed(10).replace(/0+$/, "").replace(/\.$/, "")
        : null,
    pricedModels: [
      ...new Set(summaries.flatMap((summary) => summary.pricedModels)),
    ],
    unpricedModels: [
      ...new Set(summaries.flatMap((summary) => summary.unpricedModels)),
    ],
  };
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

function runtimeAdaptationLabel(
  transport: CodexProtocolTransport,
  reasoningSource: CodexReasoningSource | null,
  historyReplay: CodexHistoryReplay | null,
) {
  if (transport === "open_ai_chat") {
    const source =
      reasoningSource && reasoningSource !== "none"
        ? reasoningSource
        : "已识别的推理字段";
    return `运行时适配：Codex Responses 转换为 Chat Completions；推理从 ${source} 读取并投影回 Codex。`;
  }
  if (historyReplay === "responses_reasoning_text_content") {
    return "运行时适配：Responses 请求保持原协议；续轮推理按 reasoning_text content 重建。";
  }
  if (historyReplay === "omit") {
    return "运行时适配：Responses 请求保持原协议；续轮仅移除不兼容的推理项，工具调用和工具结果仍会保留。";
  }
  return "运行时适配：Responses 请求保持原协议；上游原生推理项按原结构回放。";
}

function runtimeMappingRows(
  transport: CodexProtocolTransport,
  reasoningSemantic: CodexReasoningSemantic | null,
  reasoningSource: CodexReasoningSource | null,
  historyReplay: CodexHistoryReplay | null,
) {
  if (transport === "open_ai_chat") {
    const source = (() => {
      if (reasoningSource === "reasoning_content") {
        return "choices[].delta.reasoning_content";
      }
      if (reasoningSource === "reasoning") {
        return "choices[].delta.reasoning（字符串或 content/text/summary）";
      }
      if (reasoningSource === "reasoning_details") {
        return "choices[].delta.reasoning_details[]";
      }
      if (reasoningSource === "think_tags") {
        return "choices[].delta.content 中的 <think>...</think>";
      }
      return "探测到的 Chat 推理字段";
    })();
    const projection =
      reasoningSemantic === "readable"
        ? "response.reasoning_text.delta"
        : reasoningSemantic === "summary"
          ? "response.reasoning_summary_text.delta"
          : "不生成可见推理事件";
    return [
      "Codex instructions → Chat messages[role=system]",
      "Codex input[] → Chat messages[]",
      "function_call / custom_tool_call → assistant.tool_calls[]",
      "function_call_output → role=tool + tool_call_id",
      "tools[] → Chat tools[].function（按探测到的 Schema 方言编译）",
      `${source} → ${projection}`,
      "choices[].delta.content → response.output_text.delta",
      "Chat tool_calls[] → Responses function/custom/tool_search call items",
      "prompt/completion usage → input/output usage",
    ];
  }

  const replay =
    historyReplay === "responses_reasoning_text_content"
      ? "reasoning.content / 可读 summary → content[type=reasoning_text]；summary=[]"
      : historyReplay === "omit"
        ? "不兼容的 reasoning item → 删除（仅删除推理项）"
        : "reasoning item → 按上游原生结构回放";
  return [
    "Codex Responses 请求 → Responses 上游（不改协议）",
    "system / developer message → 顶层 instructions",
    replay,
    "tools[].parameters → 按探测到的 Schema 方言编译",
    "工具调用与工具结果保持 Responses 原结构",
    "SSE、推理事件与 usage → 规范化后返回 Codex",
  ];
}

export function CodexProtocolProbeProgressDialog({
  open,
  running,
  expectedModels = [],
  expectedTargets = [],
  events,
  outcome,
  outcomes = [],
  storedRecords = [],
  error,
  onOpenChange,
  onRetry,
}: CodexProtocolProbeProgressDialogProps) {
  const [expandedMappings, setExpandedMappings] = useState<Set<string>>(
    () => new Set(),
  );
  const resolvedOutcomes = useMemo(
    () => (outcomes.length > 0 ? outcomes : outcome ? [outcome] : []),
    [outcome, outcomes],
  );
  const models = useMemo(
    () =>
      buildProgress(
        expectedModels,
        expectedTargets,
        events,
        resolvedOutcomes,
        storedRecords,
      ),
    [expectedModels, expectedTargets, events, resolvedOutcomes, storedRecords],
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
  const missingResultCount = models.filter(
    (model) => model.readiness === null,
  ).length;
  const hasMissingResults =
    !running &&
    missingResultCount > 0 &&
    (Boolean(error) ||
      batchSummaries.length > 0 ||
      resolvedOutcomes.length > 0);
  const probeUsage = useMemo(
    () => aggregateProbeUsage(resolvedOutcomes),
    [resolvedOutcomes],
  );
  const unreportedResponses = probeUsage
    ? Math.max(0, probeUsage.successfulResponses - probeUsage.reportedResponses)
    : 0;

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
              ? `正在验证模型 ${completed}/${models.length || "…"}。模型采用有限并发，Responses 与 Chat 同时检查；工具续轮按顺序验证。`
              : hasMissingResults
                ? `探测未完成：${missingResultCount} 个模型没有结果。`
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
          <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-800 dark:text-amber-200">
            深度探测会向每个模型的 Responses 和 Chat
            端点发送多次真实请求，会消耗 Token
            并可能产生费用。探测成功后，所选协议的请求、推理、工具和续轮映射会由
            CCSM 在运行时自动应用。
          </div>
          {!running && resolvedOutcomes.length > 0 && (
            <div className="rounded-md border border-border-default bg-muted/20 p-3 text-sm">
              {probeUsage && probeUsage.reportedResponses > 0 ? (
                <div className="space-y-1">
                  <p className="font-medium text-foreground">
                    本次上游已报告 {probeUsage.totalTokens.toLocaleString()}{" "}
                    tokens （输入 {probeUsage.inputTokens.toLocaleString()}
                    ，输出 {probeUsage.outputTokens.toLocaleString()}）
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {probeUsage.estimatedCostUsd
                      ? `按当前模型定价和 Provider 倍率估算费用约 US$${probeUsage.estimatedCostUsd}。`
                      : "当前模型没有可用定价，暂时无法估算费用。"}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    已报告的数字用量会随本次 Responses/Chat
                    双协议脱敏证据保存，便于重开配置后复核；不会保存响应正文。
                  </p>
                  {unreportedResponses > 0 && (
                    <p className="text-xs text-amber-700 dark:text-amber-300">
                      {unreportedResponses} 个成功响应未返回 usage；上述 Token
                      和费用只是已报告部分，实际消耗可能更高。
                    </p>
                  )}
                  {probeUsage.unpricedModels.length > 0 && (
                    <p className="text-xs text-amber-700 dark:text-amber-300">
                      未找到定价：{probeUsage.unpricedModels.join("、")}
                      ；费用估算不包含这些模型。
                    </p>
                  )}
                </div>
              ) : (
                <p className="text-xs text-amber-700 dark:text-amber-300">
                  本次上游没有返回 usage，无法可靠统计 Token
                  和费用；实际请求仍可能已经计费。
                </p>
              )}
            </div>
          )}
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
              key={model.key}
              aria-label={`${
                model.providerName ? `${model.providerName} · ` : ""
              }${model.model} 探测进度`}
              className="space-y-3 rounded-lg border border-border-default bg-muted/10 p-4"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h3 className="font-medium text-foreground">{model.model}</h3>
                  {model.providerName && (
                    <p className="text-xs text-muted-foreground">
                      {model.providerName}
                    </p>
                  )}
                </div>
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
                  const mappingKey = `${model.key}\u0000${transport}`;
                  const mappingExpanded = expandedMappings.has(mappingKey);
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
                          {hasMissingResults && model.readiness === null
                            ? "未返回探测结果"
                            : "等待开始"}
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
                            <p className="pt-1 text-foreground/80">
                              {runtimeAdaptationLabel(
                                transport,
                                branch.reasoningSource,
                                branch.historyReplay,
                              )}
                            </p>
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              className="h-auto w-full justify-between px-0 py-1 text-xs font-medium text-foreground/80 hover:bg-transparent"
                              aria-expanded={mappingExpanded}
                              onClick={() =>
                                setExpandedMappings((current) => {
                                  const next = new Set(current);
                                  if (next.has(mappingKey)) {
                                    next.delete(mappingKey);
                                  } else {
                                    next.add(mappingKey);
                                  }
                                  return next;
                                })
                              }
                            >
                              查看字段映射与生效方式
                              <ChevronDown
                                className={cn(
                                  "h-3.5 w-3.5 transition-transform",
                                  mappingExpanded && "rotate-180",
                                )}
                                aria-hidden
                              />
                            </Button>
                            {mappingExpanded && (
                              <div className="space-y-2 rounded-md border border-border-default bg-muted/20 p-2 text-[11px] leading-5">
                                <p className="font-medium text-foreground">
                                  实际运行字段映射
                                </p>
                                <ul className="space-y-0.5 font-mono text-muted-foreground">
                                  {runtimeMappingRows(
                                    transport,
                                    branch.reasoningSemantic,
                                    branch.reasoningSource,
                                    branch.historyReplay,
                                  ).map((row) => (
                                    <li key={row}>{row}</li>
                                  ))}
                                </ul>
                                <p className="border-t pt-2 font-sans text-muted-foreground">
                                  保存并启用 Provider
                                  后，每次运行请求都会自动应用；仅运行探测不会改写当前配置。
                                </p>
                              </div>
                            )}
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

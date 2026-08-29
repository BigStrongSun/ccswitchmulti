import type { CodexProviderSetPreview } from "@/lib/api/protocol-compatibility";
import { AlertTriangle, GitBranch } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface CodexProviderSetPreviewDialogProps {
  open: boolean;
  preview: CodexProviderSetPreview | null;
  pending: boolean;
  onBack: () => void;
  onConfirmSplit: () => void;
  onRetry: () => void;
}

export function CodexProviderSetPreviewDialog(
  props: CodexProviderSetPreviewDialogProps,
) {
  const { open, preview, pending, onBack, onConfirmSplit, onRetry } = props;
  if (!preview || preview.plan.kind === "single") return null;

  const blocked = preview.plan.kind === "blocked";
  const title = blocked ? "暂时无法保存" : "按协议自动拆分";

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !pending) onBack();
      }}
    >
      <DialogContent className="max-w-2xl" zIndex="top">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {blocked ? (
              <AlertTriangle className="h-5 w-5 text-amber-600" />
            ) : (
              <GitBranch className="h-5 w-5 text-sky-600" />
            )}
            {title}
          </DialogTitle>
          <DialogDescription>
            {blocked
              ? "至少一个启用模型还没有可安全执行 Codex 工具事务的唯一协议。当前配置未写入，请重新探测或返回调整模型。"
              : "同一模型源中的模型通过了不同的最佳 Codex 协议。保存后仍显示为一个模型源，CCSM 会在内部生成两个单协议 Provider 并自动路由。"}
          </DialogDescription>
        </DialogHeader>

        {preview.plan.kind === "split" ? (
          <div className="grid gap-3 sm:grid-cols-2">
            <ModelGroup
              title="Responses 模型"
              models={preview.responsesModels}
            />
            <ModelGroup
              title="Chat Completions 模型"
              models={preview.chatModels}
            />
          </div>
        ) : (
          <div className="space-y-2">
            {preview.plan.models.map((model) => (
              <article
                key={`${model.model}:${model.upstreamModel}`}
                className="rounded-md border border-amber-500/30 bg-amber-500/5 p-3"
              >
                <div className="font-medium">{model.model}</div>
                {model.upstreamModel !== model.model && (
                  <div className="text-xs text-muted-foreground">
                    {model.upstreamModel}
                  </div>
                )}
                <div className="mt-1 text-sm text-amber-700 dark:text-amber-300">
                  {blockedReasonLabel(model.reason)}
                </div>
                {model.stage && (
                  <div className="mt-2 space-y-0.5 text-xs text-muted-foreground">
                    <div>失败阶段：{blockedStageLabel(model.stage)}</div>
                    {model.failureKind && (
                      <div>
                        失败类型：{blockedFailureLabel(model.failureKind)}
                        {model.statusCode ? `（${model.statusCode}）` : ""}
                      </div>
                    )}
                  </div>
                )}
              </article>
            ))}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onBack} disabled={pending}>
            返回调整模型
          </Button>
          {blocked ? (
            <Button onClick={onRetry} disabled={pending}>
              重新探测
            </Button>
          ) : (
            <Button onClick={onConfirmSplit} disabled={pending}>
              确认按协议拆分
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ModelGroup({ title, models }: { title: string; models: string[] }) {
  return (
    <section className="rounded-lg border bg-muted/20 p-3">
      <h3 className="text-sm font-medium">{title}</h3>
      <ul className="mt-2 space-y-1 text-sm text-muted-foreground">
        {models.map((model) => (
          <li key={model} className="rounded bg-background px-2 py-1">
            {model}
          </li>
        ))}
      </ul>
    </section>
  );
}

function blockedReasonLabel(reason: string) {
  switch (reason) {
    case "probe_required":
      return "尚未完成协议深度探测";
    case "probe_stale":
      return "探测结果已失效，请重新探测";
    case "probe_not_verified":
      return "探测结果尚未通过完整验证";
    case "probe_has_no_selection":
      return "两个协议都没有形成唯一可用结论";
    case "conflicting_probe_records":
      return "探测结果互相冲突，请重新探测";
    case "probe_target_mismatch":
      return "探测结果与当前模型配置不匹配";
    case "duplicate_model_identity":
      return "模型目录中存在重复模型标识";
    default:
      return "当前探测结果不能用于自动保存";
  }
}

function blockedStageLabel(stage: string) {
  switch (stage) {
    case "baseline":
      return "基础请求";
    case "streaming":
      return "流式响应";
    case "reasoning":
      return "推理内容";
    case "forced_tool":
      return "强制工具调用";
    case "continuation":
      return "工具续轮";
    default:
      return stage;
  }
}

function blockedFailureLabel(kind: string) {
  switch (kind) {
    case "http_status":
      return "HTTP 状态";
    case "timeout":
      return "请求超时";
    case "network":
      return "网络错误";
    case "response_too_large":
      return "响应过大";
    case "invalid_response":
      return "响应格式无效";
    case "invalid_request":
      return "请求无效";
    default:
      return kind;
  }
}

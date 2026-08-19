import { Button } from "@/components/ui/button";
import type {
  CodexModelReasoningResolution,
  CodexReasoningControlKind,
  CodexReasoningSupportStatus,
} from "@/types/codexSubagentV2";

/**
 * P3 模型卡片：展示单模型「最终生效」的推理能力（与 catalog / 请求 / Sub-Agent
 * 同源），以及 unknown 状态下的动作。
 *
 * 设计原则：
 * - 只读展示 resolved 结果，不在此处编辑（编辑走下方既有手动声明 UI）。
 * - 具体模型只显示其 resolved capability 的子集，不因公共词表存在就宣称全部支持。
 * - unknown 状态默认「使用服务端默认」，并提供重新检测 / 采用检测结果 /
 *   手动声明 / 恢复内置值 动作。
 */

const CONTROL_KIND_LABEL: Record<CodexReasoningControlKind, string> = {
  none: "无控制",
  boolean: "布尔开关",
  graded: "分档 effort",
  budget: "token 预算",
  unknown: "未知",
};

const SOURCE_LABEL: Record<string, string> = {
  user: "用户声明",
  detection: "自动检测",
  library: "维护能力库",
  builtin: "内置预设",
  official: "官方模型",
  unknown: "未知",
};

export type CodexReasoningCardStatus = "supported" | "unsupported" | "unknown";

/** 由 resolution 推导卡片状态（三态）。 */
export function reasoningCardStatus(
  resolution: CodexModelReasoningResolution,
): CodexReasoningCardStatus {
  const status: CodexReasoningSupportStatus | undefined =
    resolution.capability?.supportStatus;
  if (status === "confirmed_supported") return "supported";
  if (status === "confirmed_unsupported") return "unsupported";
  // 无三态字段时回退到 resolved.supportKind。
  switch (resolution.resolved.supportKind) {
    case "effort_levels":
    case "boolean_only":
      return "supported";
    case "unsupported":
      return "unsupported";
    case "unknown":
    default:
      return "unknown";
  }
}

export const REASONING_STATUS_LABEL: Record<CodexReasoningCardStatus, string> =
  {
    supported: "支持推理",
    unsupported: "不支持推理",
    unknown: "未知（使用服务端默认）",
  };

/** 控制类型：优先用声明的 controlKind，否则由 resolved.supportKind 推导。 */
export function reasoningControlKind(
  resolution: CodexModelReasoningResolution,
): CodexReasoningControlKind {
  const declared = resolution.capability?.controlKind;
  if (declared) return declared;
  switch (resolution.resolved.supportKind) {
    case "effort_levels":
      return "graded";
    case "boolean_only":
      return "boolean";
    case "unsupported":
    case "unknown":
    default:
      return "unknown";
  }
}

/** 最终行为：描述 Codex 实际会向上游发送什么。 */
export function describeFinalBehavior(
  resolution: CodexModelReasoningResolution,
): string {
  const { resolved } = resolution;
  const parameter = resolution.capability?.upstream?.parameter;
  switch (resolved.supportKind) {
    case "effort_levels": {
      const selectable = resolved.codexSelectableEfforts.join(" / ") || "无";
      const def = resolved.providerDefaultEffort ?? "模型默认";
      const param = parameter ? `（参数 ${parameter}）` : "";
      return `按档位发送 reasoning effort${param}；Codex 可选：${selectable}；默认：${def}。`;
    }
    case "boolean_only": {
      const param = parameter ? `（参数 ${parameter}）` : "";
      return `发送布尔开关${param}，不区分档位。`;
    }
    case "unsupported":
      return "不发送推理参数（模型不支持推理）。";
    case "unknown":
    default:
      return "使用服务端默认（不发送推理参数）。";
  }
}

function formatFetchedAt(ms: number): string {
  if (!ms) return "—";
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return String(ms);
  }
}

function EffortList({ efforts }: { efforts: string[] }) {
  if (!efforts.length) return <span className="text-muted-foreground">无</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {efforts.map((e) => (
        <code
          key={e}
          className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]"
        >
          {e}
        </code>
      ))}
    </span>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid gap-0.5">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="text-foreground">{children}</dd>
    </div>
  );
}

export interface CodexModelReasoningCardProps {
  resolution: CodexModelReasoningResolution;
  /** 是否存在内置预设（决定「恢复内置值」动作是否可用）。 */
  hasBuiltinPreset: boolean;
  redetecting: boolean;
  onRedetect: () => void;
  onAdoptDetection: () => void;
  onManualDeclare: () => void;
  onRestoreBuiltin: () => void;
}

export function CodexModelReasoningCard({
  resolution,
  hasBuiltinPreset,
  redetecting,
  onRedetect,
  onAdoptDetection,
  onManualDeclare,
  onRestoreBuiltin,
}: CodexModelReasoningCardProps) {
  const status = reasoningCardStatus(resolution);
  const controlKind = reasoningControlKind(resolution);
  const { resolved, detection } = resolution;
  const isUnknown = status === "unknown";

  return (
    <div className="space-y-3 rounded-md border bg-muted/30 p-3">
      <div className="flex items-center justify-between gap-2">
        <span className="font-medium">最终生效能力</span>
        <span
          className={
            "rounded-full px-2 py-0.5 text-[11px] " +
            (status === "supported"
              ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
              : status === "unsupported"
                ? "bg-rose-500/15 text-rose-600 dark:text-rose-400"
                : "bg-amber-500/15 text-amber-600 dark:text-amber-400")
          }
        >
          {REASONING_STATUS_LABEL[status]}
        </span>
      </div>

      <dl className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <Field label="控制类型">{CONTROL_KIND_LABEL[controlKind]}</Field>
        <Field label="能力来源">
          {SOURCE_LABEL[resolution.source] ?? resolution.source}
        </Field>
        <Field label="能力指纹">
          <code
            className="font-mono text-[11px]"
            title={resolution.fingerprint || "未生成"}
          >
            {resolution.fingerprint
              ? `${resolution.fingerprint.slice(0, 16)}…`
              : "未生成（未知能力）"}
          </code>
        </Field>
        {detection ? (
          <Field label="核验时间">
            {formatFetchedAt(detection.fetchedAt)}
            <span className="ml-1 text-muted-foreground">
              （{detection.source}）
            </span>
          </Field>
        ) : null}
        <Field label="Provider 原生档位">
          <EffortList efforts={resolved.providerAcceptedEfforts} />
        </Field>
        <Field label="Codex 可选档位">
          <EffortList efforts={resolved.codexSelectableEfforts} />
        </Field>
        <Field label="默认值">
          {resolved.providerDefaultEffort ?? "模型默认"}
        </Field>
        <Field label="关闭能力">
          {resolved.disableAllowed ? "可关闭（none）" : "不可关闭"}
        </Field>
        <Field label="映射">
          {Object.keys(resolved.effortMap).length ? (
            <span className="font-mono text-[11px]">
              {Object.entries(resolved.effortMap)
                .map(([k, v]) => `${k}→${v}`)
                .join("，")}
            </span>
          ) : (
            <span className="text-muted-foreground">恒等</span>
          )}
        </Field>
      </dl>

      <div className="rounded-md border bg-background p-2">
        <div className="text-muted-foreground">最终行为</div>
        <div>{describeFinalBehavior(resolution)}</div>
      </div>

      {isUnknown ? (
        <div className="space-y-2">
          <p className="text-muted-foreground">
            当前未读到该模型的推理能力声明，默认使用服务端默认。可执行以下动作：
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onRedetect}
              disabled={redetecting}
            >
              {redetecting ? "检测中…" : "重新检测"}
            </Button>
            {resolution.hasDetectionCandidate ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onAdoptDetection}
              >
                采用检测结果
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onManualDeclare}
            >
              手动声明
            </Button>
            {hasBuiltinPreset ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onRestoreBuiltin}
              >
                恢复内置值
              </Button>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

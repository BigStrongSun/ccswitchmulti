import { Button } from "@/components/ui/button";

export interface CodexModelReasoningSummaryProps {
  model: string;
  source: string;
  selectableEfforts: string[];
  defaultEffort?: string;
  ultraEnabled: boolean;
  expanded: boolean;
  onToggle: () => void;
}

export function CodexModelReasoningSummary({
  model,
  source,
  selectableEfforts,
  defaultEffort,
  ultraEnabled,
  expanded,
  onToggle,
}: CodexModelReasoningSummaryProps) {
  const displayModel = model || "未命名模型";

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border bg-background p-3 text-xs">
      <div className="min-w-0 space-y-1">
        <p className="font-medium text-foreground">{displayModel}</p>
        <p className="text-muted-foreground">能力来源：{source}</p>
        <p className="text-muted-foreground">
          Codex 档位：{selectableEfforts.join(" / ") || "未声明"}
        </p>
        <p className="text-muted-foreground">
          默认值：{defaultEffort ?? "模型默认"} ·{" "}
          <span>Ultra：{ultraEnabled ? "开启" : "关闭"}</span>
        </p>
      </div>
      <Button
        type="button"
        variant={expanded ? "secondary" : "outline"}
        size="sm"
        aria-label={`${expanded ? "收起" : "配置"} ${displayModel} 的推理能力`}
        onClick={onToggle}
      >
        {expanded ? "收起配置" : "配置推理能力"}
      </Button>
    </div>
  );
}

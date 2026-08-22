import { Button } from "@/components/ui/button";
import type { CodexReasoningEffort } from "@/types";

export interface CodexModelReasoningSummaryProps {
  model: string;
  source: string;
  selectableEfforts: string[];
  defaultEffort?: string;
  ultraEnabled: boolean;
  ultraEffort?: CodexReasoningEffort;
  ultraEfforts: CodexReasoningEffort[];
  onUltraChange: (ultra: {
    enabled: boolean;
    providerEffort?: CodexReasoningEffort;
  }) => void;
  expanded: boolean;
  onToggle: () => void;
}

export function CodexModelReasoningSummary({
  model,
  source,
  selectableEfforts,
  defaultEffort,
  ultraEnabled,
  ultraEffort,
  ultraEfforts,
  onUltraChange,
  expanded,
  onToggle,
}: CodexModelReasoningSummaryProps) {
  const displayModel = model || "未命名模型";

  return (
    <div className="grid gap-3 rounded-md border bg-background p-3 text-xs lg:grid-cols-[minmax(10rem,1fr)_minmax(12rem,1.2fr)_minmax(16rem,1.2fr)_auto] lg:items-center">
      <div className="min-w-0">
        <p className="font-medium text-foreground">{displayModel}</p>
      </div>
      <div className="space-y-1 border-t pt-3 lg:border-l lg:border-t-0 lg:pl-3 lg:pt-0">
        <p className="text-muted-foreground">能力来源：{source}</p>
        <p className="text-muted-foreground">
          Codex 档位：{selectableEfforts.join(" / ") || "未声明"}
        </p>
        <p className="text-muted-foreground">
          默认值：{defaultEffort ?? "模型默认"}
        </p>
      </div>
      <div className="space-y-1 border-t pt-3 lg:border-l lg:border-t-0 lg:pl-3 lg:pt-0">
        <span className="label text-muted-foreground">Ultra</span>
        <label className="flex items-center gap-2 font-medium">
          <input
            type="checkbox"
            aria-label={`解锁 ${displayModel} 的 Ultra 档`}
            checked={ultraEnabled}
            disabled={ultraEfforts.length === 0}
            onChange={(event) =>
              onUltraChange({
                enabled: event.target.checked,
                providerEffort: ultraEffort,
              })
            }
          />
          解锁 Ultra 档
        </label>
        <select
          className="w-full rounded border bg-background px-2 py-1"
          aria-label={`${displayModel} Ultra 对应的 Provider 推理强度`}
          value={ultraEffort ?? ""}
          disabled={!ultraEnabled}
          onChange={(event) =>
            onUltraChange({
              enabled: ultraEnabled,
              providerEffort: (event.target.value || undefined) as
                | CodexReasoningEffort
                | undefined,
            })
          }
        >
          <option value="">选择 Provider 强度…</option>
          {ultraEfforts.map((effort) => (
            <option key={effort} value={effort}>
              {effort}
            </option>
          ))}
        </select>
        <p className="text-muted-foreground">
          {ultraEnabled && ultraEffort
            ? `已解锁，使用 ${ultraEffort}`
            : "独立于能力来源；解锁后请选择强度"}
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

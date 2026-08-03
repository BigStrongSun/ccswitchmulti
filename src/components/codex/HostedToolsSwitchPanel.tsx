import { Image, Search } from "lucide-react";

export interface HostedToolsSwitchPanelProps {
  webSearchEnabled: boolean;
  imageGenerationEnabled: boolean;
  onChange: (next: {
    webSearchEnabled: boolean;
    imageGenerationEnabled: boolean;
  }) => void;
  disabled?: boolean;
}

export function HostedToolsSwitchPanel({
  webSearchEnabled,
  imageGenerationEnabled,
  onChange,
  disabled = false,
}: HostedToolsSwitchPanelProps) {
  return (
    <div className="space-y-3 rounded-lg border border-blue-200 bg-blue-50/70 p-3 dark:border-blue-700/40 dark:bg-blue-950/10">
      <div>
        <div className="text-xs font-semibold text-muted-foreground dark:text-slate-300">
          OpenAI Hosted Tools
        </div>
        <p className="mt-1 text-xs leading-5 text-muted-foreground dark:text-slate-400">
          Codex 请求里的 OpenAI hosted tool 会由 CCSM 转成第三方模型可调用的普通
          function；实际执行优先复用已登录的 Codex OAuth，也可使用独立 OpenAI
          hosted 凭据。
        </p>
      </div>
      <label className="flex cursor-pointer items-start justify-between gap-3 rounded-md border border-border bg-background/80 p-3 dark:border-slate-700 dark:bg-slate-950/60">
        <span className="flex min-w-0 items-start gap-2">
          <Search className="mt-0.5 h-4 w-4 shrink-0 text-blue-600 dark:text-blue-300" />
          <span>
            <span className="block text-sm font-semibold text-foreground dark:text-slate-100">
              Web Search
            </span>
            <span className="mt-1 block text-xs leading-5 text-muted-foreground dark:text-slate-400">
              开启后，Codex 的 hosted web_search 会桥接给第三方 Chat 模型。
            </span>
          </span>
        </span>
        <input
          type="checkbox"
          checked={webSearchEnabled}
          onChange={(event) =>
            onChange({
              webSearchEnabled: event.target.checked,
              imageGenerationEnabled,
            })
          }
          className="mt-1 h-5 w-5 accent-blue-500"
          disabled={disabled}
          aria-label="Web Search hosted tool bridge"
        />
      </label>
      <label className="flex cursor-pointer items-start justify-between gap-3 rounded-md border border-border bg-background/80 p-3 dark:border-slate-700 dark:bg-slate-950/60">
        <span className="flex min-w-0 items-start gap-2">
          <Image className="mt-0.5 h-4 w-4 shrink-0 text-blue-600 dark:text-blue-300" />
          <span>
            <span className="block text-sm font-semibold text-foreground dark:text-slate-100">
              Image Generation
            </span>
            <span className="mt-1 block text-xs leading-5 text-muted-foreground dark:text-slate-400">
              开启后，Codex 的 hosted image_generation 会桥接成 generate_image
              函数。
            </span>
          </span>
        </span>
        <input
          type="checkbox"
          checked={imageGenerationEnabled}
          onChange={(event) =>
            onChange({
              webSearchEnabled,
              imageGenerationEnabled: event.target.checked,
            })
          }
          className="mt-1 h-5 w-5 accent-blue-500"
          disabled={disabled}
          aria-label="Image Generation hosted tool bridge"
        />
      </label>
    </div>
  );
}

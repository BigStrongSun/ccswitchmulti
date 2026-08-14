import { useEffect, useState } from "react";
import { Loader2, Save } from "lucide-react";
import { toast } from "sonner";

import JsonEditor from "@/components/JsonEditor";
import { Button } from "@/components/ui/button";
import { configApi } from "@/lib/api";
import {
  isCodexGoalModeEnabled,
  setCodexGoalMode,
} from "@/utils/providerConfigUtils";

const DEFAULT_CODEX_GLOBAL_CONFIG = `# Shared Codex configuration
# Settings here are available to Codex providers that apply the common config.`;

export function CodexGlobalConfigSettings() {
  const [value, setValue] = useState(DEFAULT_CODEX_GLOBAL_CONFIG);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    configApi
      .getCommonConfigSnippet("codex")
      .then((snippet) => {
        if (!active) return;
        setValue(snippet?.trim() ? snippet : DEFAULT_CODEX_GLOBAL_CONFIG);
        setError("");
      })
      .catch((loadError) => {
        if (!active) return;
        setError(
          `加载 Codex 全局配置失败：${loadError instanceof Error ? loadError.message : String(loadError)}`,
        );
      })
      .finally(() => {
        if (active) setIsLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  async function save() {
    setIsSaving(true);
    setError("");
    try {
      await configApi.setCommonConfigSnippet("codex", value);
      toast.success("Codex 全局配置已保存");
    } catch (saveError) {
      const message = `保存 Codex 全局配置失败：${saveError instanceof Error ? saveError.message : String(saveError)}`;
      setError(message);
      toast.error(message);
    } finally {
      setIsSaving(false);
    }
  }

  if (isLoading) {
    return (
      <div
        role="status"
        className="flex items-center gap-2 text-sm text-muted-foreground"
      >
        <Loader2 className="h-4 w-4 animate-spin" />
        正在加载 Codex 全局配置…
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-blue-500/20 bg-blue-500/5 p-4">
        <p className="text-sm font-medium text-foreground">
          跨 Provider 的 Codex 行为
        </p>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
          在这里维护 Goal mode 和共享 TOML。API Key、Base
          URL、模型目录与路由规则仍属于各自的 Provider 或 MultiRouter。
        </p>
      </div>

      <label className="flex items-start justify-between gap-4 rounded-md border border-border-default p-3">
        <span className="space-y-1">
          <span className="block text-sm font-medium text-foreground">
            启用 Goal mode
          </span>
          <span className="block text-xs leading-relaxed text-muted-foreground">
            写入共享 Codex TOML；不再要求在每一个 Provider 配置页重复设置。
          </span>
        </span>
        <input
          aria-label="启用 Goal mode"
          type="checkbox"
          checked={isCodexGoalModeEnabled(value)}
          onChange={(event) =>
            setValue((current) =>
              setCodexGoalMode(current, event.target.checked),
            )
          }
          className="mt-0.5 h-4 w-4 rounded border-border-default text-blue-500 focus:ring-blue-500"
        />
      </label>

      <div className="space-y-2">
        <label
          className="text-sm font-medium text-foreground"
          htmlFor="codex-global-config"
        >
          Codex 全局 TOML
        </label>
        <JsonEditor
          value={value}
          onChange={setValue}
          placeholder={DEFAULT_CODEX_GLOBAL_CONFIG}
          darkMode={document.documentElement.classList.contains("dark")}
          rows={12}
          showValidation={false}
          language="javascript"
        />
      </div>

      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}

      <div className="flex justify-end">
        <Button
          type="button"
          onClick={() => void save()}
          disabled={isSaving}
          className="gap-2"
        >
          {isSaving ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Save className="h-4 w-4" />
          )}
          保存 Codex 全局配置
        </Button>
      </div>
    </div>
  );
}

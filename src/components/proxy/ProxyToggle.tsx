/**
 * 代理模式切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭代理模式
 * 启用时自动接管 Live 配置，关闭时恢复原始配置
 */

import { Radio, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";

interface ProxyToggleProps {
  className?: string;
  activeApp: AppId;
}

export function ProxyToggle({ className, activeApp }: ProxyToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    takeoverStatus,
    setTakeoverForApp,
    interactionMode,
    setInteractionMode,
    isPending,
    status,
  } = useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    try {
      await setTakeoverForApp({ appType: activeApp, enabled: checked });
    } catch (error) {
      console.error("[ProxyToggle] Toggle takeover failed:", error);
    }
  };

  const takeoverEnabled = takeoverStatus?.[activeApp] || false;

  const appLabel =
    activeApp === "claude"
      ? "Claude"
      : activeApp === "codex"
        ? "Codex"
        : activeApp === "gemini"
          ? "Gemini"
          : "OpenCode";

  const tooltipText = takeoverEnabled
    ? isRunning
      ? t("proxy.takeover.tooltip.active", {
          appLabel,
          address: status?.address,
          port: status?.port,
          defaultValue: `${appLabel} 已接管 - ${status?.address}:${status?.port}\n切换该应用供应商为热切换`,
        })
      : t("proxy.takeover.tooltip.broken", {
          appLabel,
          defaultValue: `${appLabel} 已接管，但代理服务未运行`,
        })
    : t("proxy.takeover.tooltip.inactive", {
        appLabel,
        defaultValue: `接管 ${appLabel} 的 Live 配置，让该应用请求走本地代理`,
      });

  return (
    <div className={cn("flex items-center gap-1", className)}>
      {activeApp === "codex" && (
        <div className="flex h-8 items-center rounded-lg border bg-background p-0.5">
          <Button
            type="button"
            size="sm"
            variant={interactionMode === "Chat" ? "default" : "ghost"}
            className={cn(
              "h-7 px-2 text-xs",
              interactionMode === "Chat" &&
                "bg-primary text-primary-foreground shadow-sm",
            )}
            aria-pressed={interactionMode === "Chat"}
            disabled={isPending}
            onClick={() => setInteractionMode("Chat")}
          >
            Chat
          </Button>
          <Button
            type="button"
            size="sm"
            variant={interactionMode === "Ask" ? "default" : "ghost"}
            className={cn(
              "h-7 px-2 text-xs",
              interactionMode === "Ask" &&
                "bg-primary text-primary-foreground shadow-sm",
            )}
            aria-pressed={interactionMode === "Ask"}
            disabled={isPending}
            onClick={() => setInteractionMode("Ask")}
          >
            Ask
          </Button>
          <Button
            type="button"
            size="sm"
            variant={interactionMode === "Code" ? "default" : "ghost"}
            className={cn(
              "h-7 px-2 text-xs",
              interactionMode === "Code" &&
                "bg-primary text-primary-foreground shadow-sm",
            )}
            aria-pressed={interactionMode === "Code"}
            disabled={isPending}
            onClick={() => setInteractionMode("Code")}
          >
            Code
          </Button>
        </div>
      )}
      <div
        className="flex h-8 items-center gap-1 rounded-lg bg-muted/50 px-1.5 transition-all"
        title={tooltipText}
      >
        {isPending ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : (
          <Radio
            className={cn(
              "h-4 w-4 transition-colors",
              takeoverEnabled
                ? "text-emerald-500 animate-pulse"
                : "text-muted-foreground",
            )}
          />
        )}
        <Switch
          checked={takeoverEnabled}
          onCheckedChange={handleToggle}
          disabled={isPending}
        />
      </div>
    </div>
  );
}

import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import { Monitor, MoreHorizontal, Terminal } from "lucide-react";

const APP_BADGE_ICON: Partial<
  Record<AppId, { icon: typeof Terminal; offsetY?: number }>
> = {
  claude: { icon: Terminal },
  "claude-desktop": { icon: Monitor, offsetY: 0.5 },
};

interface AppSwitcherProps {
  activeApp: AppId;
  onSwitch: (app: AppId) => void;
  visibleApps?: VisibleApps;
}

const ALL_APPS: AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
];
const STORAGE_KEY = "cc-switch-last-app";

const APP_ICON_NAME: Record<AppId, string> = {
  claude: "claude",
  "claude-desktop": "claude",
  codex: "openai",
  gemini: "gemini",
  grokbuild: "grok",
  opencode: "opencode",
  openclaw: "openclaw",
  hermes: "hermes",
};

const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
};

function AppGlyph({ app, isActive }: { app: AppId; isActive: boolean }) {
  const badgeConfig = APP_BADGE_ICON[app];
  const BadgeIcon = badgeConfig?.icon;
  return (
    <span className="relative inline-flex shrink-0">
      <ProviderIcon
        icon={APP_ICON_NAME[app]}
        name={APP_DISPLAY_NAME[app]}
        size={20}
      />
      {BadgeIcon && (
        <span
          className={cn(
            "absolute -bottom-0.5 -right-0.5 flex items-center justify-center rounded-[3px] border h-[11px] w-[11px]",
            isActive
              ? "bg-background border-border text-foreground"
              : "bg-muted border-background text-muted-foreground group-hover:bg-background group-hover:text-foreground",
          )}
          aria-hidden="true"
        >
          <BadgeIcon
            className="h-[8px] w-[8px]"
            strokeWidth={2.5}
            style={
              badgeConfig.offsetY
                ? { transform: `translateY(${badgeConfig.offsetY}px)` }
                : undefined
            }
          />
        </span>
      )}
    </span>
  );
}

export function AppSwitcher({
  activeApp,
  onSwitch,
  visibleApps,
}: AppSwitcherProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const [moreOpen, setMoreOpen] = useState(false);

  const handleSwitch = (app: AppId) => {
    if (app === activeApp) return;
    localStorage.setItem(STORAGE_KEY, app);
    onSwitch(app);
  };

  const appsToShow = ALL_APPS.filter((app) => {
    if (!visibleApps) return true;
    return visibleApps[app];
  });
  const appCount = appsToShow.length;
  const [visibleCount, setVisibleCount] = useState(appCount);

  useLayoutEffect(() => {
    const root = rootRef.current;
    const slot = root?.parentElement;
    if (!root || !slot) return;

    const compute = () => {
      const sample = root.querySelector("button");
      if (!sample) return;
      const itemWidth = sample.offsetWidth;
      if (itemWidth <= 0) return;

      const rootStyle = window.getComputedStyle(root);
      const gap = parseFloat(rootStyle.columnGap) || 0;
      const padding =
        (parseFloat(rootStyle.paddingLeft) || 0) +
        (parseFloat(rootStyle.paddingRight) || 0);
      const available = slot.clientWidth;
      const widthAll = padding + appCount * itemWidth + (appCount - 1) * gap;

      if (widthAll <= available) {
        setVisibleCount(appCount);
        return;
      }

      const fit = Math.floor(
        (available - padding - itemWidth) / (itemWidth + gap),
      );
      setVisibleCount(Math.max(1, Math.min(appCount - 1, fit)));
    };

    compute();
    const observer = new ResizeObserver(compute);
    observer.observe(slot);
    return () => observer.disconnect();
  }, [appCount]);

  const visibleList = appsToShow.slice(0, Math.max(1, visibleCount));
  if (appsToShow.includes(activeApp) && !visibleList.includes(activeApp)) {
    visibleList[visibleList.length - 1] = activeApp;
  }
  const overflowList = appsToShow.filter((app) => !visibleList.includes(app));
  const moreLabel = t("appSwitcher.more", { defaultValue: "More apps" });

  return (
    <div
      ref={rootRef}
      className="inline-flex gap-1 rounded-xl bg-muted p-1"
      style={{ WebkitAppRegion: "no-drag" } as any}
    >
      {visibleList.map((app) => {
        const isActive = activeApp === app;
        return (
          <button
            key={app}
            type="button"
            onClick={() => handleSwitch(app)}
            title={APP_DISPLAY_NAME[app]}
            aria-label={APP_DISPLAY_NAME[app]}
            className={cn(
              "group inline-flex h-8 items-center rounded-md px-3 text-sm font-medium transition-all duration-200",
              isActive
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:bg-background/50 hover:text-foreground",
            )}
          >
            <AppGlyph app={app} isActive={isActive} />
          </button>
        );
      })}
      {overflowList.length > 0 && (
        <Popover open={moreOpen} onOpenChange={setMoreOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={moreLabel}
              aria-label={moreLabel}
              className={cn(
                "inline-flex h-8 items-center rounded-md px-3 transition-all duration-200",
                moreOpen
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:bg-background/50 hover:text-foreground",
              )}
            >
              <MoreHorizontal size={20} className="shrink-0" />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="end"
            sideOffset={6}
            className="z-[100] w-56 p-1"
          >
            {overflowList.map((app) => (
              <button
                key={app}
                type="button"
                aria-label={APP_DISPLAY_NAME[app]}
                onClick={() => {
                  setMoreOpen(false);
                  handleSwitch(app);
                }}
                className="group flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <AppGlyph app={app} isActive={false} />
                <span className="truncate">{APP_DISPLAY_NAME[app]}</span>
              </button>
            ))}
          </PopoverContent>
        </Popover>
      )}
    </div>
  );
}

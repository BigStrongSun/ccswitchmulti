import { Toaster as SonnerToaster } from "sonner";
import { useTheme } from "@/components/theme-provider";

export function Toaster() {
  const { theme } = useTheme();

  // 将应用主题映射到 Sonner 的主题
  // 如果是 "system"，Sonner 会自己处理
  const sonnerTheme = theme === "system" ? "system" : theme;

  return (
    <SonnerToaster
      position="top-center"
      richColors
      theme={sonnerTheme}
      // 全局通知层必须比 Radix 模态锁更优先。
      //
      // Radix 的模态弹窗（@radix-ui/react-dismissable-layer）打开时会给
      // `document.body` 加内联 `pointer-events: none`，只把弹窗内容自己标成
      // `auto`。sonner 的通知层挂在 #root 里，会继承这个 none，于是弹窗打开
      // 期间 toast 上的操作按钮（例如「查看日志」）点不动——不是 z-index 遮挡，
      // 而是继承下来的 pointer-events 被关掉了。
      // 这里在通知容器上显式声明 auto，覆盖继承值；sonner 自己的
      // `[data-sonner-toast][data-visible='false'] { pointer-events: none }`
      // 仍然生效，隐藏/移出中的 toast 不会变得可点。
      style={{ pointerEvents: "auto" }}
      toastOptions={{
        duration: 2000,
        classNames: {
          toast:
            "group rounded-md border bg-background text-foreground shadow-lg",
          title: "text-sm font-semibold",
          description: "text-sm text-muted-foreground",
          closeButton:
            "absolute right-2 top-2 rounded-full p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
          actionButton:
            "rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90",
        },
      }}
    />
  );
}

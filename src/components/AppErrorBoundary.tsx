import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";

/** 应用根错误边界的属性。 */
interface AppErrorBoundaryProps {
  /** 正常情况下要渲染的应用内容。 */
  children: ReactNode;
}

/** 应用根错误边界的内部状态。 */
interface AppErrorBoundaryState {
  /** 捕获到的渲染异常；为空代表子树可正常显示。 */
  error: Error | null;
}

/**
 * 把未捕获的 React 渲染异常转换为可恢复页面，避免整个 WebView 只剩白屏。
 *
 * 该边界覆盖渲染、构造和 React 提交阶段的异常；网络请求与异步事件仍由各自调用点处理。
 */
export class AppErrorBoundary extends Component<
  AppErrorBoundaryProps,
  AppErrorBoundaryState
> {
  /** 初始化错误状态，首次渲染默认透传子组件。 */
  public state: AppErrorBoundaryState = { error: null };

  /**
   * 从子树错误派生降级状态，让下一次渲染展示恢复界面。
   *
   * @param error React 捕获到的原始异常。
   * @returns 用于替换当前状态的错误对象。
   */
  public static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { error };
  }

  /**
   * 把组件栈写入控制台，保留诊断证据而不影响恢复页面渲染。
   *
   * @param error 原始异常。
   * @param errorInfo React 提供的组件栈信息。
   */
  public componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    console.error("[AppErrorBoundary] 未捕获的 React 渲染错误", {
      error,
      componentStack: errorInfo.componentStack,
    });
  }

  /**
   * 渲染正常子树或错误恢复界面。
   *
   * @returns 子组件内容，或包含重新加载操作的恢复页。
   */
  public render(): ReactNode {
    if (!this.state.error) {
      return this.props.children;
    }

    const detail = this.state.error.message || "未提供错误详情";
    return (
      <main className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
        <section className="w-full max-w-lg border border-destructive/40 bg-card p-6 shadow-sm">
          <div className="flex items-start gap-3">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-destructive" />
            <div className="min-w-0 space-y-3">
              <div>
                <h1 className="text-lg font-semibold">应用界面加载失败</h1>
                <p className="mt-1 text-sm leading-6 text-muted-foreground">
                  请重新加载应用；若仍失败，请将下方错误详情和日志提供给维护者。
                </p>
              </div>
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words border bg-muted/40 p-3 text-xs text-muted-foreground">
                {detail}
              </pre>
              <Button type="button" onClick={() => window.location.reload()}>
                <RefreshCw className="h-4 w-4" />
                重新加载
              </Button>
            </div>
          </div>
        </section>
      </main>
    );
  }
}

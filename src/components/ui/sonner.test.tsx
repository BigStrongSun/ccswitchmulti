import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Toaster } from "@/components/ui/sonner";

afterEach(() => {
  toast.dismiss();
});

// 主题只影响通知配色，这里固定为 dark，避免为了 matchMedia 引入无关的环境桩。
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "dark" }),
}));

describe("global toaster interactions under modal dialogs", () => {
  /// 真实缺陷回归：Radix 模态弹窗打开时会给 body 加内联 pointer-events: none
  /// （@radix-ui/react-dismissable-layer），sonner 通知层挂在 #root 里会一起被
  /// 继承成不可点击，于是「正在刷新 Codex 状态」弹窗打开期间点不到 toast 上的
  /// 「查看日志」。修复前这里的 user-event 点击会直接报
  /// “element has `pointer-events: none`”。
  it("keeps toast action buttons clickable while a modal dialog locks the page", async () => {
    const onOpenLogs = vi.fn();
    render(
      <>
        <Dialog open>
          <DialogContent zIndex="top">
            <DialogTitle>正在刷新 Codex 状态</DialogTitle>
            <DialogDescription>请保持 CCSM 运行。</DialogDescription>
          </DialogContent>
        </Dialog>
        <Toaster />
      </>,
    );

    // 模态弹窗确实把 body 锁成 pointer-events: none（机制前提）。
    await waitFor(() => expect(document.body.style.pointerEvents).toBe("none"));

    toast.warning("检测到上次运行未正常退出，请检查配置恢复结果", {
      action: { label: "查看日志", onClick: onOpenLogs },
    });

    const action = await screen.findByRole("button", { name: "查看日志" });
    const toastEl = action.closest("[data-sonner-toast]") as HTMLElement;
    const toaster = document.querySelector(
      "[data-sonner-toaster]",
    ) as HTMLElement;

    // 通知层必须自己声明 auto，才能覆盖从 body 继承来的 none。
    expect(toaster.style.pointerEvents).toBe("auto");
    expect(window.getComputedStyle(toastEl).pointerEvents).toBe("auto");
    expect(window.getComputedStyle(action).pointerEvents).toBe("auto");

    await userEvent.click(action);
    expect(onOpenLogs).toHaveBeenCalledTimes(1);
  });
});

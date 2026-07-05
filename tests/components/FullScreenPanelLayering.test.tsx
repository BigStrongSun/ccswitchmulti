import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("FullScreenPanel 弹层上下文", () => {
  it("让面板内部的普通 Dialog 默认显示在面板上方", () => {
    render(
      <FullScreenPanel isOpen title="父面板" onClose={vi.fn()}>
        <Dialog open>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>内部弹窗</DialogTitle>
              <DialogDescription>测试内部弹窗层级</DialogDescription>
            </DialogHeader>
          </DialogContent>
        </Dialog>
      </FullScreenPanel>,
    );

    const dialog = screen.getByRole("dialog", { name: "内部弹窗" });

    expect(dialog).toHaveClass("z-[200]");
  });

  it("让面板内部的 ConfirmDialog 默认显示在面板上方", () => {
    render(
      <FullScreenPanel isOpen title="父面板" onClose={vi.fn()}>
        <ConfirmDialog
          isOpen
          title="确认保存"
          message="保存会写入配置"
          onConfirm={vi.fn()}
          onCancel={vi.fn()}
        />
      </FullScreenPanel>,
    );

    const dialog = screen.getByRole("dialog", { name: "确认保存" });

    expect(dialog).toHaveClass("z-[200]");
  });

  it("让向导内新增 provider 面板继续打开更高层级的子面板", () => {
    render(
      <FullScreenPanel
        isOpen
        title="新增 provider"
        onClose={vi.fn()}
        zIndexClassName="z-[140]"
      >
        <FullScreenPanel isOpen title="统一供应商" onClose={vi.fn()}>
          <div>子面板内容</div>
        </FullScreenPanel>
      </FullScreenPanel>,
    );

    const childTitle = screen.getByText("统一供应商");
    const childPanel = childTitle.closest(".fixed");

    expect(childPanel).toHaveClass("z-[160]");
  });
});

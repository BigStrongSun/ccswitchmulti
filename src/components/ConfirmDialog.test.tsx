import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

// 从 z-index class 字符串中提取数值，用于验证层级高低顺序
function zValue(className: string): number {
  if (className.startsWith("z-[")) return Number(className.slice(3, -1));
  return Number(className.slice(2));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

// ============================================================
// 层级行为回归测试 --- 防止默认 alert(z-[60]) 和 top(z-[200]) 被改坏
// ============================================================
describe("ConfirmDialog 层级行为回归测试", () => {
  /** 默认（非面板内）使用 alert 层 z-[60]，保证不被普通页面元素挡住 */
  it("在普通页面中默认使用 alert 层(z-[60])", () => {
    render(
      <ConfirmDialog
        isOpen
        title="确认操作"
        message="此操作不可撤销"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "确认操作" });
    expect(dialog).toHaveClass("z-[60]");
  });

  /** FullScreenPanel 内默认使用 top 层 z-[200]，防止 portal 后被面板遮住 */
  it("在 FullScreenPanel 内自动提升到 top 层(z-[200])", () => {
    render(
      <FullScreenPanel isOpen title="父面板" onClose={vi.fn()}>
        <ConfirmDialog
          isOpen
          title="确认保存"
          message="保存会写入配置文件"
          onConfirm={vi.fn()}
          onCancel={vi.fn()}
        />
      </FullScreenPanel>,
    );

    const dialog = screen.getByRole("dialog", { name: "确认保存" });
    expect(dialog).toHaveClass("z-[200]");
  });

  /** 显式 zIndex 应当覆盖上下文提供的默认层级 */
  it("显式传递 zIndex=\"base\" 时覆盖 FullScreenPanel 上下文，使用 base 层(z-40)", () => {
    render(
      <FullScreenPanel isOpen title="父面板" onClose={vi.fn()}>
        <ConfirmDialog
          isOpen
          title="低层级弹窗"
          message="测试显式覆盖"
          zIndex="base"
          onConfirm={vi.fn()}
          onCancel={vi.fn()}
        />
      </FullScreenPanel>,
    );

    const dialog = screen.getByRole("dialog", { name: "低层级弹窗" });
    expect(dialog).toHaveClass("z-40");
  });

  /** 数值验证：alert 层必须低于 top 层，防止层级顺序被颠倒 */
  it("alert 层(z-[60]) 层级值低于 top 层(z-[200])，保证高低关系正确", () => {
    expect(zValue("z-[60]")).toBeLessThan(zValue("z-[200]"));
  });
});

// ============================================================
// 基本功能回归测试 --- 防止渲染逻辑被改坏
// ============================================================
describe("ConfirmDialog 基本功能回归测试", () => {
  /** 渲染标题、消息和操作按钮 */
  it("正确渲染标题和消息文本", () => {
    render(
      <ConfirmDialog
        isOpen
        title="测试标题"
        message="测试消息内容"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByText("测试标题")).toBeInTheDocument();
    expect(screen.getByText("测试消息内容")).toBeInTheDocument();
  });

  /** 默认 cancel/confirm 按钮使用 i18n 键值 */
  it("未传 cancelText/confirmText 时使用 i18n 默认值", () => {
    render(
      <ConfirmDialog
        isOpen
        title="测试"
        message="默认按钮文案"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "common.confirm" }),
    ).toBeInTheDocument();
  });

  /** 自定义按钮文案可正常覆盖默认值 */
  it("支持自定义按钮文案", () => {
    render(
      <ConfirmDialog
        isOpen
        title="测试"
        message="自定义按钮"
        confirmText="确定"
        cancelText="取消"
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "取消" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "确定" })).toBeInTheDocument();
  });

  /** checkboxLabel 为空时不渲染勾选框，onConfirm 传 false */
  it("未提供 checkboxLabel 时不渲染勾选框，点击确认传 false", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        isOpen
        title="无勾选框"
        message="确认"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenCalledWith(false);
  });

  /** checkboxLabel 存在时渲染勾选框，勾选后点击确认传 true */
  it("提供 checkboxLabel 时渲染勾选框，勾选后点击确认回传 true", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        isOpen
        title="带勾选框"
        message="已阅读并同意"
        checkboxLabel="我已知晓"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    const checkbox = screen.getByRole("checkbox");
    expect(checkbox).toBeInTheDocument();
    expect(screen.getByText("我已知晓")).toBeInTheDocument();

    // 勾选
    fireEvent.click(checkbox);
    // 点击确认
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenCalledWith(true);
  });

  /** checkboxLabel 存在但未勾选时，点击确认传 false */
  it("提供 checkboxLabel 但未勾选时，点击确认回传 false", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        isOpen
        title="带勾选框"
        message="确认操作"
        checkboxLabel="我已知晓风险"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByRole("checkbox")).toBeInTheDocument();

    // 不勾选，直接确认
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenCalledWith(false);
  });

  /** 打开弹窗时重置勾选框到默认状态 */
  it("每次 isOpen=true 时重置 checkbox 到 checkboxDefaultChecked 状态", () => {
    const onConfirm = vi.fn();
    const { rerender } = render(
      <ConfirmDialog
        isOpen
        title="重置测试"
        message="重置勾选框"
        checkboxLabel="同意条款"
        checkboxDefaultChecked={true}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    // checkbox 初始为勾选状态
    const checkbox = screen.getByRole("checkbox") as HTMLButtonElement;
    expect(checkbox.getAttribute("data-state")).toBe("checked");

    // 取消勾选
    fireEvent.click(checkbox);
    expect(checkbox.getAttribute("data-state")).toBe("unchecked");

    // 重新打开弹窗（模拟关闭后再打开）
    rerender(
      <ConfirmDialog
        isOpen={false}
        title="重置测试"
        message="重置勾选框"
        checkboxLabel="同意条款"
        checkboxDefaultChecked={true}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );
    rerender(
      <ConfirmDialog
        isOpen
        title="重置测试"
        message="重置勾选框"
        checkboxLabel="同意条款"
        checkboxDefaultChecked={true}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    const checkboxAfterReopen = screen.getByRole(
      "checkbox",
    ) as HTMLButtonElement;
    expect(checkboxAfterReopen.getAttribute("data-state")).toBe("checked");
  });
});

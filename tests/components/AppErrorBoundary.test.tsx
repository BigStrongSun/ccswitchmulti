import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "@/components/AppErrorBoundary";

/** 用于验证根错误边界会接住子树渲染异常的测试组件。 */
function ThrowDuringRender(): never {
  throw new Error("测试渲染异常");
}

describe("AppErrorBoundary", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("将子组件渲染异常转换为可见的恢复界面", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <ThrowDuringRender />
      </AppErrorBoundary>,
    );

    expect(
      screen.getByRole("heading", { name: "应用界面加载失败" }),
    ).toBeInTheDocument();
    expect(screen.getByText("测试渲染异常")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新加载" })).toBeEnabled();
  });
});

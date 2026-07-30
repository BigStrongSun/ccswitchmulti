import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { OAuthDeleteConfirmDialog } from "./OAuthDeleteConfirmDialog";

describe("OAuthDeleteConfirmDialog", () => {
  it("取消单账号删除时不执行删除", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <OAuthDeleteConfirmDialog
        target={{
          kind: "account",
          accountId: "account-1",
          label: "user@example.com",
        }}
        providerLabel="ChatGPT"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    await user.click(screen.getByRole("button", { name: "取消" }));

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("确认移除全部账号后才执行操作", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <OAuthDeleteConfirmDialog
        target={{ kind: "all" }}
        providerLabel="xAI"
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByText(/移除所有 xAI 账号/)).toBeInTheDocument();
    expect(onConfirm).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "移除全部" }));

    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

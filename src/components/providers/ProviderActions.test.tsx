import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderActions } from "./ProviderActions";

describe("ProviderActions official authentication entry", () => {
  it("renders an explicit authentication-settings action when requested", () => {
    const onOfficialAuthSettings = vi.fn();
    render(
      <ProviderActions
        appId="codex"
        isCurrent
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        showOfficialAuthSettings
        onOfficialAuthSettings={onOfficialAuthSettings}
      />,
    );

    const action = screen.getByRole("button", { name: "认证设置" });
    expect(action).toHaveClass("opacity-100");
    fireEvent.click(action);
    expect(onOfficialAuthSettings).toHaveBeenCalledTimes(1);
  });
});

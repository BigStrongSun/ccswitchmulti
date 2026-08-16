import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AppSwitcher } from "@/components/AppSwitcher";

describe("AppSwitcher responsive overflow", () => {
  let offsetWidthSpy: ReturnType<typeof vi.spyOn>;
  let clientWidthSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    offsetWidthSpy = vi
      .spyOn(HTMLElement.prototype, "offsetWidth", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.tagName === "BUTTON" ? 40 : 0;
      });
    clientWidthSpy = vi
      .spyOn(HTMLElement.prototype, "clientWidth", "get")
      .mockReturnValue(120);
  });

  afterEach(() => {
    offsetWidthSpy.mockRestore();
    clientWidthSpy.mockRestore();
  });

  it("keeps the active app visible and moves apps that do not fit into an accessible menu", () => {
    render(
      <div>
        <AppSwitcher
          activeApp="hermes"
          onSwitch={vi.fn()}
          visibleApps={undefined}
        />
      </div>,
    );

    expect(screen.getByRole("button", { name: "Hermes" })).toBeVisible();
    const more = screen.getByRole("button", { name: "More apps" });
    fireEvent.click(more);

    const menu = screen.getByRole("dialog");
    expect(within(menu).getByRole("button", { name: "Codex" })).toBeVisible();
    expect(within(menu).queryByRole("button", { name: "Hermes" })).toBeNull();
  });

  it("shows every app directly when the switcher slot is wide enough", () => {
    clientWidthSpy.mockReturnValue(1000);

    render(
      <div>
        <AppSwitcher
          activeApp="codex"
          onSwitch={vi.fn()}
          visibleApps={undefined}
        />
      </div>,
    );

    expect(screen.getByRole("button", { name: "Claude Code" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Hermes" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "More apps" })).toBeNull();
  });
});

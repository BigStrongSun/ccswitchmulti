import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DirectorySettings } from "@/components/settings/DirectorySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("DirectorySettings", () => {
  it("shows the Pi directory and routes edits to the Pi app", () => {
    const onDirectoryChange = vi.fn();

    render(
      <DirectorySettings
        appConfigDir="/app"
        resolvedDirs={{
          appConfig: "/app",
          claude: "/claude",
          codex: "/codex",
          gemini: "/gemini",
          grokbuild: "/grok",
          opencode: "/opencode",
          openclaw: "/openclaw",
          hermes: "/hermes",
          pi: "/pi/default",
        }}
        onAppConfigChange={vi.fn()}
        onBrowseAppConfig={vi.fn()}
        onResetAppConfig={vi.fn()}
        piDir="/pi/custom"
        onDirectoryChange={onDirectoryChange}
        onBrowseDirectory={vi.fn()}
        onResetDirectory={vi.fn()}
      />,
    );

    const piInput = screen.getByDisplayValue("/pi/custom");
    fireEvent.change(piInput, { target: { value: "/pi/next" } });

    expect(screen.getByText("settings.piConfigDir")).toBeInTheDocument();
    expect(onDirectoryChange).toHaveBeenCalledWith("pi", "/pi/next");
  });
});

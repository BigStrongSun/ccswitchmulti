import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsFormState } from "@/hooks/useSettingsForm";
import type { CodexOfficialContextLimit } from "@/types";

const { getLimits } = vi.hoisted(() => ({ getLimits: vi.fn() }));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: {
      ...actual.settingsApi,
      getCodexOfficialContextLimits: getLimits,
      hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(false),
    },
  };
});

import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";

const limits: CodexOfficialContextLimit[] = [
  {
    model: "gpt-6.1",
    defaultContextWindow: 272000,
    maxContextWindow: 872000,
    requestedContextWindow: 1000000,
    effectiveContextWindow: 872000,
    autoCompactTokenLimit: 784800,
  },
];

const defaultSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  language: "zh",
} as SettingsFormState;

describe("Codex official context setting", () => {
  beforeEach(() => {
    getLimits.mockReset().mockResolvedValue(limits);
  });

  it("shows the trusted catalog calculation and persists the 1M preference", async () => {
    const onChange = vi.fn().mockResolvedValue(true);
    const view = render(
      <CodexAuthSettings settings={defaultSettings} onChange={onChange} />,
    );

    expect(await screen.findByText("872,000")).toBeInTheDocument();
    expect(screen.getByText("784,800")).toBeInTheDocument();
    expect(screen.getByText("272,000")).toBeInTheDocument();
    expect(screen.getByText("1,000,000")).toBeInTheDocument();
    expect(
      screen.getByText("settings.codexOfficialContextCalculated"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "settings.codexOfficialContext1M" }),
    );
    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith({ codexOfficialContext1M: true }),
    );
    expect(getLimits).toHaveBeenLastCalledWith(true);

    view.rerender(
      <CodexAuthSettings
        settings={{ ...defaultSettings, codexOfficialContext1M: true }}
        onChange={onChange}
      />,
    );
    expect(
      screen.getByRole("button", { name: "settings.codexOfficialContext1M" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("does not refresh as saved when persistence fails, and can restore default", async () => {
    const onChange = vi.fn().mockResolvedValue(false);
    render(
      <CodexAuthSettings
        settings={{ ...defaultSettings, codexOfficialContext1M: true }}
        onChange={onChange}
      />,
    );
    await screen.findByText("872,000");
    const callsBefore = getLimits.mock.calls.length;

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexOfficialContextRestore",
      }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "settings.saveFailedGeneric",
    );
    expect(onChange).toHaveBeenCalledWith({ codexOfficialContext1M: false });
    expect(getLimits).toHaveBeenCalledTimes(callsBefore);
  });

  it("reports preview refresh failure separately after a successful save", async () => {
    getLimits
      .mockReset()
      .mockResolvedValueOnce(limits)
      .mockRejectedValueOnce(new Error("preview unavailable"));
    const onChange = vi.fn().mockResolvedValue(true);
    render(
      <CodexAuthSettings settings={defaultSettings} onChange={onChange} />,
    );
    await screen.findByText("872,000");

    fireEvent.click(
      screen.getByRole("button", { name: "settings.codexOfficialContext1M" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "settings.codexOfficialContextLoadFailed",
    );
    expect(onChange).toHaveBeenCalledWith({ codexOfficialContext1M: true });
    expect(getLimits).toHaveBeenCalledTimes(2);
  });
});

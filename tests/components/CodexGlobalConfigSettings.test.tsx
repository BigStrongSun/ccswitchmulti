import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { CodexGlobalConfigSettings } from "@/components/settings/CodexGlobalConfigSettings";
import { configApi } from "@/lib/api";
import { isCodexGoalModeEnabled } from "@/utils/providerConfigUtils";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="Codex 全局 TOML"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/lib/api", () => ({
  configApi: {
    getCommonConfigSnippet: vi.fn(),
    setCommonConfigSnippet: vi.fn(),
  },
}));

describe("CodexGlobalConfigSettings", () => {
  beforeEach(() => {
    vi.mocked(configApi.getCommonConfigSnippet).mockResolvedValue(
      'model_reasoning_summary = "auto"\n',
    );
    vi.mocked(configApi.setCommonConfigSnippet).mockResolvedValue(undefined);
  });

  it("loads and saves the shared Codex TOML from global settings", async () => {
    render(<CodexGlobalConfigSettings />);

    expect(await screen.findByLabelText("Codex 全局 TOML")).toHaveValue(
      'model_reasoning_summary = "auto"\n',
    );

    fireEvent.change(screen.getByLabelText("Codex 全局 TOML"), {
      target: { value: 'model_reasoning_summary = "none"\n' },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存 Codex 全局配置" }));

    await waitFor(() =>
      expect(configApi.setCommonConfigSnippet).toHaveBeenCalledWith(
        "codex",
        'model_reasoning_summary = "none"\n',
      ),
    );
  });

  it("owns the Goal mode switch outside individual providers", async () => {
    render(<CodexGlobalConfigSettings />);

    const goalMode = await screen.findByRole("checkbox", {
      name: "启用 Goal mode",
    });
    expect(goalMode).not.toBeChecked();

    fireEvent.click(goalMode);

    expect(
      isCodexGoalModeEnabled(
        (screen.getByLabelText("Codex 全局 TOML") as HTMLTextAreaElement).value,
      ),
    ).toBe(true);
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  EnvInjectionSettings as EnvInjectionSettingsValue,
  EnvInjectionSyncReport,
} from "@/types";
import { settingsApi } from "@/lib/api/settings";
import { EnvInjectionSettings } from "./EnvInjectionSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    inspectEnvInjectionStatus: vi.fn(),
    retryEnvInjectionSync: vi.fn(),
  },
}));

const value: EnvInjectionSettingsValue = {
  enabled: true,
  targets: { claude: true, codex: true },
  variables: { TZ: "Asia/Shanghai" },
};

const target = (
  state: "disabled" | "synced" | "conflict" | "pending" | "failed",
  overrides: Partial<EnvInjectionSyncReport["claude"]> = {},
): EnvInjectionSyncReport["claude"] => ({
  state,
  managedKeys: [],
  addedKeys: [],
  updatedKeys: [],
  removedKeys: [],
  relinquishedKeys: [],
  conflictedKeys: [],
  ...overrides,
});

const report = (
  state: EnvInjectionSyncReport["state"],
  claude: EnvInjectionSyncReport["claude"],
  codex: EnvInjectionSyncReport["codex"],
): EnvInjectionSyncReport => ({
  state,
  claude,
  codex,
  codexIncludeAllowlist: false,
});

describe("EnvInjectionSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows a saved-but-partial result and lets the user retry the failed target", async () => {
    const partial = report(
      "partial",
      target("synced", { managedKeys: ["TZ"] }),
      target("failed", { error: "config.toml is locked" }),
    );
    const synced = report(
      "synced",
      target("synced", { managedKeys: ["TZ"] }),
      target("synced", { managedKeys: ["TZ"] }),
    );
    vi.mocked(settingsApi.inspectEnvInjectionStatus).mockResolvedValue(partial);
    vi.mocked(settingsApi.retryEnvInjectionSync).mockResolvedValue(synced);

    render(
      <EnvInjectionSettings
        value={value}
        onChange={vi.fn().mockResolvedValue(partial)}
      />,
    );

    expect(
      await screen.findByText("设置已保存，但部分 CLI 尚未同步"),
    ).toBeInTheDocument();
    expect(screen.getByText("config.toml is locked")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "重试同步" }));

    await waitFor(() =>
      expect(settingsApi.retryEnvInjectionSync).toHaveBeenCalledTimes(1),
    );
    expect(
      await screen.findByText("所有已启用目标均已同步"),
    ).toBeInTheDocument();
  });

  it("labels an equal pre-existing value as user-owned instead of managed", async () => {
    vi.mocked(settingsApi.inspectEnvInjectionStatus).mockResolvedValue(
      report(
        "warning",
        target("conflict", { conflictedKeys: ["TZ"] }),
        target("disabled"),
      ),
    );

    render(<EnvInjectionSettings value={value} onChange={vi.fn()} />);

    expect(
      await screen.findByText("TZ 已存在并保持用户所有，CCSM 不会覆盖或删除它"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("所有已启用目标均已同步"),
    ).not.toBeInTheDocument();
  });

  it("renders the structured result returned by the save action", async () => {
    vi.mocked(settingsApi.inspectEnvInjectionStatus).mockResolvedValue(
      report("synced", target("synced"), target("synced")),
    );
    const failed = report(
      "failed",
      target("failed", { error: "settings.json denied" }),
      target("failed", { error: "config.toml denied" }),
    );
    const onChange = vi.fn().mockResolvedValue(failed);

    render(<EnvInjectionSettings value={value} onChange={onChange} />);
    await screen.findByText("所有已启用目标均已同步");

    fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));

    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("CLI 同步失败，请重试")).toBeInTheDocument();
    expect(screen.getByText("settings.json denied")).toBeInTheDocument();
    expect(screen.getByText("config.toml denied")).toBeInTheDocument();
  });
});
